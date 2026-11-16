//! Property tests of parse and layout: whatever the Markdown and the width,
//! layout does not panic, and no line is wider than the window.
//!
//! The documents are built from pieces so that the cases reach the layout
//! code: lines of the fixture documents, the starts of each kind of block,
//! and text with wide, combining and joined characters and long words.
//! Reading mode's layout is not covered here.

use std::path::Path;

use kiri::images::Source;
use kiri::typeset::caps::Capabilities;
use kiri::typeset::wrap::width;
use proptest::prelude::*;

use crate::common::{fixtures, one_image, props_config, quadrants, with_images};

/// The lines of the fixture documents, as pieces of documents that are known
/// to have meaning.
fn fixture_lines() -> Vec<String> {
    let mut lines = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(fixtures())
        .expect("the fixtures are readable")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    names.sort();
    for path in names {
        lines.extend(read_lines(&path));
    }
    lines.retain(|line| !line.trim().is_empty());
    lines
}

fn read_lines(path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).expect("a fixture is readable");
    text.lines().map(str::to_owned).collect()
}

/// What starts a line of some block.
const STARTS: &[&str] = &[
    "# ",
    "## ",
    "###### ",
    "> ",
    "> > ",
    "> [!NOTE]\n> ",
    "- ",
    "  - ",
    "* [ ] ",
    "1. ",
    "10. ",
    "| ",
    "|---|---|",
    "| a | b |",
    "```",
    "```rust\n",
    "```mermaid\n",
    "    ",
    "<details>\n<summary>Title</summary>\n\n",
    "</details>",
    "$$\n",
    "$$",
    "---",
    "***",
    "[note]: http://example.com\n",
    "![alt](missing.png)",
    "![wide](wide.png)",
    "![tall](tall.png)",
    "![square](square.png) ",
    "- ![tall](tall.png) ",
    "> ![wide](wide.png) ",
    "# ![square](square.png) ",
    "# ![wide](wide.png) ",
    "<div>",
];

/// Text for the inside of a line.
fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => "[a-zA-Z]{1,9}( [a-zA-Z]{1,9}){0,6}",
        2 => Just("日本語のテキストと漢字".to_owned()),
        1 => Just("emoji 🦀 and 😀 and 🇫🇮 and 👨\u{200d}👩\u{200d}👧".to_owned()),
        1 => Just("combining e\u{301}\u{301} and a\u{200b}b".to_owned()),
        1 => Just("`code with spaces` and **bold** and *italic* and ~~strike~~".to_owned()),
        1 => Just("[a link](http://example.com/a/path) <http://example.com>".to_owned()),
        1 => Just("https://example.com/a/very/long/path/that/never/ends?with=query#anchor".to_owned()),
        1 => Just("averyveryverylongwordwithoutanyspacesatalltobreak".to_owned()),
        1 => Just("inline $x^2$ formula and footnote[^1]".to_owned()),
        1 => "\\PC{0,12}",
    ]
}

/// One line of a document.
fn line(fixture: Vec<String>) -> impl Strategy<Value = String> {
    prop_oneof![
        3 => proptest::sample::select(fixture),
        3 => (proptest::sample::select(STARTS.to_vec()), text())
            .prop_map(|(start, text)| format!("{start}{text}")),
        2 => text(),
        1 => Just(String::new()),
    ]
}

fn document() -> impl Strategy<Value = String> {
    let fixture = fixture_lines();
    proptest::collection::vec(line(fixture), 1..14).prop_map(|lines| lines.join("\n") + "\n")
}

/// Blocks that may be wider than the window on purpose, each with its
/// reason. None yet: a line that is too wide is a finding.
const WIDE_ON_PURPOSE: &[&str] = &[];

/// The pieces of the documents reach what the property is for: a tall image
/// is drawn in rows, and a table and a code block are laid out.
#[test]
fn the_pieces_reach_images_tables_and_code() {
    let caps = Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(40)
    };
    let images = one_image("tall.png", quadrants(30, 900));
    let inputs = with_images(&caps, images);
    let source =
        "![tall](tall.png)\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n";
    let lines = kiri::typeset::driver::layout(&kiri::typeset::parse::parse(source), &inputs);
    assert!(lines.iter().filter(|line| line.image.is_some()).count() > 3);
    let text: String = lines
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.text.as_str()))
        .collect();
    assert!(text.contains("fn main()") && text.contains('1'), "{text:?}");
}

proptest! {
    #![proptest_config(props_config())]

    #[test]
    fn layout_fits_the_window_at_any_width(
        source in document(),
        columns in 1u16..=300,
        which in 0usize..3,
    ) {
        let caps = match which {
            0 => Capabilities::plain(columns),
            1 => Capabilities::ansi(columns),
            _ => Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(columns)
            },
        };
        // A wide, a tall and a square image, for the documents that show
        // them.
        let mut images = one_image("wide.png", quadrants(400, 20));
        images.insert("tall.png", Ok(Source::Raster(quadrants(30, 900))));
        images.insert("square.png", Ok(Source::Raster(quadrants(60, 60))));
        let inputs = with_images(&caps, images);
        // The layout takes a window narrower than 10 cells to be 10 wide
        // (`width.max(10)` in `src/typeset/layout/mod.rs`).
        let window = usize::from(columns).max(10);
        let document = kiri::typeset::parse::parse(&source);
        let lines = kiri::typeset::driver::layout(&document, &inputs);
        for line in &lines {
            let image = line.image.as_ref().map_or(0, |row| usize::from(row.placement.size.columns));
            let cells = width(&line.lead) + width(&line.spans) + image + width(&line.trail);
            let shown: String = line.spans.iter().map(|span| span.text.as_str()).collect();
            if cells > window && !WIDE_ON_PURPOSE.iter().any(|reason| shown.contains(reason)) {
                prop_assert!(false, "{cells} cells in a window of {columns} (laid out as {window}): {shown:?}");
            }
        }
    }
}
