#!/usr/bin/env python3
"""Prepare and publish immutable main-branch releases (Python 3.11+, Git, gh)."""

import argparse
import base64
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("release_version", ROOT / "scripts/check-release-version.py")
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)
STABLE_VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
SHA = re.compile(r"[0-9a-f]{40}")
SOURCE_MARKER = re.compile(r"<!-- catabase-source: ([0-9a-f]{40}) -->")
APPLE_KEYS = ("APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD", "APPLE_SIGNING_IDENTITY", "APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID")
ASSET_ZIP = "Catabase-macos-universal.app.zip"
ASSET_DMG = "Catabase-macos-universal.dmg"
ASSET_UPDATE = "Catabase.app.tar.gz"


def run(*args, cwd=ROOT, input_text=None):
    return subprocess.run(args, cwd=cwd, input=input_text, text=True, capture_output=True, check=True).stdout.strip()


def github(repo, path, method="GET", payload=None):
    args = ["gh", "api", f"repos/{repo}/{path}", "--method", method]
    if payload is not None:
        args += ["--input", "-"]
    result = run(*args, input_text=json.dumps(payload) if payload is not None else None)
    return json.loads(result) if result else None


def releases(repo):
    pages = json.loads(run("gh", "api", f"repos/{repo}/releases?per_page=100", "--paginate", "--slurp"))
    return [release for page in pages for release in page]


def version_tuple(version):
    match = STABLE_VERSION.fullmatch(version)
    if not match:
        raise ValueError("Automatic releases require a stable X.Y.Z version without prerelease/build metadata.")
    return tuple(map(int, match.groups()))


def stable_releases(items):
    return sorted((item for item in items if not item["draft"] and not item["prerelease"] and STABLE_VERSION.fullmatch(item["tag_name"].removeprefix("v")) and item["tag_name"].startswith("v")), key=lambda item: version_tuple(item["tag_name"][1:]), reverse=True)


def next_version(base, previous=None):
    base_parts = version_tuple(base)
    if previous is None or base_parts > version_tuple(previous):
        return base
    major, minor, patch = version_tuple(previous)
    return f"{major}.{minor}.{patch + 1}"


def source_marker(release):
    match = SOURCE_MARKER.search(release.get("body") or "")
    return match[1] if match else None


def signing_mode(env):
    apple = [key for key in APPLE_KEYS if env.get(key, "").strip()]
    if apple and len(apple) != len(APPLE_KEYS):
        raise ValueError("Incomplete Apple signing setup; missing: " + ", ".join(key for key in APPLE_KEYS if key not in apple))
    if apple and env["APPLE_SIGNING_IDENTITY"].strip() == "-":
        raise ValueError("Configured Apple signing must use a Developer ID identity, not '-'.")
    private = bool(env.get("TAURI_SIGNING_PRIVATE_KEY", "").strip())
    public = bool(env.get("TAURI_SIGNING_PUBLIC_KEY", "").strip())
    if private != public or (env.get("TAURI_SIGNING_PRIVATE_KEY_PASSWORD") and not private):
        raise ValueError("Updater signing requires TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PUBLIC_KEY together.")
    return bool(apple), private


def build_environment(plan, env):
    result = dict(env)
    if plan["apple"]:
        missing = [key for key in APPLE_KEYS if not result.get(key, "").strip()]
        if missing or result.get("APPLE_SIGNING_IDENTITY", "").strip() == "-":
            raise ValueError("Apple signing credentials are missing from the build environment.")
    else:
        # Presence, even with an empty value, makes Tauri attempt certificate
        # import/notarization. Ad-hoc builds must omit these variables entirely.
        for key in (*APPLE_KEYS, "APPLE_API_ISSUER", "APPLE_API_KEY", "APPLE_API_KEY_PATH", "APPLE_KEYCHAIN_PROFILE"):
            result.pop(key, None)
        result["APPLE_SIGNING_IDENTITY"] = "-"
    if plan["updater"]:
        if not result.get("TAURI_SIGNING_PRIVATE_KEY", "").strip():
            raise ValueError("Updater private key is missing from the build environment.")
        # An empty updater password is valid and is preserved if explicitly set.
    else:
        for key in ("TAURI_SIGNING_PRIVATE_KEY", "TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "TAURI_SIGNING_PRIVATE_KEY_PATH"):
            result.pop(key, None)
    return result


def build(plan_path):
    plan = json.loads(plan_path.read_text())
    command = ["pnpm", "tauri", "build", "--ci", "--target", "universal-apple-darwin", "--bundles", "app,dmg", "--config", str(plan_path.with_suffix(".config.json")), "--", "--locked"]
    # Inherit stdout/stderr so compiler progress remains visible in Actions.
    # Signing credentials stay in the child environment, never command arguments.
    subprocess.run(command, cwd=ROOT, env=build_environment(plan, os.environ), check=True)


def validate_signing_continuity(previous, apple, updater, public_key):
    if not previous:
        return
    body = previous.get("body") or ""
    if "<!-- catabase-apple-signed: true -->" in body and not apple:
        raise ValueError("Apple signing was enabled in the previous release; restore its credentials before publishing.")
    previous_key = re.search(r"<!-- catabase-updater-key: ([0-9a-f]{64}) -->", body)
    had_updates = previous_key or any(asset["name"] == "latest.json" for asset in previous.get("assets", []))
    if had_updates and not updater:
        raise ValueError("Updater signing was enabled in the previous release; restore its keypair before publishing.")
    if previous_key and previous_key[1] != hashlib.sha256(public_key.encode()).hexdigest():
        raise ValueError("Updater public key changed. Existing clients require a deliberate key-rotation migration.")


def validate_run(data, repo):
    if (data.get("conclusion") != "success" or data.get("status") != "completed"
            or data.get("head_branch") != "main"
            or data.get("event") not in ("push", "workflow_dispatch")
            or data.get("path", "").split("@")[0] != ".github/workflows/ci.yml"
            or data.get("head_repository", {}).get("full_name") != repo
            or not SHA.fullmatch(data.get("head_sha", ""))):
        raise ValueError("Release requires a successful CI run on this repository's main branch.")
    return data["head_sha"]


def output(key, value):
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as handle:
        handle.write(f"{key}={value}\n")


def git_ancestor(ancestor, descendant):
    result = subprocess.run(["git", "merge-base", "--is-ancestor", ancestor, descendant], cwd=ROOT, capture_output=True, check=False)
    if result.returncode not in (0, 1):
        raise ValueError("Cannot compare release source ancestry.")
    return result.returncode == 0


def tag_source(tag):
    source = run("git", "rev-parse", f"refs/tags/{tag}^{{commit}}")
    if not SHA.fullmatch(source):
        raise ValueError("Release tag does not resolve to a commit.")
    return source


def write_version(root, version):
    CHECK.check_versions(root)
    version_tuple(version)
    path = root / "Cargo.toml"
    text = path.read_text(encoding="utf-8")
    text, count = re.subn(r'(?ms)(^\[workspace\.package\]\n(?:(?!^\[).)*?^version\s*=\s*)"[^"]+"', lambda match: f'{match[1]}"{version}"', text)
    if count != 1:
        raise ValueError("Cannot update exactly one Cargo workspace version.")
    path.write_text(text, encoding="utf-8")
    path = root / "Cargo.lock"
    text = path.read_text(encoding="utf-8")
    for name in ("catabase", "catabase-core"):
        pattern = rf'(\[\[package\]\]\nname = "{re.escape(name)}"\nversion = )"[^"]+"'
        text, count = re.subn(pattern, lambda match: f'{match[1]}"{version}"', text)
        if count != 1:
            raise ValueError(f"Cannot update exactly one {name} lockfile version.")
    path.write_text(text, encoding="utf-8")
    for filename in ("ui/package.json", "src-tauri/tauri.conf.json"):
        path = root / filename
        document = json.loads(path.read_text(encoding="utf-8"))
        document["version"] = version
        path.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    CHECK.check_versions(root, "v" + version)


def release_notes(repo, source, previous_tag, apple, updater):
    revision = f"{previous_tag}..{source}" if previous_tag else source
    commits = run("git", "log", "--format=%H%x09%s", revision).splitlines()
    lines = [f"<!-- catabase-source: {source} -->", "", f"Universal macOS build from [{source[:7]}](https://github.com/{repo}/commit/{source}), for Apple Silicon and Intel.", ""]
    lines += ["Developer ID signed and Apple notarized." if apple else "**Preview quality: ad-hoc signed, not Apple notarized. macOS may block this app.**"]
    lines += ["Updater signatures are included." if updater else "In-app installation is unavailable in this build because updater signing is not configured.", "", "## Changes" if previous_tag else "## Changes in the first release", ""]
    for commit in commits:
        sha, subject = commit.split("\t", 1)
        # Commit subjects are data, never shell input or unescaped Markdown/HTML.
        subject = re.sub(r"([\\`*_{}\[\]()<>#])", r"\\\1", subject).replace("@", "&#64;")
        lines.append(f"- {subject} ([{sha[:7]}](https://github.com/{repo}/commit/{sha}))")
    if previous_tag:
        lines += ["", f"[Full diff since {previous_tag}](https://github.com/{repo}/compare/{previous_tag}...{source})"]
    notes = "\n".join(lines) + "\n"
    if len(notes.encode()) > 120_000:
        raise ValueError("Release notes exceed the publication limit; split this release range.")
    return notes


def prepare(repo, plan_path):
    source = run("git", "rev-parse", "HEAD")
    if not git_ancestor(source, "origin/main"):
        raise ValueError("The tested commit is no longer part of main; refusing release.")
    items = releases(repo)
    if any(not item["draft"] and source_marker(item) == source for item in items):
        output("skip", "true")
        print("This source commit already has a published release.")
        return
    published = stable_releases(items)
    previous = published[0] if published else None
    previous_source = tag_source(previous["tag_name"]) if previous else None
    if previous_source and git_ancestor(source, previous_source):
        output("skip", "true")
        print("A release already contains this source commit; skipping the older run.")
        return
    if previous_source and not git_ancestor(previous_source, source):
        raise ValueError("Latest release is not an ancestor of this commit; refusing a divergent release.")
    apple, updater = signing_mode(os.environ)
    public_key = os.environ.get("TAURI_SIGNING_PUBLIC_KEY", "").strip()
    validate_signing_continuity(previous, apple, updater, public_key)
    base = CHECK.check_versions(ROOT)
    draft = next((item for item in items if item["draft"] and source_marker(item) == source), None)
    version = draft["tag_name"][1:] if draft else next_version(base, previous["tag_name"][1:] if previous else None)
    version_tuple(version)
    if previous and version_tuple(version) <= version_tuple(previous["tag_name"][1:]):
        raise ValueError("Existing draft is older than the current release; remove that stale draft before retrying.")
    tag = "v" + version
    if any(item["tag_name"] == tag and item.get("id") != (draft or {}).get("id") for item in items):
        raise ValueError(f"Release {tag} already exists for another commit.")
    existing_tag = subprocess.run(["git", "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}"], cwd=ROOT, capture_output=True, text=True, check=False)
    if existing_tag.returncode == 0 and existing_tag.stdout.strip() != source:
        raise ValueError(f"Tag {tag} already points to another commit.")
    notes = release_notes(repo, source, previous["tag_name"] if previous else None, apple, updater)
    notes += f"\n<!-- catabase-apple-signed: {str(apple).lower()} -->\n"
    if updater:
        notes += f"<!-- catabase-updater-key: {hashlib.sha256(public_key.encode()).hexdigest()} -->\n"
    plan = {"repo": repo, "source": source, "tag": tag, "version": version, "previousId": previous["id"] if previous else None, "draftId": draft["id"] if draft else None, "apple": apple, "updater": updater, "publicKey": public_key if updater else None, "notes": notes}
    plan_path.write_text(json.dumps(plan), encoding="utf-8")
    config = {"bundle": {"createUpdaterArtifacts": updater}}
    if updater:
        config["plugins"] = {"updater": {"pubkey": os.environ["TAURI_SIGNING_PUBLIC_KEY"].strip(), "endpoints": [f"https://github.com/{repo}/releases/latest/download/latest.json"]}}
    plan_path.with_suffix(".config.json").write_text(json.dumps(config), encoding="utf-8")
    write_version(ROOT, version)
    for key, value in (("skip", "false"), ("tag", tag), ("version", version), ("apple", str(apple).lower()), ("updater", str(updater).lower())):
        output(key, value)
    print(f"Prepared {tag} from {source[:7]}.")


def file_hash(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def validate_updater_key(signature, public_key):
    # Tauri CLI 2.11.4 only warns when these key IDs differ. Fail the release
    # instead, before publishing an archive existing clients cannot verify.
    # Minisign envelope: algorithm (2), key ID (8), key/signature (32/64).
    # Cryptographic verification remains mandatory in the runtime updater.
    try:
        key_lines = base64.b64decode(public_key, validate=True).decode("utf-8").splitlines()
        sig_lines = base64.b64decode(signature, validate=True).decode("utf-8").splitlines()
        if (len(key_lines) != 2 or len(sig_lines) != 4
                or not key_lines[0].startswith("untrusted comment:")
                or not sig_lines[0].startswith("untrusted comment:")
                or not sig_lines[2].startswith("trusted comment:")):
            raise ValueError("Unexpected minisign envelope.")
        key = base64.b64decode(key_lines[1], validate=True)
        sig = base64.b64decode(sig_lines[1], validate=True)
        global_sig = base64.b64decode(sig_lines[3], validate=True)
        if len(key) != 42 or key[:2] != b"Ed" or len(sig) != 74 or sig[:2] not in (b"Ed", b"ED") or len(global_sig) != 64:
            raise ValueError("Unexpected minisign record.")
    except (ValueError, UnicodeError, TypeError) as error:
        raise ValueError("Updater public key or signature is malformed.") from error
    if key[2:10] != sig[2:10]:
        raise ValueError("Updater signature does not match the configured public key; restore the matching keypair.")


def expected_assets(plan):
    names = {ASSET_ZIP, ASSET_DMG, "SHA256SUMS"}
    if plan["updater"]:
        names.update({ASSET_UPDATE, ASSET_UPDATE + ".sig", "latest.json"})
    return names


def validate_assets(plan, artifact_dir):
    paths = {path.name: path for path in artifact_dir.iterdir()}
    if set(paths) != expected_assets(plan) or any(not path.is_file() or path.stat().st_size == 0 for path in paths.values()):
        raise ValueError("Release assets are missing, empty or unexpected.")
    checksums = {}
    for line in paths["SHA256SUMS"].read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.-]+)", line)
        if not match or match[2] in checksums:
            raise ValueError("Malformed release checksum manifest.")
        checksums[match[2]] = match[1]
    if set(checksums) != set(paths) - {"SHA256SUMS"} or any(file_hash(paths[name]) != digest for name, digest in checksums.items()):
        raise ValueError("Release checksum verification failed.")
    if plan["updater"]:
        manifest = json.loads(paths["latest.json"].read_text())
        signature = paths[ASSET_UPDATE + ".sig"].read_text().strip()
        validate_updater_key(signature, plan.get("publicKey"))
        url = f'https://github.com/{plan["repo"]}/releases/download/{plan["tag"]}/{ASSET_UPDATE}'
        platforms = {target: {"url": url, "signature": signature} for target in ("darwin-aarch64", "darwin-x86_64")}
        if manifest.get("version") != plan["version"] or manifest.get("platforms") != platforms:
            raise ValueError("Updater manifest does not match this release.")
    return paths


def make_assets(plan_path, artifact_dir):
    plan = json.loads(plan_path.read_text())
    bundle = ROOT / "target/universal-apple-darwin/release/bundle"
    app = bundle / "macos/Catabase.app"
    with open(app / "Contents/Info.plist", "rb") as handle:
        if plistlib.load(handle)["CFBundleShortVersionString"] != plan["version"]:
            raise ValueError("Built application version differs from release version.")
    artifact_dir.mkdir(parents=True, exist_ok=True)
    if any(artifact_dir.iterdir()):
        raise ValueError("Release artifact directory must be empty before packaging.")
    run("ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(app), str(artifact_dir / ASSET_ZIP))
    dmgs = list((bundle / "dmg").glob("*.dmg"))
    if len(dmgs) != 1:
        raise ValueError("Expected exactly one universal DMG.")
    shutil.copyfile(dmgs[0], artifact_dir / ASSET_DMG)
    if plan["updater"]:
        archive = bundle / "macos" / ASSET_UPDATE
        signature = (archive.with_name(archive.name + ".sig")).read_text().strip()
        validate_updater_key(signature, plan.get("publicKey"))
        shutil.copyfile(archive, artifact_dir / ASSET_UPDATE)
        (artifact_dir / (ASSET_UPDATE + ".sig")).write_text(signature + "\n")
        url = f'https://github.com/{plan["repo"]}/releases/download/{plan["tag"]}/{ASSET_UPDATE}'
        manifest = {"version": plan["version"], "notes": plan["notes"], "pub_date": datetime.now(timezone.utc).isoformat(), "platforms": {target: {"url": url, "signature": signature} for target in ("darwin-aarch64", "darwin-x86_64")}}
        (artifact_dir / "latest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    checksums = []
    for path in sorted(artifact_dir.iterdir()):
        if path.name == "SHA256SUMS":
            continue
        digest = file_hash(path)
        checksums.append(f"{digest}  {path.name}\n")
    (artifact_dir / "SHA256SUMS").write_text("".join(checksums))
    validate_assets(plan, artifact_dir)


def publish(plan_path, artifact_dir):
    plan = json.loads(plan_path.read_text())
    repo = plan["repo"]
    validate_assets(plan, artifact_dir)
    items = releases(repo)
    if any(not item["draft"] and source_marker(item) == plan["source"] for item in items):
        print("This source was already published; leaving all assets unchanged.")
        return
    published = stable_releases(items)
    latest_id = published[0]["id"] if published else None
    if latest_id != plan["previousId"]:
        raise ValueError("Latest release changed during the build; retry to rebase the release version.")
    draft = next((item for item in items if item["tag_name"] == plan["tag"]), None)
    if draft and (not draft["draft"] or source_marker(draft) != plan["source"]):
        raise ValueError("Refusing to modify an existing release from another source.")
    if not draft:
        draft = github(repo, "releases", "POST", {"tag_name": plan["tag"], "target_commitish": plan["source"], "name": "Catabase " + plan["tag"], "body": plan["notes"], "draft": True, "prerelease": False})
    expected = validate_assets(plan, artifact_dir)
    files = [expected[name] for name in sorted(expected)]
    # Only unpublished drafts are replaceable. Remove stale assets from a failed
    # draft build before retrying, e.g. when updater signing was reconfigured.
    for asset in draft.get("assets", []):
        if asset["name"] not in expected:
            github(repo, f'releases/assets/{asset["id"]}', "DELETE")
    run("gh", "release", "upload", plan["tag"], *map(str, files), "--repo", repo, "--clobber")
    uploaded = github(repo, f'releases/{draft["id"]}')
    if not uploaded["draft"]:
        raise ValueError("Draft was published concurrently; refusing further changes.")
    assets = {asset["name"]: asset for asset in uploaded["assets"]}
    if set(assets) != set(expected):
        raise ValueError("Uploaded assets do not match the complete local release.")
    for name, path in expected.items():
        if assets[name]["size"] != path.stat().st_size:
            raise ValueError("Uploaded asset size does not match: " + name)
        digest = assets[name].get("digest")
        if digest and digest != "sha256:" + file_hash(path):
            raise ValueError("Uploaded asset checksum does not match: " + name)
    release = github(repo, f'releases/{draft["id"]}', "PATCH", {"body": plan["notes"], "draft": False, "prerelease": False, "make_latest": "true"})
    print(release["html_url"])
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary, "a", encoding="utf-8") as handle:
            handle.write(f'Published [{plan["tag"]}]({release["html_url"]}) from `{plan["source"]}`.\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("source", "prepare", "build", "assets", "publish"))
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", "smolijar/catabase"))
    parser.add_argument("--run-id", default=os.environ.get("CI_RUN_ID"))
    parser.add_argument("--plan", type=Path, default=Path(os.environ.get("RUNNER_TEMP", "/tmp")) / "catabase-release.json")
    parser.add_argument("--artifacts", type=Path, default=ROOT / "artifacts")
    args = parser.parse_args()
    try:
        if args.command == "source":
            if not args.run_id or not args.run_id.isdigit():
                raise ValueError("A numeric successful CI run ID is required.")
            output("sha", validate_run(github(args.repo, f"actions/runs/{args.run_id}"), args.repo))
        elif args.command == "prepare":
            prepare(args.repo, args.plan)
        elif args.command == "build":
            build(args.plan)
        elif args.command == "assets":
            make_assets(args.plan, args.artifacts.resolve())
        else:
            publish(args.plan, args.artifacts.resolve())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        # Never print subprocess arguments/input: some calls handle release notes,
        # and signing configuration must not accidentally leak to workflow logs.
        message = f"{error.cmd[0]} failed (exit {error.returncode})" if isinstance(error, subprocess.CalledProcessError) else str(error)
        print(f"Release failed: {message}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
