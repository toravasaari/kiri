#!/usr/bin/env python3
"""Tests of scripts/module-map.py. Run by hand after changing the script, and
by CI; not part of `make check`:

    python3 scripts/test_module_map.py
"""

import importlib.util
import re
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("module_map", HERE / "module-map.py")
mm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mm)


def tree(files):
    """A src/ directory in a temporary folder with the given files."""
    directory = tempfile.TemporaryDirectory()
    src = Path(directory.name) / "src"
    for name, text in files.items():
        path = src / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    return directory, src


class Sentence(unittest.TestCase):
    def cut(self, source):
        return mm.cut_sentence(Path("a.rs"), source)

    def test_first_sentence_of_first_paragraph(self):
        self.assertEqual(self.cut("//! One. Two.\n//!\n//! Three.\n"), "One.")

    def test_sentence_may_span_lines(self):
        self.assertEqual(self.cut("//! One\n//! two. Three.\n"), "One two.")

    def test_abbreviations_and_dotted_names_do_not_end_it(self):
        self.assertEqual(self.cut("//! Reads a.b and e.g. this.\n"), "Reads a.b and e.g. this.")

    def test_plain_comment_before_the_document_is_skipped(self):
        self.assertEqual(self.cut("// licence\n//! Doc.\n"), "Doc.")

    def test_no_document_names_the_file(self):
        with self.assertRaisesRegex(ValueError, "src/a.rs: no `//!` document"):
            self.cut("fn main() {}\n")

    def test_no_full_stop_names_the_file(self):
        with self.assertRaisesRegex(ValueError, "src/a.rs: .*no full stop"):
            self.cut("//! No end\n")


class TestFiles(unittest.TestCase):
    def test_by_name(self):
        for name in ["tests.rs", "x/tests.rs", "x/testing.rs", "x/tests/a.rs"]:
            self.assertTrue(mm.is_test_file(Path(name)), name)
        for name in ["x/test.rs", "x/contests.rs", "x/mod.rs", "main.rs"]:
            self.assertFalse(mm.is_test_file(Path(name)), name)

    def test_declared_for_tests(self):
        parent = "#[cfg(test)]\nmod props;\n\nmod real;\n"
        self.assertTrue(mm.declared_for_tests(parent, "props"))
        self.assertFalse(mm.declared_for_tests(parent, "real"))
        self.assertFalse(mm.declared_for_tests("#[cfg(test)]\n\nmod props;\n", "props"))

    def test_declared_test_module_has_no_row_and_is_mentioned(self):
        directory, src = tree({
            "main.rs": "//! Main.\nmod a;\n",
            "a.rs": "//! A.\n#[cfg(test)]\nmod props;\n#[cfg(test)]\nmod tests;\n",
            "a/props.rs": "//! Props.\n",
            "a/tests.rs": "//! Tests.\n",
            "a/tests/deep.rs": "//! Deep.\n",
        })
        with directory:
            page = mm.render(src)
        self.assertIn("[`a.rs`](../src/a.rs): A. Tests: `props.rs`, `tests.rs`.", page)
        self.assertNotIn("](../src/a/props.rs)", page)
        self.assertNotIn("](../src/a/tests.rs)", page)

    def test_mod_rs_is_the_heading_not_a_row(self):
        directory, src = tree({
            "main.rs": "//! Main.\nmod typeset;\n",
            "typeset/mod.rs": "//! The engine. More.\n#[cfg(test)]\nmod tests;\nmod x;\n",
            "typeset/x.rs": "//! X.\n",
            "typeset/tests.rs": "//! Tests.\n",
        })
        with directory:
            page = mm.render(src)
        self.assertIn("### [`src/typeset/`](../src/typeset/mod.rs)\n\nThe engine. Tests: `tests.rs`.", page)
        self.assertNotIn("`mod.rs`", page)
        self.assertIn("## The typesetting engine", page)
        self.assertIn("[`main.rs`](../src/main.rs): Main.", page)


class Parts(unittest.TestCase):
    def test_parts_match_the_guard(self):
        guard = (HERE.parent / "tests" / "it" / "guard_direction.rs").read_text()
        found = re.search(r"const PARTS: \[&str; \d+\] = \[([^\]]*)\]", guard)
        self.assertIsNotNone(found, "PARTS not found in tests/it/guard_direction.rs")
        listed = re.findall(r'"([^"]+)"', found.group(1))
        self.assertEqual(mm.PARTS, listed)

    def test_part_of(self):
        self.assertEqual(mm.part_of(Path("typeset/x.rs")), "typeset")
        self.assertEqual(mm.part_of(Path("main.rs")), "platform")
        self.assertEqual(mm.part_of(Path("term/x.rs")), "platform")


class Check(unittest.TestCase):
    def test_every_production_file_of_the_real_src_has_a_row(self):
        src = HERE.parent / "src"
        page = mm.render(src)
        for relative in mm.production_files(src):
            if relative.name != "mod.rs":
                self.assertIn(f"(../src/{relative})", page)


if __name__ == "__main__":
    unittest.main()
