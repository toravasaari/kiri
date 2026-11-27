#!/usr/bin/env python3
"""Lists the parts of docs/roadmap.md that are not done, with their line
numbers, so that an agent can read the roadmap's open work without reading
the whole file:

    python3 scripts/roadmap.py        (or `make roadmap-open`)

A part is a heading. It is done when its title ends with "(done)" or
"(fixed)", and so is every part under it. Its own tests:
`python3 scripts/test_roadmap.py`.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HEADING = re.compile(r"(#{1,6})\s+(.*\S)")
DONE = re.compile(r"\((done|fixed)\)$")


def open_parts(text):
    """(line, level, title) of each heading that is not done and is under no
    done heading, in the order of the file. Fenced code is skipped."""
    parts = []
    done_level = None
    fenced = False
    for number, line in enumerate(text.splitlines(), start=1):
        if line.startswith("```"):
            fenced = not fenced
            continue
        match = None if fenced else HEADING.fullmatch(line)
        if not match:
            continue
        level, title = len(match.group(1)), match.group(2)
        if done_level is not None and level > done_level:
            continue
        done_level = level if DONE.search(title) else None
        if done_level is None:
            parts.append((number, level, title))
    return parts


def main():
    path = ROOT / "docs" / "roadmap.md"
    for number, level, title in open_parts(path.read_text(encoding="utf-8")):
        print(f"{number:5}  {'  ' * (level - 1)}{title}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
