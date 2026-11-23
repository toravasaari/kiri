use super::*;
use crate::typeset::caps::Capabilities;
use crate::typeset::driver::{self, Content};
use crate::typeset::inputs::Inputs;
use crate::typeset::model::File;
use crate::typeset::parse::csv::parse as csv;
use crate::typeset::parse::{file as text_file, parse};
use crate::typeset::source_rows::Separator;
use crate::typeset::targets::targets;
use std::rc::Rc;

fn of(source: &str) -> Vec<(u8, String, String)> {
    let document = parse(source);
    let found = targets([&document]);
    let contents = contents([(None, &document)], &found);
    contents
        .entries()
        .iter()
        .map(|e| (e.level, e.text.clone(), e.id.clone().unwrap()))
        .collect()
}

fn row(level: u8, text: &str, id: &str) -> (u8, String, String) {
    (level, text.to_owned(), id.to_owned())
}

#[test]
fn levels_one_to_six() {
    let source = "# a\n## b\n### c\n#### d\n##### e\n###### f\n";
    let levels: Vec<u8> = of(source).iter().map(|e| e.0).collect();
    assert_eq!(levels, [1, 2, 3, 4, 5, 6]);
    assert_eq!(of(source)[2], row(3, "c", "c"));
}

#[test]
fn a_document_without_headings_has_no_entries() {
    assert_eq!(of("just text\n\n- a list\n"), []);
    assert_eq!(of(""), []);
}

#[test]
fn an_image_in_a_heading_is_its_alt_text_and_a_formula_its_source() {
    let found = of("# Logo ![the logo](a.png) here\n\n## Euler $e^{i\\pi}$ says\n");
    assert_eq!(found[0].1, "Logo the logo here");
    assert_eq!(found[1].1, "Euler e^{i\\pi} says");
}

#[test]
fn markup_and_breaks_in_a_heading_are_one_line_of_plain_text() {
    let found = of("# *Em* **strong** `code` [link](u)\n\n<h2>Two<br>lines</h2>\n");
    assert_eq!(found[0].1, "Em strong code link");
    assert_eq!(found[1], row(2, "Two lines", "two-lines"));
}

#[test]
fn an_html_heading_has_its_level() {
    assert_eq!(of("<h2>Html two</h2>\n"), [row(2, "Html two", "html-two")]);
}

#[test]
fn a_heading_inside_details_a_quote_and_a_list_is_listed_with_its_own_level() {
    let source = "<details>\n<summary>S</summary>\n\n### In details\n\n</details>\n\n\
            > ## In quote\n\n- item\n\n  #### In list\n";
    let found = of(source);
    assert_eq!(
        found,
        [
            row(3, "In details", "in-details"),
            row(2, "In quote", "in-quote"),
            row(4, "In list", "in-list"),
        ]
    );
}

#[test]
fn an_image_in_an_html_heading_in_a_block_is_its_alt_text_as_in_a_lone_one() {
    // The same three ways to write a logo as a heading.
    assert_eq!(
        of("<h2><img src=\"a.png\" alt=\"Logo\"></h2>\n")[0],
        row(2, "Logo", "logo")
    );
    assert_eq!(of("# ![Three](c.png)\n")[0], row(1, "Three", "three"));
    assert_eq!(
        of("<div>\n<h3><img src=\"b.png\" alt=\"Two\"> words</h3>\n</div>\n")[0],
        row(3, "Two words", "two-words")
    );
}

#[test]
fn a_control_character_in_a_heading_is_the_replacement_character() {
    assert_eq!(
        of("# a\u{1b}[31mb\u{7f}c\n")[0].1,
        "a\u{fffd}[31mb\u{fffd}c"
    );
}

#[test]
fn an_empty_heading_has_a_key_unlike_the_entry_of_its_file() {
    let files = [File {
        name: "a.md".to_owned(),
        document: parse("#\n\n# ![](a.png)\n"),
    }];
    let contents = of_files(&files);
    let keys: Vec<_> = (contents.entries().iter())
        .map(|e| (e.file, e.id.clone()))
        .collect();
    assert_eq!(
        keys,
        [(0, None), (0, Some(String::new())), (0, Some("-1".into()))]
    );
    assert_eq!(contents.entries()[1].text, "");
}

#[test]
fn headings_in_a_block_of_html_are_listed_with_the_level_of_their_tag() {
    let source = "<div align=\"center\">\n<h2>In <em>div</em></h2>\n<h3 id=\"x\">Three</h3>\n</div>\n\n\
            <details>\n<summary>S</summary>\n\n<div>\n<h4>Inside</h4>\n</div>\n\n</details>\n";
    assert_eq!(
        of(source),
        [
            row(2, "In div", "in-div"),
            row(3, "Three", "x"),
            row(4, "Inside", "inside"),
        ]
    );
}

#[test]
fn duplicate_headings_have_the_ids_of_the_links() {
    let found = of("# Same\n\n# Same\n");
    assert_eq!(found[1].2, "same-1");
}

#[test]
fn csv_and_plain_text_have_no_entries() {
    for document in [
        csv("a,b\n1,2\n", Separator::Comma).unwrap(),
        text_file("# not a heading\nline\n", None),
    ] {
        let found = targets([&document]);
        assert_eq!(contents([(None, &document)], &found).entries(), []);
    }
}

fn files() -> Vec<File> {
    let file = |name: &str, source: &str| File {
        name: name.to_owned(),
        document: parse(source),
    };
    vec![
        file("a.md", "# A one\n\n## A two\n"),
        file("b.md", "text only\n"),
        file("c.md", "# C one\n"),
    ]
}

fn of_files(files: &[File]) -> Contents {
    let found = targets(files.iter().map(|f| &f.document));
    contents(
        files.iter().map(|f| (Some(f.name.as_str()), &f.document)),
        &found,
    )
}

#[test]
fn each_file_is_an_entry_above_its_headings() {
    let files = files();
    let contents = of_files(&files);
    let seen: Vec<_> = contents
        .entries()
        .iter()
        .map(|e| (e.file, e.level, e.text.as_str(), e.path.clone()))
        .collect();
    assert_eq!(
        seen,
        [
            (0, 0, "a.md", vec![]),
            (0, 1, "A one", vec![0]),
            (0, 2, "A two", vec![1]),
            (1, 0, "b.md", vec![]),
            (2, 0, "c.md", vec![]),
            (2, 1, "C one", vec![0]),
        ]
    );
}

// The current heading.

const LONG: &str = "para one para one para one para one para one para one\n\n\
        # First\n\ntext text text text text text text text text text text text\n\n\
        ## Second\n\nmore more more more more more more more more more more more\n\n\
        ### Third\n\nlast last last last last last last last last last last\n";

fn cells(width: u16) -> Inputs {
    Inputs::new(Capabilities::kitty(width))
}

fn reading(width: u16) -> Inputs {
    Inputs {
        reading: Some(crate::typeset::reading::engine::Reading {
            pictures: Default::default(),
            glyphs: Default::default(),
            fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
            snap: false,
            zoom: 100,
        }),
        caps: Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(width)
        },
        ..Inputs::new(Capabilities::kitty(width))
    }
}

/// Checks `current` at the lines around each heading of `LONG` laid out
/// with `inputs`.
fn assert_current(inputs: &Inputs, what: &str) {
    let document = parse(LONG);
    let found = targets([&document]);
    let contents = contents([(None, &document)], &found);
    let laid = driver::laid(Content::Document(&document), inputs);
    assert_eq!(contents.entries().len(), 3, "{what}");
    let lines: Vec<usize> = contents
        .entries()
        .iter()
        .map(|e| laid.line_of(&Pos::block(e.file, &e.path)).unwrap())
        .collect();
    assert!(
        lines[0] < lines[1] && lines[1] < lines[2],
        "{what}: {lines:?}"
    );
    // Before the first heading.
    assert_eq!(contents.current(laid.map(), 0), None, "{what}: top");
    assert_eq!(
        contents.current(laid.map(), lines[0] - 1),
        None,
        "{what}: before"
    );
    for (i, &line) in lines.iter().enumerate() {
        // On the heading.
        assert_eq!(
            contents.current(laid.map(), line),
            Some(i),
            "{what}: on {i}"
        );
        // Between: a line below the heading, above the next.
        let below = line + 1;
        if let Some(&next) = lines.get(i + 1) {
            assert_eq!(
                contents.current(laid.map(), below),
                Some(i),
                "{what}: below {i}"
            );
            assert_eq!(
                contents.current(laid.map(), next - 1),
                Some(i),
                "{what}: above {}",
                i + 1
            );
        }
    }
    // After the last.
    let last = laid.len() - 1;
    assert_eq!(contents.current(laid.map(), last), Some(2), "{what}: end");
}

#[test]
fn the_current_heading_in_the_cell_layout() {
    assert_current(&cells(60), "cells 60");
}

#[test]
fn the_current_heading_in_reading_mode() {
    assert_current(&reading(60), "reading 60");
}

#[test]
fn the_current_heading_is_the_same_at_every_width_of_both_layouts() {
    for make in [cells, reading] {
        let document = parse(LONG);
        let found = targets([&document]);
        let contents = contents([(None, &document)], &found);
        let second = &contents.entries()[1];
        // The reader is on `Second` at each width, whichever line it
        // is on there.
        let at: Vec<Option<usize>> = [24, 60, 100]
            .into_iter()
            .map(|width| {
                let laid = driver::laid(Content::Document(&document), &make(width));
                let top = laid.line_of(&Pos::block(0, &second.path)).unwrap();
                contents.current(laid.map(), top)
            })
            .collect();
        assert_eq!(at, [Some(1), Some(1), Some(1)]);
        for width in [24, 100] {
            assert_current(&make(width), &format!("width {width}"));
        }
    }
}

#[test]
fn a_file_is_current_from_its_header_on() {
    let files = files();
    let contents = of_files(&files);
    let laid = driver::laid(Content::Files(&files), &cells(60));
    let header = laid.line_of(&Pos::block(1, &[])).unwrap();
    // `b.md` is entry 3; its header is a few lines, and the blank line
    // above it still shows the last heading of `a.md`.
    assert_eq!(contents.entries()[3].text, "b.md");
    assert_eq!(contents.current(laid.map(), header), Some(3));
    assert_eq!(contents.current(laid.map(), header + 1), Some(3));
    assert_eq!(contents.current(laid.map(), header - 1), Some(2));
}

#[test]
fn the_top_of_several_files_is_in_the_first_file() {
    let files = files();
    let contents = of_files(&files);
    for inputs in [cells(60), reading(60)] {
        let laid = driver::laid(Content::Files(&files), &inputs);
        assert_eq!(contents.current(laid.map(), 0), Some(0));
    }
}

/// Checks that each entry is current on the line that shows it, and
/// that every line is in the last entry at or above it.
fn assert_each_entry_is_current(source: &str, inputs: &Inputs, what: &str) -> Vec<Entry> {
    let document = parse(source);
    let found = targets([&document]);
    let contents = contents([(None, &document)], &found);
    let laid = driver::laid(Content::Document(&document), inputs);
    let lines: Vec<usize> = (contents.entries().iter())
        .map(|e| laid.line_of(&Pos::block(e.file, &e.path)).unwrap())
        .collect();
    assert!(
        lines.windows(2).all(|pair| pair[0] < pair[1]),
        "{what}: {lines:?}"
    );
    for top in 0..laid.len() {
        let last = lines.iter().rposition(|&line| line <= top);
        assert_eq!(contents.current(laid.map(), top), last, "{what}: top {top}");
    }
    for (i, &line) in lines.iter().enumerate() {
        assert_eq!(
            contents.current(laid.map(), line),
            Some(i),
            "{what}: on {i}"
        );
    }
    contents.entries().to_vec()
}

#[test]
fn nested_headings_are_current_in_both_layouts() {
    let source = "# Top\n\ntext text text\n\n> ## In quote\n>\n> quoted text\n>\n> > ### In nested quote\n\n\
            - item\n\n  #### In list\n\n  list text\n\n\
            <details>\n<summary>S</summary>\n\n##### In details\n\ndetails text\n\n</details>\n\n\
            > [!NOTE]\n> ###### In alert\n\nlast text\n";
    for (inputs, what) in [(cells(60), "cells"), (reading(60), "reading")] {
        let entries = assert_each_entry_is_current(source, &inputs, what);
        let levels: Vec<u8> = entries.iter().map(|e| e.level).collect();
        assert_eq!(levels, [1, 2, 3, 4, 5, 6], "{what}");
    }
}

#[test]
fn an_empty_heading_and_an_image_without_alt_text_are_current() {
    let source = "# Before\n\ntext\n\n#\n\ntext\n\n# ![](a.png)\n\ntext\n";
    for (inputs, what) in [(cells(60), "cells"), (reading(60), "reading")] {
        let entries = assert_each_entry_is_current(source, &inputs, what);
        let texts: Vec<&str> = entries.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["Before", "", ""], "{what}");
    }
}

/// Checks that each entry of `source` is current on the line that
/// `line_of` gives for its place, and that the line before it is in the
/// entry before. Where that line is in a block of reading mode is not
/// checked here: see the test of a heading deep in a block.
fn assert_html_entries_are_current(source: &str, expect: &[&str]) {
    for (inputs, what) in [(cells(60), "cells"), (reading(60), "reading")] {
        let document = parse(source);
        let found = targets([&document]);
        let contents = contents([(None, &document)], &found);
        let laid = driver::laid(Content::Document(&document), &inputs);
        let texts: Vec<&str> = contents.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, expect, "{what}");
        let lines: Vec<usize> = (contents.entries().iter())
            .map(|e| laid.line_of(&e.start).unwrap())
            .collect();
        if what == "cells" {
            assert!(lines.windows(2).all(|pair| pair[0] < pair[1]), "{lines:?}");
        }
        for (i, &line) in lines.iter().enumerate() {
            // Reading mode can show two headings of one block on one
            // line; the last of them is the one the reader is in.
            let on = lines.iter().rposition(|&other| other <= line);
            assert_eq!(contents.current(laid.map(), line), on, "{what}: on {i}");
            let before = line.checked_sub(1);
            let was = before.and_then(|top| lines.iter().rposition(|&other| other <= top));
            assert_eq!(
                before.and_then(|top| contents.current(laid.map(), top)),
                was,
                "{what}: before {i}"
            );
        }
    }
}

#[test]
fn two_headings_in_one_block_of_html_are_current_on_their_own_lines() {
    assert_html_entries_are_current(
        "# Top\n\n<div>\n<h2>Alpha</h2>\n<h3>Beta</h3>\n</div>\n\nlast text\n",
        &["Top", "Alpha", "Beta"],
    );
}

#[test]
fn a_heading_below_an_image_in_a_block_of_html_is_current_from_its_own_line() {
    assert_html_entries_are_current(
        "# Top\n\n<div align=\"center\">\n<img src=\"x.png\" alt=\"x\">\n<h2>Mid</h2>\n</div>\n\nlast text\n",
        &["Top", "Mid"],
    );
}

#[test]
fn a_heading_deep_in_a_block_of_html_is_current_from_its_own_row_in_reading_mode() {
    let source = "# Top\n\n<div>\n\
            <p>lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua</p>\n\
            <h2>Alpha</h2>\n\
            <p>ut enim ad minim veniam quis nostrud exercitation ullamco laboris nisi</p>\n\
            </div>\n\nlast text\n";
    let document = parse(source);
    let found = targets([&document]);
    let contents = contents([(None, &document)], &found);
    let laid = driver::laid(Content::Document(&document), &reading(60));
    let block = laid.line_of(&Pos::block(0, &[1])).unwrap();
    let alpha = laid.line_of(&contents.entries()[1].start).unwrap();
    let last = laid.line_of(&Pos::block(0, &[2])).unwrap();
    assert!(block < alpha && alpha < last, "{block} {alpha} {last}");
    // The first row of the lines that the page's parts draw the element
    // of `Alpha` on (`drawn_rows`, which asks the page for the rows of
    // the text, as `locate` does for a search), not `line_of`.
    let named = found
        .named
        .iter()
        .find(|named| named.id == "alpha")
        .unwrap();
    let (_, rows) = laid.drawn_rows(&named.range).expect("drawn");
    assert_eq!(alpha, rows.start);
    assert_eq!(contents.current(laid.map(), alpha - 1), Some(0));
    assert_eq!(contents.current(laid.map(), alpha), Some(1));
}

#[test]
fn the_current_heading_with_several_files() {
    let files = files();
    let contents = of_files(&files);
    let laid = driver::laid(Content::Files(&files), &cells(60));
    let line = |at: usize| {
        let e = &contents.entries()[at];
        laid.line_of(&Pos::block(e.file, &e.path)).unwrap()
    };
    // On the text of the second file's heading `A two`, and on the
    // first block of the file `c.md`.
    assert_eq!(contents.current(laid.map(), line(2)), Some(2));
    assert_eq!(contents.current(laid.map(), line(5)), Some(5));
    assert_eq!(contents.current(laid.map(), laid.len() - 1), Some(5));
}

#[test]
fn nothing_is_current_in_a_document_without_headings() {
    let document = parse("a\n\nb\n");
    let found = targets([&document]);
    let contents = contents([(None, &document)], &found);
    let laid = driver::laid(Content::Document(&document), &cells(60));
    assert_eq!(contents.current(laid.map(), 0), None);
}
