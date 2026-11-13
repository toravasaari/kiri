//! The driver of the layout: it walks what is to be laid out, one
//! document or several files, and gives back the lines, with the table of
//! the blocks they come from.
//!
//! The layout of the blocks themselves is the cell layout engine's
//! (`src/typeset/layout/`) or reading mode's (`src/typeset/reading/`).

use crate::render::Inputs;
use crate::typeset::block::Blocks;
use crate::typeset::layout::{append_block, with_cells};
use crate::typeset::model::{Block, Document, File};
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::pen::Pixels;
use crate::typeset::style::Line;
use crate::typeset::wrap::width;

/// `document` laid out, without the table of its blocks, as [`file`] has
/// it.
pub fn layout(document: &Document, inputs: &Inputs) -> Vec<Line> {
    document_stream(document, inputs).lines
}

/// `document` laid out as a lone document, with the blocks its lines come
/// from.
fn document_stream(document: &Document, inputs: &Inputs) -> Stream {
    let mut blocks = Blocks::default();
    let lines = lay_out(inputs, 0, &mut blocks, None, document);
    end(inputs);
    Stream { lines, blocks }
}

/// What a piece is: a block, or a part of one that the engines treat as a
/// block of its own.
#[derive(Clone, Copy)]
pub(crate) enum Shape<'a> {
    /// A block as a whole.
    Block(&'a Block),
    /// The summary of the `<details>` block; its blocks are pieces of
    /// their own.
    Summary(&'a Block),
    /// The header of a file, when files are shown together.
    Header(&'a str),
}

/// A piece of the document: a block, or the summary of a `<details>`, or a
/// file's header, which the lines of the layout mark as their own
/// (`Line::block`), with its place in the document.
pub(crate) struct Piece<'a> {
    pub shape: Shape<'a>,
    /// The place of the piece's block in the document, as the table of
    /// blocks has it; empty for a header.
    pub path: Vec<usize>,
}

/// An engine's pen: what lays the pieces out in cells or in pixels, and
/// decides where its flow can be cut.
pub(crate) trait Pen<'d> {
    /// Lays `piece` out, and stacks what the pen decides to stack now.
    fn piece(&mut self, piece: Piece<'d>, stack: &mut Stack);
    /// Cuts the flow: the pieces the pen holds are laid out and stacked.
    fn cut(&mut self, stack: &mut Stack);
}

/// The lines of the pieces of a file, a blank line between the pieces.
#[derive(Default)]
pub(crate) struct Stack {
    lines: Vec<Line>,
}

impl Stack {
    /// Adds the lines of a piece, after a blank line if something came
    /// before. A piece without lines, such as an empty code block, gets no
    /// separator either.
    pub(crate) fn push(&mut self, lines: Vec<Line>) {
        append_block(&mut self.lines, lines, true);
    }
}

/// The engine that a layout is made in.
#[derive(Clone, Copy)]
enum Engine<'a> {
    Cells,
    Pixels(&'a Reading),
}

impl<'a> Engine<'a> {
    /// Reading mode where the terminal draws images, which its text is;
    /// the cell layout otherwise.
    fn of(inputs: &'a Inputs) -> Self {
        match &inputs.reading {
            Some(reading) if inputs.caps.draws_images() => Engine::Pixels(reading),
            _ => Engine::Cells,
        }
    }
}

/// The lines of the document `document`, numbered `number` among those
/// shown together, under a header that names the file when `header` is
/// given, with the blocks added to `blocks`.
fn lay_out<'d>(
    inputs: &Inputs,
    number: usize,
    blocks: &mut Blocks,
    header: Option<&'d str>,
    document: &'d Document,
) -> Vec<Line> {
    with_cells(inputs, number, blocks, |cells| match Engine::of(inputs) {
        Engine::Cells => walk(cells, header, document),
        Engine::Pixels(reading) => walk(Pixels::new(cells, reading), header, document),
    })
}

/// The pieces of a file in the order of the document, laid out by `pen`
/// and stacked, with each band settled.
fn walk<'d>(mut pen: impl Pen<'d>, header: Option<&'d str>, document: &'d Document) -> Vec<Line> {
    let mut stack = Stack::default();
    if let Some(name) = header {
        let (shape, path) = (Shape::Header(name), Vec::new());
        pen.piece(Piece { shape, path }, &mut stack);
    }
    pieces(&mut pen, &document.blocks, &mut Vec::new(), &mut stack);
    pen.cut(&mut stack);
    let mut lines = stack.lines;
    settle_bands(&mut lines);
    lines
}

/// The pieces of `blocks`, at `path`: a block, or the summary of a
/// `<details>` and then the pieces of its blocks.
fn pieces<'d>(
    pen: &mut impl Pen<'d>,
    blocks: &'d [Block],
    path: &mut Vec<usize>,
    stack: &mut Stack,
) {
    for (index, block) in blocks.iter().enumerate() {
        path.push(index);
        match block {
            Block::Details { blocks, .. } => {
                let (shape, path_of) = (Shape::Summary(block), path.clone());
                pen.piece(
                    Piece {
                        shape,
                        path: path_of,
                    },
                    stack,
                );
                pieces(pen, blocks, path, stack);
            }
            _ => {
                let (shape, path_of) = (Shape::Block(block), path.clone());
                pen.piece(
                    Piece {
                        shape,
                        path: path_of,
                    },
                    stack,
                );
            }
        }
        path.pop();
    }
}

/// Puts each band in `lines` where it is drawn: on the blank line after
/// its block when there is one, a little below its top, so that the band
/// takes no row and nothing moves when the image comes; otherwise over the
/// bottom of the block's last line. It starts where the block's text or
/// image does.
fn settle_bands(lines: &mut [Line]) {
    let marked: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].loading.is_some())
        .collect();
    for index in marked {
        let Some(mut loading) = lines[index].loading.take() else {
            continue;
        };
        let line = &lines[index];
        let mut indent = width(&line.lead);
        if line.image.is_some() {
            indent += width(&line.spans);
        }
        let blank = (lines.get(index + 1)).is_some_and(|below| {
            below.is_blank() && below.lead.is_empty() && below.block.is_none()
        });
        let at = if blank { index + 1 } else { index };
        loading.above += usize::from(blank);
        if let Some(band) = &mut loading.band {
            (band.indent, band.low) = (indent + band.indent, !blank);
        }
        lines[at].loading = Some(loading);
    }
}

/// What is laid out: one document, or several files one after another.
#[derive(Debug, Clone, Copy)]
pub enum Content<'a> {
    Document(&'a Document),
    Files(&'a [File]),
}

/// `content` laid out with `inputs`.
pub fn content(content: Content, inputs: &Inputs) -> Stream {
    match content {
        Content::Document(document) => document_stream(document, inputs),
        Content::Files(shown) => files(shown, inputs),
    }
}

/// Ends a layout: the pictures of reading mode that it did not show are
/// let go.
fn end(inputs: &Inputs) {
    inputs.images.end_notes();
    if let Some(reading) = &inputs.reading {
        reading.pictures.borrow_mut().finish();
    }
}

/// Lines laid out, with the blocks they come from.
#[derive(Debug, Default)]
pub struct Stream {
    pub lines: Vec<Line>,
    /// The blocks that the lines' [`Line::block`] numbers.
    pub blocks: Blocks,
}

/// `files` laid out one after another, a blank line apart, each as
/// [`file`] lays it out. The blank line between two files has no block,
/// as the blank lines between blocks have none.
pub fn files(files: &[File], inputs: &Inputs) -> Stream {
    let mut stream = Stream::default();
    for (number, shown) in files.iter().enumerate() {
        if number > 0 {
            stream.lines.push(Line::blank());
        }
        let lines = file_into(shown, number, inputs, &mut stream.blocks);
        stream.lines.extend(lines);
    }
    end(inputs);
    stream
}

/// `file`, numbered `number` among the files shown together, laid out
/// under a header that names it, as [`Cells`] draws it, and a blank line.
/// An image file below it is fitted to the window with its header, however
/// many rows a long name takes. The table of its blocks is not kept, so the
/// block numbers of its lines point into nothing; [`files`] keeps the table.
pub fn file(file: &File, number: usize, inputs: &Inputs) -> Vec<Line> {
    let lines = file_into(file, number, inputs, &mut Blocks::default());
    end(inputs);
    lines
}

/// [`file`], with its blocks added to `blocks`. The header lines come from
/// a block of their own, which has no path.
fn file_into(file: &File, number: usize, inputs: &Inputs, blocks: &mut Blocks) -> Vec<Line> {
    lay_out(inputs, number, blocks, Some(&file.name), &file.document)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::model::Block;
    use crate::typeset::parse::parse;
    use crate::typeset::style::text;
    use std::rc::Rc;

    fn inputs() -> Inputs {
        Inputs::new(Capabilities::kitty(60))
    }

    fn lone(source: &str) -> Stream {
        content(Content::Document(&parse(source)), &inputs())
    }

    fn file_of(name: &str, source: &str) -> File {
        File {
            name: name.to_owned(),
            document: parse(source),
        }
    }

    /// Each line's text with the path of its block: `None` for a line of
    /// no block, and an empty path for the header of a file.
    fn marked(stream: &Stream) -> Vec<(String, Option<Vec<usize>>)> {
        let table = &stream.blocks;
        stream
            .lines
            .iter()
            .map(|line| {
                let path = line.block.map(|id| table.path(id).to_vec());
                (text(&line.spans), path)
            })
            .collect()
    }

    /// The paths of the blocks of the lines that have text, one for each
    /// run of lines of a block, skipping the blank ones.
    fn paths(stream: &Stream) -> Vec<Vec<usize>> {
        let mut paths: Vec<Vec<usize>> = Vec::new();
        for (text, path) in marked(stream) {
            let Some(path) = path else { continue };
            if !text.trim().is_empty() && paths.last() != Some(&path) {
                paths.push(path);
            }
        }
        paths
    }

    #[test]
    fn a_block_marks_all_its_lines_and_the_blank_between_blocks_none() {
        let stream = lone("One\n\n- a\n\n  b\n\nTwo\n");
        let lines = marked(&stream);
        let blocks: Vec<_> = lines
            .iter()
            .map(|(_, path)| path.as_ref().map(|path| path[0]))
            .collect();
        // The blank line between the two paragraphs of the loose list item
        // is inside the list, so it keeps the list's block.
        let a = lines
            .iter()
            .position(|(text, _)| text.contains('a'))
            .unwrap();
        assert!(stream.lines[a + 1].is_blank(), "{lines:?}");
        assert_eq!(blocks[a + 1], Some(1), "{lines:?}");
        // Blank separators between blocks have no block.
        assert_eq!(blocks.iter().filter(|block| block.is_none()).count(), 2);
        assert_eq!(paths(&stream), [[0], [1], [2]]);
        // Only blank lines go without a block.
        for (text, path) in &lines {
            if path.is_none() {
                assert!(text.is_empty(), "{lines:?}");
            }
        }
    }

    #[test]
    fn two_lines_of_a_block_are_the_same_block_and_two_blocks_are_not() {
        let stream = lone(
            "One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen\n\nNext\n",
        );
        let ids: Vec<_> = stream.lines.iter().filter_map(|line| line.block).collect();
        assert!(ids.len() >= 3, "{ids:?}");
        assert_eq!(ids[0], ids[1]);
        assert_ne!(ids[0], ids[ids.len() - 1]);
    }

    #[test]
    fn a_details_summary_is_the_details_block_and_its_content_is_deeper() {
        let stream = lone(
            "Before\n\n<details>\n<summary>Outer</summary>\n\nIn outer\n\n\
             <details>\n<summary>Inner</summary>\n\nDeep\n\n</details>\n\n\
             Back in outer\n\n</details>\n\nAfter\n",
        );
        assert_eq!(
            paths(&stream),
            [
                vec![0],
                vec![1],
                vec![1, 0],
                vec![1, 1],
                vec![1, 1, 0],
                vec![1, 2],
                vec![2]
            ],
            "{:?}",
            marked(&stream)
        );
        let lines = marked(&stream);
        let path = |find: &str| {
            let (_, path) = lines.iter().find(|(text, _)| text.contains(find)).unwrap();
            path.clone()
        };
        assert_eq!(path("Outer"), Some(vec![1]));
        assert_eq!(path("Inner"), Some(vec![1, 1]));
        assert_eq!(path("Deep"), Some(vec![1, 1, 0]));
    }

    #[test]
    fn a_details_in_reading_mode_keeps_a_place_for_its_summary_and_each_block_in_it() {
        let source = "Before\n\n<details>\n<summary>Outer</summary>\n\nIn outer\n\n\
                      <details>\n<summary>Inner</summary>\n\nDeep\n\n</details>\n\n\
                      Back in outer\n\n</details>\n\nAfter\n";
        let inputs = Inputs {
            reading: Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Wrap,
            }),
            caps: Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            },
            ..Inputs::new(Capabilities::kitty(80))
        };
        let stream = content(Content::Document(&parse(source)), &inputs);
        let mut seen = Vec::new();
        for id in stream.lines.iter().filter_map(|line| line.block) {
            let path = stream.blocks.path(id).to_vec();
            if seen.last() != Some(&path) {
                seen.push(path);
            }
        }
        assert_eq!(
            seen,
            [
                vec![0],
                vec![1],
                vec![1, 0],
                vec![1, 1],
                vec![1, 1, 0],
                vec![1, 2],
                vec![2]
            ]
        );
    }

    #[test]
    fn blocks_are_found_in_the_source_map_by_their_path() {
        let document = parse(
            "Before\n\n<details>\n<summary>Outer</summary>\n\nIn outer\n\n</details>\n\n- item\n",
        );
        let stream = content(Content::Document(&document), &inputs());
        let mut seen = Vec::new();
        for line in stream.lines.iter().filter_map(|line| line.block) {
            if seen.last() != Some(&line) {
                seen.push(line);
            }
        }
        let lines: Vec<_> = seen
            .iter()
            .map(|&id| document.places.line(stream.blocks.get(id).path()))
            .collect();
        assert_eq!(lines, [Some(1), Some(3), Some(6), Some(10)]);
    }

    #[test]
    fn the_lines_of_blocks_are_those_of_their_own_file() {
        let shown = [
            file_of("a.md", "One\n\nTwo\n"),
            file_of("b.md", "\n\nThree\n"),
        ];
        let stream = files(&shown, &inputs());
        let line_of = |find: &str| {
            let line = stream
                .lines
                .iter()
                .find(|line| text(&line.spans).contains(find))
                .unwrap();
            let info = stream.blocks.get(line.block.unwrap());
            shown[info.file()].document.places.line(info.path())
        };
        assert_eq!(line_of("Two"), Some(3));
        assert_eq!(line_of("Three"), Some(3));
        // The header of a file is not in the source.
        let header = stream.lines[0].block.unwrap();
        let info = stream.blocks.get(header);
        assert_eq!(shown[info.file()].document.places.line(info.path()), None);
    }

    #[test]
    fn files_mark_their_headers_with_the_file() {
        let shown = [file_of("a.md", "One\n\nTwo\n"), file_of("b.md", "Three\n")];
        let stream = files(&shown, &inputs());
        let table = &stream.blocks;
        let file_of_line = |line: &Line| line.block.map(|id| table.get(id).file());
        let start = stream
            .lines
            .iter()
            .position(|line| file_of_line(line) == Some(1))
            .unwrap();
        // The header, up to the first block, has the file's entry.
        let header = &stream.lines[..stream
            .lines
            .iter()
            .position(|line| text(&line.spans).contains("One"))
            .unwrap()
            - 1];
        assert!(header.len() >= 3);
        for line in header {
            assert_eq!(file_of_line(line), Some(0));
            assert_eq!(table.path(line.block.unwrap()), &[] as &[usize]);
        }
        // The blank line before the second file has no block.
        assert!(stream.lines[start - 1].is_blank());
        assert_eq!(stream.lines[start - 1].block, None);
        // The second file's header and block.
        assert_eq!(file_of_line(&stream.lines[start]), Some(1));
        let three = stream
            .lines
            .iter()
            .find(|line| text(&line.spans).contains("Three"))
            .unwrap();
        assert_eq!(file_of_line(three), Some(1));
        assert_eq!(table.path(three.block.unwrap()), [0]);
        // The blank line between a header and the first block has none.
        let first = stream
            .lines
            .iter()
            .position(|line| text(&line.spans).contains("One"))
            .unwrap();
        assert_eq!(stream.lines[first - 1].block, None);
    }

    /// The paths the layout records for a document with blocks nested in
    /// every kind of container are paths of blocks of the document, as
    /// `compare` counts them.
    #[test]
    fn the_paths_of_the_blocks_table_are_paths_of_the_documents_blocks() {
        let document = parse(crate::typeset::compare::NESTED);
        let known: Vec<Vec<usize>> = crate::typeset::compare::blocks(&document)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        let reading = Inputs {
            reading: Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Wrap,
            }),
            caps: Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            },
            ..Inputs::new(Capabilities::kitty(80))
        };
        for inputs in [inputs(), reading] {
            let stream = content(Content::Document(&document), &inputs);
            let mut recorded = 0;
            for line in &stream.lines {
                if let Some(id) = line.block {
                    let path = stream.blocks.path(id);
                    recorded += 1;
                    assert!(known.iter().any(|known| known == path), "{path:?}");
                }
            }
            assert!(recorded > 0);
        }
    }

    /// The lines of the fixture and its table agree: the blocks are in
    /// order, each is used, and all belong to the one document.
    fn assert_agree(fixture: &str) {
        let source = std::fs::read_to_string(format!("tests/fixtures/{fixture}")).unwrap();
        let stream = lone(&source);
        let mut seen = Vec::new();
        for line in &stream.lines {
            if let Some(id) = line.block {
                assert_eq!(stream.blocks.get(id).file(), 0);
                if seen.last() != Some(&id) {
                    seen.push(id);
                }
            }
        }
        let paths: Vec<_> = seen.iter().map(|&id| stream.blocks.path(id)).collect();
        // Each block is one run of lines, so none is seen twice, and the
        // blocks follow each other as the document does.
        for (number, path) in paths.iter().enumerate() {
            assert!(!paths[..number].contains(path), "{fixture}: {path:?}");
            assert!(!path.is_empty());
        }
        assert!(
            paths.windows(2).all(|pair| pair[0] < pair[1]),
            "{fixture}: {paths:?}"
        );
        // Every block of the table is a block of the document, in the
        // document's order, and the innermost block of some line unless it
        // holds blocks that have all its lines, or is not shown.
        let known: Vec<Vec<usize>> = (parse(&source).blocks().into_iter())
            .map(|(path, _)| path)
            .collect();
        let table: Vec<&[usize]> = (stream.blocks.ids())
            .map(|id| stream.blocks.path(id))
            .collect();
        assert!(
            table.windows(2).all(|pair| pair[0] < pair[1]),
            "{fixture}: {table:?}"
        );
        let inner: Vec<_> = stream.lines.iter().filter_map(|line| line.inner).collect();
        for id in stream.blocks.ids() {
            let path = stream.blocks.path(id);
            assert!(
                known.iter().any(|known| known == path),
                "{fixture}: {path:?}"
            );
            // A block whose lines are all of the blocks inside it, such as a
            // list, is the innermost block of none.
            let holds_blocks = table
                .iter()
                .any(|at| at.len() > path.len() && at.starts_with(path));
            // An empty heading and an empty code block have no lines.
            let empty = (parse(&source).blocks().into_iter()).any(|(at, block)| {
                at == path
                    && match block {
                        Block::Heading { content, .. } => content.is_empty(),
                        Block::CodeBlock { code, .. } => code.is_empty(),
                        _ => false,
                    }
            });
            assert!(
                inner.contains(&id) || holds_blocks || empty || stream.blocks.is_hidden(id),
                "{fixture}: {path:?} is the innermost block of no line"
            );
        }
    }

    #[test]
    fn the_table_and_the_lines_agree_for_blocks_in_quotes_and_lists() {
        assert_agree("nested.md");
    }

    #[test]
    fn the_table_and_the_lines_agree_for_html_tags() {
        assert_agree("html-tags.md");
    }

    /// Reading mode with fixture fonts and cells of 10 × 20 pixels.
    fn reading() -> Inputs {
        Inputs {
            reading: Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Wrap,
            }),
            caps: Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            },
            ..Inputs::new(Capabilities::kitty(80))
        }
    }

    fn pages_of(stream: &Stream) -> usize {
        crate::typeset::reading::engine::pages(&stream.lines).len()
    }

    #[test]
    fn pieces_are_stacked_a_blank_line_apart_and_a_piece_without_lines_gets_none() {
        let stream = lone("One\n\n#\n\nTwo\n");
        let texts: Vec<String> = stream.lines.iter().map(|l| text(&l.spans)).collect();
        assert_eq!(texts, ["One", "", "Two"], "{texts:?}");
    }

    #[test]
    fn in_reading_mode_a_heading_a_paragraph_and_a_details_with_its_blocks_are_one_page() {
        let source =
            "# Title\n\nText.\n\n<details>\n<summary>S</summary>\n\nInside.\n\n</details>\n";
        let stream = content(Content::Document(&parse(source)), &reading());
        assert_eq!(pages_of(&stream), 1);
    }

    #[test]
    fn an_error_between_two_reading_documents_is_cells_and_each_document_is_a_page() {
        let shown = [
            file_of("a.md", "One\n"),
            File {
                name: "b.md".to_owned(),
                document: crate::typeset::model::Document::new(vec![Block::Error("bad".into())]),
            },
            file_of("c.md", "Three\n"),
        ];
        let stream = files(&shown, &reading());
        let error = stream
            .lines
            .iter()
            .find(|line| text(&line.spans).contains("bad"))
            .unwrap();
        assert!(error.image.is_none(), "{error:?}");
        // The document before the error and the one after are pages of
        // their own, and the error is not on either.
        assert_eq!(pages_of(&stream), 2);
    }

    #[test]
    fn the_header_of_a_file_is_cells_in_reading_mode_too() {
        let shown = [file_of("a.md", "One\n")];
        let stream = files(&shown, &reading());
        let header = stream
            .lines
            .iter()
            .find(|l| text(&l.spans).contains("a.md"));
        assert!(
            header.is_some_and(|line| line.image.is_none()),
            "{header:?}"
        );
    }

    #[test]
    fn a_band_settles_on_the_last_line_of_its_file_and_not_on_the_blank_line_between_files() {
        use crate::typeset::images::Placeholder;
        let mut inputs = Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(60)
        });
        let key = inputs.drawn("a.png");
        let pixels = crate::typeset::images::Source::Raster(::image::RgbaImage::new(40, 30));
        inputs.images.wait(key, Placeholder::Dimmed(pixels));
        let shown = [file_of("a.md", "![a](a.png)\n"), file_of("b.md", "Two\n")];
        let stream = files(&shown, &inputs);
        let at = stream
            .lines
            .iter()
            .position(|line| line.loading.is_some())
            .unwrap();
        assert!(stream.lines[at].image.is_some(), "{:?}", stream.lines[at]);
        assert!(stream.lines[at + 1].is_blank() && stream.lines[at + 1].loading.is_none());
    }

    /// The table is the header, if there is one, and then every block of
    /// the document in its order, whichever engine lays it out.
    #[test]
    fn the_table_is_the_header_and_the_blocks_of_the_document_in_both_engines() {
        let dir = std::fs::read_dir("tests/fixtures").unwrap();
        let mut seen = 0;
        for entry in dir.map(|entry| entry.unwrap().path()) {
            if entry.extension().is_none_or(|ext| ext != "md") {
                continue;
            }
            let source = std::fs::read_to_string(&entry).unwrap();
            let document = parse(&source);
            let known: Vec<Vec<usize>> = document.blocks().into_iter().map(|(p, _)| p).collect();
            for inputs in [inputs(), reading()] {
                let stream = content(Content::Document(&document), &inputs);
                let table: Vec<Vec<usize>> = stream
                    .blocks
                    .ids()
                    .map(|id| stream.blocks.path(id).to_vec())
                    .collect();
                assert_eq!(table, known, "{entry:?}");
                let shown = [File {
                    name: "a.md".to_owned(),
                    document: parse(&source),
                }];
                let stream = files(&shown, &inputs);
                let table: Vec<Vec<usize>> = stream
                    .blocks
                    .ids()
                    .map(|id| stream.blocks.path(id).to_vec())
                    .collect();
                let mut with_header = vec![vec![]];
                with_header.extend(known.clone());
                assert_eq!(table, with_header, "{entry:?} as a file");
            }
            seen += 1;
        }
        assert!(seen > 10);
    }

    #[test]
    fn hidden_front_matter_is_in_the_table_as_hidden_and_has_no_lines_in_both_engines() {
        use crate::render::FrontMatterStyle;
        for mut inputs in [inputs(), reading()] {
            inputs.front_matter = FrontMatterStyle::Hide;
            let stream = content(
                Content::Document(&parse("---\ntitle: A\n---\n\nText.\n")),
                &inputs,
            );
            let first = stream.blocks.ids().next().unwrap();
            assert_eq!(stream.blocks.path(first), [0]);
            assert!(stream.blocks.is_hidden(first));
            assert!(stream.lines.iter().all(|line| line.block != Some(first)));
        }
    }
}
