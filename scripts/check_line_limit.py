#!/usr/bin/env python3
"""Enforce the project rule: no source file may exceed 100 lines of code.

Counts physical lines but skips blank lines and lines that contain only a
comment in the file's own comment syntax. Exits non-zero and lists every
offending file so CI fails loudly instead of silently growing.

Usage:
    python scripts/check_line_limit.py [--limit 100] [--root .]
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

# Suffixes that carry program logic or application configuration.
CODE_SUFFIXES = {
    ".rs",
    ".sql",
    ".yml",
    ".yaml",
    ".toml",
    ".py",
    ".sh",
    ".ps1",
    ".js",
    ".ts",
}

# Generated or vendored trees that are not ours to police.
SKIP_DIRS = {
    "target",
    ".git",
    "node_modules",
    "fixtures",
    ".sqlx",
}

LINE_COMMENT_PREFIXES = ("#", "//", "--")


def is_comment_only(line: str, suffix: str) -> bool:
    """True when a line carries no code, only a comment or a blank."""
    stripped = line.strip()
    if not stripped:
        return True
    if suffix in {".yml", ".yaml"}:
        return stripped.startswith("#")
    if suffix in {".py", ".sh", ".ps1", ".toml", ".yml", ".yaml"}:
        return stripped.startswith("#")
    if suffix == ".rs":
        return stripped.startswith("//")
    if suffix == ".sql":
        return stripped.startswith("--")
    if suffix in {".js", ".ts"}:
        return stripped.startswith("//") or stripped.startswith("*")
    return False


def code_lines(path: Path) -> int:
    try:
        raw = path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, OSError):
        return 0
    return sum(
        0 if is_comment_only(line, path.suffix.lower()) else 1 for line in raw.splitlines()
    )


def collect(root: Path) -> list[Path]:
    found: list[Path] = []
    for path in root.rglob("*"):
        if not path.is_file() or path.suffix.lower() not in CODE_SUFFIXES:
            continue
        if any(part in SKIP_DIRS for part in path.parts):
            continue
        found.append(path)
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--limit", type=int, default=100)
    parser.add_argument("--root", default=".")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    offenders: list[tuple[Path, int]] = []
    total_files = 0

    for path in sorted(collect(root)):
        total_files += 1
        count = code_lines(path)
        if count > args.limit:
            offenders.append((path.relative_to(root), count))

    if offenders:
        print(f"FAIL: {len(offenders)} file(s) exceed {args.limit} lines of code\n")
        for path, count in offenders:
            print(f"  {count:>5}  {path}")
        print(f"\nSplit each file along logical responsibility boundaries.")
        return 1

    print(f"OK: {total_files} source files, all within {args.limit} lines of code.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
