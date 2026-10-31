//! The line caps: the hub files may not grow past the number of non-test
//! lines they have now, so that growth is seen in review.
//!
//! A file's count is its lines above `#[cfg(test)] mod tests`, or all its
//! lines when it has no such module. A cap is a ceiling. When a file
//! shrinks, its cap may be lowered to the new count; the test does not ask.

use std::fs;
use std::path::Path;

/// The hub files, with their caps in non-test lines. A hub file is one that
/// collects code of several parts, so new code goes to the part's own file
/// instead (docs/architecture.md, "Where a new thing goes").
const CAPS: &[(&str, usize)] = &[
    ("src/main.rs", 1800),
    ("src/pager/mod.rs", 1842),
    ("src/render/mod.rs", 345),
    ("src/typeset/layout/mod.rs", 1366),
    ("src/typeset/reading/engine.rs", 989),
    ("src/typeset/reading/page.rs", 1398),
    ("src/images/mod.rs", 934),
    ("src/diagrams/mod.rs", 428),
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

/// The lines of `source` above its `#[cfg(test)] mod tests`, or all of them.
fn non_test_lines(source: &str) -> usize {
    let lines: Vec<&str> = source.lines().collect();
    let end = lines
        .windows(2)
        .position(|pair| pair[0].trim() == "#[cfg(test)]" && pair[1].contains("mod tests"))
        .unwrap_or(lines.len());
    // A module declared for tests alone, `#[cfg(test)] mod name;`, is not
    // counted, nor is the blank line after it.
    let declared: usize = (1..end)
        .filter(|&at| lines[at - 1].trim() == "#[cfg(test)]" && lines[at].trim_end().ends_with(";"))
        .map(|at| {
            if lines.get(at + 1).is_some_and(|line| line.trim().is_empty()) {
                3
            } else {
                2
            }
        })
        .sum();
    end - declared
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
