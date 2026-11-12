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

/// The bytes of each place of `term` in `text`, from left to right and
/// none overlapping another.
pub fn places(text: &str, term: &[char], ignore_case: bool) -> Vec<Range<usize>> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut found = Vec::new();
    if term.is_empty() {
        return found;
    }
    let mut start = 0;
    while start + term.len() <= chars.len() {
        let candidate = &chars[start..start + term.len()];
        let matches = candidate
            .iter()
            .zip(term)
            .all(|(&(_, c), &t)| same(c, t, ignore_case));
        if matches {
            let end = chars
                .get(start + term.len())
                .map_or(text.len(), |&(at, _)| at);
            found.push(chars[start].0..end);
            start += term.len();
        } else {
            start += 1;
        }
    }
    found
}

/// Whether the characters `a` and `b` match: a no-break space matches a
/// space, and with `ignore_case` an upper-case letter its lower case.
fn same(a: char, b: char, ignore_case: bool) -> bool {
    let plain = |c| if c == '\u{a0}' { ' ' } else { c };
    let (a, b) = (plain(a), plain(b));
    a == b || (ignore_case && a.to_lowercase().eq(b.to_lowercase()))
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

    /// The places of `term` in the texts, from the start and from left to
    /// right, none overlapping another. A term is found in one text: it is
    /// not found across the cells of a table or across blocks, and is found
    /// across a soft break, which is a space. A no-break space matches a
    /// space; with `ignore_case`, a letter matches its other case. The
    /// bytes of a place are those of the original text.
    pub fn find(&self, term: &str, ignore_case: bool) -> Vec<Range<Pos>> {
        let term: Vec<char> = term.chars().collect();
        let mut found = Vec::new();
        for (file, blocks) in self.files.iter().enumerate() {
            for (path, pieces) in blocks {
                for (text, piece) in pieces.iter().enumerate() {
                    for bytes in places(&piece.text, &term, ignore_case) {
                        found.push(
                            Pos::text(file, path, text, bytes.start)
                                ..Pos::text(file, path, text, bytes.end),
                        );
                    }
                }
            }
        }
        found
    }

    /// The text between the two ends of `range`, as a copy would give it:
    /// a line break between texts that are on lines of their own, and between
    /// blocks and files, and a tab between the cells of a row. A range that
    /// is empty or backwards gives nothing. A place that is in the middle of
    /// a character, or past the end of a text, is moved to the nearest
    /// place there is.
    pub fn text(&self, range: &Range<Pos>) -> String {
        let mut out = String::new();
        let mut started = false;
        if range.start >= range.end {
            return out;
        }
        for (file, blocks) in self.files.iter().enumerate().skip(range.start.file) {
            if file > range.end.file {
                break;
            }
            for (path, pieces) in blocks {
                for (text, piece) in pieces.iter().enumerate() {
                    let first = Pos::text(file, path, text, 0);
                    let last = Pos::text(file, path, text, piece.text.len());
                    // A text is in the range when some of it is, or when it
                    // is empty and starts in it.
                    let some_of_it = match piece.text.is_empty() {
                        true => first >= range.start && first < range.end,
                        false => last > range.start && first < range.end,
                    };
                    if !some_of_it {
                        continue;
                    }
                    let from = Self::offset(&range.start, &first, &piece.text).unwrap_or(0);
                    let to =
                        Self::offset(&range.end, &first, &piece.text).unwrap_or(piece.text.len());
                    if started {
                        out.push(match piece.before {
                            Before::Line => '\n',
                            Before::Cell => '\t',
                        });
                    }
                    started = true;
                    out.push_str(&piece.text[from..to.max(from)]);
                }
            }
        }
        out
    }

    /// The byte of `text` that `at` names, moved back to a character, if
    /// `at` is a place in the text whose first place is `first`.
    fn offset(at: &Pos, first: &Pos, text: &str) -> Option<usize> {
        match (&at.within, &first.within) {
            (
                Within::Text {
                    text: number,
                    offset,
                },
                Within::Text { text: mine, .. },
            ) if (at.file, &at.path, number) == (first.file, &first.path, mine) => {
                let mut offset = (*offset).min(text.len());
                while !text.is_char_boundary(offset) {
                    offset -= 1;
                }
                Some(offset)
            }
            _ => None,
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

    fn text_of_files(sources: &[&str]) -> (DocumentText, Vec<Document>) {
        let documents: Vec<Document> = sources.iter().map(|source| parse(source)).collect();
        (DocumentText::new(&documents), documents)
    }

    /// The matches of `term` in `sources`, as `file/path:text@bytes`.
    fn found(sources: &[&str], term: &str, ignore_case: bool) -> Vec<String> {
        let (text, _) = text_of_files(sources);
        (text.find(term, ignore_case).into_iter())
            .map(|range| {
                let (Within::Text { text: t, offset: a }, Within::Text { offset: b, .. }) =
                    (&range.start.within, &range.end.within)
                else {
                    panic!("{range:?}");
                };
                assert_eq!(
                    (range.start.file, &range.start.path),
                    (range.end.file, &range.end.path)
                );
                format!("{}/{:?}:{t}@{a}..{b}", range.start.file, range.start.path)
            })
            .collect()
    }

    #[test]
    fn a_term_is_found_in_every_kind_of_text_and_not_in_what_is_not_text() {
        let source = [
            "---\nkey: needle value\nneedle: x\n---\n",
            "# needle heading\n",
            "paragraph needle\n",
            "| h |  |\n|---|--|\n| needle | 2 |\n",
            "<details>\n<summary>needle sum</summary>\n\nBody\n\n</details>\n",
            "> [!NOTE] needle title\n> text\n",
            "```needle\ncode needle\n```\n",
            "<div>needle</div>\n",
        ]
        .join("\n");
        assert_eq!(
            found(&[&source], "needle", false),
            [
                "0/[0]:1@0..6",
                "0/[0]:2@0..6",
                "0/[1]:0@0..6",
                "0/[2]:0@10..16",
                "0/[3]:2@0..6",
                "0/[4]:0@0..6",
                "0/[5]:0@0..6",
                "0/[6]:0@5..11",
                "0/[7]:0@5..11",
            ]
        );
    }

    #[test]
    fn kiris_own_notes_and_errors_are_not_found() {
        let document = Document::new(vec![
            Block::Note("needle".into()),
            Block::Error("needle".into()),
        ]);
        assert!(
            DocumentText::new([&document])
                .find("needle", false)
                .is_empty()
        );
    }

    #[test]
    fn a_term_is_found_across_a_soft_break_and_not_across_a_cell_or_a_block() {
        assert_eq!(found(&["one\ntwo\n"], "one two", false), ["0/[0]:0@0..7"]);
        assert!(found(&["| a | b |\n|---|---|\n"], "a b", false).is_empty());
        assert!(found(&["a\n\nb\n"], "a b", false).is_empty());
        assert!(found(&["a\n\nb\n"], "a\nb", false).is_empty());
    }

    #[test]
    fn the_case_and_the_no_break_space_follow_the_search() {
        // `İ` is two bytes and lower-cases to two characters.
        assert_eq!(
            found(&["İ Äiti äiti\n"], "äiti", true),
            ["0/[0]:0@3..8", "0/[0]:0@9..14"]
        );
        assert_eq!(found(&["İ Äiti äiti\n"], "äiti", false), ["0/[0]:0@9..14"]);
        assert_eq!(
            found(&["run\u{a0}make\u{a0}check\n"], "make check", false),
            ["0/[0]:0@5..16"]
        );
        assert_eq!(
            found(&["aaaa\n"], "aa", false),
            ["0/[0]:0@0..2", "0/[0]:0@2..4"]
        );
        assert!(found(&["a\n"], "", false).is_empty());
    }

    #[test]
    fn matches_come_in_the_order_of_the_documents() {
        let found = found(&["x\n\n> x\n", "x\n"], "x", false);
        assert_eq!(found, ["0/[0]:0@0..1", "0/[1, 0]:0@0..1", "1/[0]:0@0..1"]);
    }

    /// The text between the places `a` and `b`.
    fn between(sources: &[&str], a: Pos, b: Pos) -> String {
        text_of_files(sources).0.text(&(a..b))
    }

    #[test]
    fn the_text_of_a_range_inside_one_text_is_a_part_of_it() {
        let at = |offset| Pos::text(0, &[0], 0, offset);
        assert_eq!(between(&["hello world\n"], at(2), at(7)), "llo w");
        assert_eq!(between(&["hello world\n"], at(0), at(11)), "hello world");
    }

    #[test]
    fn the_text_of_a_range_across_blocks_has_a_line_break_between_them() {
        let sources = ["first\n\nsecond\n\nthird\n"];
        let a = Pos::text(0, &[0], 0, 2);
        assert_eq!(
            between(&sources, a.clone(), Pos::text(0, &[2], 0, 3)),
            "rst\nsecond\nthi"
        );
        assert_eq!(between(&sources, a, Pos::block(0, &[2])), "rst\nsecond");
    }

    #[test]
    fn the_text_of_a_range_across_a_table_has_tabs_between_cells_and_lines_between_rows() {
        let sources = ["| a | b |\n|---|---|\n| c | d |\n"];
        assert_eq!(
            between(&sources, Pos::block(0, &[0]), Pos::end(0, &[0])),
            "a\tb\nc\td"
        );
        assert_eq!(
            between(&sources, Pos::text(0, &[0], 1, 0), Pos::text(0, &[0], 3, 1)),
            "b\nc\td"
        );
    }

    #[test]
    fn the_text_of_a_range_across_the_border_of_files_and_the_ends_of_blocks() {
        let sources = ["a\n\n> b\n", "c\n"];
        let all = between(&sources, Pos::block(0, &[0]), Pos::end(1, &[0]));
        assert_eq!(all, "a\nb\nc");
        // The end of a quote is after what is in it, and its start before.
        assert_eq!(
            between(&sources, Pos::block(0, &[1]), Pos::end(0, &[1])),
            "b"
        );
        assert_eq!(
            between(&sources, Pos::end(0, &[0]), Pos::block(1, &[0])),
            "b"
        );
    }

    #[test]
    fn the_text_of_a_range_over_a_block_without_text_is_empty_and_so_is_a_range_with_no_extent() {
        let sources = ["a\n\n---\n\nb\n"];
        assert_eq!(
            between(&sources, Pos::block(0, &[1]), Pos::end(0, &[1])),
            ""
        );
        assert_eq!(
            between(&sources, Pos::block(0, &[0]), Pos::end(0, &[2])),
            "a\nb"
        );
        let here = Pos::text(0, &[0], 0, 0);
        assert_eq!(between(&sources, here.clone(), here), "");
        assert_eq!(
            between(&sources, Pos::end(0, &[2]), Pos::block(0, &[0])),
            "",
            "backwards"
        );
    }

    #[test]
    fn a_place_in_the_middle_of_a_character_or_past_the_end_is_moved_to_one_there_is() {
        let at = |offset| Pos::text(0, &[0], 0, offset);
        // `ä` and `好` take two and three bytes.
        assert_eq!(between(&["ä好b\n"], at(1), at(5)), "ä好");
        assert_eq!(between(&["ä好b\n"], at(2), at(100)), "好b");
        assert_eq!(
            between(&["ä好b\n"], at(0), Pos::text(0, &[0], 7, 0)),
            "ä好b"
        );
    }

    #[test]
    fn an_empty_cell_in_the_middle_of_a_row_keeps_its_tabs() {
        let sources = ["| a | | c |\n|---|---|---|\n"];
        assert_eq!(
            between(&sources, Pos::block(0, &[0]), Pos::end(0, &[0])),
            "a\t\tc"
        );
    }

    /// Words that a layout adds to a document, which are not its text:
    /// what comes with a list item (a bullet, a number such as `3.`), a
    /// task's box, the frame of a table or a code block and a heading's rule
    /// (the marks that layouts draw them with, and nothing else), the name
    /// of an alert's kind, and the label of a code block's language
    /// (`languages` of the document). Three more are told apart by their
    /// shape and need the text of the block (`block`, the text of the block
    /// the line is of and of the blocks inside it): the header of a column
    /// and a colon before a value when a table is shown as a card for each
    /// row, the brackets and the word `image` around the alt text of an
    /// image that is not drawn, and a formula that is not drawn, which shows
    /// as it is written between dollars. Each only when the document does
    /// not have the word as text.
    fn is_added_by_the_layout(block: &str, word: &str, languages: &[&str]) -> bool {
        let marker = word.strip_suffix('.').unwrap_or(word);
        let number = !marker.is_empty() && marker.chars().all(|c| c.is_ascii_digit());
        let kind = ["Note", "Tip", "Important", "Warning", "Caution"].contains(&word);
        // Box drawing, block elements and geometric shapes, such as the arrow of a
        // folded block, the icon of an alert (a private use character), the bullets of lists and the
        // boxes of tasks.
        let mark = |c: char| {
            ('\u{2500}'..='\u{25ff}').contains(&c)
                || ('\u{e000}'..='\u{f8ff}').contains(&c)
                || "•◦▪‣⁃☐☑☒".contains(c)
        };
        let drawn = word.chars().all(mark);
        let label = (word.strip_suffix(':')).is_some_and(|name| block.contains(name));
        // An image that is not drawn shows as `[image: alt text]`.
        let image = word == "[image:"
            || (word.contains(']') && block.contains(&word.replace(['[', ']'], "")));
        // A formula that is not drawn shows as it is written between
        // dollars, as Markdown reads it, where `\\` is `\`: each part
        // between backslashes has to be in the text of the block.
        let written = word.contains('$')
            && (word.replace('$', "").split('\\')).all(|part| block.contains(part));
        number || kind || drawn || label || image || written || languages.contains(&word)
    }

    #[test]
    fn every_word_a_layout_shows_is_in_the_text_of_the_document() {
        use crate::render::Inputs;
        use crate::typeset::caps::Capabilities;
        use crate::typeset::layout::{Content, Stream, content};
        use crate::typeset::style;
        let inputs = Inputs::new(Capabilities::kitty(200));
        let mut checked = 0;
        for path in crate::testing::documents() {
            let document = parse(&std::fs::read_to_string(&path).unwrap());
            let text = DocumentText::new([&document]);
            let languages: Vec<&str> = (document.blocks().into_iter())
                .filter_map(|(_, block)| match block {
                    Block::CodeBlock { language, .. } => language.as_deref(),
                    Block::FrontMatter(front_matter) => Some(front_matter.language.as_str()),
                    _ => None,
                })
                .collect();
            let mut missing = Vec::new();
            let Stream { lines, blocks } = content(Content::Document(&document), &inputs);
            // The text of a block and of the blocks inside it.
            let block_text = |path: &[usize]| -> String {
                (document.blocks().into_iter())
                    .filter(|(at, _)| at.starts_with(path))
                    .flat_map(|(at, _)| text.texts(0, &at).iter().map(|piece| piece.text.clone()))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            for line in &lines {
                let block = line
                    .block
                    .map(|id| block_text(blocks.get(id).path()))
                    .unwrap_or_default();
                for word in style::text(&line.spans).split_whitespace() {
                    checked += 1;
                    if text.find(word, false).is_empty()
                        && !is_added_by_the_layout(&block, word, &languages)
                    {
                        missing.push(word.to_owned());
                    }
                }
            }
            assert!(missing.is_empty(), "{}: {missing:?}", path.display());
        }
        assert!(checked > 5000, "{checked}");
    }
}
