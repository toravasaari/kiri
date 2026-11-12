//! What a document says, as text: the texts of each block, and the text of
//! the shown documents as a whole.
//!
//! What a document holds is asked of the model, and where it is shown is
//! asked of a layout. A search finds a term here, and a copy asks here for
//! the text between two places; neither looks at a line or a page.
//!
//! A block has a list of texts: the one text of a paragraph or a heading,
//! the code of a code block, a cell each of a table, the keys and values of
//! front matter, the summary of a `<details>`, the title of an alert, the
//! source of an HTML block. Kiri's own notes and errors are not the
//! document's text, and neither are the language of a code block, the
//! marker of a list item or anything else that a layout adds.
//!
//! Where an inline goes in the text is one walk, [`flatten`], which also
//! tells the bytes of each inline in it, so that nothing else walks inlines
//! to find a link or an image.

use std::ops::Range;

use crate::typeset::grid::{self, Cell, Grid, Leaves};
use crate::typeset::model::{Block, Document, Image, Inline, Value};
use crate::typeset::position::{Pos, Within};

/// One text of a block, and what comes before it when the texts of a block
/// are read as one: a line break or a tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub before: Before,
    pub text: String,
}

/// What comes between a text and the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Before {
    /// A line break. The first text of a block has it, too: between blocks
    /// and files there is always a line break.
    Line,
    /// A tab: the next cell of a row of a table.
    Cell,
}

impl Piece {
    fn line(text: impl Into<String>) -> Self {
        Self {
            before: Before::Line,
            text: text.into(),
        }
    }
}

/// Writes the text of `inlines` to `out`, and calls `visit` with each inline
/// and the bytes of its text in `out`, after the inline is written, so that
/// an inline inside another is visited before it.
///
/// Text, code and inline HTML are as they are; emphasis and the like, a
/// link, give the text of what is in them; an image gives its alt text; a
/// formula its LaTeX; a soft break a space, a hard break a line break.
/// (`Inline::plain_text` is another thing: it gives a formula as written
/// without math, and a hard break as a space.)
pub fn flatten(
    inlines: &[Inline],
    out: &mut String,
    visit: &mut impl FnMut(&Inline, Range<usize>),
) {
    for inline in inlines {
        let start = out.len();
        match inline {
            Inline::Text(text) | Inline::Code(text) | Inline::Html(text) => out.push_str(text),
            Inline::Emphasis(content)
            | Inline::Strong(content)
            | Inline::Strikethrough(content)
            | Inline::Kbd(content)
            | Inline::Mark(content)
            | Inline::Subscript(content)
            | Inline::Superscript(content)
            | Inline::Link { content, .. }
            | Inline::Image(Image { alt: content, .. }) => flatten(content, out, visit),
            Inline::Formula { source, .. } => out.push_str(source),
            Inline::SoftBreak => out.push(' '),
            Inline::LineBreak => out.push('\n'),
        }
        visit(inline, start..out.len());
    }
}

/// The texts of `block`. `visit` is called with the number of the text, an
/// inline in it and the bytes of the inline there, as [`flatten`] does.
pub fn pieces_with(
    block: &Block,
    visit: &mut impl FnMut(usize, &Inline, Range<usize>),
) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut inlines = |pieces: &mut Vec<Piece>, before, content: &[Inline]| {
        let number = pieces.len();
        let mut text = String::new();
        flatten(content, &mut text, &mut |inline, bytes| {
            visit(number, inline, bytes);
        });
        pieces.push(Piece { before, text });
    };
    match block {
        Block::Heading { content, .. } | Block::Paragraph(content) => {
            inlines(&mut pieces, Before::Line, content);
        }
        Block::Details { summary, .. } => inlines(&mut pieces, Before::Line, summary),
        Block::Table(table) => {
            let rows = std::iter::once(&table.header).chain(&table.rows);
            for row in rows {
                for (column, cell) in row.iter().enumerate() {
                    let before = if column == 0 {
                        Before::Line
                    } else {
                        Before::Cell
                    };
                    inlines(&mut pieces, before, cell);
                }
            }
        }
        // The line break that ends the code or the HTML is not part of it:
        // the break between blocks is.
        Block::CodeBlock { code, .. } => pieces.push(Piece::line(without_last_break(code))),
        Block::Html(source) => pieces.push(Piece::line(without_last_break(source))),
        Block::Alert {
            title: Some(title), ..
        } => pieces.push(Piece::line(title.as_str())),
        Block::FrontMatter(front_matter) => {
            if front_matter.source.trim().is_empty() {
                return pieces;
            }
            match &front_matter.value {
                Some(Ok(Value::Map(entries))) => {
                    leaves_of(&grid::front_matter_grid(entries, &Texts), &mut pieces);
                }
                // Another language, content that does not parse, or
                // content that is not a map: the layout shows it as code.
                _ => pieces.push(Piece::line(without_last_break(&front_matter.source))),
            }
        }
        Block::Alert { title: None, .. }
        | Block::BlockQuote(_)
        | Block::List(_)
        | Block::ThematicBreak
        | Block::Note(_)
        | Block::Error(_)
        | Block::ImageFile(_) => {}
    }
    pieces
}

/// The texts of `block`.
pub fn pieces(block: &Block) -> Vec<Piece> {
    pieces_with(block, &mut |_, _, _| {})
}

fn without_last_break(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

/// Front matter's leaves as texts, as the table of its keys and values has
/// them.
struct Texts;

impl Leaves<String> for Texts {
    fn key(&self, key: &str) -> String {
        key.to_owned()
    }

    fn text(&self, text: &str) -> String {
        text.to_owned()
    }

    fn empty(&self) -> String {
        String::new()
    }

    fn yaml(&self, yaml: &str) -> String {
        without_last_break(yaml).to_owned()
    }
}

/// The leaves of `grid` as texts, a row at a time: the first leaf of a row
/// after a line break, the others after a tab. A grid in a cell starts its
/// rows on lines of their own, and so does each of the cells one below
/// another.
fn leaves_of(grid: &Grid<String>, pieces: &mut Vec<Piece>) {
    for row in &grid.rows {
        for (column, cell) in row.iter().enumerate() {
            let before = if column == 0 {
                Before::Line
            } else {
                Before::Cell
            };
            cell_leaves(cell, before, pieces);
        }
    }
}

fn cell_leaves(cell: &Cell<String>, before: Before, pieces: &mut Vec<Piece>) {
    match cell {
        Cell::Leaf(text) => pieces.push(Piece {
            before,
            text: text.clone(),
        }),
        Cell::Table(grid) => leaves_of(grid, pieces),
        Cell::Stack(cells) => {
            for (index, cell) in cells.iter().enumerate() {
                let before = if index == 0 { before } else { Before::Line };
                cell_leaves(cell, before, pieces);
            }
        }
    }
}

/// The texts of the blocks of the shown documents, by file and path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentText {
    /// By file, each block with its path, in the order of the paths.
    files: Vec<Vec<(Vec<usize>, Vec<Piece>)>>,
}

impl DocumentText {
    /// The text of `files`, shown in this order: file 0 is the first.
    pub fn new<'a>(files: impl IntoIterator<Item = &'a Document>) -> Self {
        Self {
            files: (files.into_iter())
                .map(|document| {
                    (document.blocks().into_iter())
                        .map(|(path, block)| (path, pieces(block)))
                        .collect()
                })
                .collect(),
        }
    }

    /// The texts of the block at `path` of file `file`; none for a block
    /// that has none, or one that is not there.
    pub fn texts(&self, file: usize, path: &[usize]) -> &[Piece] {
        let Some(blocks) = self.files.get(file) else {
            return &[];
        };
        // The blocks are in the order of the paths, which is the order of
        // `Vec`s compared as lists.
        match blocks.binary_search_by(|(at, _)| at.as_slice().cmp(path)) {
            Ok(index) => &blocks[index].1,
            Err(_) => &[],
        }
    }

    /// Every place there is to name in the shown documents, in the order of
    /// the documents: for each block, itself and the start of each of its
    /// texts, and its end after the blocks inside it.
    pub fn positions(&self) -> impl Iterator<Item = Pos> + '_ {
        self.files.iter().enumerate().flat_map(|(file, blocks)| {
            let mut open: Vec<&[usize]> = Vec::new();
            let mut out = Vec::new();
            for (path, pieces) in blocks {
                while let Some(last) = open.last().filter(|last| !path.starts_with(last)) {
                    out.push(Pos::end(file, last));
                    open.pop();
                }
                out.push(Pos::block(file, path));
                out.extend((0..pieces.len()).map(|text| Pos {
                    file,
                    path: path.clone(),
                    within: Within::Text { text, offset: 0 },
                }));
                open.push(path);
            }
            while let Some(last) = open.pop() {
                out.push(Pos::end(file, last));
            }
            out
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::parse::{code, parse};

    /// The texts of the first block of `source`, as `before text` strings,
    /// with `\t` and `\n` for the two kinds of before.
    fn texts_of(source: &str) -> Vec<String> {
        let document = parse(source);
        pieces(&document.blocks[0])
            .into_iter()
            .map(|piece| {
                let before = match piece.before {
                    Before::Line => "\n",
                    Before::Cell => "\t",
                };
                format!("{before}{}", piece.text)
            })
            .collect()
    }

    #[test]
    fn a_paragraph_and_a_heading_have_one_text_with_the_inlines_flattened() {
        assert_eq!(
            texts_of("one *two* **three** [four](u)\n"),
            ["\none two three four"]
        );
        assert_eq!(texts_of("## A `b` c\n"), ["\nA b c"]);
        assert_eq!(texts_of("![alt *text*](a.png) end\n"), ["\nalt text end"]);
        assert_eq!(texts_of("the $x^2$ and $`y`$ z\n"), ["\nthe x^2 and y z"]);
        assert_eq!(texts_of("a\nb  \nc\n"), ["\na b\nc"]);
        assert_eq!(
            texts_of("a <kbd>b</kbd> <i>c</i> <u>d</u>\n"),
            ["\na b c <u>d</u>"]
        );
    }

    #[test]
    fn a_code_block_has_its_code_without_the_last_line_break_and_not_its_language() {
        assert_eq!(
            texts_of("```rust\nfn a() {}\n\nb\n```\n"),
            ["\nfn a() {}\n\nb"]
        );
        assert_eq!(texts_of("```\n\n```\n"), ["\n"]);
        let file = code("one\ntwo\n", Some("txt"));
        assert_eq!(pieces(&file.blocks[0]), [Piece::line("one\ntwo")]);
    }

    #[test]
    fn a_table_has_a_text_for_each_cell_and_a_tab_between_those_of_a_row() {
        let source = "| a | *b* |\n|---|---|\n| 1 | 2 |\n| 3 |\n";
        assert_eq!(texts_of(source), ["\na", "\tb", "\n1", "\t2", "\n3", "\t"]);
    }

    #[test]
    fn details_have_the_summary_and_an_alert_its_title_and_html_its_source() {
        let source = "<details>\n<summary>The <b>sum</b></summary>\n\nBody\n\n</details>\n";
        assert_eq!(texts_of(source), ["\nThe sum"]);
        assert_eq!(texts_of("> [!NOTE] A title\n> text\n"), ["\nA title"]);
        assert!(texts_of("> [!NOTE]\n> text\n").is_empty());
        assert_eq!(texts_of("<div>\nraw\n</div>\n"), ["\n<div>\nraw\n</div>"]);
    }

    #[test]
    fn blocks_that_hold_blocks_and_blocks_without_text_have_none() {
        for source in ["> quote\n", "- a\n- b\n", "---\n"] {
            assert!(texts_of(source).is_empty(), "{source:?}");
        }
        for block in [
            Block::Note("N rows are left out".into()),
            Block::Error("cannot be shown".into()),
            Block::ImageFile("a.png".into()),
        ] {
            assert!(pieces(&block).is_empty());
        }
    }

    #[test]
    fn front_matter_has_its_keys_and_values_as_the_table_shows_them() {
        let source = "---\ntitle: One\ntags: [a, b]\nowner:\nnone: {}\nmeta:\n  year: 2026\n  deep:\n    more: 1\nstops:\n  - name: A\n    km: 0\n  - plain\n---\n";
        assert_eq!(
            texts_of(source),
            [
                "\ntitle", "\tOne", "\ntags", "\ta, b", "\nowner", "\t", "\nnone", "\t", "\nmeta",
                "\nyear", "\t2026", "\ndeep", "\nmore", "\t1", "\nstops", "\nname", "\tA", "\nkm",
                "\t0", "\nplain",
            ]
        );
    }

    #[test]
    fn front_matter_that_is_not_a_map_is_its_source_and_an_empty_one_has_no_text() {
        assert_eq!(texts_of("---\n- a\n- b\n---\n"), ["\n- a\n- b"]);
        assert_eq!(texts_of("---php\n$a = 1;\n---\n"), ["\n$a = 1;"]);
        assert_eq!(texts_of("---\n: [\n---\n"), ["\n: ["]);
        assert!(texts_of("---\n---\n").is_empty());
        assert!(texts_of("---\n\n---\n").is_empty());
    }

    #[test]
    fn the_texts_of_a_block_come_by_file_and_path_and_a_missing_one_has_none() {
        let first = parse("# A\n\n> b\n>\n> - c\n");
        let second = parse("d\n");
        let text = DocumentText::new([&first, &second]);
        assert_eq!(text.texts(0, &[0]), [Piece::line("A")]);
        assert_eq!(text.texts(0, &[1, 1, 0, 0]), [Piece::line("c")]);
        assert_eq!(text.texts(1, &[0]), [Piece::line("d")]);
        assert!(text.texts(0, &[1]).is_empty());
        assert!(text.texts(0, &[7]).is_empty());
        assert!(text.texts(2, &[0]).is_empty());
    }

    #[test]
    fn the_places_of_the_fixtures_and_examples_only_grow() {
        let documents: Vec<Document> = crate::testing::documents()
            .iter()
            .map(|path| parse(&std::fs::read_to_string(path).unwrap()))
            .collect();
        let grows = |text: &DocumentText, what: &str| {
            let all: Vec<Pos> = text.positions().collect();
            assert!(all.len() > 3, "{what}");
            for pair in all.windows(2) {
                assert!(pair[0] < pair[1], "{what}: {:?} {:?}", pair[0], pair[1]);
            }
            all.len()
        };
        let mut together = 0;
        for (index, document) in documents.iter().enumerate() {
            together += grows(&DocumentText::new([document]), &format!("document {index}"));
        }
        assert_eq!(
            grows(&DocumentText::new(&documents), "all documents"),
            together
        );
    }

    #[test]
    fn the_places_of_a_block_are_itself_its_texts_and_its_end_after_the_blocks_inside() {
        let document = parse("> a\n>\n> b\n\nc\n");
        let text = DocumentText::new([&document]);
        let places: Vec<(Vec<usize>, Within)> =
            text.positions().map(|p| (p.path, p.within)).collect();
        let text_start = |text| Within::Text { text, offset: 0 };
        assert_eq!(
            places,
            [
                (vec![0], Within::Block),
                (vec![0, 0], Within::Block),
                (vec![0, 0], text_start(0)),
                (vec![0, 0], Within::End),
                (vec![0, 1], Within::Block),
                (vec![0, 1], text_start(0)),
                (vec![0, 1], Within::End),
                (vec![0], Within::End),
                (vec![1], Within::Block),
                (vec![1], text_start(0)),
                (vec![1], Within::End),
            ]
        );
    }
}
