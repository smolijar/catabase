#!/usr/bin/env python3
"""Validate the versions used to build and publish Catabase (Python 3.11+)."""

import argparse
import json
from pathlib import Path
import re
import sys
import tomllib


class ReleaseVersionError(ValueError):
    """A release input is missing, malformed, or inconsistent."""


_NUMBER = r"(?:0|[1-9][0-9]*)"
_PRERELEASE_IDENTIFIER = r"(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
_VERSION = re.compile(
    rf"{_NUMBER}\.{_NUMBER}\.{_NUMBER}"
    rf"(?:-{_PRERELEASE_IDENTIFIER}(?:\.{_PRERELEASE_IDENTIFIER})*)?"
)


def read_manifest(root: Path, relative_path: str) -> dict:
    path = root / relative_path
    try:
        text = path.read_text(encoding="utf-8")
        document = tomllib.loads(text) if path.suffix in {".toml", ".lock"} else json.loads(text)
    except (OSError, UnicodeError, ValueError) as error:
        raise ReleaseVersionError(f"Cannot read {relative_path}: {error}") from error
    if not isinstance(document, dict):
        raise ReleaseVersionError(f"{relative_path} must contain an object/table")
    return document


def read_version(document: dict, keys: tuple[str, ...], source: str) -> str:
    value = document
    for key in keys:
        if not isinstance(value, dict) or key not in value:
            raise ReleaseVersionError(f"Missing {source}")
        value = value[key]
    if not isinstance(value, str) or _VERSION.fullmatch(value) is None:
        raise ReleaseVersionError(
            f"{source} must be valid SemVer without build metadata; got {value!r}"
        )
    return value


def check_versions(root: Path, tag: str | None = None) -> str:
    """Return the common version, or explain why these inputs cannot be released."""
    cargo = read_manifest(root, "Cargo.toml")
    version = read_version(cargo, ("workspace", "package", "version"), "Cargo.toml workspace.package.version")
    versions = {
        "ui/package.json version": read_version(
            read_manifest(root, "ui/package.json"), ("version",), "ui/package.json version"
        ),
        "src-tauri/tauri.conf.json version": read_version(
            read_manifest(root, "src-tauri/tauri.conf.json"),
            ("version",),
            "src-tauri/tauri.conf.json version",
        ),
    }

    packages = read_manifest(root, "Cargo.lock").get("package")
    if not isinstance(packages, list):
        raise ReleaseVersionError("Cargo.lock is missing its package list")
    for name in ("catabase", "catabase-core"):
        matching = [package for package in packages if isinstance(package, dict) and package.get("name") == name]
        if len(matching) != 1:
            raise ReleaseVersionError(
                f"Cargo.lock must contain exactly one {name!r} package; found {len(matching)}"
            )
        source = f"Cargo.lock {name} version"
        versions[source] = read_version(matching[0], ("version",), source)

    if tag is not None:
        if not tag.startswith("v") or _VERSION.fullmatch(tag[1:]) is None:
            raise ReleaseVersionError(
                f"Release tag must be vX.Y.Z with optional SemVer prerelease and no build metadata; got {tag!r}"
            )
        versions["release tag"] = tag[1:]

    mismatches = [f"{source} is {value!r}" for source, value in versions.items() if value != version]
    if mismatches:
        raise ReleaseVersionError(
            f"Expected {version!r} from Cargo.toml workspace.package.version; " + "; ".join(mismatches)
        )
    return version


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", help="Release tag, including its v prefix (for example v1.2.3-rc.1)")
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parent.parent, help="Repository root"
    )
    args = parser.parse_args()
    try:
        version = check_versions(args.root, args.tag)
    except ReleaseVersionError as error:
        print(f"Release version check failed: {error}", file=sys.stderr)
        return 1
    print(version)
    return 0


if __name__ == "__main__":
    sys.exit(main())
