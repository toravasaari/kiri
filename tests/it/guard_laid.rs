//! The laid guard: the layout's answers come from `Laid`, so no code outside
//! `src/typeset/` names `Stream` or makes a `Map` with `Map::new`.
//!
//! `Stream` is what the driver lays out and `Map` is what finds places in
//! it. `typeset::laid::Laid` holds both and answers for them, so that a
//! layout that is made in pieces, or not whole, can sit behind it without
//! its callers changing. A caller that keeps a `Stream` or builds a `Map`
//! of its own again holds the layout in two parts.
//!
//! The test reads the source as text (`rust_source`): comments, strings and
//! test code are not read. A test may build a `Stream` and a `Map` to hold
//! `Laid` to them. It sees the names written in code, not a `Stream` that
//! a caller only gets from a function and never names.

use std::fs;
use std::path::Path;

use crate::rust_source::{
    declared_for_tests, is_ident, is_test_file, module_path, non_test_code, parent_source,
    rust_files,
};

/// What is not to be written outside the typesetting engine.
const NAMES: &[&str] = &["Stream", "Map::new"];

#[test]
fn nothing_outside_the_typesetting_engine_names_stream_or_makes_a_map() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(!files.is_empty(), "no source files under src");
    let mut found = Vec::new();
    for file in files {
        let relative = file.strip_prefix(&src).unwrap();
        if relative.starts_with("typeset") {
            continue;
        }
        let name = module_path(relative).last().cloned().unwrap_or_default();
        let for_tests =
            parent_source(&src, relative).is_some_and(|parent| declared_for_tests(&parent, &name));
        if is_test_file(relative) || for_tests {
            continue;
        }
        let code = non_test_code(&fs::read_to_string(&file).unwrap());
        for (number, line) in code.lines().enumerate() {
            for name in NAMES {
                if names(line, name) {
                    found.push(format!(
                        "  src/{}:{}: {name}",
                        relative.display(),
                        number + 1
                    ));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "code outside src/typeset/ holds the layout in parts again:\n{}\n\
         Ask `typeset::laid::Laid` instead: it makes the layout, keeps its map and \
         answers for it.",
        found.join("\n")
    );
}

/// Whether `line`, which has no comments or strings, holds `name` as a whole
/// name: not the end of a longer one, as `HashMap::new` is not `Map::new`.
fn names(line: &str, name: &str) -> bool {
    line.match_indices(name).any(|(at, found)| {
        let before = line[..at].chars().next_back();
        let after = line[at + found.len()..].chars().next();
        before.is_none_or(|c| !is_ident(c)) && after.is_none_or(|c| !is_ident(c))
    })
}

#[test]
fn a_whole_name_is_seen_and_the_end_of_a_longer_one_is_not() {
    assert!(names("let s: Stream = x;", "Stream"));
    assert!(names(
        "use crate::typeset::driver::{Content, Stream};",
        "Stream"
    ));
    assert!(names("let map = Map::new(&lines, &blocks);", "Map::new"));
    assert!(names("locate::Map::new(a, b)", "Map::new"));
    assert!(!names("let s: UnixStream = x;", "Stream"));
    assert!(!names("let s = Streams::new();", "Stream"));
    assert!(!names("let m = HashMap::new();", "Map::new"));
    assert!(!names("let m = BTreeMap::new();", "Map::new"));
    assert!(!names("let m = Map::news();", "Map::new"));
    // What the reader blanks is not read.
    assert!(!names(
        &non_test_code("// Stream\nlet s = \"Map::new\";\n"),
        "Stream"
    ));
}
