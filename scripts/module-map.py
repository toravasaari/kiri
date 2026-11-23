#!/usr/bin/env python3
"""Makes docs/module-map.md from the `//!` documents of src/, or checks it.

    python3 scripts/module-map.py           write docs/module-map.md
    python3 scripts/module-map.py --check   fail if the page is out of date
    python3 scripts/test_module_map.py      this script's own tests

A file of src/ that is test code (`tests.rs`, `testing.rs`, a `tests/`
directory, or a module declared under `#[cfg(test)]`) gets no row. It is named
as "Tests:" on the row of the file that declares it. The `//!` of a directory's
`mod.rs` is the text under the directory's heading, not a row.
"""

import re
import sys
from pathlib import Path

# The parts, in the order of docs/architecture.md, "The four parts". The three
# first are directories of src/; the platform is everything else.
# Also in tests/it/guard_direction.rs (PARTS), which the tests compare.
PARTS = ["typeset", "present", "session", "platform"]
LIMIT = 160  # the longest first sentence, in characters, that `--check` accepts
# A rustdoc link such as [`Laid`]: a backticked name in brackets, no `(` after.
INTRA_DOC_LINK = re.compile(r"\[`[^`]+`\](?!\()")
TITLES = {
    "typeset": "The typesetting engine",
    "present": "The terminal layer",
    "session": "The state engine",
    "platform": "The platform",
}


def is_test_file(relative: Path) -> bool:
    """A test file by its name: the same rule as rust_source::is_test_file."""
    in_tests_dir = "tests" in relative.parent.parts
    return in_tests_dir or relative.name in ("tests.rs", "testing.rs")


def declared_for_tests(parent: str, name: str) -> bool:
    """`#[cfg(test)]` and `mod name;` on two lines in a row, as in
    rust_source::declared_for_tests."""
    lines = [line.strip() for line in parent.splitlines()]
    for first, second in zip(lines, lines[1:]):
        if first == "#[cfg(test)]" and second.endswith(";"):
            if second[:-1].rpartition("mod ")[2] == name:
                return True
    return False


def parent_source(src: Path, relative: Path):
    """The source of the file that declares the module in `relative`."""
    directory = relative.parent
    if relative.name == "mod.rs":
        directory = directory.parent
    if directory == Path("."):
        candidates = [src / "lib.rs", src / "main.rs"]
    else:
        candidates = [src / directory.with_suffix(".rs"), src / directory / "mod.rs"]
    for path in candidates:
        if path.is_file():
            return read(path)
    return None


def module_name(relative: Path) -> str:
    return relative.parent.name if relative.name == "mod.rs" else relative.stem


def production_files(src: Path):
    for path in sorted(src.rglob("*.rs")):
        relative = path.relative_to(src)
        parent = parent_source(src, relative)
        for_tests = parent is not None and declared_for_tests(
            parent, module_name(relative)
        )
        if not is_test_file(relative) and not for_tests:
            yield relative


def part_of(relative: Path) -> str:
    if len(relative.parts) > 1 and relative.parts[0] in PARTS[:3]:
        return relative.parts[0]
    return "platform"


def owner_of(src: Path, relative: Path):
    """The production file that a test file belongs to: the first file up its
    chain of declaring files that is not itself test code. None when no file
    declares it."""
    seen = set()
    while relative not in seen:
        seen.add(relative)
        declaring = declaring_file(src, relative)
        if declaring is None:
            return None
        if not is_test_file(declaring) and not is_declared_test(src, declaring):
            return declaring
        relative = declaring
    return None


def declaring_file(src: Path, relative: Path):
    directory = relative.parent
    if relative.name == "mod.rs":
        directory = directory.parent
    if directory == Path("."):
        candidates = [Path("lib.rs"), Path("main.rs")]
    else:
        candidates = [directory.with_suffix(".rs"), directory / "mod.rs"]
    for path in candidates:
        if (src / path).is_file():
            return path
    return None


def is_declared_test(src: Path, relative: Path) -> bool:
    parent = parent_source(src, relative)
    return parent is not None and declared_for_tests(parent, module_name(relative))


def test_mentions(src: Path):
    """For each production file, the names of its test files and directories,
    relative to the directory of its module, sorted and without repeats."""
    mentions = {}
    production = set(production_files(src))
    for path in sorted(src.rglob("*.rs")):
        relative = path.relative_to(src)
        if relative in production:
            continue
        owner = owner_of(src, relative)
        if owner is None:
            continue
        base = owner.parent if owner.name == "mod.rs" else owner.with_suffix("")
        try:
            under = relative.relative_to(base)
        except ValueError:
            under = Path(relative.name)
        name = under.parts[0] + ("/" if len(under.parts) > 1 else "")
        names = mentions.setdefault(owner, [])
        if name not in names:
            names.append(name)
    for names in mentions.values():
        if "tests.rs" in names and "tests/" in names:
            names.remove("tests/")  # the directory holds what tests.rs declares
    return mentions


def read(path: Path) -> str:
    """The text of a file, as UTF-8 whatever the locale is. Raises ValueError,
    naming the file, when it is not UTF-8."""
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError as error:
        raise ValueError(f"{path}: not UTF-8 ({error.reason} at byte {error.start})")


def end_of_code_span(text: str, start: int):
    """The index just after the code span that opens at `start`, where `text`
    has a run of backticks. A span closes with a run of the same length, as in
    CommonMark, so a run of three inside a span of one does not close it.
    None when the span is not closed."""
    n = 1
    while start + n < len(text) and text[start + n] == "`":
        n += 1
    i = start + n
    while i < len(text):
        if text[i] != "`":
            i += 1
            continue
        run = 1
        while i + run < len(text) and text[i + run] == "`":
            run += 1
        if run == n:
            return i + run
        i += run
    return None


def first_sentence(path: Path) -> str:
    """The first sentence of the first paragraph of the file's `//!` document.
    Raises ValueError, naming the file, when there is none."""
    return cut_sentence(path, read(path))


def cut_sentence(path, source: str) -> str:
    paragraph = []
    for line in source.splitlines():
        if line.startswith("//!"):
            text = line[3:].strip()
            if text == "":
                if paragraph:
                    break
            else:
                paragraph.append(text)
        elif paragraph or (line.strip() and not line.startswith("//")):
            break
    if not paragraph:
        raise ValueError(
            f"src/{path}: no `//!` document at the top of the file; add one"
        )
    text = " ".join(paragraph)
    i = 0
    while i < len(text):
        c = text[i]
        if c == "`":
            after = end_of_code_span(text, i)
            if after is None:
                raise ValueError(
                    f"src/{path}: a code span in the first sentence of the `//!` "
                    "document is not closed; close it before the full stop"
                )
            i = after
            continue
        if c == "." and (i + 1 == len(text) or text[i + 1] == " "):
            if not text[: i + 1].endswith(("e.g.", "i.e.", "etc.")):
                sentence = text[: i + 1]
                if INTRA_DOC_LINK.search(sentence):
                    raise ValueError(
                        f"src/{path}: the first sentence of the `//!` document has "
                        "a rustdoc intra-doc link, which is no link on this page; "
                        "write the name in backticks without the brackets"
                    )
                return sentence
        i += 1
    raise ValueError(
        f"src/{path}: the first paragraph of the `//!` document has no full stop; "
        "end the first sentence with one"
    )


def sentence_of(src: Path, relative: Path) -> str:
    return cut_sentence(relative, read(src / relative))


def mention(names) -> str:
    if not names:
        return ""
    return " Tests: " + ", ".join(f"`{name}`" for name in sorted(names)) + "."


def render(src: Path) -> str:
    groups = {part: {} for part in PARTS}
    for relative in production_files(src):
        groups[part_of(relative)].setdefault(relative.parent, []).append(relative)
    tests = test_mentions(src)
    out = [
        "# Module map\n",
        "\n",
        "This page is made from the `//!` documents of `src/` by "
        "`scripts/module-map.py`. Do not edit it: change the document of the "
        "file and run `make module-map`. A file that is only test code has no "
        "row; it is named as \"Tests:\" on the row of the file that declares it.\n",
    ]
    for part in PARTS:
        out.append(f"\n## {TITLES[part]}\n")
        for directory, files in sorted(groups[part].items()):
            rows = []
            heading = None
            for relative in files:
                if relative.name == "mod.rs":
                    heading = relative
                    continue
                sentence = sentence_of(src, relative)
                text = f"{sentence}{mention(tests.get(relative))}"
                rows.append(
                    f"| [`{relative.name}`](../src/{relative}) | "
                    f"{text.replace('|', chr(92) + '|')} |\n"
                )
            if directory != Path("."):
                if heading is not None:
                    out.append(
                        f"\n### [`src/{directory}/`](../src/{heading})\n\n"
                        f"{sentence_of(src, heading)}{mention(tests.get(heading))}\n"
                    )
                else:
                    out.append(f"\n### `src/{directory}/`\n")
            out.append("\n")
            if rows:
                out.append("| File | What it does |\n|---|---|\n")
                out.extend(rows)
    return "".join(out)


def too_long(src: Path):
    """The (file, sentence) pairs whose first sentence is over LIMIT characters.
    A directory's `mod.rs` counts too: its sentence is on the page as well."""
    for relative in production_files(src):
        sentence = sentence_of(src, relative)
        if len(sentence) > LIMIT:
            yield relative, sentence


def first_difference(old: str, new: str) -> str:
    import difflib

    lines = list(
        difflib.unified_diff(
            old.splitlines(), new.splitlines(), "docs/module-map.md (now)",
            "docs/module-map.md (from the `//!` documents)", lineterm="", n=0,
        )
    )
    return "\n".join(lines[:12])


def main(argv, root=None) -> int:
    root = root or Path(__file__).resolve().parent.parent
    page = root / "docs" / "module-map.md"
    unknown = [arg for arg in argv if arg != "--check"]
    if unknown:
        print(
            f"module-map: unknown argument {unknown[0]!r}; "
            "use no argument to write the page or --check to check it",
            file=sys.stderr,
        )
        return 2
    try:
        made = render(root / "src")
        old = read(page) if "--check" in argv and page.is_file() else ""
        long = list(too_long(root / "src")) if "--check" in argv else []
    except ValueError as error:
        print(f"module-map: {error}", file=sys.stderr)
        return 1
    if long:
        for relative, sentence in long:
            print(
                f"module-map: src/{relative}: the first sentence of the `//!` "
                f"document is {len(sentence)} characters, over the limit of "
                f"{LIMIT}; shorten it, and say the rest in the next sentence:\n"
                f"  {sentence}",
                file=sys.stderr,
            )
        return 1
    if "--check" in argv:
        if old != made:
            print(
                "module-map: docs/module-map.md does not match the `//!` documents "
                "of src/; run `make module-map` and commit the result.\n"
                + first_difference(old, made),
                file=sys.stderr,
            )
            return 1
        return 0
    page.parent.mkdir(parents=True, exist_ok=True)
    page.write_text(made, encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
