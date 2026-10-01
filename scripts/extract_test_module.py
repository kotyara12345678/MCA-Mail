#!/usr/bin/env python3
"""Move an inline `#[cfg(test)] mod tests { .. }` into a sibling file.

Splits production code from its tests, which is a genuine separation of
concerns and is what keeps most domain modules under the 100-line limit.
Replaces the inline block with:

    #[cfg(test)]
    #[path = "<stem>_test.rs"]
    mod tests;

Usage: python scripts/extract_test_module.py <file.rs> [...]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

MARKER = "#[cfg(test)]\nmod tests {"


def extract(path: Path) -> bool:
    text = path.read_text(encoding="utf-8")
    if MARKER not in text:
        return False

    head, _, rest = text.partition(MARKER)
    # The block ends with the final `}` at column 0.
    end = rest.rfind("\n}\n")
    if end == -1:
        return False
    body = rest[:end]
    # Drop the `mod tests {` line and one level of indentation.
    lines = body.splitlines()[1:]
    dedented = [line[4:] if line.startswith("    ") else line for line in lines]
    trailing = rest[end + 3 :]

    test_file = path.with_name(f"{path.stem}_test.rs")
    header = f"//! Tests for `{path.name}`.\n\n#![cfg(test)]\n\nuse super::*;\n"
    test_file.write_text(header + "\n".join(dedented).rstrip() + "\n", encoding="utf-8")

    decl = (
        "#[cfg(test)]\n"
        f'#[path = "{test_file.name}"]\n'
        "mod tests;\n"
    )
    path.write_text(head + decl + trailing, encoding="utf-8")
    return True


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    changed = [p for p in map(Path, sys.argv[1:]) if extract(p)]
    for p in changed:
        print(f"extracted: {p}")
    if not changed:
        print("nothing to do")
    return 0


if __name__ == "__main__":
    sys.exit(main())
