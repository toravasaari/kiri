#!/usr/bin/env python3
"""Tests of scripts/slow-tests.py. Run by hand after changing the script:

    python3 scripts/test_slow_tests.py
"""

import importlib.util
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("slow_tests", HERE / "slow-tests.py")
st = importlib.util.module_from_spec(spec)
spec.loader.exec_module(st)

JUNIT = """<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="3" failures="0" errors="0">
  <testsuite name="kiri-the-reader" tests="3">
    <testcase name="a::fast" classname="kiri-the-reader" time="0.2"/>
    <testcase name="a::slow" classname="kiri-the-reader" time="7.5"/>
    <testcase name="a::skipped" classname="kiri-the-reader" time="9.0">
      <skipped/>
    </testcase>
  </testsuite>
</testsuites>
"""

MAKEFILE = """# The slow tests.
SLOW_TESTS = \\
\tb::one \\
\tb::two

other:
\techo hi
"""


class Durations(unittest.TestCase):
    def test_reads_each_test_that_ran(self):
        self.assertEqual(st.durations(JUNIT), {"a::fast": 0.2, "a::slow": 7.5})


class Update(unittest.TestCase):
    def test_slow_once_is_remembered_but_not_moved(self):
        history, moved = st.update({}, {"a::slow": 7.5, "a::fast": 0.2}, 5.0)
        self.assertEqual(history, {"a::slow": True, "a::fast": False})
        self.assertEqual(moved, [])

    def test_slow_in_two_runs_in_a_row_is_moved(self):
        _, moved = st.update({"a::slow": True}, {"a::slow": 6.0}, 5.0)
        self.assertEqual(moved, ["a::slow"])

    def test_a_fast_run_in_between_starts_over(self):
        history, _ = st.update({"a::slow": True}, {"a::slow": 1.0}, 5.0)
        _, moved = st.update(history, {"a::slow": 6.0}, 5.0)
        self.assertEqual(moved, [])

    def test_a_run_without_the_test_keeps_its_last_measure(self):
        history, _ = st.update({"a::slow": True}, {"a::fast": 0.1}, 5.0)
        _, moved = st.update(history, {"a::slow": 6.0}, 5.0)
        self.assertEqual(moved, ["a::slow"])


class Makefile(unittest.TestCase):
    def test_lists_the_slow_tests(self):
        self.assertEqual(st.listed(MAKEFILE), {"b::one", "b::two"})

    def test_adds_new_names_first_and_keeps_the_rest(self):
        text = st.add_to_list(MAKEFILE, ["c::new"])
        self.assertEqual(st.listed(text), {"b::one", "b::two", "c::new"})
        self.assertIn("SLOW_TESTS = \\\n\tc::new \\\n\tb::one \\\n", text)
        self.assertTrue(text.endswith("other:\n\techo hi\n"))


if __name__ == "__main__":
    unittest.main()
