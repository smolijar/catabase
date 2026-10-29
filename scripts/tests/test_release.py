import base64
import importlib.util
import hashlib
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "release.py"
SPEC = importlib.util.spec_from_file_location("release", SCRIPT)
RELEASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE)
SOURCE = "a" * 40


def key_fixture(key_id=b"12345678"):
    key_record = base64.b64encode(b"Ed" + key_id + bytes(32)).decode()
    return base64.b64encode(f"untrusted comment: test public key\n{key_record}\n".encode()).decode()


def signature_fixture(key_id=b"12345678"):
    signature = base64.b64encode(b"ED" + key_id + bytes(64)).decode()
    global_signature = base64.b64encode(bytes(64)).decode()
    return base64.b64encode(f"untrusted comment: test signature\n{signature}\ntrusted comment: test\n{global_signature}\n".encode()).decode()


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.plan = {"repo": "smolijar/catabase", "source": SOURCE, "tag": "v0.1.2", "version": "0.1.2", "previousId": None, "draftId": None, "apple": False, "updater": False, "notes": f"<!-- catabase-source: {SOURCE} -->"}

    def assets(self, updater=False):
        self.plan["updater"] = updater
        folder = self.root / "artifacts"
        folder.mkdir(exist_ok=True)
        for name in RELEASE.expected_assets(self.plan) - {"SHA256SUMS"}:
            (folder / name).write_bytes(b"fixture package")
        if updater:
            signature = signature_fixture()
            self.plan["publicKey"] = key_fixture()
            (folder / (RELEASE.ASSET_UPDATE + ".sig")).write_text(signature)
            url = f'https://github.com/{self.plan["repo"]}/releases/download/{self.plan["tag"]}/{RELEASE.ASSET_UPDATE}'
            (folder / "latest.json").write_text(json.dumps({"version": self.plan["version"], "platforms": {target: {"url": url, "signature": signature} for target in ("darwin-aarch64", "darwin-x86_64")}}))
        self.checksums(folder)
        return folder

    def checksums(self, folder):
        (folder / "SHA256SUMS").write_text("".join(f"{RELEASE.file_hash(path)}  {path.name}\n" for path in sorted(folder.iterdir()) if path.name != "SHA256SUMS"))

    def test_next_version_uses_base_then_monotonic_patch(self):
        self.assertEqual(RELEASE.next_version("0.1.0"), "0.1.0")
        self.assertEqual(RELEASE.next_version("0.1.0", "0.1.0"), "0.1.1")
        self.assertEqual(RELEASE.next_version("0.1.0", "0.1.9"), "0.1.10")
        self.assertEqual(RELEASE.next_version("0.2.0", "0.1.9"), "0.2.0")
        for invalid in ("01.2.3", "1.2.3-rc.1", "1.2.3+ci", "1.2", "v1.2.3"):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                RELEASE.next_version(invalid)

    def test_version_order_is_numeric_and_ignores_drafts_prereleases(self):
        items = [{"tag_name": tag, "draft": draft, "prerelease": pre} for tag, draft, pre in (("v0.1.9", False, False), ("v0.1.10", False, False), ("v9.0.0", True, False), ("v9.0.1", False, True), ("hello", False, False))]
        self.assertEqual([item["tag_name"] for item in RELEASE.stable_releases(items)], ["v0.1.10", "v0.1.9"])

    def test_signing_independent_and_partial_configuration_fails(self):
        self.assertEqual(RELEASE.signing_mode({}), (False, False))
        apple = dict.fromkeys(RELEASE.APPLE_KEYS, "configured")
        updater = {"TAURI_SIGNING_PRIVATE_KEY": "secret", "TAURI_SIGNING_PUBLIC_KEY": "public"}
        self.assertEqual(RELEASE.signing_mode(apple), (True, False))
        self.assertEqual(RELEASE.signing_mode(updater), (False, True))
        self.assertEqual(RELEASE.signing_mode(apple | updater), (True, True))
        for invalid in ({"APPLE_ID": "configured"}, {"TAURI_SIGNING_PRIVATE_KEY": "secret"}, {"TAURI_SIGNING_PUBLIC_KEY": "public"}, {"TAURI_SIGNING_PRIVATE_KEY_PASSWORD": "password"}, apple | {"APPLE_SIGNING_IDENTITY": "-"}):
            with self.subTest(keys=list(invalid)), self.assertRaises(ValueError):
                RELEASE.signing_mode(invalid)

    def test_ad_hoc_build_omits_empty_and_disabled_credentials(self):
        source = dict.fromkeys(RELEASE.APPLE_KEYS, "") | {
            "PATH": "/example/bin", "TAURI_SIGNING_PRIVATE_KEY": "",
            "TAURI_SIGNING_PRIVATE_KEY_PASSWORD": "", "TAURI_SIGNING_PRIVATE_KEY_PATH": "/unused",
            "APPLE_API_KEY": "unused", "APPLE_API_ISSUER": "unused", "APPLE_API_KEY_PATH": "/unused",
            "APPLE_KEYCHAIN_PROFILE": "unused",
        }
        result = RELEASE.build_environment(self.plan, source)
        self.assertEqual(result, {"PATH": "/example/bin", "APPLE_SIGNING_IDENTITY": "-"})
        self.assertEqual(source["APPLE_SIGNING_IDENTITY"], "")
        self.assertIn("APPLE_CERTIFICATE", source)

    def test_apple_build_retains_complete_credentials(self):
        source = dict.fromkeys(RELEASE.APPLE_KEYS, "configured") | {"PATH": "/example/bin"}
        self.assertEqual(RELEASE.build_environment(self.plan | {"apple": True}, source), source)
        with self.assertRaisesRegex(ValueError, "Apple signing credentials"):
            RELEASE.build_environment(self.plan | {"apple": True}, source | {"APPLE_CERTIFICATE": ""})

    def test_updater_build_retains_key_and_optional_empty_password(self):
        for password in (None, "", "passphrase"):
            source = {"TAURI_SIGNING_PRIVATE_KEY": "private"}
            if password is not None:
                source["TAURI_SIGNING_PRIVATE_KEY_PASSWORD"] = password
            result = RELEASE.build_environment(self.plan | {"updater": True}, source)
            self.assertEqual(result, source | {"APPLE_SIGNING_IDENTITY": "-"})
        with self.assertRaisesRegex(ValueError, "Updater private key"):
            RELEASE.build_environment(self.plan | {"updater": True}, {"TAURI_SIGNING_PRIVATE_KEY": ""})

    def test_combined_signing_keeps_both_sets_and_build_streams_output(self):
        source = dict.fromkeys(RELEASE.APPLE_KEYS, "configured") | {"TAURI_SIGNING_PRIVATE_KEY": "private", "TAURI_SIGNING_PRIVATE_KEY_PASSWORD": ""}
        plan = self.plan | {"apple": True, "updater": True}
        self.assertEqual(RELEASE.build_environment(plan, source), source)
        plan_path = self.root / "plan.json"
        plan_path.write_text(json.dumps(plan))
        with patch.dict(os.environ, source, clear=True), patch.object(RELEASE.subprocess, "run") as runner:
            RELEASE.build(plan_path)
            self.assertEqual(runner.call_args.kwargs, {"cwd": RELEASE.ROOT, "env": source, "check": True})
            self.assertEqual(runner.call_args.args[0], ["pnpm", "tauri", "build", "--ci", "--target", "universal-apple-darwin", "--bundles", "app,dmg", "--config", str(plan_path.with_suffix(".config.json")), "--", "--locked"])

    def test_signed_release_cannot_silently_lose_or_rotate_signing(self):
        public_key = "public"
        digest = hashlib.sha256(public_key.encode()).hexdigest()
        previous = {"body": f"<!-- catabase-apple-signed: true -->\n<!-- catabase-updater-key: {digest} -->", "assets": [{"name": "latest.json"}]}
        RELEASE.validate_signing_continuity(previous, True, True, public_key)
        for apple, updater, key in ((False, True, public_key), (True, False, ""), (True, True, "other")):
            with self.subTest(apple=apple, updater=updater, key=key), self.assertRaises(ValueError):
                RELEASE.validate_signing_continuity(previous, apple, updater, key)
        with self.assertRaisesRegex(ValueError, "restore its keypair"):
            RELEASE.validate_signing_continuity({"assets": [{"name": "latest.json"}]}, False, False, "")

    def test_only_successful_trusted_main_ci_is_releasable(self):
        valid = {"conclusion": "success", "status": "completed", "head_branch": "main", "event": "push", "path": ".github/workflows/ci.yml", "head_repository": {"full_name": "smolijar/catabase"}, "head_sha": SOURCE}
        self.assertEqual(RELEASE.validate_run(valid, "smolijar/catabase"), SOURCE)
        for change in ({"conclusion": "failure"}, {"status": "in_progress"}, {"head_branch": "feature"}, {"event": "pull_request"}, {"path": ".github/workflows/other.yml"}, {"head_repository": {"full_name": "fork/catabase"}}, {"head_sha": "main; do something"}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                RELEASE.validate_run(valid | change, "smolijar/catabase")

    def test_bumping_all_versions_preserves_dependency_versions(self):
        for filename in ("Cargo.toml", "Cargo.lock", "ui/package.json", "src-tauri/tauri.conf.json"):
            target = self.root / filename
            target.parent.mkdir(exist_ok=True)
            shutil.copyfile(SCRIPT.parent.parent / filename, target)
        old_lock = RELEASE.CHECK.read_manifest(self.root, "Cargo.lock")
        old_deps = {entry["name"]: entry["version"] for entry in old_lock["package"] if entry["name"] not in ("catabase", "catabase-core")}
        RELEASE.write_version(self.root, "1.2.3")
        self.assertEqual(RELEASE.CHECK.check_versions(self.root, "v1.2.3"), "1.2.3")
        new_lock = RELEASE.CHECK.read_manifest(self.root, "Cargo.lock")
        self.assertEqual(old_deps, {entry["name"]: entry["version"] for entry in new_lock["package"] if entry["name"] not in ("catabase", "catabase-core")})

    def test_changelog_uses_only_commits_since_previous_tag(self):
        with patch.object(RELEASE, "run", return_value=f'{SOURCE}\tFix <render> and @mention\n') as runner:
            notes = RELEASE.release_notes("smolijar/catabase", SOURCE, "v0.1.1", False, True)
        self.assertEqual(runner.call_args.args[-1], f"v0.1.1..{SOURCE}")
        self.assertIn("\\<render\\>", notes)
        self.assertIn("&#64;mention", notes)
        self.assertIn("not Apple notarized", notes)
        self.assertIn("Updater signatures are included", notes)
        self.assertEqual(RELEASE.source_marker({"body": notes}), SOURCE)

    def test_complete_unsigned_and_update_assets_validate(self):
        for updater in (False, True):
            folder = self.assets(updater)
            self.assertEqual(set(RELEASE.validate_assets(self.plan, folder)), RELEASE.expected_assets(self.plan))
            shutil.rmtree(folder)

    def test_missing_extra_and_modified_assets_are_rejected(self):
        folder = self.assets()
        (folder / RELEASE.ASSET_ZIP).unlink()
        with self.assertRaises(ValueError):
            RELEASE.validate_assets(self.plan, folder)
        shutil.rmtree(folder)
        folder = self.assets()
        (folder / "unexpected.txt").write_text("should not upload")
        with self.assertRaises(ValueError):
            RELEASE.validate_assets(self.plan, folder)
        (folder / "unexpected.txt").unlink()
        (folder / RELEASE.ASSET_ZIP).write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "checksum"):
            RELEASE.validate_assets(self.plan, folder)

    def test_wrong_updater_keypair_blocks_publication(self):
        RELEASE.validate_updater_key(signature_fixture(), key_fixture())
        with self.assertRaisesRegex(ValueError, "does not match"):
            RELEASE.validate_updater_key(signature_fixture(b"87654321"), key_fixture())
        for signature, key in (("garbage", key_fixture()), (signature_fixture(), None), (signature_fixture(), "garbage"), (base64.b64encode(b"untrusted comment: truncated").decode(), key_fixture())):
            with self.subTest(signature=signature), self.assertRaisesRegex(ValueError, "malformed"):
                RELEASE.validate_updater_key(signature, key)
        folder = self.assets(True)
        self.plan["publicKey"] = key_fixture(b"87654321")
        with self.assertRaisesRegex(ValueError, "does not match"):
            RELEASE.validate_assets(self.plan, folder)

    def test_update_manifest_requires_both_architectures_and_pinned_release(self):
        folder = self.assets(True)
        path = folder / "latest.json"
        manifest = json.loads(path.read_text())
        manifest["platforms"]["darwin-aarch64"]["url"] = "https://github.com/smolijar/catabase/releases/latest/download/Catabase.app.tar.gz"
        path.write_text(json.dumps(manifest))
        self.checksums(folder)
        with self.assertRaisesRegex(ValueError, "Updater manifest"):
            RELEASE.validate_assets(self.plan, folder)

    def test_prepare_skips_a_published_source_without_allocating(self):
        with patch.object(RELEASE, "run", return_value=SOURCE), patch.object(RELEASE, "git_ancestor", return_value=True), patch.object(RELEASE, "releases", return_value=[{"draft": False, "body": self.plan["notes"]}]), patch.object(RELEASE, "output") as output, patch.object(RELEASE, "write_version") as writer:
            RELEASE.prepare("smolijar/catabase", self.root / "plan.json")
            output.assert_called_once_with("skip", "true")
            writer.assert_not_called()

    def test_prepare_skips_a_run_older_than_latest_release(self):
        previous = {"id": 1, "draft": False, "prerelease": False, "tag_name": "v0.1.3", "body": ""}
        with patch.object(RELEASE, "run", return_value=SOURCE), patch.object(RELEASE, "git_ancestor", return_value=True), patch.object(RELEASE, "tag_source", return_value="b" * 40), patch.object(RELEASE, "releases", return_value=[previous]), patch.object(RELEASE, "output") as output, patch.object(RELEASE, "write_version") as writer:
            RELEASE.prepare("smolijar/catabase", self.root / "plan.json")
            output.assert_called_once_with("skip", "true")
            writer.assert_not_called()

    def test_prepare_reuses_a_failed_drafts_version(self):
        previous = {"id": 1, "draft": False, "prerelease": False, "tag_name": "v0.1.3", "body": ""}
        draft = {"id": 2, "draft": True, "prerelease": False, "tag_name": "v0.1.4", "body": self.plan["notes"]}
        plan_path = self.root / "plan.json"
        with patch.object(RELEASE, "run", return_value=SOURCE), patch.object(RELEASE, "git_ancestor", side_effect=[True, False, True]), patch.object(RELEASE, "tag_source", return_value="b" * 40), patch.object(RELEASE, "releases", return_value=[previous, draft]), patch.object(RELEASE, "output"), patch.object(RELEASE.CHECK, "check_versions", return_value="0.1.0"), patch.object(RELEASE, "write_version") as writer, patch.object(RELEASE, "release_notes", return_value=self.plan["notes"]), patch.object(RELEASE.subprocess, "run") as process, patch.dict(os.environ, {}, clear=True):
            process.return_value.returncode = 1
            RELEASE.prepare("smolijar/catabase", plan_path)
            writer.assert_called_once_with(RELEASE.ROOT, "0.1.4")
            self.assertEqual(json.loads(plan_path.read_text())["draftId"], 2)
            self.assertFalse(json.loads(plan_path.with_suffix(".config.json").read_text())["bundle"]["createUpdaterArtifacts"])

    def test_published_source_rerun_never_uploads_or_mutates(self):
        folder = self.assets()
        plan_path = self.root / "plan.json"
        plan_path.write_text(json.dumps(self.plan))
        with patch.object(RELEASE, "releases", return_value=[{"draft": False, "body": self.plan["notes"]}]), patch.object(RELEASE, "github") as api, patch.object(RELEASE, "run") as runner:
            RELEASE.publish(plan_path, folder)
            api.assert_not_called()
            runner.assert_not_called()

    def test_publication_waits_for_complete_verified_upload(self):
        folder = self.assets()
        plan_path = self.root / "plan.json"
        plan_path.write_text(json.dumps(self.plan))
        uploaded_assets = [{"name": p.name, "size": p.stat().st_size, "digest": "sha256:" + RELEASE.file_hash(p)} for p in folder.iterdir()]
        responses = [{"id": 7, "assets": []}, {"draft": True, "assets": uploaded_assets}, {"html_url": "https://github.com/smolijar/catabase/releases/tag/v0.1.2"}]
        with patch.object(RELEASE, "releases", return_value=[]), patch.object(RELEASE, "github", side_effect=responses) as api, patch.object(RELEASE, "run") as runner, patch.dict(os.environ, {}, clear=True):
            RELEASE.publish(plan_path, folder)
            self.assertEqual(api.call_args_list[0].args[3]["draft"], True)
            self.assertIn("--clobber", runner.call_args.args)
            self.assertEqual(api.call_args_list[-1].args[3], {"body": self.plan["notes"], "draft": False, "prerelease": False, "make_latest": "true"})

    def test_bad_uploaded_asset_cannot_publish(self):
        folder = self.assets()
        plan_path = self.root / "plan.json"
        plan_path.write_text(json.dumps(self.plan))
        with patch.object(RELEASE, "releases", return_value=[]), patch.object(RELEASE, "github", side_effect=[{"id": 7, "assets": []}, {"draft": True, "assets": []}]) as api, patch.object(RELEASE, "run"):
            with self.assertRaisesRegex(ValueError, "Uploaded assets"):
                RELEASE.publish(plan_path, folder)
            self.assertFalse(any(call.args[2:3] == ("PATCH",) for call in api.call_args_list))

    def test_changed_latest_release_blocks_publication(self):
        folder = self.assets()
        plan_path = self.root / "plan.json"
        plan_path.write_text(json.dumps(self.plan))
        with patch.object(RELEASE, "releases", return_value=[{"id": 9, "tag_name": "v0.1.1", "draft": False, "prerelease": False}]), patch.object(RELEASE, "github") as api:
            with self.assertRaisesRegex(ValueError, "Latest release changed"):
                RELEASE.publish(plan_path, folder)
            api.assert_not_called()


if __name__ == "__main__":
    unittest.main()
