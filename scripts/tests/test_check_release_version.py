import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "check-release-version.py"
REPOSITORY = SCRIPT.parent.parent
SPEC = importlib.util.spec_from_file_location("check_release_version", SCRIPT)
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)


class ReleaseVersionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.write_manifests()

    def write_manifests(self, version="1.2.3", overrides=None):
        versions = dict.fromkeys(("workspace", "ui", "tauri", "catabase", "catabase-core"), version)
        versions.update(overrides or {})
        (self.root / "Cargo.toml").write_text(
            f'[workspace.package]\nversion = "{versions["workspace"]}"\n', encoding="utf-8"
        )
        for folder, filename, key in (("ui", "package.json", "ui"), ("src-tauri", "tauri.conf.json", "tauri")):
            (self.root / folder).mkdir(exist_ok=True)
            (self.root / folder / filename).write_text(json.dumps({"version": versions[key]}), encoding="utf-8")
        (self.root / "Cargo.lock").write_text(
            "version = 4\n" + "".join(
                f'\n[[package]]\nname = "{name}"\nversion = "{versions[name]}"\n'
                for name in ("catabase", "catabase-core")
            ),
            encoding="utf-8",
        )

    def run_cli(self, *arguments):
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(self.root), *arguments],
            capture_output=True,
            text=True,
            check=False,
            cwd=self.root,
        )

    def test_matching_versions_and_tag_print_only_version(self):
        for arguments in ((), ("--tag", "v1.2.3")):
            with self.subTest(arguments=arguments):
                result = self.run_cli(*arguments)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, "1.2.3\n")
                self.assertEqual(result.stderr, "")

    def test_current_repository_manifests_work_from_temporary_fixture(self):
        for filename in ("Cargo.toml", "Cargo.lock", "ui/package.json", "src-tauri/tauri.conf.json"):
            shutil.copyfile(REPOSITORY / filename, self.root / filename)
        expected = json.loads((self.root / "ui/package.json").read_text(encoding="utf-8"))["version"]
        result = self.run_cli("--tag", f"v{expected}")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, f"{expected}\n")

    def test_every_app_version_must_match_workspace(self):
        for key, source in (
            ("ui", "ui/package.json"),
            ("tauri", "src-tauri/tauri.conf.json"),
            ("catabase", "Cargo.lock catabase version"),
            ("catabase-core", "Cargo.lock catabase-core version"),
            ("workspace", "Cargo.toml workspace.package.version"),
        ):
            with self.subTest(key=key):
                self.write_manifests(overrides={key: "1.2.4"})
                result = self.run_cli()
                self.assertEqual(result.returncode, 1)
                self.assertEqual(result.stdout, "")
                self.assertIn("Expected", result.stderr)
                self.assertIn(source, result.stderr)

    def test_valid_prereleases(self):
        for version in ("0.0.0", "1.2.3-rc.1", "1.2.3-0", "1.2.3-01alpha", "1.2.3-alpha-beta.9"):
            with self.subTest(version=version):
                self.write_manifests(version)
                self.assertEqual(CHECKER.check_versions(self.root, f"v{version}"), version)

    def test_invalid_tags_and_build_metadata(self):
        for tag in (
            "1.2.3", "V1.2.3", "v1.2", "v1.2.3.4", "v01.2.3", "v1.02.3", "v1.2.03",
            "v1.2.3-01", "v1.2.3-rc.01", "v1.2.3-", "v1.2.3-rc..1", "v1.2.3-alpha_1",
            "v1.2.3+build.1", "v1.2.3-rc.1+build", "v1.2.3\n", "v1.2.3/unsafe", "v１.2.3", "",
        ):
            with self.subTest(tag=tag):
                with self.assertRaisesRegex(CHECKER.ReleaseVersionError, "Release tag must"):
                    CHECKER.check_versions(self.root, tag)

    def test_mismatched_tag_fails_clearly(self):
        result = self.run_cli("--tag", "v2.0.0")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertIn("release tag is '2.0.0'", result.stderr)

    def test_invalid_manifest_version(self):
        for invalid in ("1.2", "01.2.3", "1.2.3-01", "1.2.3+build"):
            with self.subTest(version=invalid):
                self.write_manifests(overrides={"ui": invalid})
                with self.assertRaisesRegex(CHECKER.ReleaseVersionError, "ui/package.json version must be valid SemVer"):
                    CHECKER.check_versions(self.root)

    def test_missing_or_duplicate_lock_package(self):
        for name in ("catabase", "catabase-core"):
            for count in (0, 2):
                with self.subTest(name=name, count=count):
                    self.write_manifests()
                    path = self.root / "Cargo.lock"
                    entry = f'\n[[package]]\nname = "{name}"\nversion = "1.2.3"\n'
                    path.write_text(path.read_text(encoding="utf-8").replace(entry, entry * count), encoding="utf-8")
                    with self.assertRaisesRegex(CHECKER.ReleaseVersionError, f"exactly one '{name}' package; found {count}"):
                        CHECKER.check_versions(self.root)

    def test_missing_and_malformed_manifests_fail_without_traceback(self):
        for filename, contents in (("ui/package.json", None), ("Cargo.toml", "[broken"), ("src-tauri/tauri.conf.json", "{")):
            with self.subTest(filename=filename):
                self.write_manifests()
                path = self.root / filename
                if contents is None:
                    path.unlink()
                else:
                    path.write_text(contents, encoding="utf-8")
                result = self.run_cli()
                self.assertEqual(result.returncode, 1)
                self.assertIn(f"Cannot read {filename}", result.stderr)
                self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
