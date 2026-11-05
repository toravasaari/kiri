//! The line caps: the hub files may not grow past the number of non-test
//! lines they have now, so that growth is seen in review.
//!
//! A file's count is its lines except its test code: a test module that is
//! only declared (`#[cfg(test)] mod tests;`) wherever it is, and an inline
//! `#[cfg(test)] mod tests { ... }` wherever it is. A cap is a ceiling. When
//! a file shrinks, its cap may be lowered to the new count; the test does not
//! ask.

use std::fs;
use std::path::Path;

/// The hub files, with their caps in non-test lines. A hub file is one that
/// collects code of several parts, so new code goes to the part's own file
/// instead (docs/architecture.md, "Where a new thing goes").
const CAPS: &[(&str, usize)] = &[
    // `main` and `Real`, the process's `Platform`, are all that is left of
    // the binary; the frontends are in `src/frontend/`.
    ("src/main.rs", 207),
    // The interface of the frontends, so that it does not become the next
    // hub: a new frontend goes in a file of its own.
    ("src/frontend/mod.rs", 137),
    ("src/pager/mod.rs", 832),
    ("src/render/mod.rs", 339),
    ("src/typeset/layout/mod.rs", 1359),
    ("src/typeset/reading/engine.rs", 951),
    ("src/typeset/reading/page.rs", 1398),
    ("src/images/mod.rs", 45),
    ("src/diagrams/mod.rs", 35),
];

#[test]
fn hub_files_stay_within_their_caps() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut over = Vec::new();
    for &(path, cap) in CAPS {
        let source = fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        let lines = non_test_lines(&source);
        if lines > cap {
            over.push(format!("  {path}: {lines} non-test lines, cap {cap}"));
        }
    }
    assert!(
        over.is_empty(),
        "hub files grew past their caps:\n{}\n\
         A full file is a reason to ask the maintainer where the new code goes, \
         not to split the file mechanically: the new code goes where the map in \
         docs/architecture.md puts it. Raising a cap is the maintainer's decision.\n\
         A cap may be lowered when its file shrinks.",
        over.join("\n")
    );
}

/// The lines of `source` that are not test code. Test code is an item marked
/// `#[cfg(test)]` that is only declared here, its line ending in `;`, with
/// one blank line after it, wherever it is; and a top-level inline test
/// module, `#[cfg(test)]` at column 0 and then `mod name {`, up to the `}`
/// at column 0 that closes it (rustfmt indents what is inside). Code after
/// such a module is counted. Other test-only items are counted.
fn non_test_lines(source: &str) -> usize {
    let lines: Vec<&str> = source.lines().collect();
    let (mut counted, mut at) = (0, 0);
    while at < lines.len() {
        if lines[at].trim() != "#[cfg(test)]" {
            counted += 1;
            at += 1;
            continue;
        }
        // Further attributes belong to the same item.
        let mut item = at + 1;
        while lines
            .get(item)
            .is_some_and(|line| line.trim_start().starts_with("#["))
        {
            item += 1;
        }
        let Some(line) = lines.get(item) else { break };
        if line.trim_end().ends_with(';') {
            // Declared for tests alone: skip it and one blank line after it.
            at = item + 1;
            if lines.get(at).is_some_and(|line| line.trim().is_empty()) {
                at += 1;
            }
        } else if !lines[at].starts_with(char::is_whitespace) && opens_module(line) {
            // An inline test module: skip it up to its closing brace at
            // column 0, so code after it is counted.
            at = (item + 1..lines.len())
                .find(|&end| lines[end].trim_end() == "}")
                .map_or(lines.len(), |end| end + 1);
        } else {
            counted += 1;
            at += 1;
        }
    }
    counted
}

/// Whether `line` is `mod name {`, with any visibility.
fn opens_module(line: &str) -> bool {
    let line = line.trim();
    let rest = match line.strip_prefix("pub") {
        Some(rest) => match rest.strip_prefix('(').and_then(|rest| rest.split_once(')')) {
            Some((_, rest)) => rest,
            None => rest,
        },
        None => line,
    };
    rest.trim_start().starts_with("mod ") && rest.contains('{')
}

#[test]
fn a_file_is_counted_above_its_test_module() {
    let with_tests = "fn a() {}\n\n#[cfg(test)]\nmod tests {\n    fn b() {}\n}\n";
    assert_eq!(non_test_lines(with_tests), 2);
    assert_eq!(non_test_lines("fn a() {}\n"), 1);
    let restricted = "fn a() {}\n\n#[cfg(test)]\npub(super) mod tests {\n    fn b() {}\n}\n";
    assert_eq!(non_test_lines(restricted), 2);
    let declared = "fn a() {}\n\n#[cfg(test)]\nmod summary;\n\n#[cfg(test)]\nmod tests {}\n";
    assert_eq!(non_test_lines(declared), 2);
}

#[test]
fn a_declared_test_module_is_not_counted_wherever_it_is() {
    // The bug: a declaration first used to end the count at once.
    assert_eq!(
        non_test_lines("#[cfg(test)]\nmod tests;\n\nfn a() {}\nfn b() {}\n"),
        2
    );
    assert_eq!(non_test_lines("fn a() {}\n\n#[cfg(test)]\nmod tests;\n"), 2);
    // Further attributes belong to the same item.
    assert_eq!(
        non_test_lines("#[cfg(test)]\n#[macro_use]\nmod tests;\n\nfn a() {}\n"),
        1
    );
    // Any name, and in the middle of the file.
    assert_eq!(
        non_test_lines("fn a() {}\n#[cfg(test)]\nmod tests_util;\nfn b() {}\n"),
        2
    );
    // A test-only `use` ends in `;` too.
    assert_eq!(
        non_test_lines("use a;\n#[cfg(test)]\npub(crate) use b::C;\nfn a() {}\n"),
        2
    );
    // Code after a declaration at the top is counted.
    assert_eq!(non_test_lines("#[cfg(test)]\nmod tests;\nfn a() {\n}\n"), 2);
}

#[test]
fn an_inline_test_module_of_any_name_is_not_counted() {
    assert_eq!(
        non_test_lines("fn a() {}\n#[cfg(test)]\nmod test {\n    fn b() {}\n}\n"),
        1
    );
}

#[test]
fn a_test_only_helper_is_counted() {
    assert_eq!(non_test_lines("#[cfg(test)]\nfn helper() {\n}\n"), 3);
}

#[test]
fn code_between_two_inline_test_modules_is_counted() {
    let source = "fn a() {}\n\n#[cfg(test)]\nmod first {\n    fn t() {}\n}\n\nfn b() {}\nfn c() {}\n\n#[cfg(test)]\nmod second {\n    fn u() {}\n}\n";
    // `fn a`, the blank line, then `fn b`, `fn c` and the blank line.
    assert_eq!(non_test_lines(source), 6);
}
