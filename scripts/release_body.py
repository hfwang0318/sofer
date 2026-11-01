#!/usr/bin/env python3
"""Render the GitHub release page body for a tag.

The release page must never ship without the changes it contains, so the body
is generated instead of being typed into the workflow: the CHANGELOG section
for the tag followed by the shared install instructions.

Usage: python3 scripts/release_body.py v1.0.0 [--out release-body.md]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CHANGELOG = ROOT / "CHANGELOG.md"
INSTALL = ROOT / "docs" / "release-install.md"


def changelog_section(text: str, version: str) -> str | None:
    start = re.search(rf"^## v{re.escape(version)}\b.*$", text, re.M)
    if not start:
        return None
    rest = text[start.end():]
    end = re.search(r"^## ", rest, re.M)
    section = rest[: end.start()] if end else rest
    return f"## 本次更新（v{version}）\n\n{section.strip()}"


def main() -> int:
    args = sys.argv[1:]
    if not args or args[0].startswith("-"):
        print("usage: release_body.py <tag> [--out <path>]", file=sys.stderr)
        return 2
    version = args[0].removeprefix("v")
    out_index = args.index("--out") if "--out" in args else -1
    if out_index != -1 and len(args) <= out_index + 1:
        print("usage: release_body.py <tag> [--out <path>]", file=sys.stderr)
        return 2
    section = changelog_section(CHANGELOG.read_text(encoding="utf-8"), version)
    if section is None:
        print(
            f"CHANGELOG.md has no '## v{version}' section; add it before tagging.",
            file=sys.stderr,
        )
        return 1
    install = INSTALL.read_text(encoding="utf-8").strip()
    body = f"{section}\n\n{install}\n"
    if out_index != -1:
        Path(args[out_index + 1]).write_text(body, encoding="utf-8")
    else:
        print(body)
    return 0


if __name__ == "__main__":
    sys.exit(main())
