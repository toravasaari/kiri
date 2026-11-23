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
        path.write_text(text, encoding="utf-8")
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

    def test_dots_inside_a_code_span_do_not_end_it(self):
        self.assertEqual(
            self.cut("//! Gives `a ... b` end. More.\n"), "Gives `a ... b` end."
        )

    def test_a_span_closes_with_a_run_of_its_own_length(self):
        self.assertEqual(
            self.cut("//! Gives ` ```x ` end. More.\n"), "Gives ` ```x ` end."
        )
        self.assertEqual(
            self.cut("//! Gives ``a ` b. c`` end. More.\n"), "Gives ``a ` b. c`` end."
        )

    def test_an_intra_doc_link_names_the_file(self):
        with self.assertRaisesRegex(ValueError, "src/a.rs: .*intra-doc link"):
            self.cut("//! What it answers: [`Laid`].\n")

    def test_brackets_in_a_code_span_or_with_a_target_are_fine(self):
        self.assertEqual(self.cut("//! The `[contents]` table.\n"), "The `[contents]` table.")
        self.assertEqual(self.cut("//! See [`a`](b.md).\n"), "See [`a`](b.md).")

    def test_an_unclosed_code_span_names_the_file(self):
        with self.assertRaisesRegex(ValueError, "src/a.rs: a code span .* not closed"):
            self.cut("//! Gives `a. More.\n")

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
        guard_file = HERE.parent / "tests" / "it" / "guard_direction.rs"
        guard = guard_file.read_text(encoding="utf-8")
        found = re.search(r"const PARTS: \[&str; \d+\] = \[([^\]]*)\]", guard)
        self.assertIsNotNone(found, "PARTS not found in tests/it/guard_direction.rs")
        listed = re.findall(r'"([^"]+)"', found.group(1))
        self.assertEqual(mm.PARTS, listed)

    def test_part_of(self):
        self.assertEqual(mm.part_of(Path("typeset/x.rs")), "typeset")
        self.assertEqual(mm.part_of(Path("main.rs")), "platform")
        self.assertEqual(mm.part_of(Path("term/x.rs")), "platform")


class Check(unittest.TestCase):
    """The page of the real src/, against a fixed list that does not come from
    the generator's own classification."""

    # Test code only: by `#[cfg(test)]` in the parent, or by name.
    NO_ROW = [
        "term/pty.rs",
        "present/keys/props.rs",
        "typeset/parity.rs",
        "frontend/sim.rs",
        "session/sidebar/tests_coloured.rs",
    ]
    ROW = [
        "main.rs",
        "typeset/driver.rs",
        "session/sources.rs",
        "term/probe.rs",
        "present/presenter.rs",
    ]

    def test_known_files_have_a_row_or_none(self):
        page = mm.render(HERE.parent / "src")
        for name in self.NO_ROW:
            self.assertNotIn(f"(../src/{name})", page, name)
        for name in self.ROW:
            self.assertIn(f"(../src/{name})", page, name)


class Main(unittest.TestCase):
    def root(self, page):
        directory, src = tree({"main.rs": "//! Main.\n"})
        root = src.parent
        (root / "docs").mkdir()
        if page is not None:
            (root / "docs" / "module-map.md").write_text(page, encoding="utf-8")
        return directory, root

    def run_main(self, argv, root):
        import contextlib
        import io

        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code = mm.main(argv, root)
        return code, err.getvalue()

    def test_check_of_a_stale_page_fails_with_the_command(self):
        directory, root = self.root("old\n")
        with directory:
            code, message = self.run_main(["--check"], root)
        self.assertEqual(code, 1)
        self.assertIn("run `make module-map`", message)

    def test_write_then_check_passes(self):
        directory, root = self.root("old\n")
        with directory:
            self.assertEqual(self.run_main([], root)[0], 0)
            page = (root / "docs" / "module-map.md").read_text(encoding="utf-8")
            self.assertIn("[`main.rs`](../src/main.rs): Main.", page)
            self.assertEqual(self.run_main(["--check"], root), (0, ""))

    def test_unknown_argument_is_rejected_and_nothing_is_written(self):
        directory, root = self.root("old\n")
        with directory:
            code, message = self.run_main(["--chek"], root)
            page = (root / "docs" / "module-map.md").read_text(encoding="utf-8")
        self.assertEqual(code, 2)
        self.assertIn("unknown argument '--chek'", message)
        self.assertEqual(page, "old\n")

    def test_a_first_sentence_over_the_limit_names_file_and_sentence(self):
        long = "Word " * 40 + "end."
        directory, root = self.root(None)
        with directory:
            (root / "src" / "long.rs").write_text(f"//! {long}\n", encoding="utf-8")
            self.run_main([], root)  # writing is allowed
            code, message = self.run_main(["--check"], root)
        self.assertEqual(code, 1)
        self.assertIn("src/long.rs", message)
        self.assertIn(f"{len(long)} characters, over the limit of {mm.LIMIT}", message)
        self.assertIn(long, message)

    def test_a_sentence_of_one_over_the_limit_fails(self):
        sentence = "x" * mm.LIMIT + "."
        self.assertEqual(len(sentence), mm.LIMIT + 1)
        directory, root = self.root(None)
        with directory:
            (root / "src" / "one.rs").write_text(f"//! {sentence}\n", encoding="utf-8")
            code, message = self.run_main(["--check"], root)
        self.assertEqual(code, 1)
        self.assertIn("src/one.rs", message)

    def test_a_mod_rs_over_the_limit_fails_and_is_named(self):
        sentence = "x" * mm.LIMIT + "."
        directory, root = self.root(None)
        with directory:
            (root / "src" / "typeset").mkdir()
            (root / "src" / "typeset" / "mod.rs").write_text(
                f"//! {sentence}\n", encoding="utf-8"
            )
            code, message = self.run_main(["--check"], root)
        self.assertEqual(code, 1)
        self.assertIn("src/typeset/mod.rs", message)

    def test_write_mode_overwrites_a_page_that_is_not_utf8(self):
        directory, root = self.root(None)
        with directory:
            (root / "docs" / "module-map.md").write_bytes(b"bad \xff\n")
            code, message = self.run_main([], root)
            self.assertEqual((code, message), (0, ""))
            self.assertEqual(self.run_main(["--check"], root), (0, ""))

    def test_a_sentence_of_exactly_the_limit_passes(self):
        sentence = "x" * (mm.LIMIT - 1) + "."
        directory, root = self.root(None)
        with directory:
            (root / "src" / "ok.rs").write_text(f"//! {sentence}\n", encoding="utf-8")
            self.run_main([], root)
            self.assertEqual(self.run_main(["--check"], root), (0, ""))

    def test_a_file_that_is_not_utf8_is_named(self):
        directory, root = self.root(None)
        with directory:
            (root / "src" / "bad.rs").write_bytes(b"//! Bad \xff.\n")
            code, message = self.run_main(["--check"], root)
        self.assertEqual(code, 1)
        self.assertIn("bad.rs: not UTF-8", message)


if __name__ == "__main__":
    unittest.main()
