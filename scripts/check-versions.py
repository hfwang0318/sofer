#!/usr/bin/env python3
"""Fail when the desktop version drifts across the files that ship it.

The version is written by hand in three places; a release that updates only
one of them produces an installer whose About page disagrees with the updater
feed. Run this from CI and before tagging.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def read_package_json() -> str:
    data = json.loads((ROOT / "app" / "package.json").read_text(encoding="utf-8"))
    return str(data["version"])


def read_cargo_toml() -> str:
    text = (ROOT / "app" / "src-tauri" / "Cargo.toml").read_text(encoding="utf-8")
    package = re.search(r"^\[package\](.*?)(?=^\[|\Z)", text, re.M | re.S)
    if not package:
        raise SystemExit("app/src-tauri/Cargo.toml has no [package] section")
    version = re.search(r'^version\s*=\s*"([^"]+)"', package.group(1), re.M)
    if not version:
        raise SystemExit("app/src-tauri/Cargo.toml has no package version")
    return version.group(1)


def read_tauri_conf() -> str:
    data = json.loads(
        (ROOT / "app" / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8")
    )
    return str(data["version"])


def main() -> int:
    versions = {
        "app/package.json": read_package_json(),
        "app/src-tauri/Cargo.toml": read_cargo_toml(),
        "app/src-tauri/tauri.conf.json": read_tauri_conf(),
    }
    unique = set(versions.values())
    if len(unique) == 1:
        print(f"version {unique.pop()} is consistent")
        return 0
    print("version mismatch:", file=sys.stderr)
    for path, version in versions.items():
        print(f"  {path}: {version}", file=sys.stderr)
    print(
        "Update all three files (and CHANGELOG.md) in the same change.",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
