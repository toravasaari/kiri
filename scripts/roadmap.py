#!/usr/bin/env python3
"""Lists the parts of docs/roadmap.md that are not done, with their line
numbers, so that an agent can read the roadmap's open work without reading
the whole file:

    python3 scripts/roadmap.py        (or `make roadmap-open`)

A part is a heading. It is done when its title ends with "(done)" or
"(fixed)", or with them after a comma, as in "(security, fixed)"; a part
done only in some places, as "(done for xterm and foot)", is open. A done
part is listed only above an open part under it. Its own tests:
`python3 scripts/test_roadmap.py`.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HEADING = re.compile(r"(#{1,6})\s+(.*\S)\s*")
DONE = re.compile(r"\((?:[^()]*,\s*)?(done|fixed)\)$", re.IGNORECASE)
FENCE = re.compile(r"\s*(```|~~~)")


def open_parts(text):
    """(line, level, title) of each heading that is not done, in the order of
    the file, each after the headings above it that it sits under, done or
    not, so that it is read in its place. Fenced code is skipped."""
    parts = []
    above = []  # (line, level, title, listed) of the headings it sits under
    fence = None
    for number, line in enumerate(text.splitlines(), start=1):
        opening = FENCE.match(line)
        if opening:
            if fence is None:
                fence = opening.group(1)
            elif opening.group(1) == fence:
                fence = None
            continue
        match = None if fence else HEADING.fullmatch(line)
        if not match:
            continue
        level, title = len(match.group(1)), match.group(2)
        while above and above[-1][1] >= level:
            above.pop()
        if not DONE.search(title):
            for index, (line_above, level_above, title_above, listed) in enumerate(above):
                if not listed:
                    parts.append((line_above, level_above, title_above))
                    above[index] = (line_above, level_above, title_above, True)
            parts.append((number, level, title))
            above.append((number, level, title, True))
        else:
            above.append((number, level, title, False))
    return parts


def main():
    path = ROOT / "docs" / "roadmap.md"
    for number, level, title in open_parts(path.read_text(encoding="utf-8")):
        print(f"{number:5}  {'  ' * (level - 1)}{title}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
