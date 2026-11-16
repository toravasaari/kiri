//! Property tests of parse and layout: whatever the Markdown and the width,
//! layout does not panic, and from `NARROW` cells on no line is wider than
//! the window.
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

/// Below this many cells the chrome of list and task list items, which is
/// their indent and markers, may fill the window and more (B43); the
/// property then asks for no panic only. Quotes nested deep enough overflow
/// at any width, but the generator does not reach that depth, so the
/// property needs no limit for them. Nothing is wider than the window on
/// purpose.
const NARROW: usize = 24;

/// The cells of a line: its margin, text, image row and fill.
fn cells(line: &kiri::typeset::style::Line) -> usize {
    let image = line
        .image
        .as_ref()
        .map_or(0, |row| usize::from(row.placement.size.columns));
    width(&line.lead) + width(&line.spans) + image + width(&line.trail)
}

/// B43: the bars of quotes nested deeper than the window is wide go past its
/// edge, at any width: 63 cells for 30 levels in a window of 40, and 123 for
/// 60 levels in a window of 80, so a fix for narrow windows alone does not
/// make this pass.
#[test]
#[ignore = "B43: the bars of deeply nested quotes are wider than the window"]
fn deeply_nested_quotes_fit_the_window() {
    for (levels, columns) in [(30, 40), (60, 80)] {
        let source = format!("{}deep\n", "> ".repeat(levels));
        let inputs = crate::common::fixed_inputs(Capabilities::plain(columns));
        let lines = kiri::typeset::driver::layout(&kiri::typeset::parse::parse(&source), &inputs);
        let widest = lines.iter().map(cells).max().unwrap_or(0);
        assert!(
            widest <= usize::from(columns),
            "{widest} cells in a window of {columns} ({levels} levels)"
        );
    }
}

/// B43: a task list marker adds four cells to the indent of each
/// level, and a list in it adds more: 11 cells in a window of 10, 24 for
/// four levels in a window of 20.
#[test]
#[ignore = "B43: nested task lists are wider than a narrow window"]
fn nested_task_lists_fit_a_narrow_window() {
    let inputs = crate::common::fixed_inputs(Capabilities::plain(10));
    let source = "* [ ] A\n    1. Ordered inside\n";
    let lines = kiri::typeset::driver::layout(&kiri::typeset::parse::parse(source), &inputs);
    let widest = lines.iter().map(cells).max().unwrap_or(0);
    assert!(widest <= 10, "{widest} cells in a window of 10");
}

/// Capabilities of the kind `which` picks, `columns` wide: plain, ANSI or
/// kitty with cells of 10 × 20 pixels.
fn caps_for(which: usize, columns: u16) -> Capabilities {
    match which {
        0 => Capabilities::plain(columns),
        1 => Capabilities::ansi(columns),
        _ => Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(columns)
        },
    }
}

/// The test images by name: a wide, a tall and a square one, for the
/// documents that show them.
const IMAGES: [&str; 3] = ["wide.png", "tall.png", "square.png"];

fn test_images() -> kiri::images::Images {
    let mut images = one_image("wide.png", quadrants(400, 20));
    images.insert("tall.png", Ok(Source::Raster(quadrants(30, 900))));
    images.insert("square.png", Ok(Source::Raster(quadrants(60, 60))));
    images
}

/// The test images are laid out: a tall one is drawn in rows, and a table
/// and a code block are laid out.
#[test]
fn the_test_images_are_laid_out() {
    let inputs = with_images(&caps_for(2, 40), test_images());
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

/// The generator reaches what the width property is for: among a few
/// hundred documents from a fixed seed, some lay out each of the
/// three test images, as a row or in a line,, a table line and a code block. If a change to the
/// generator or the layout leaves one out, the property tests nothing of it.
#[test]
fn the_generated_documents_reach_images_tables_and_code() {
    use kiri::typeset::images::ImageKey;
    use kiri::typeset::model::Block;
    use proptest::strategy::ValueTree;
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

    let mut runner = TestRunner::new_with_rng(
        Config::default(),
        TestRng::from_seed(RngAlgorithm::ChaCha, &[7; 32]),
    );
    let strategy = (document(), 24u16..=120);
    let mut images = [false; 3];
    let (mut tables, mut code) = (false, false);
    for _ in 0..400 {
        let (source, columns) = strategy.new_tree(&mut runner).unwrap().current();
        let inputs = with_images(&caps_for(2, columns), test_images());
        let document = kiri::typeset::parse::parse(&source);
        code |= (document.blocks.iter()).any(|block| matches!(block, Block::CodeBlock { .. }));
        for line in kiri::typeset::driver::layout(&document, &inputs) {
            // A block image is a row; one in a line is an embed in a span.
            let keys: Vec<&ImageKey> = (line.image.iter().map(|row| &row.placement.key))
                .chain(
                    line.spans
                        .iter()
                        .filter_map(|span| span.embed.as_ref())
                        .map(|embed| &embed.placement.key),
                )
                .collect();
            for (seen, name) in images.iter_mut().zip(IMAGES) {
                *seen |= keys.contains(&&ImageKey::from(name));
            }
            let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
            tables |= text.contains('│');
        }
    }
    assert_eq!(images, [true; 3], "rows of wide, tall and square images");
    assert!(tables, "a table line");
    assert!(code, "a code block");
}

proptest! {
    #![proptest_config(props_config())]

    #[test]
    fn layout_fits_the_window_at_any_width(
        source in document(),
        columns in 1u16..=300,
        which in 0usize..3,
    ) {
        let inputs = with_images(&caps_for(which, columns), test_images());
        // The layout takes a window narrower than 10 cells to be 10 wide
        // (`width.max(10)` in `src/typeset/layout/mod.rs`).
        let window = usize::from(columns).max(10);
        let document = kiri::typeset::parse::parse(&source);
        let lines = kiri::typeset::driver::layout(&document, &inputs);
        for line in &lines {
            let cells = cells(line);
            if window >= NARROW && cells > window {
                let shown: String = line.spans.iter().map(|span| span.text.as_str()).collect();
                prop_assert!(false, "{cells} cells in a window of {columns}: {shown:?}");
            }
        }
    }
}
