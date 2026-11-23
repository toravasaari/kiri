use super::*;
use crate::typeset::diagrams::renderer::NoRenderer;
use crate::typeset::driver::{self, Content};
use crate::typeset::inputs::Inputs;
use crate::typeset::model::{Document, File};
use crate::typeset::parse::{file as text_file, parse};

const MARKDOWN: &str = "# Title\n\nSome *words* with a [link](https://example.com) and `code`.\n\n\
        ## Section\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n\
        | a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\n\
        ![the first](one.png)\n\nText between.\n\n![the second](two.png)\n\n\
        - one\n- two\n\n<details>\n<summary>More</summary>\n\nHidden text.\n\n</details>\n";

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

fn cells() -> Inputs {
    Inputs::new(Capabilities::kitty(60))
}

fn files() -> Vec<File> {
    let file = |name: &str, document| File {
        name: name.to_owned(),
        document,
    };
    vec![
        file("a.md", parse(MARKDOWN)),
        file("b.txt", text_file("one\ntwo\n\nfour\n", None)),
    ]
}

/// What `lines` show, as text. A link of reading mode is told by the
/// address of the page's parts, which no two layouts share.
fn shown(lines: &[Line]) -> Vec<String> {
    let mut found = Vec::new();
    for line in lines {
        let text = format!("{line:?}");
        let mut line = String::new();
        let mut rest = text.as_str();
        while let Some(at) = rest.find("link: (") {
            let from = at + "link: (".len();
            let end = rest[from..].find(", ").map_or(rest.len(), |end| from + end);
            line += &rest[..from];
            line += "_";
            rest = &rest[end..];
        }
        line += rest;
        found.push(line);
    }
    found
}

/// The ranges the answers are held on: what each line shows, what all
/// of them show, and some places that no line shows.
fn ranges(map: &Map, lines: usize) -> Vec<Range<Pos>> {
    let mut ranges: Vec<Range<Pos>> = (0..lines).filter_map(|i| map.span_of(i..i + 1)).collect();
    ranges.extend(map.span_of(0..lines));
    ranges.push(Pos::block(0, &[0])..Pos::end(0, &[0]));
    ranges.push(Pos::block(0, &[7])..Pos::end(0, &[9]));
    ranges.push(Pos::block(1, &[0])..Pos::end(1, &[0]));
    ranges.push(Pos::block(5, &[0])..Pos::end(5, &[0]));
    ranges
}

/// Holds every answer of `laid` to what the stream and the map made of
/// the same lines say.
///
/// Each layout gets inputs of its own: reading mode keeps the pictures it
/// drew in them, and a second layout with the same ones is another.
fn assert_the_same(what: &str, content: Content, inputs: fn() -> Inputs) {
    let stream = driver::content(content, &inputs());
    let inputs = inputs();
    let map = Map::new(&stream.lines, &stream.blocks);
    let laid = driver::laid(content, &inputs);
    let lines = &stream.lines;
    assert!(lines.len() > 3, "{what}: too few lines to hold anything");
    assert_eq!(shown(laid.lines()), shown(lines), "{what}: lines");
    assert_eq!(
        (laid.len(), laid.is_empty()),
        (lines.len(), false),
        "{what}"
    );
    let n = lines.len();
    for range in [
        0..n,
        0..1,
        2..5,
        n - 2..n,
        n - 1..n + 7,
        n..n,
        n + 3..n + 9,
        Range { start: 5, end: 2 },
    ] {
        let end = range.end.min(n);
        let want = &lines[range.start.min(end)..end];
        assert_eq!(
            shown(laid.rows(range.clone())),
            shown(want),
            "{what}: rows {range:?}"
        );
    }
    for caps in [&inputs.caps.clone(), &Capabilities::plain(60)] {
        let want = crate::present::output::total_rows(lines, caps);
        assert_eq!(laid.height(caps), want, "{what}: height");
    }
    same_lines_hit(what, &laid, lines, &map);
    same_ranges(what, &laid, lines, &map);
    same_bands_and_pages(what, &laid, lines, &map);
}

/// What is on each line and at each column of it.
fn same_lines_hit(what: &str, laid: &Laid, lines: &[Line], map: &Map) {
    for line in 0..lines.len() {
        assert_eq!(
            laid.first_on(line),
            map.first_on(line),
            "{what}: first_on {line}"
        );
        for column in 0..70 {
            let want = map.hit(lines, line, column);
            assert_eq!(laid.hit(line, column), want, "{what}: hit {line}:{column}");
        }
    }
}

/// Where each range is shown.
fn same_ranges(what: &str, laid: &Laid, lines: &[Line], map: &Map) {
    let n = lines.len();
    let ranges = ranges(map, n);
    assert!(ranges.len() > 8, "{what}: {} ranges", ranges.len());
    let mut shown = 0;
    for range in &ranges {
        let located = map.locate(lines, range);
        shown += usize::from(located.as_ref().is_ok_and(|parts| !parts.is_empty()));
        for part in located.iter().flatten() {
            assert_eq!(
                laid.start(&part.piece),
                map.start(&part.piece),
                "{what}: start"
            );
        }
        assert_eq!(laid.locate(range), located, "{what}: locate {range:?}");
        assert_eq!(laid.drawn(range), map.drawn(lines, range), "{what}: drawn");
        assert_eq!(
            laid.drawn_rows(range),
            map.drawn_rows(lines, range),
            "{what}: drawn_rows"
        );
        assert_eq!(
            laid.line_bytes(range),
            map.line_bytes(range),
            "{what}: line_bytes"
        );
        assert_eq!(
            laid.holds_cells(range),
            map.holds_cells(range),
            "{what}: holds_cells"
        );
        assert_eq!(
            laid.line_of(&range.start),
            map.line_of(&range.start),
            "{what}: line_of"
        );
        assert_eq!(
            laid.line_of(&range.end),
            map.line_of(&range.end),
            "{what}: line_of"
        );
        assert_eq!(
            laid.picture(range, &[]),
            map.picture(lines, range, &[]),
            "{what}: picture"
        );
    }
    assert!(shown > 3, "{what}: only {shown} ranges are shown");
    for window in [0..n, 0..1, 1..4, n - 1..n + 4, n + 1..n + 2] {
        assert_eq!(
            laid.span_of(window.clone()),
            map.span_of(window.clone()),
            "{what}"
        );
    }
}

/// Reading mode's bands, and the pages that say what shows a range.
fn same_bands_and_pages(what: &str, laid: &Laid, lines: &[Line], map: &Map) {
    let ranges = ranges(map, lines.len());
    let kept = |bands: &[Band]| -> Vec<(Range<usize>, Option<_>, u32, u32, u32)> {
        let band = |band: &Band| {
            let block = band.block().cloned();
            (
                band.lines.clone(),
                block,
                band.most(),
                band.visible(),
                band.step(),
            )
        };
        bands.iter().map(band).collect()
    };
    assert_eq!(kept(laid.bands()), kept(map.bands()), "{what}: bands");
    // Reading mode's pages say what shows a range, each against its own.
    let own = crate::typeset::reading::engine::pages(laid.lines());
    let old = crate::typeset::reading::engine::pages(lines);
    assert_eq!(own.len(), old.len(), "{what}: pages");
    for (own, old) in own.iter().zip(&old) {
        for range in &ranges {
            let want = map.page_spots(&old.parts, range);
            assert_eq!(
                laid.page_spots(&own.parts, range),
                want,
                "{what}: page_spots"
            );
        }
    }
}

#[test]
fn laid_answers_as_the_stream_and_the_map_do_for_a_document_in_the_cell_layout() {
    let document = parse(MARKDOWN);
    assert_the_same("cells", Content::Document(&document), cells);
}

#[test]
fn laid_answers_as_the_stream_and_the_map_do_for_a_document_in_reading_mode() {
    let document = parse(MARKDOWN);
    assert_the_same("reading", Content::Document(&document), reading);
    let inputs = reading();
    // Reading mode has pages, whose texts the answers come from.
    let laid = driver::laid(Content::Document(&document), &inputs);
    assert!(!crate::typeset::reading::engine::pages(laid.lines()).is_empty());
}

#[test]
fn laid_answers_as_the_stream_and_the_map_do_for_a_text_file() {
    let source: String = (1..=40)
        .map(|n| format!("line {n} of the file\n"))
        .collect();
    let document = text_file(&source, Some("rust"));
    assert_the_same("text, cells", Content::Document(&document), cells);
    assert_the_same("text, reading", Content::Document(&document), reading);
}

#[test]
fn laid_answers_as_the_stream_and_the_map_do_for_several_files() {
    let files = files();
    assert_the_same("files, cells", Content::Files(&files), cells);
    assert_the_same("files, reading", Content::Files(&files), reading);
}

#[test]
fn a_laid_made_from_its_parts_is_the_one_the_driver_made() {
    let document = parse(MARKDOWN);
    let inputs = cells();
    let stream = driver::content(Content::Document(&document), &inputs);
    let made = Laid::new(stream.lines.clone(), stream.blocks.clone());
    let given = driver::laid(Content::Document(&document), &inputs);
    let from = Laid::from(driver::content(Content::Document(&document), &inputs));
    assert_eq!(made.lines(), given.lines());
    assert_eq!(from.lines(), given.lines());
    assert_eq!(
        format!("{:?}", made.blocks()),
        format!("{:?}", given.blocks())
    );
    assert!(Laid::default().is_empty());
    assert_eq!(Laid::default().rows(0..4), &[] as &[Line]);
}

#[test]
fn a_plain_terminal_takes_a_row_a_line_and_a_styling_one_the_scale_of_each() {
    let document = parse(MARKDOWN);
    let laid = driver::laid(Content::Document(&document), &cells());
    let plain = laid.height(&Capabilities::plain(60));
    assert_eq!(plain, laid.len());
    assert!(laid.height(&Capabilities::kitty(60)) > plain);
}

#[test]
fn the_map_is_made_once_and_when_asked() {
    let document = parse(MARKDOWN);
    let mut laid = driver::laid(Content::Document(&document), &cells());
    let before = crate::typeset::locate::maps_made();
    let (lines, map) = laid.parts_mut();
    assert!(map.span_of(0..lines.len()).is_some());
    let _ = laid.locate(&(Pos::block(0, &[0])..Pos::end(0, &[0])));
    laid.hit(1, 1);
    laid.lines_mut()[0].to_normal_size();
    assert_eq!(crate::typeset::locate::maps_made() - before, 1);
}

/// What `needs` says for `documents`, as the keys of their images.
fn wanted(documents: &[&Document], inline_images: bool) -> Vec<String> {
    let parts: Vec<ImagePart> = (documents.iter().enumerate())
        .map(|(number, document)| ImagePart {
            document,
            number,
            base: std::path::Path::new(""),
            by_line: false,
            inline_images,
        })
        .collect();
    let looks = Looks {
        diagram_base: 0,
        headings: None,
        fonts: None,
    };
    let keys = Laid::needs_keys(&parts, &NoRenderer, &looks);
    let (wanted, kept) = Laid::needs(&parts, &NoRenderer, &looks, None);
    assert_eq!(kept.len(), wanted.len());
    let again = (wanted.iter())
        .map(|(index, image)| image.key(&looks).in_document(parts[*index].number))
        .collect::<Vec<_>>();
    assert_eq!(again, keys);
    let old = plan::keys(&parts, &NoRenderer, &looks);
    assert_eq!(keys, old);
    keys.iter().map(|key| format!("{key:?}")).collect()
}

#[test]
fn needs_names_the_images_before_anything_is_laid_out() {
    let document = parse(MARKDOWN);
    let other = parse("![third](three.png)\n");
    let keys = wanted(&[&document, &other], false);
    assert_eq!(keys.len(), 3, "{keys:?}");
    assert!(
        keys[0].contains("one.png") && keys[1].contains("two.png"),
        "{keys:?}"
    );
    assert!(keys[2].contains("three.png"), "{keys:?}");
    let none = wanted(&[&parse("No images.\n")], true);
    assert!(none.is_empty());
}
