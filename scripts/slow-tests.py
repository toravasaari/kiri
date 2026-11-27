#!/usr/bin/env python3
"""Runs cargo-nextest, and puts a test that is slow in two runs in a row on
the SLOW_TESTS list in the Makefile.

    python3 scripts/slow-tests.py [ARGUMENTS OF cargo nextest run...]

The make targets that run tests call it instead of `cargo nextest run`. It
exits with the status of the run. Afterwards it reads the run's JUnit report,
which .config/nextest.toml has nextest write, and remembers in
target/slow-tests.json which tests took longer than the limit: 5 s, or 10 s
in a container, which has fewer CPUs for the same tests; SLOW_TEST_LIMIT
sets another number of seconds. A test over the limit in this run and in the
last run that measured it goes on the list, and the script says so: commit
the Makefile. Its own tests: `python3 scripts/test_slow_tests.py`.
"""

import json
import os
import subprocess
import sys
import xml.etree.ElementTree as ElementTree
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LIST_START = "SLOW_TESTS = \\\n"


def durations(junit):
    """Seconds per test name for the tests that ran in a JUnit report."""
    times = {}
    for case in ElementTree.fromstring(junit).iter("testcase"):
        if case.find("skipped") is None:
            times[case.get("name")] = float(case.get("time", "0"))
    return times


def update(history, times, limit):
    """The history after a run, and the tests over the limit in this run and
    in the one before that measured them, sorted. History maps a test name
    to whether its last measured run was over the limit."""
    moved = sorted(name for name, time in times.items() if time > limit and history.get(name))
    after = dict(history)
    after.update({name: time > limit for name, time in times.items()})
    return after, moved


def listed(makefile):
    """The test names on the SLOW_TESTS list of a Makefile's text."""
    start = makefile.index(LIST_START) + len(LIST_START)
    names = set()
    for line in makefile[start:].splitlines():
        name = line.strip().rstrip("\\").strip()
        if name:
            names.add(name)
        if not line.rstrip().endswith("\\"):
            break
    return names


def add_to_list(makefile, names):
    """The Makefile's text with names put first on its SLOW_TESTS list."""
    at = makefile.index(LIST_START) + len(LIST_START)
    lines = "".join(f"\t{name} \\\n" for name in names)
    return makefile[:at] + lines + makefile[at:]


def limit():
    if "SLOW_TEST_LIMIT" in os.environ:
        return float(os.environ["SLOW_TEST_LIMIT"])
    try:
        detect = ["systemd-detect-virt", "--container", "--quiet"]
        in_container = subprocess.run(detect).returncode == 0
    except FileNotFoundError:
        in_container = False
    return 10.0 if in_container else 5.0


def main():
    status = subprocess.run(["cargo", "nextest", "run", *sys.argv[1:]]).returncode
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    junit = target / "nextest" / os.environ.get("NEXTEST_PROFILE", "default") / "junit.xml"
    if not junit.exists():
        return status
    times = durations(junit.read_text(encoding="utf-8"))
    # A later run that stops before its report must not read this one again.
    junit.unlink()
    history_file = target / "slow-tests.json"
    history = json.loads(history_file.read_text()) if history_file.exists() else {}
    seconds = limit()
    history, slow = update(history, times, seconds)
    history_file.write_text(json.dumps(history, indent=0, sort_keys=True))
    makefile_path = ROOT / "Makefile"
    makefile = makefile_path.read_text(encoding="utf-8")
    new = [name for name in slow if name not in listed(makefile)]
    if new:
        makefile_path.write_text(add_to_list(makefile, new), encoding="utf-8")
        print(
            f"slow-tests: over {seconds:g} s in two runs in a row, now on SLOW_TESTS "
            "in the Makefile (commit it):",
            *new,
            sep="\n  ",
            file=sys.stderr,
        )
    return status


if __name__ == "__main__":
    sys.exit(main())
