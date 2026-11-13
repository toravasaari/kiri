//! The spans guard: which files of the state engine read the spans of a
//! line.
//!
//! The state engine (`src/session/`) speaks of places in the document, and
//! the typesetting engine says where they are shown (`typeset::locate`). A
//! file that reads the spans of a `Line` to find a place does the engines'
//! work, and another way of showing a place breaks it. The files that still
//! do are listed, each with the work that removes it. A file that is not
//! listed fails the test, and so does a listed file that no longer reads
//! spans, so the list can only shrink. Adding an entry is the maintainer's
//! decision. At the end of the line, drawing is all that is left on it.
//!
//! The test reads the source as text (`rust_source`), and it sees a read of
//! the field, `line.spans`, in code that is not a test. It does not see the
//! method of the same name, `status.spans(`, nor a call of `style::text`,
//! which takes spans from any place: the status line and the warnings call
//! it on spans of their own, and one that reads the spans of a line has the
//! field in its argument.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::rust_source::{
    declared_for_tests, is_ident, is_test_file, module_path, non_test_code, parent_source,
    rust_files,
};

/// Every file of `src/session/` that reads the spans of a line, and the work
/// that removes the read, in the words of docs/architecture.md and the plan
/// of the line.
const READERS: &[(&str, &str)] = &[
    (
        "src/session/animation.rs",
        "task 4: the images that play come from `typeset::targets`",
    ),
    (
        "src/session/frame.rs",
        "task 4: the rows of a split are painted by the engines",
    ),
    (
        "src/session/layers.rs",
        "task 4: painting a range is the engines'",
    ),
    (
        "src/session/links.rs",
        "task 4: the links come from `typeset::targets`",
    ),
    (
        "src/session/tint.rs",
        "task 4: the tint of a changed block is a layer the engines paint",
    ),
];

#[test]
fn only_the_listed_files_read_the_spans_of_a_line() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let found = readers(&src);
    let listed: BTreeSet<String> = READERS.iter().map(|&(file, _)| file.to_owned()).collect();
    let message = difference(&found, &listed);
    assert!(message.is_empty(), "{message}");
}

/// What is wrong when `found` is the set of files that read spans and
/// `listed` the set that may: nothing, or what to do.
fn difference(found: &BTreeSet<String>, listed: &BTreeSet<String>) -> String {
    let mut message = String::new();
    let unlisted: Vec<String> = found
        .difference(listed)
        .map(|file| format!("  {file}"))
        .collect();
    if !unlisted.is_empty() {
        message += &format!(
            "files of the state engine read the spans of a line:\n{}\n\
             Ask the layout where a range is shown (`typeset::locate`) instead. If the \
             read has to stay, add the file to READERS with the work that removes it. \
             That is the maintainer's decision.\n",
            unlisted.join("\n")
        );
    }
    let gone: Vec<String> = listed
        .difference(found)
        .map(|file| format!("  {file}"))
        .collect();
    if !gone.is_empty() {
        message += &format!(
            "listed in READERS but no longer reading spans:\n{}\n\
             Remove those entries: the list can only shrink.\n",
            gone.join("\n")
        );
    }
    message
}

/// The files of `src/session/` that read the spans of a line in code that is
/// not a test.
fn readers(src: &Path) -> BTreeSet<String> {
    let mut files = Vec::new();
    rust_files(&src.join("session"), &mut files);
    assert!(!files.is_empty(), "no source files under src/session");
    let mut found = BTreeSet::new();
    for file in files {
        let relative = file.strip_prefix(src).unwrap();
        let name = module_path(relative).last().cloned().unwrap_or_default();
        let for_tests =
            parent_source(src, relative).is_some_and(|parent| declared_for_tests(&parent, &name));
        if is_test_file(relative) || for_tests {
            continue;
        }
        let code = non_test_code(&fs::read_to_string(&file).unwrap());
        if reads_spans(&code) {
            found.insert(format!("src/{}", relative.display()));
        }
    }
    found
}

/// Whether `code`, which has no comments or strings, reads the field `spans`
/// of something: `.spans` that is not the start of a longer name and not a
/// call.
fn reads_spans(code: &str) -> bool {
    code.match_indices(".spans").any(|(at, found)| {
        let after = code[at + found.len()..].chars().next();
        after.is_none_or(|c| !is_ident(c) && c != '(')
    })
}

#[test]
fn a_read_of_the_field_is_seen_and_a_method_or_a_longer_name_is_not() {
    assert!(reads_spans("let n = line.spans.len();"));
    assert!(reads_spans("for span in &mut line.spans {"));
    assert!(reads_spans("(line.spans\n    .iter())"));
    assert!(reads_spans("Frame { spans: &placed.spans }"));
    assert!(!reads_spans("let s = status.spans(room);"));
    assert!(!reads_spans("let n = line.spans_of_it;"));
    assert!(!reads_spans("pub spans: Vec<Span>,"));
    // What the reader blanks is not read.
    assert!(!reads_spans(&non_test_code(
        "// line.spans\nlet s = \"line.spans\";\n"
    )));
    assert!(!reads_spans(&non_test_code(
        "#[cfg(test)]\nmod tests { fn t() { line.spans; } }\n"
    )));
}

#[test]
fn a_file_that_is_not_listed_fails_and_so_does_a_listed_one_that_no_longer_reads() {
    let set =
        |files: &[&str]| -> BTreeSet<String> { files.iter().map(|f| (*f).to_owned()).collect() };
    assert_eq!(difference(&set(&["a.rs"]), &set(&["a.rs"])), "");
    let new = difference(&set(&["a.rs", "b.rs"]), &set(&["a.rs"]));
    assert!(
        new.contains("  b.rs") && !new.contains("no longer"),
        "{new}"
    );
    let gone = difference(&set(&["a.rs"]), &set(&["a.rs", "c.rs"]));
    assert!(
        gone.contains("  c.rs") && gone.contains("no longer"),
        "{gone}"
    );
}
