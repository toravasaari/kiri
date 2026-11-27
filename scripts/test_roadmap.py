#!/usr/bin/env python3
"""Tests of scripts/roadmap.py. Run by hand after changing the script:

    python3 scripts/test_roadmap.py
"""

import importlib.util
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("roadmap", HERE / "roadmap.py")
rm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rm)

ROADMAP = """# Roadmap

Intro.

## Phase 1 – Foundations (done)

### 1.1 Something

## Phase 4 – Rich content

### 4.1 Images (done)

### 4.2 Formulas

#### 4.2.1 Formula blocks (done)

#### 4.2.2 Inline formulas

```markdown
# Not a heading
```

#### G7.4 Scrolling by the terminal (done for Ghostty, foot and xterm)

## Bugs

#### B2 Tags show as source (fixed)

#### B5 Tags around a formula block
"""


class OpenParts(unittest.TestCase):
    def titles(self):
        return [title for _, _, title in rm.open_parts(ROADMAP)]

    def test_lists_the_open_parts_under_their_open_parents(self):
        self.assertEqual(
            self.titles(),
            [
                "Roadmap",
                "Phase 4 – Rich content",
                "4.2 Formulas",
                "4.2.2 Inline formulas",
                "G7.4 Scrolling by the terminal (done for Ghostty, foot and xterm)",
                "Bugs",
                "B5 Tags around a formula block",
            ],
        )

    def test_gives_each_part_its_line_and_level(self):
        parts = rm.open_parts(ROADMAP)
        self.assertEqual(parts[1], (9, 2, "Phase 4 – Rich content"))
        self.assertEqual(parts[3], (17, 4, "4.2.2 Inline formulas"))


if __name__ == "__main__":
    unittest.main()
