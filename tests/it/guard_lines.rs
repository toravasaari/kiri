//! The lines guard: the state engine does not read the lines of a layout to
//! find places in them.
//!
//! The state engine (`src/session/`) speaks of places in the document, and
//! the typesetting engine says where they are shown (`typeset::locate`). A
//! file that reads the lines of a layout to find a place does the engines'
//! work, and another way of showing a place breaks it. Two checks keep it
//! so.
//!
//! The spans of a line: the files that still read them are listed, each
//! with why. A file that is not listed fails the test, and so does a listed
//! file that no longer reads spans, so the list can only shrink. Adding an
//! entry is the maintainer's decision. Drawing is all that is left on the
//! list.
//!
//! The blocks of a line and the bands and pages of reading mode: no file
//! reads them, listed or not. The fields `block` and `inner` of a line, and
//! the calls `engine::bands(` and `engine::pages(` and the types they give,
//! are the map's to read (`Map::first_on`, `Map::bands`).
//!
//! The test reads the source as text (`rust_source`), and it sees a read of
//! a field, `line.spans` or `line.block`, in code that is not a test. It does
//! not see the method of the same name, `status.spans(` or `moves.block(`,
//! nor a call of `style::text`, which takes spans from any place: the status
//! line and the warnings call it on spans of their own, and one that reads
//! the spans of a line has the field in its argument. Nor does it see
//! destructuring, `let Line { spans, block, .. } = line`, which reads the
//! fields without their names after a dot, nor `bands(` called by another
//! path, as after `use …::engine::bands` or with the module renamed, nor
//! the image of a line read for its band (`line.image`, `ImageKey::Band`);
//! no file does that now.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::rust_source::{
    Strings, declared_for_tests, is_ident, is_test_file, module_path, non_test_code, parent_source,
    rust_files,
};

/// Every file of `src/session/` that reads the spans of a line, and why it
/// still does. What is left is drawing: which images a line places is the
/// drawing's own question, and no place of the document is asked of a span.
const READERS: &[(&str, &str)] = &[
    (
        "src/session/animation.rs",
        "drawing: which images the lines place (`clips`, `placements`, `pictures_in`) \
         and the frame round the animation selected (`frame_selected`)",
    ),
    (
        "src/session/frame.rs",
        "drawing: the rows of a split that the lines in view are some of go on \
         above and below them (`painted_range`)",
    ),
];

#[test]
fn what_is_left_on_the_list_is_drawing() {
    let listed: Vec<&str> = READERS.iter().map(|&(file, _)| file).collect();
    assert_eq!(listed, ["src/session/animation.rs", "src/session/frame.rs"]);
    assert!(READERS.iter().all(|(_, why)| why.starts_with("drawing: ")));
}

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
    (session_code(src).into_iter())
        .filter(|(_, code)| reads_field(code, "spans"))
        .map(|(file, _)| file)
        .collect()
}

/// Each file of `src/session/` that is not test code, with its code without
/// comments, strings and test modules.
fn session_code(src: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    rust_files(&src.join("session"), &mut files);
    assert!(!files.is_empty(), "no source files under src/session");
    let mut found = Vec::new();
    for file in files {
        let relative = file.strip_prefix(src).unwrap();
        let name = module_path(relative).last().cloned().unwrap_or_default();
        let for_tests =
            parent_source(src, relative).is_some_and(|parent| declared_for_tests(&parent, &name));
        if is_test_file(relative) || for_tests {
            continue;
        }
        let code = non_test_code(&fs::read_to_string(&file).unwrap(), Strings::Blank);
        found.push((format!("src/{}", relative.display()), code));
    }
    found
}

/// Whether `code`, which has no comments or strings, reads the field `field`
/// of something: `.field` that is not the start of a longer name and not a
/// call.
fn reads_field(code: &str, field: &str) -> bool {
    code.match_indices(&format!(".{field}")).any(|(at, found)| {
        let after = code[at + found.len()..].chars().next();
        after.is_none_or(|c| !is_ident(c) && c != '(')
    })
}

/// Whether `code`, which has no comments or strings, finds places in the
/// lines of a layout without the map: the block of a line, or reading
/// mode's bands and pages taken from the lines.
fn reads_layout(code: &str) -> bool {
    let named = |name: &str| {
        code.match_indices(name).any(|(at, found)| {
            let before = code[..at].chars().next_back();
            let after = code[at + found.len()..].chars().next();
            let ends = name.ends_with('(') || after.is_none_or(|c| !is_ident(c));
            before.is_none_or(|c| !is_ident(c)) && ends
        })
    };
    reads_field(code, "block")
        || reads_field(code, "inner")
        || ["engine::bands(", "engine::pages(", "BandRows", "PageRows"]
            .iter()
            .any(|name| named(name))
}

#[test]
fn no_file_of_the_state_engine_finds_places_in_the_lines() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // The files on the list of spans readers too: drawing needs no blocks.
    let found: Vec<String> = (session_code(&src).into_iter())
        .filter(|(_, code)| reads_layout(code))
        .map(|(file, _)| format!("  {file}"))
        .collect();
    assert!(
        found.is_empty(),
        "files of the state engine read the blocks of lines, or reading mode's bands or \
         pages from them:\n{}\nAsk the map instead (`Map::first_on`, `Map::bands`).",
        found.join("\n")
    );
}

#[test]
fn a_read_of_the_field_is_seen_and_a_method_or_a_longer_name_is_not() {
    let reads_spans = |code: &str| reads_field(code, "spans");
    assert!(reads_spans("let n = line.spans.len();"));
    assert!(reads_spans("for span in &mut line.spans {"));
    assert!(reads_spans("(line.spans\n    .iter())"));
    assert!(reads_spans("Frame { spans: &placed.spans }"));
    assert!(!reads_spans("let s = status.spans(room);"));
    assert!(!reads_spans("let n = line.spans_of_it;"));
    assert!(!reads_spans("pub spans: Vec<Span>,"));
    // What the reader blanks is not read.
    assert!(!reads_spans(&non_test_code(
        "// line.spans\nlet s = \"line.spans\";\n",
        Strings::Blank
    )));
    assert!(!reads_spans(&non_test_code(
        "#[cfg(test)]\nmod tests { fn t() { line.spans; } }\n",
        Strings::Blank
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

#[test]
fn a_block_or_a_band_taken_from_the_lines_is_seen_and_the_map_and_methods_are_not() {
    assert!(reads_layout("let id = self.lines.get(line)?.block?;"));
    assert!(reads_layout("lines.get(start).and_then(|line| line.block)"));
    assert!(reads_layout("let Some(inner) = lines[index].inner else {"));
    assert!(reads_layout("for band in engine::bands(lines) {"));
    assert!(reads_layout("let page = reading::engine::pages(&lines);"));
    assert!(reads_layout("fn f(band: &BandRows) {}"));
    assert!(!reads_layout("let path = moves.block(file, path)?;"));
    assert!(!reads_layout("let table = layout.shown.blocks;"));
    assert!(!reads_layout("let place = band.block_of(at);"));
    assert!(!reads_layout("let size = self.inner_size;"));
    assert!(!reads_layout("let parts = wrapped.inner();"));
    assert!(!reads_layout(
        "engine::set_band(&mut lines[rows], offset, true);"
    ));
    assert!(!reads_layout("for band in map.bands() {"));
    assert!(!reads_layout("struct MyBandRowsLike;"));
    // What the reader blanks is not read.
    assert!(!reads_layout(&non_test_code(
        "#[cfg(test)]\nmod tests { fn t() { let b = line.block; } }\n",
        Strings::Blank
    )));
}
