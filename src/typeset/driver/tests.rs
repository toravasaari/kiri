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
            glyphs: Default::default(),
            fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
            snap: false,
            zoom: 100,
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
            glyphs: Default::default(),
            fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
            snap: false,
            zoom: 100,
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
            glyphs: Default::default(),
            fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
            snap: false,
            zoom: 100,
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
    let stream = lone("One\n\n#####\n\nTwo\n");
    let texts: Vec<String> = stream.lines.iter().map(|l| text(&l.spans)).collect();
    assert_eq!(texts, ["One", "", "Two"], "{texts:?}");
}

#[test]
fn in_reading_mode_a_heading_a_paragraph_and_a_details_with_its_blocks_are_one_page() {
    let source = "# Title\n\nText.\n\n<details>\n<summary>S</summary>\n\nInside.\n\n</details>\n";
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
    let mut seen = 0;
    for entry in crate::testing::documents() {
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
    use crate::typeset::inputs::FrontMatterStyle;
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

#[test]
fn blank_front_matter_is_not_hidden_in_the_table_or_the_code_style_in_both_engines() {
    use crate::typeset::inputs::FrontMatterStyle;
    for style in [FrontMatterStyle::Table, FrontMatterStyle::Code] {
        for mut inputs in [inputs(), reading()] {
            inputs.front_matter = style;
            let stream = content(Content::Document(&parse("---\n---\n\nText.\n")), &inputs);
            let first = stream.blocks.ids().next().unwrap();
            assert_eq!(stream.blocks.path(first), [0]);
            assert!(
                !stream.blocks.is_hidden(first),
                "blank front matter is hidden as {style:?}"
            );
        }
    }
}

/// The keys of the images that load, in the lines of `stream`.
fn loading_keys(stream: &Stream) -> Vec<crate::typeset::images::key::ImageKey> {
    let loading = stream.lines.iter().filter_map(|line| line.loading.as_ref());
    loading.map(|loading| loading.key.clone()).collect()
}

/// Inputs in which the image `a.png` of each of the documents
/// numbered `numbers` loads.
fn loading_in(numbers: &[usize]) -> Inputs {
    use crate::typeset::images::key::ImageKey;
    use crate::typeset::images::store::Placeholder;
    let mut inputs = inputs();
    for &number in numbers {
        let pixels = image::RgbaImage::from_pixel(40, 30, image::Rgba([9, 9, 9, 255]));
        let key = ImageKey::from("a.png").in_document(number);
        let placeholder =
            Placeholder::Dimmed(crate::typeset::images::decode::Source::Raster(pixels));
        inputs.images.wait(key, placeholder);
    }
    inputs
}

#[test]
fn content_from_numbers_the_image_keys_from_first_and_the_blocks_from_0() {
    use crate::typeset::images::key::ImageKey;
    let files = [
        file_of("a.md", "![a](a.png)\n"),
        file_of("b.md", "![a](a.png)\n"),
    ];
    let inputs = loading_in(&[7, 8]);
    let stream = content_from(Content::Files(&files), 7, &inputs);
    let keys = loading_keys(&stream);
    let expected = [7, 8].map(|number| ImageKey::from("a.png").in_document(number));
    assert_eq!(keys, expected);
    let mut at_files: Vec<usize> = (stream.lines.iter())
        .filter_map(|line| line.block)
        .map(|id| stream.blocks.get(id).file())
        .collect();
    at_files.dedup();
    assert_eq!(at_files, [0, 1]);
    // A lone document's keys carry `first` too.
    let document = parse("![a](a.png)\n");
    let inputs = loading_in(&[3]);
    let stream = content_from(Content::Document(&document), 3, &inputs);
    let expected = [ImageKey::from("a.png").in_document(3)];
    assert_eq!(loading_keys(&stream), expected);
    assert!(
        stream
            .lines
            .iter()
            .filter_map(|line| line.block)
            .all(|id| stream.blocks.get(id).file() == 0)
    );
}

/// Rust code of `count` lines with what a layout in stretches must get
/// right: a comment that runs over the seams at 512 and 1024 lines,
/// tabs, a line that wraps and one of no text.
fn rust(count: usize) -> String {
    let mut code = String::new();
    for line in 0..count {
        let text = match line % 1024 {
            508 | 1020 => "/* a comment that goes on".to_owned(),
            509..=514 | 1021..=1026 => "   still the comment".to_owned(),
            515 | 1027 => "   the end */ fn after() {}".to_owned(),
            _ if line % 97 == 0 => {
                format!("\tlet tabbed{line} = \"a\\tb\"; // {}", "word ".repeat(20))
            }
            _ if line % 89 == 0 => String::new(),
            _ => format!("fn f{line}() {{ let x = {line}; }}"),
        };
        code.push_str(&text);
        code.push('\n');
    }
    code
}

fn code_block(language: Option<&str>, code: &str, whole_file: bool) -> Document {
    crate::typeset::model::Document::new(vec![Block::CodeBlock {
        language: language.map(str::to_owned),
        code: code.into(),
        whole_file,
    }])
}

/// A document of a paragraph, the code block and a paragraph.
fn between(block: Document) -> Document {
    let mut blocks = parse("Before.\n").blocks;
    blocks.extend(block.blocks);
    blocks.extend(parse("After.\n").blocks);
    crate::typeset::model::Document::new(blocks)
}

/// The layout of `document` with its code blocks cut in stretches of
/// `size` lines.
fn cut(document: &Document, inputs: &Inputs, size: usize) -> Stream {
    content_cut(Content::Document(document), 0, inputs, size)
}

/// What a pen is given: the kind of each piece, and for a stretch
/// whether it is the first and the last.
#[derive(Default)]
struct Recorder {
    seen: Vec<String>,
}

impl<'d> Pen<'d> for Recorder {
    fn piece(&mut self, piece: Piece<'d>, _: &mut Stack) {
        self.seen.push(match piece.shape {
            Shape::Block(_) => format!("block {:?}", piece.path),
            Shape::Stretch(stretch) => format!(
                "stretch {:?} at {} {}{}",
                piece.path,
                stretch.offset,
                if stretch.first { "first" } else { "" },
                if stretch.last { "last" } else { "" },
            ),
            Shape::Summary { .. } => format!("summary {:?}", piece.path),
            Shape::Wrapper => format!("wrapper {:?}", piece.path),
            Shape::Centred(_) => format!("centred {:?}", piece.path),
            Shape::Header(_) => "header".to_owned(),
            Shape::Rows(rows) => format!("rows {:?} {:?}", piece.path, rows.rows()),
        });
    }
    fn cut(&mut self, _: &mut Stack, _: usize) -> bool {
        true
    }
    fn cuts(&self, _: &Block) -> bool {
        true
    }
    fn save(&self) -> Saved {
        Saved::default()
    }
    fn restore(&mut self, _: &Saved, _: &Walk<'d>) {}
}

fn pieces_of(document: &Document, size: usize) -> Vec<String> {
    let mut pen = Recorder::default();
    let mut stack = Stack::default();
    let walk = Walk::new(None, &document.blocks, size);
    let mut at = Place::start(false);
    while let Some((piece, next)) = walk.next(&at, |block| pen.cuts(block)) {
        pen.piece(piece, &mut stack);
        at = next;
    }
    pen.seen
}

#[test]
fn a_large_code_block_is_handed_to_the_pen_as_stretches_and_a_small_one_whole() {
    let code = "a\n".repeat(2000);
    let document = between(code_block(Some("rust"), &code, false));
    assert_eq!(
        pieces_of(&document, 512),
        [
            "block [0]".to_owned(),
            "stretch [1] at 0 first".to_owned(),
            "stretch [1] at 1024 ".to_owned(),
            "stretch [1] at 2048 ".to_owned(),
            "stretch [1] at 3072 last".to_owned(),
            "block [2]".to_owned(),
        ]
    );
    // Exactly 512 lines is not cut; a block of one stretch is a block.
    let code = "a\n".repeat(512);
    let document = code_block(Some("rust"), &code, false);
    assert_eq!(pieces_of(&document, 512), ["block [0]"]);
    // Inside a `<details>` the block is cut as well.
    let code = "a\n".repeat(30);
    let source = format!("<details>\n<summary>S</summary>\n\n```rust\n{code}```\n\n</details>\n");
    let seen = pieces_of(&parse(&source), 10);
    assert_eq!(seen[0], "summary [0]");
    assert_eq!(seen.len(), 4, "{seen:?}");
    assert!(seen[1].starts_with("stretch [0, 0] at 0 first"), "{seen:?}");
    assert!(seen[3].ends_with("last"), "{seen:?}");
}

#[test]
fn a_long_table_in_a_centred_wrapper_is_handed_to_the_pen_in_rows() {
    let rows: String = (0..30).map(|row| format!("| {row} | x |\n")).collect();
    let source = format!(
        "<div align=\"center\">\n\nIntro.\n\n| a | b |\n|---|---|\n{rows}\n</div>\n\nAfter.\n"
    );
    let seen = pieces_of(&parse(&source), 10);
    // The wrapper has a piece of its own that lays out nothing, and each of
    // its blocks is a piece inside it, the table in stretches of rows.
    assert_eq!(seen[0], "wrapper [0]", "{seen:?}");
    assert_eq!(seen[1], "centred [0, 0]", "{seen:?}");
    assert!(seen[2].starts_with("rows [0, 1]"), "{seen:?}");
    assert_eq!(seen.len(), 6, "{seen:?}");
    assert_eq!(seen[5], "block [1]", "{seen:?}");
}

/// A document of a centred wrapper that holds a heading, a paragraph and a
/// table of `rows` rows, with a paragraph after it.
fn wrapper_with_a_table(rows: usize) -> Document {
    let rows: String = (0..rows).map(|row| format!("| {row} | x |\n")).collect();
    parse(&format!(
        "<div align=\"center\">\n\n# Name\n\nIntro.\n\n| a | b |\n|---|---|\n{rows}\n</div>\n\nAfter.\n"
    ))
}

#[test]
fn a_long_table_in_a_centred_wrapper_is_laid_out_in_stretches_as_whole() {
    let document = wrapper_with_a_table(40);
    let mut inputs = inputs();
    inputs.tables = Some(crate::typeset::stretch::Tables::default());
    let whole = cut(&document, &inputs, usize::MAX);
    let stretched = cut(&document, &inputs, 10);
    assert_eq!(stretched.lines.len(), whole.lines.len());
    for (at, (a, b)) in stretched.lines.iter().zip(&whole.lines).enumerate() {
        assert_eq!(a, b, "line {at}");
    }
    // The stretches are pieces of the table, which is a block of the wrapper.
    let kinds = pieces_of(&document, 10);
    assert!(
        kinds.iter().any(|kind| kind.starts_with("rows [0, 2]")),
        "{kinds:?}"
    );
    // The heading and the paragraph are still centred in their room.
    let rows = marked(&stretched);
    let pad = |find: &str| {
        let (row, _) = rows.iter().find(|(text, _)| text.contains(find)).unwrap();
        row.find(find).unwrap()
    };
    assert!(pad("Intro.") > 10 && pad("Name") > 10, "{rows:?}");
}

#[test]
fn a_centred_wrapper_has_a_place_for_each_block_in_it_in_both_engines() {
    let document = wrapper_with_a_table(5);
    let reading = Inputs {
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
            ..Capabilities::kitty(80)
        },
        ..Inputs::new(Capabilities::kitty(80))
    };
    for inputs in [inputs(), reading] {
        let stream = content(Content::Document(&document), &inputs);
        let mut seen: Vec<Vec<usize>> = Vec::new();
        for id in stream.lines.iter().filter_map(|line| line.block) {
            let path = stream.blocks.path(id).to_vec();
            if seen.last() != Some(&path) {
                seen.push(path);
            }
        }
        // The wrapper shows nothing of its own.
        assert_eq!(seen, [vec![0, 0], vec![0, 1], vec![0, 2], vec![1]]);
    }
}

/// What each span of a line's `lead`, `spans` and `trail` shows of the
/// document. `Origins` compares equal whatever it holds, so `Line`'s own
/// equality does not see it.
fn origins(line: &Line) -> Vec<Vec<crate::typeset::style::Origin>> {
    let spans = line.lead.iter().chain(&line.spans).chain(&line.trail);
    spans
        .map(|span| span.origins.into_iter().cloned().collect())
        .collect()
}

/// A block in stretches and the same block whole: their lines (spans and
/// styles, block ids), what each span shows of the document, and the
/// table of blocks are equal, and the table has the block once.
fn assert_stretches_equal_whole(document: &Document, inputs: &Inputs, size: usize) {
    let whole = cut(document, inputs, usize::MAX);
    let stretched = cut(document, inputs, size);
    assert_eq!(stretched.lines.len(), whole.lines.len());
    for (at, (a, b)) in stretched.lines.iter().zip(&whole.lines).enumerate() {
        assert_eq!(a, b, "line {at}");
        assert_eq!(origins(a), origins(b), "origins of line {at}");
    }
    assert_eq!(stretched.blocks, whole.blocks);
    let entries = stretched.blocks.ids().count();
    assert_eq!(entries, document.blocks.len());
    // A blank line has no block. The only ones are before and after the
    // code block, which has none inside.
    let blank: Vec<usize> = (0..stretched.lines.len())
        .filter(|&at| stretched.lines[at].block.is_none())
        .collect();
    assert_eq!(blank.len(), document.blocks.len() - 1, "{blank:?}");
}

fn no_colours(width: u16) -> Inputs {
    let caps = Capabilities {
        color: false,
        ..Capabilities::kitty(width)
    };
    Inputs::new(caps)
}

#[test]
fn a_highlighted_block_in_stretches_is_the_block_whole() {
    let code = rust(2000);
    for size in [512, 100, 7] {
        let fenced = between(code_block(Some("rust"), &code, false));
        assert_stretches_equal_whole(&fenced, &inputs(), size);
    }
    // With its label and box.
    let fenced = between(code_block(Some("rust"), &code, false));
    let stream = cut(&fenced, &inputs(), 512);
    let label = text(&stream.lines[2].spans);
    assert!(label.trim_end().ends_with("rust"), "{label:?}");
    let last = text(&stream.lines[stream.lines.len() - 3].spans);
    assert!(last.trim().is_empty(), "{last:?}");
}

#[test]
fn a_whole_file_and_an_uncoloured_block_in_stretches_are_the_block_whole() {
    let code = rust(1300);
    for language in [Some("rust"), None] {
        let file = code_block(language, &code, true);
        assert_stretches_equal_whole(&file, &inputs(), 512);
    }
    let plain = between(code_block(Some("rust"), &code, false));
    assert_stretches_equal_whole(&plain, &no_colours(60), 512);
    let unknown = between(code_block(None, &code, false));
    assert_stretches_equal_whole(&unknown, &inputs(), 400);
}

#[test]
fn a_failing_line_leaves_the_block_in_its_geometry_and_stretches_equal_whole() {
    let mut lines: Vec<String> = rust(1500).lines().map(str::to_owned).collect();
    // A NUL makes the highlighter fail in a test build.
    lines[700].push('\0');
    let code = lines.join("\n") + "\n";
    for size in [512, 64] {
        let document = between(code_block(Some("rust"), &code, false));
        assert_stretches_equal_whole(&document, &inputs(), size);
    }
    // The lines from the error on have no colours, and the box and its
    // label stay: every row is as wide as the block.
    let document = between(code_block(Some("rust"), &code, false));
    let stream = cut(&document, &inputs(), 512);
    // The spans that have a colour: the code, in one piece or in many.
    let colours = |line: &Line| line.spans.iter().filter(|s| s.style.fg.is_some()).count();
    let row = |text: &str| {
        let at = stream
            .lines
            .iter()
            .position(|line| crate::typeset::style::text(&line.spans).contains(text));
        &stream.lines[at.unwrap()]
    };
    assert!(colours(row("fn f699()")) > 1);
    assert_eq!(colours(row("fn f701()")), 1);
    let label = text(&stream.lines[2].spans);
    assert!(label.trim_end().ends_with("rust"), "{label:?}");
    let width = crate::typeset::wrap::width(&stream.lines[2].spans);
    assert!(
        stream.lines[2..stream.lines.len() - 2]
            .iter()
            .all(|line| { crate::typeset::wrap::width(&line.spans) == width })
    );
}

#[test]
fn a_diagram_is_never_cut() {
    use crate::typeset::images::{ImageKey, Placeholder};
    let code = "graph TD\n".to_owned() + &"  A --> B\n".repeat(900);
    let document = code_block(Some("mermaid"), &code, false);
    let mut inputs = inputs();
    let key = inputs.drawn(ImageKey::rendered("mermaid", &code));
    inputs.images.wait(key, Placeholder::Rows(3));
    let stretched = cut(&document, &inputs, 100);
    assert_eq!(stretched.lines.len(), 3);
    assert_eq!(stretched.blocks.ids().count(), 1);
    assert_eq!(stretched.lines, cut(&document, &inputs, usize::MAX).lines);
}

#[test]
fn the_reason_a_diagram_failed_goes_below_the_last_stretch_only() {
    use crate::typeset::images::{ImageError, ImageKey};
    use crate::typeset::inputs::RenderErrors;
    let code = "graph TD\n".to_owned() + &"  A --> B\n".repeat(900);
    let document = code_block(Some("mermaid"), &code, false);
    let mut inputs = Inputs {
        render_errors: RenderErrors::Both,
        ..inputs()
    };
    let key = inputs.drawn(ImageKey::rendered("mermaid", &code));
    let error = ImageError::Diagram {
        name: "diagram".into(),
        reason: "no program".into(),
    };
    inputs.images.insert(key, Err(error));
    assert_stretches_equal_whole(&document, &inputs, 100);
    let stretched = cut(&document, &inputs, 100);
    let texts: Vec<String> = stretched.lines.iter().map(|l| text(&l.spans)).collect();
    let told = |text: &String| text.contains("could not be drawn");
    assert_eq!(texts.iter().filter(|text| told(text)).count(), 1);
    assert!(told(texts.last().unwrap()), "{:?}", texts.last());
}

#[test]
fn a_loading_band_goes_on_the_last_stretch_and_covers_the_lines_above_it_in_the_block() {
    use crate::typeset::images::{ImageKey, Placeholder, Source};
    let code = "graph TD\n".to_owned() + &"  A --> B\n".repeat(300);
    let document = code_block(Some("mermaid"), &code, false);
    let mut inputs = inputs();
    let key = inputs.drawn(ImageKey::rendered("mermaid", &code));
    // An image without size is not placed, so the code is shown.
    let empty = Source::Raster(image::RgbaImage::new(0, 0));
    inputs.images.wait(key, Placeholder::Dimmed(empty));
    let whole = cut(&document, &inputs, usize::MAX);
    let loading: Vec<usize> = (0..whole.lines.len())
        .filter(|&at| whole.lines[at].loading.is_some())
        .collect();
    assert_eq!(
        loading,
        [whole.lines.len() - 1],
        "the block is shown as code"
    );
    assert_stretches_equal_whole(&document, &inputs, 100);
}

#[test]
fn a_range_across_two_stretches_is_found_as_in_the_block_whole() {
    use crate::typeset::locate::Map;
    use crate::typeset::position::Pos;
    let code = rust(1100);
    let document = code_block(Some("rust"), &code, false);
    let (whole, stretched) = (
        cut(&document, &inputs(), usize::MAX),
        cut(&document, &inputs(), 512),
    );
    let (a, b) = (
        Map::new(&whole.lines, &whole.blocks),
        Map::new(&stretched.lines, &stretched.blocks),
    );
    // From the last line of the first stretch to the first line of the
    // second: lines 511 and 512 of the code.
    let at = |line: usize| -> usize { code.split_inclusive('\n').take(line).map(str::len).sum() };
    let range = Pos::text(0, &[0], 0, at(511) + 3)..Pos::text(0, &[0], 0, at(513) - 2);
    let found = |map: &Map, lines: &[Line]| map.locate(lines, &range).unwrap();
    let (in_whole, in_stretches) = (found(&a, &whole.lines), found(&b, &stretched.lines));
    assert_eq!(in_whole, in_stretches);
    let rows: usize = in_stretches.iter().map(|part| part.rows.len()).sum();
    assert!(rows >= 2, "{in_stretches:?}");
    // Each row of the seam says where it is.
    for row in 0..whole.lines.len() {
        for column in [0, 2, 9] {
            assert_eq!(
                a.hit(&whole.lines, row, column),
                b.hit(&stretched.lines, row, column),
                "row {row} column {column}"
            );
        }
    }
    assert_eq!(a.line_of(&range.start), b.line_of(&range.start));
    assert_eq!(a.line_of(&range.end), b.line_of(&range.end));
    assert!(b.line_of(&range.start).is_some());
    assert_eq!(a.line_bytes(&range), b.line_bytes(&range));
}

#[test]
fn content_from_0_is_content() {
    let files = [
        file_of("a.md", "One\n\n![a](a.png)\n"),
        file_of("b.md", "![a](a.png)\n"),
    ];
    let inputs = loading_in(&[0, 1]);
    let (from, plain) = (
        content_from(Content::Files(&files), 0, &inputs),
        content(Content::Files(&files), &inputs),
    );
    assert_eq!(marked(&from), marked(&plain));
    assert_eq!(loading_keys(&from), loading_keys(&plain));
    assert_eq!(loading_keys(&from).len(), 2);
}

/// The table that `table` makes before anything is laid out is the one
/// the layout makes, whichever engine lays it out and however front
/// matter shows.
#[test]
fn the_table_of_blocks_is_known_before_anything_is_laid_out() {
    use crate::typeset::inputs::FrontMatterStyle;
    let mut seen = 0;
    let mut documents = vec!["---\ntitle: A\nlist: [1, 2]\n---\n\nText.\n".to_owned()];
    for entry in crate::testing::documents() {
        documents.push(std::fs::read_to_string(&entry).unwrap());
    }
    for source in &documents {
        let document = parse(source);
        let shown = [file_of("a.md", source), file_of("b.md", "Two\n\n- x\n")];
        for mut inputs in [inputs(), reading()] {
            for style in [
                FrontMatterStyle::Table,
                FrontMatterStyle::Code,
                FrontMatterStyle::Hide,
            ] {
                inputs.front_matter = style;
                let lone = content(Content::Document(&document), &inputs);
                assert_eq!(
                    table(Content::Document(&document), 0, &inputs),
                    lone.blocks,
                    "{style:?}"
                );
                let several = content(Content::Files(&shown), &inputs);
                assert_eq!(
                    table(Content::Files(&shown), 0, &inputs),
                    several.blocks,
                    "{style:?} as files"
                );
                seen += 1;
            }
        }
    }
    assert!(seen > 60);
}

/// `laid` extended to its end, with `inputs`.
fn whole(content: Content, inputs: &Inputs, size: usize) -> Laid {
    let mut laid = begin(content, (0, 0), inputs, size);
    assert!(laid.extend(content, inputs, usize::MAX));
    assert!(laid.is_whole());
    laid
}

/// `a` and `b` have the same lines, with what each shows of the
/// document, and the same table of blocks.
fn assert_same(a: &Laid, b: &Laid, what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: lines");
    for at in 0..a.len() {
        assert_eq!(a.lines()[at], b.lines()[at], "{what}: line {at}");
        assert_eq!(
            origins(&a.lines()[at]),
            origins(&b.lines()[at]),
            "{what}: origins of line {at}"
        );
    }
    assert_eq!(a.blocks(), b.blocks(), "{what}: table");
}

#[test]
fn a_stretch_laid_out_again_from_its_checkpoint_equals_itself() {
    let code = rust(520);
    for size in [200, 64] {
        let document = between(code_block(Some("rust"), &code, false));
        let content = Content::Document(&document);
        let inputs = inputs();
        let eager = whole(content, &inputs, size);
        let mut laid = whole(content, &inputs, size);
        let total = laid.len();
        let mut rewound_far = false;
        for line in (0..=total + 3)
            .step_by(97)
            .chain([1, size, size + 1, total - 1])
        {
            laid.restart(content, line);
            let from = laid.len();
            assert!(from <= line.min(total), "restart({line}) is at {from}");
            assert!(!laid.is_whole(), "restart({line})");
            // What is kept is the layout's own lines.
            for at in 0..from {
                assert_eq!(laid.lines()[at], eager.lines()[at], "restart({line}): {at}");
            }
            // What the layout has not laid out again is not told as laid
            // out, and the rest is told as the whole layout tells it.
            let mut not_laid = 0;
            for range in crate::steps::ranges(content) {
                match laid.locate(&range) {
                    Err(crate::typeset::locate::Hidden::NotLaidOut) => not_laid += 1,
                    found => assert_eq!(found, eager.locate(&range), "restart({line})"),
                }
            }
            assert!(not_laid > 0, "restart({line}) at {from}: all is laid out");
            rewound_far |= from > 2 * size;
            assert!(laid.extend(content, &inputs, usize::MAX));
            assert_same(&laid, &eager, &format!("size {size}: restart({line})"));
        }
        assert!(rewound_far, "no restart began after the second stretch");
    }
}

#[test]
fn a_restart_goes_back_to_the_start_of_the_stretch_not_the_top() {
    let code = rust(2000);
    let document = code_block(Some("rust"), &code, false);
    let content = Content::Document(&document);
    let inputs = inputs();
    let mut laid = whole(content, &inputs, 512);
    let total = laid.len();
    laid.restart(content, total - 1);
    // The last stretch has the 2000 - 3 * 512 = 464 last lines and the
    // box's bottom row; the checkpoint is where it starts.
    assert!(
        laid.len() > total - 520 && laid.len() < total - 400,
        "{}",
        laid.len()
    );
    laid.restart(content, 0);
    assert_eq!(laid.len(), 0);
}

#[test]
fn an_image_that_arrives_after_a_stretch_does_not_lay_the_stretches_out_again() {
    let code = rust(1300);
    let mut blocks = parse("![a](a.png)\n").blocks;
    blocks.extend(code_block(Some("rust"), &code, false).blocks);
    blocks.extend(parse("Between.\n\n![a](a.png)\n\nAfter.\n").blocks);
    let document = Document::new(blocks);
    let content = Content::Document(&document);
    let (before, after) = (inputs(), loading_in(&[0]));
    let want = whole(content, &after, 512);
    let mut laid = whole(content, &before, 512);
    assert_ne!(laid.lines(), want.lines(), "the image changes the lines");
    // The second image is where the loading is told, on the last lines.
    let at = (0..want.len())
        .rev()
        .find(|&at| want.lines()[at].loading.is_some());
    let at = at.expect("a loading band");
    laid.restart(content, at);
    assert!(laid.len() > 1000, "restarted at {}", laid.len());
    assert!(laid.extend(content, &after, usize::MAX));
    // The lines before the restart are the first layout's: the first image
    // is still laid out as it was, which is one line fewer than it takes
    // now. The lines from the restart on are the new layout's.
    let fewer = want.len() - laid.len();
    assert_eq!(fewer, 1);
    for line in 1000..laid.len() {
        assert_eq!(
            laid.lines()[line],
            want.lines()[line + fewer],
            "line {line}"
        );
    }
    assert_eq!(laid.blocks(), want.blocks());
    // From the top, the first image is changed too.
    laid.restart(content, 0);
    assert!(laid.extend(content, &after, usize::MAX));
    assert_same(&laid, &want, "restarted from the top");
}

#[test]
fn a_band_waiting_for_the_line_below_it_is_kept_by_a_checkpoint() {
    // The image's last row has a band that settles on the blank row
    // that the next piece brings; a checkpoint between them keeps it.
    let code = rust(90);
    let mut blocks = parse("![a](a.png)\n").blocks;
    blocks.extend(code_block(Some("rust"), &code, false).blocks);
    blocks.extend(parse("![a](a.png)\n").blocks);
    let document = Document::new(blocks);
    let content = Content::Document(&document);
    let inputs = loading_in(&[0]);
    let eager = whole(content, &inputs, 30);
    assert!(
        eager
            .lines()
            .iter()
            .filter(|line| line.loading.is_some())
            .count()
            >= 2
    );
    let mut laid = whole(content, &inputs, 30);
    for line in 0..=eager.len() {
        laid.restart(content, line);
        assert!(laid.extend(content, &inputs, usize::MAX));
        assert_same(&laid, &eager, &format!("restart({line})"));
    }
}

#[test]
fn a_layout_in_reading_mode_goes_on_from_a_checkpoint_between_pages() {
    let long: String = (0..4000)
        .map(|n| format!("Paragraph {n} of a long document, with words to wrap.\n\n"))
        .collect();
    let document = parse(&long);
    let content = Content::Document(&document);
    let inputs = reading();
    let eager = whole(content, &inputs, 512);
    // The checkpoints are the starts of the pages, and a restart goes back
    // to the last at or before the line.
    let starts: Vec<usize> = crate::typeset::reading::engine::pages(eager.lines())
        .iter()
        .map(|page| page.lines.start)
        .collect();
    assert!(starts.len() > 2, "{starts:?}");
    let mut laid = whole(content, &inputs, 512);
    let total = laid.len();
    let mut not_the_top = 0;
    for line in [
        0,
        1,
        starts[1] - 1,
        starts[1],
        starts[1] + 5,
        total / 2,
        total - 1,
    ] {
        let want = starts.iter().copied().filter(|&at| at <= line).max();
        laid.restart(content, line);
        assert_eq!(
            laid.len(),
            want.unwrap_or(0),
            "restart({line}) of {starts:?}"
        );
        not_the_top += usize::from(!laid.is_empty());
        assert!(laid.extend(content, &inputs, usize::MAX));
        assert_same(&laid, &eager, &format!("restart({line})"));
    }
    assert!(not_the_top >= 3);
}

/// Files with images that are loading, and a code block in stretches
/// between them: the bands settle on the lines they settle on, wherever
/// the steps stop.
#[test]
fn bands_settle_as_they_do_whole_wherever_the_steps_stop() {
    use crate::steps::{alike, assert_in_steps};
    let code = rust(700);
    let first = format!("![a](a.png)\n\n```rust\n{code}```\n\nTwo ![a](a.png)\n\n![a](a.png)\n");
    let shown = [file_of("a.md", &first), file_of("b.md", "![a](a.png)\n")];
    let inputs = loading_in(&[0, 1]);
    assert_in_steps("files with bands", Content::Files(&shown), alike(&inputs));
    let document = parse(&first);
    assert_in_steps(
        "a document with bands",
        Content::Document(&document),
        alike(&loading_in(&[0])),
    );
}

/// A header leaves rows to the shell and the status line, which an image
/// file below it is fitted to the rest of: the pen that the next step
/// makes knows how many.
#[test]
fn the_rows_a_header_leaves_to_others_are_kept_between_the_steps() {
    use crate::steps::{alike, assert_in_steps};
    use crate::typeset::images::{ImageKey, Source};
    let mut inputs = Inputs::new(Capabilities {
        height: Some(30),
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(60)
    });
    let key = ImageKey::from("a.png").in_document(0);
    inputs
        .images
        .insert(key, Ok(Source::Raster(image::RgbaImage::new(100, 3000))));
    let image = File {
        name: "a.png".to_owned(),
        document: Document::new(vec![Block::ImageFile("a.png".to_owned())]),
    };
    let shown = [image, file_of("b.md", "Two\n")];
    assert_in_steps("an image file", Content::Files(&shown), alike(&inputs));
    let whole = stream(Content::Files(&shown), (0, 0), &inputs, 512);
    let rows = whole
        .lines
        .iter()
        .filter(|line| line.image.is_some())
        .count();
    // The image is as tall as the window leaves room for: 30 rows less the
    // header's three and its blank line and the row below.
    assert_eq!(rows, 30 - 1 - 4, "{rows} rows");
}

/// Front matter that is hidden, or shown as its source, is known not to
/// be on a line before any line is laid out, and the layout in steps is
/// the layout whole in each style, in both engines.
#[test]
fn hidden_front_matter_is_known_at_the_start_and_the_steps_equal_the_whole() {
    use crate::steps::{Inputses, assert_in_steps};
    use crate::typeset::inputs::FrontMatterStyle;
    let document = parse("---\ntitle: A\nlist: [1, 2]\n---\n\nText.\n\nMore text.\n");
    for style in [
        FrontMatterStyle::Table,
        FrontMatterStyle::Code,
        FrontMatterStyle::Hide,
    ] {
        let each: Vec<Inputs> = (0..8)
            .map(|at| Inputs {
                front_matter: style,
                ..if at < 4 { inputs() } else { reading() }
            })
            .collect();
        for engine in each.chunks(4) {
            let refs: Inputses = std::array::from_fn(|at| &engine[at]);
            let name = format!("{style:?}");
            assert_in_steps(&name, Content::Document(&document), refs);
            let laid = Laid::begin(Content::Document(&document), 0, &engine[0]);
            let first = laid.blocks().ids().next().unwrap();
            assert_eq!(
                laid.blocks().is_hidden(first),
                style != FrontMatterStyle::Table
            );
        }
    }
}

/// What the layout says of the images it did not show again is said
/// when the layout is whole, once, and not at each step.
#[test]
fn a_layout_in_steps_lets_go_of_what_it_did_not_show_when_it_is_whole() {
    use crate::typeset::images::ImageKey;
    let document = parse("One\n\nTwo\n\nThree\n");
    let content = Content::Document(&document);
    let inputs = inputs();
    // A note of an earlier layout, and one that it says again.
    let key = ImageKey::from("a.png").in_document(0);
    inputs
        .images
        .note_still(&key, (2, 2), "an earlier note".to_owned());
    inputs.images.end_notes(0..1);
    assert_eq!(inputs.images.notes().len(), 1);
    let mut laid = Laid::begin(content, 0, &inputs);
    let mut steps = 0;
    while !laid.extend(content, &inputs, laid.len() + 1) {
        steps += 1;
        assert_eq!(inputs.images.notes().len(), 1, "after step {steps}");
    }
    assert!(steps >= 3, "{steps}");
    // The note was not said again: it goes with the end.
    assert_eq!(inputs.images.notes().len(), 0);
    // A layout that is whole does not end again.
    assert!(laid.extend(content, &inputs, usize::MAX));
}

/// The band of an image that loads covers the lines of the code above
/// it, those of every stretch: the pen that lays the last stretch out
/// knows how many rows the stretches before it made.
#[test]
fn the_rows_of_the_stretches_before_a_loading_band_are_kept_between_the_steps() {
    use crate::steps::{alike, assert_in_steps};
    use crate::typeset::images::{ImageKey, Placeholder, Source};
    let code = "graph TD\n".to_owned() + &"  A --> B\n".repeat(1200);
    let document = code_block(Some("mermaid"), &code, false);
    let mut inputs = inputs();
    let key = inputs.drawn(ImageKey::rendered("mermaid", &code));
    // An image without size is not placed, so the code is shown.
    let empty = Source::Raster(image::RgbaImage::new(0, 0));
    inputs.images.wait(key, Placeholder::Dimmed(empty));
    let content = Content::Document(&document);
    let laid = whole(content, &inputs, 512);
    let above = laid.lines().iter().filter_map(|line| line.loading.as_ref());
    // The band is on the last line and covers all the lines above it.
    assert_eq!(
        above.map(|loading| loading.above).max(),
        Some(laid.len() - 1)
    );
    assert_in_steps("a loading band", content, alike(&inputs));
}

/// A page is laid out of the blocks it takes and not of the whole run
/// that is held: a layout of many pages hands the layout of pages a few
/// times as many blocks as there are, not as many as pages times blocks.
#[test]
fn a_run_of_many_pages_is_handed_to_the_layout_a_few_times_over() {
    use crate::typeset::reading::pen::handed;
    let long: String = (0..20_000)
        .map(|n| format!("Paragraph {n} of a long document, with words to wrap.\n\n"))
        .collect();
    let document = parse(&long);
    let before = handed();
    let laid = whole(Content::Document(&document), &reading(), 512);
    let given = handed() - before;
    let pages = crate::typeset::reading::engine::pages(laid.lines()).len();
    assert!(pages > 8, "{pages} pages");
    assert!(
        given <= 4 * 20_000,
        "{given} blocks handed for 20000 in {pages} pages"
    );
}

/// A page may start at a block inside a `<details>`, or at its summary,
/// and a layout in steps and a restart go on from there as the whole
/// layout does. The top paragraphs shift where the pages start.
#[test]
fn a_page_that_starts_inside_a_details_is_restarted_as_it_is_laid_out_whole() {
    let mut nested_starts = 0;
    for shift in 0..5 {
        let mut source: String = (0..shift).map(|n| format!("Top {n}.\n\n")).collect();
        for n in 0..900 {
            source += &format!(
                "Paragraph {n} of a long document, with words to wrap.\n\n\
                     <details>\n<summary>Summary {n}</summary>\n\n\
                     First in {n}, with some words more.\n\nSecond in {n}.\n\n</details>\n\n"
            );
        }
        let document = parse(&source);
        let content = Content::Document(&document);
        let inputs = reading();
        let eager = whole(content, &inputs, 512);
        let starts: Vec<usize> = crate::typeset::reading::engine::pages(eager.lines())
            .iter()
            .map(|page| page.lines.start)
            .collect();
        assert!(starts.len() >= 3, "shift {shift}: {starts:?}");
        // One step per page is the layout whole.
        let mut laid = Laid::begin(content, 0, &inputs);
        while !laid.extend(content, &inputs, laid.len() + 1) {}
        assert_same(&laid, &eager, &format!("shift {shift}: in steps"));
        // The kept checkpoints are the page starts, and some of those
        // are inside a `<details>`.
        for &start in &starts[1..] {
            let lines = &eager.lines()[start..];
            let block = lines.iter().find_map(|line| line.inner).expect("a block");
            nested_starts += usize::from(eager.blocks().path(block).len() > 1);
        }
        for &start in &starts {
            for line in [start, start + 7] {
                laid.restart(content, line);
                let want = starts.iter().copied().filter(|&at| at <= line).max();
                assert_eq!(
                    laid.len(),
                    want.unwrap_or(0),
                    "shift {shift}: restart({line})"
                );
                assert!(laid.extend(content, &inputs, usize::MAX));
                assert_same(&laid, &eager, &format!("shift {shift}: restart({line})"));
            }
        }
    }
    assert!(nested_starts > 0, "no page started inside a <details>");
}
