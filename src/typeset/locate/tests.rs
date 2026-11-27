use super::*;
use crate::typeset::caps::Capabilities;
use crate::typeset::driver::{Content, Stream, content};
use crate::typeset::inputs::{FrontMatterStyle, Inputs};
use crate::typeset::model::Document;
use crate::typeset::parse::parse;
use crate::typeset::position::Within;
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::page::Wide;
use crate::typeset::style::{Origin, Span};
use crate::typeset::text::DocumentText;
use std::rc::Rc;

/// Which engine and how wide a window.
#[derive(Debug, Clone, Copy)]
enum Engine {
    Text(u16),
    Reading(u16, Wide),
}

fn inputs(engine: Engine, front_matter: FrontMatterStyle) -> Inputs {
    match engine {
        Engine::Text(width) => Inputs {
            front_matter,
            ..Inputs::new(Capabilities::kitty(width))
        },
        Engine::Reading(width, wide) => Inputs {
            reading: Some(Reading {
                pictures: Default::default(),
                glyphs: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide,
                snap: false,
                zoom: 100,
            }),
            front_matter,
            ..Inputs::new(Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(width)
            })
        },
    }
}

const ENGINES: [Engine; 5] = [
    Engine::Text(200),
    Engine::Text(40),
    Engine::Reading(200, Wide::Scroll),
    Engine::Reading(60, Wide::Scroll),
    Engine::Reading(60, Wide::Wrap),
];

const STYLES: [FrontMatterStyle; 3] = [
    FrontMatterStyle::Table,
    FrontMatterStyle::Hide,
    FrontMatterStyle::Code,
];

/// Every fixture and example in every engine and with each way of
/// showing front matter: the name, the document, its layout and map.
fn each_layout(mut check: impl FnMut(&str, &Document, &Stream, &Map, Engine)) {
    for path in crate::testing::documents() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let source = std::fs::read_to_string(&path).unwrap();
        let document = parse(&source);
        for engine in ENGINES {
            for style in STYLES {
                let stream = content(Content::Document(&document), &inputs(engine, style));
                let map = Map::new(&stream.lines, &stream.blocks);
                check(
                    &format!("{name} {engine:?} {style:?}"),
                    &document,
                    &stream,
                    &map,
                    engine,
                );
            }
        }
    }
}

#[test]
fn the_map_of_a_code_block_of_20000_lines_is_made_in_time() {
    // A code block shows a stretch of text for each of its lines, and
    // the map asks the rows of every one: it must not walk the lines of
    // the whole block each time, which took a minute in a debug build.
    // The bound is far above what the map takes and far below the walk's.
    let source = format!("```\n{}```\n", "let x = 1;\n".repeat(20_000));
    let document = parse(&source);
    let engine = Engine::Reading(100, Wide::Scroll);
    let stream = content(
        Content::Document(&document),
        &inputs(engine, FrontMatterStyle::Hide),
    );
    let started = std::time::Instant::now();
    let map = Map::new(&stream.lines, &stream.blocks);
    let took = started.elapsed();
    assert!(stream.lines.len() > 10_000, "{} lines", stream.lines.len());
    drop(map);
    assert!(took < std::time::Duration::from_secs(10), "{took:?}");
}

/// `Map::line_of` as it was before it stopped at the first candidate:
/// every stretch after the place is asked, and the smallest line wins.
fn line_of_by_scan(map: &Map, pos: &Pos) -> Option<usize> {
    let from = map.texts.partition_point(|shown| shown.hi < *pos);
    let from_point = map.points.partition_point(|(at, _)| at < pos);
    let mut best = (map.points[from_point..].iter())
        .map(|(_, line)| *line)
        .min();
    let texts = map.texts[from..]
        .iter()
        .filter(|shown| shown.hi > *pos || (shown.lo == shown.hi && shown.lo >= *pos))
        .filter(|shown| !shown.rows.is_empty());
    for shown in texts {
        let line = match &shown.at {
            At::Measured(measured) => {
                Map::measured_rows(shown, measured, &(pos.clone()..shown.hi.clone())).start
            }
            At::Item { page, item } if !shown.origin.picture => {
                let bytes = Map::shown_bytes(shown, &(pos.clone()..shown.hi.clone()));
                let page = &map.pages[*page];
                (page.parts.rows_of(*item, bytes)).map_or(shown.rows.start, |found| {
                    (page.rows.start + found.rows.start).clamp(shown.rows.start, page.rows.end)
                })
            }
            _ => shown.rows.start,
        };
        best = Some(best.map_or(line, |best| best.min(line)));
    }
    best
}

/// `line_of` is the smallest line of all the stretches at or after a
/// place, as the scan of all of them says, for a sample of the ends of
/// the stretches and the points the map has (every twentieth) and of the
/// the midpoint of each stretch inside one text (every fourth), in each
/// layout of each fixture; a stretch from one text to another has none. The scan is slow, so all of them would take over a minute.
#[test]
fn line_of_is_the_smallest_line_of_the_stretches_after_the_place() {
    let mut asked = 0;
    each_layout(|name, _, _, map, _| {
        let (mut places, mut inside): (Vec<Pos>, Vec<Pos>) = (Vec::new(), Vec::new());
        for shown in &map.texts {
            places.extend([shown.lo.clone(), shown.hi.clone()]);
            // A place inside the stretch, where the rows of a measured
            // stretch and of a text of a page are of their own.
            if let (
                Within::Text { text, offset: from },
                Within::Text {
                    text: same,
                    offset: to,
                },
            ) = (&shown.lo.within, &shown.hi.within)
                && text == same
                && to > &(from + 1)
            {
                let mut middle = shown.lo.clone();
                middle.within = Within::Text {
                    text: *text,
                    offset: (from + to) / 2,
                };
                inside.push(middle);
            }
        }
        places.extend(map.points.iter().map(|(at, _)| at.clone()));
        let sampled = (places.iter().step_by(20)).chain(inside.iter().step_by(4));
        for pos in sampled {
            assert_eq!(
                map.line_of(pos),
                line_of_by_scan(map, pos),
                "{name}: {pos:?}"
            );
            asked += 1;
        }
    });
    assert!(asked > 300, "{asked}");
}

/// The first line of each piece is where [`Map::start`] says.
#[test]
fn a_piece_starts_at_its_first_line() {
    each_layout(|name, _, stream, map, _| {
        let places = stream.blocks.places(&stream.lines);
        let mut checked = 0;
        for (line, place) in places.iter().enumerate() {
            let Some(piece) = place else { continue };
            let first = places
                .iter()
                .position(|other| other.as_ref() == Some(piece));
            assert_eq!(map.start(piece), first, "{name}: line {line}");
            checked += 1;
        }
        assert!(checked > 0 || stream.lines.is_empty(), "{name}");
    });
}

#[test]
fn a_range_of_a_text_is_shown_on_the_lines_that_show_it() {
    let document = parse("First paragraph.\n\nSecond paragraph.\n");
    let text = DocumentText::new([&document]);
    for engine in [Engine::Text(80), Engine::Reading(80, Wide::Scroll)] {
        let stream = content(
            Content::Document(&document),
            &inputs(engine, FrontMatterStyle::Table),
        );
        let map = Map::new(&stream.lines, &stream.blocks);
        let found = text.find("Second", false);
        assert_eq!(found.len(), 1);
        let parts = map.locate(&stream.lines, &found[0]).unwrap();
        assert_eq!(parts.len(), 1, "{engine:?}: {parts:?}");
        assert_eq!(parts[0].piece.path(), [1], "{engine:?}");
        let line = map.start(&parts[0].piece).unwrap() + parts[0].rows.start;
        assert!(stream.lines[line].inner.is_some(), "{engine:?}");
    }
}

/// The place of the byte `byte` of the text that `origin` is of, in the
/// block of line `line`.
fn place(stream: &Stream, line: &Line, origin: &Origin, byte: usize) -> Pos {
    let info = stream.blocks.get(line.inner.expect("a line with texts"));
    let at = origin
        .at(u32::try_from(byte).unwrap())
        .expect("a byte of it");
    Pos::text(info.file(), info.path(), origin.text as usize, at as usize)
}

/// The columns of the graphemes of a span of the cell layout, as the
/// span's size draws them: where each starts, from the span's start.
fn graphemes(span: &Span) -> Vec<(usize, usize)> {
    let size = span.style.size;
    if span.embed.is_some() {
        return vec![(0, 0)];
    }
    if size.fraction().is_some() {
        let mut column = 0;
        return (size.groups(&span.text).into_iter())
            .map(|group| {
                let found = (group.at, column);
                column += usize::from(size.scale()) * usize::from(group.cells);
                found
            })
            .collect();
    }
    (span.text.grapheme_indices(true))
        .map(|(at, _)| (at, size.width(&span.text[..at])))
        .collect()
}

/// The characters of the document text that the stretch of the text of
/// `pos` that starts a grapheme shows, as a range: one character on, or
/// the whole of a whole piece.
fn stretch(texts: &DocumentText, origin: &Origin, pos: &Pos) -> Range<Pos> {
    let Within::Text { text, offset } = pos.within else {
        panic!("a place in a text")
    };
    let pieces = texts.texts(pos.file, &pos.path);
    let doc = &pieces
        .get(text)
        .unwrap_or_else(|| panic!("{pos:?} is in text {text}, which the model does not have"))
        .text;
    let end = match origin.exact {
        true => doc[offset..]
            .chars()
            .next()
            .map_or(offset, |c| offset + c.len_utf8()),
        false => origin.doc.end as usize,
    };
    pos.clone()..Pos::text(pos.file, &pos.path, text, end.max(offset))
}

/// Whether the parts have line `line`.
fn has(map: &Map, lines: &[Line], parts: &[Part], line: usize) -> bool {
    let _ = lines;
    parts.iter().any(|part| {
        let start = map.start(&part.piece).unwrap();
        (start + part.rows.start..start + part.rows.end).contains(&line)
    })
}

#[test]
fn what_is_at_the_start_of_each_grapheme_of_a_line_is_its_place_and_locate_finds_the_line() {
    let (mut exact, mut whole) = (0, 0);
    for path in crate::testing::documents() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let document = parse(&std::fs::read_to_string(&path).unwrap());
        let texts = DocumentText::new([&document]);
        for width in [200, 40] {
            for style in STYLES {
                let stream = content(
                    Content::Document(&document),
                    &inputs(Engine::Text(width), style),
                );
                let map = Map::new(&stream.lines, &stream.blocks);
                for (index, line) in stream.lines.iter().enumerate() {
                    let mut column = wrap::width(&line.lead);
                    for span in &line.spans {
                        for (at, cells) in graphemes(span) {
                            let Some(origin) = span.origins.of(at) else {
                                continue;
                            };
                            let at_place = place(&stream, line, origin, at);
                            let found = map.hit(&stream.lines, index, column + cells);
                            let label = format!(
                                "{name} {width} {style:?}: line {index}, {:?} at {at}",
                                span.text
                            );
                            assert_eq!(found, Some(at_place.clone()), "{label}");
                            let range = stretch(&texts, origin, &at_place);
                            if origin.picture || range.start == range.end {
                                continue;
                            }
                            let parts = map.locate(&stream.lines, &range);
                            assert!(
                                has(&map, &stream.lines, &parts.expect(&label), index),
                                "{label}: {range:?}"
                            );
                            match origin.exact {
                                true => exact += 1,
                                false => whole += 1,
                            }
                        }
                        column += wrap::width(std::slice::from_ref(span));
                    }
                }
            }
        }
    }
    assert!(exact > 5000 && whole > 20, "{exact} {whole}");
}

/// The pieces of the parts of a result, or why there are none.
fn pieces(found: Result<Vec<Part>, Hidden>) -> Result<Vec<Vec<usize>>, Hidden> {
    found.map(|parts| {
        let mut pieces: Vec<Vec<usize>> = parts
            .iter()
            .map(|part| part.piece.path().to_vec())
            .collect();
        pieces.dedup();
        pieces
    })
}

#[test]
fn an_image_file_is_a_block_without_text_whose_place_is_the_block() {
    use crate::typeset::images::{ImageKey, Source};
    use crate::typeset::model::Block;
    let document = Document::new(vec![
        Block::ImageFile("a.png".to_owned()),
        Block::Paragraph(vec![crate::typeset::model::Inline::Text(
            "after".to_owned(),
        )]),
    ]);
    let mut inputs = inputs(Engine::Text(60), FrontMatterStyle::Table);
    inputs.caps.cell_pixels = Some((10, 20));
    let key = inputs.drawn(ImageKey::from("a.png"));
    inputs
        .images
        .insert(key, Ok(Source::Raster(::image::RgbaImage::new(40, 40))));
    let stream = content(Content::Document(&document), &inputs);
    let map = Map::new(&stream.lines, &stream.blocks);
    let rows: Vec<usize> = (0..stream.lines.len())
        .filter(|&line| stream.lines[line].image.is_some())
        .collect();
    assert!(!rows.is_empty(), "the image is drawn");
    let block = Pos::block(0, &[0]);
    for &row in &rows {
        assert_eq!(map.hit(&stream.lines, row, 0), Some(block.clone()));
    }
    let parts = map
        .locate(&stream.lines, &(block..Pos::end(0, &[0])))
        .unwrap();
    let start = map.start(&parts[0].piece).unwrap();
    assert_eq!(parts[0].piece.path(), [0]);
    assert_eq!(
        start + parts[0].rows.start..start + parts[0].rows.end,
        rows[0]..rows[0] + rows.len()
    );
}

#[test]
fn white_space_only_front_matter_shows_no_text_in_any_style() {
    // A fast, focused copy of the fixture-wide check in `layout/inlines.rs`
    // (`front-matter-blank.md` is one of its documents), for three sources.
    // The parser strips spaces and tabs from the ends of the lines, so a
    // no-break space is the white space that stays in the source.
    for source in ["---\n---\n", "---\n   \n---\n", "---\n\u{a0}\n---\n"] {
        let document = parse(&format!("{source}Body\n"));
        let texts = DocumentText::new([&document]);
        for style in STYLES {
            let stream = content(
                Content::Document(&document),
                &inputs(Engine::Text(60), style),
            );
            for line in &stream.lines {
                let Some(inner) = line.inner else { continue };
                let info = stream.blocks.get(inner);
                let pieces = texts.texts(info.file(), info.path());
                for origin in line.spans.iter().flat_map(|span| span.origins.iter()) {
                    assert!(
                        pieces.get(origin.text as usize).is_some(),
                        "{source:?} {style:?}: {:?} shows text {}, which the model does not have",
                        info.path(),
                        origin.text,
                    );
                }
            }
        }
    }
}

#[test]
fn a_picture_hides_its_text_and_is_found_at_the_start_of_it() {
    use crate::typeset::images::{ImageKey, Source};
    let document = parse("before ![the alt](p.png) after\n\n![lone alt](p.png)\n");
    let texts = DocumentText::new([&document]);
    for engine in [Engine::Text(60), Engine::Reading(60, Wide::Wrap)] {
        let mut with = inputs(engine, FrontMatterStyle::Table);
        let key = with.drawn(ImageKey::from("p.png"));
        with.images
            .insert(key, Ok(Source::Raster(::image::RgbaImage::new(20, 20))));
        let stream = content(Content::Document(&document), &with);
        let map = Map::new(&stream.lines, &stream.blocks);
        // The alt text of an image that is drawn is not found.
        for term in ["the alt", "lone alt"] {
            let found = texts.find(term, false);
            assert_eq!(found.len(), 1);
            assert_eq!(
                map.locate(&stream.lines, &found[0]).map(|_| ()),
                Err(Hidden::Picture),
                "{engine:?} {term}"
            );
        }
        // The text around it is.
        let found = texts.find("after", false);
        assert!(
            map.locate(&stream.lines, &found[0])
                .is_ok_and(|p| !p.is_empty())
        );
        // A range of both has the visible part.
        let both = Pos::text(0, &[0], 0, 0)..Pos::text(0, &[0], 0, 30);
        assert_eq!(pieces(map.locate(&stream.lines, &both)), Ok(vec![vec![0]]));
    }
    // Where the same image is not drawn, its text is.
    for engine in [Engine::Text(60), Engine::Reading(60, Wide::Wrap)] {
        let stream = content(
            Content::Document(&document),
            &inputs(engine, FrontMatterStyle::Table),
        );
        let map = Map::new(&stream.lines, &stream.blocks);
        for term in ["the alt", "lone alt"] {
            let found = texts.find(term, false);
            assert!(
                map.locate(&stream.lines, &found[0])
                    .is_ok_and(|p| !p.is_empty()),
                "{engine:?} {term}"
            );
        }
    }
    // A picture is at the start of its text.
    let mut with = inputs(Engine::Text(60), FrontMatterStyle::Table);
    let key = with.drawn(ImageKey::from("p.png"));
    with.images
        .insert(key, Ok(Source::Raster(::image::RgbaImage::new(20, 20))));
    let stream = content(Content::Document(&document), &with);
    let map = Map::new(&stream.lines, &stream.blocks);
    let (line, span) = (stream.lines.iter().enumerate())
        .find_map(|(line, laid)| {
            let span = laid.spans.iter().position(|span| span.embed.is_some())?;
            Some((line, span))
        })
        .expect("an embedded image");
    let laid = &stream.lines[line];
    let before = wrap::width(&laid.lead) + wrap::width(&laid.spans[..span]);
    assert_eq!(
        map.hit(&stream.lines, line, before + 1),
        Some(Pos::text(0, &[0], 0, 7))
    );
}

const FRONT_MATTER: &str = "---\ntitle: Secret\nauthor: Someone\n---\n\nBody text.\n";

#[test]
fn front_matter_that_is_hidden_or_shown_as_its_source_is_not_shown() {
    let document = parse(FRONT_MATTER);
    let texts = DocumentText::new([&document]);
    let key = texts.find("Secret", false);
    let body = texts.find("Body", false);
    assert_eq!((key.len(), body.len()), (1, 1));
    for engine in ENGINES {
        for style in STYLES {
            let stream = content(Content::Document(&document), &inputs(engine, style));
            let map = Map::new(&stream.lines, &stream.blocks);
            let label = format!("{engine:?} {style:?}");
            let found = map.locate(&stream.lines, &key[0]);
            match style {
                FrontMatterStyle::Table => {
                    assert!(
                        found.as_ref().is_ok_and(|p| !p.is_empty()),
                        "{label}: {found:?}"
                    )
                }
                _ => assert_eq!(found.map(|_| ()), Err(Hidden::FrontMatter), "{label}"),
            }
            let shown = map.locate(&stream.lines, &body[0]);
            assert!(
                shown.as_ref().is_ok_and(|p| !p.is_empty()),
                "{label}: {shown:?} {:?}",
                body[0]
            );
            // A range across the hidden and the visible has the visible.
            let across = key[0].start.clone()..body[0].end.clone();
            let pieces = pieces(map.locate(&stream.lines, &across));
            let last = vec![1];
            assert_eq!(pieces.map(|p| p.last().cloned()), Ok(Some(last)), "{label}");
        }
    }
}

/// Three ranges in each text of the document: its first character, the
/// one in the middle and the last.
fn samples(texts: &DocumentText) -> Vec<Range<Pos>> {
    let mut found = Vec::new();
    for pos in texts.positions() {
        let Within::Text { text, offset: 0 } = pos.within else {
            continue;
        };
        let pieces = texts.texts(pos.file, &pos.path);
        let doc = &pieces
            .get(text)
            .unwrap_or_else(|| panic!("{pos:?} is in text {text}, which the model does not have"))
            .text;
        let chars: Vec<usize> = doc.char_indices().map(|(at, _)| at).collect();
        for number in [0, chars.len() / 2, chars.len().saturating_sub(1)] {
            let Some(&from) = chars.get(number) else {
                continue;
            };
            let to = from + doc[from..].chars().next().map_or(0, char::len_utf8);
            let at = |offset| Pos::text(pos.file, &pos.path, text, offset);
            found.push(at(from)..at(to));
        }
    }
    found
}

/// Whether a difference between the engines is one that is known: a space
/// or a line break where the cell layout starts a new line is shown by no
/// cell, as the break between two lines of code is, and reading mode shows
/// it as a character.
fn known(
    said: &str,
    text: &Result<Vec<Vec<usize>>, Hidden>,
    page: &Result<Vec<Vec<usize>>, Hidden>,
) -> bool {
    said.trim().is_empty()
        && text == &Ok(Vec::new())
        && page.as_ref().is_ok_and(|pieces| !pieces.is_empty())
}

#[test]
fn the_same_range_is_in_the_same_pieces_in_both_engines() {
    let (mut compared, mut breaks, mut differing): (usize, usize, Vec<String>) = (0, 0, Vec::new());
    for path in crate::testing::documents() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let document = parse(&std::fs::read_to_string(&path).unwrap());
        let texts = DocumentText::new([&document]);
        for style in STYLES {
            let layout = |engine| {
                let stream = content(Content::Document(&document), &inputs(engine, style));
                let map = Map::new(&stream.lines, &stream.blocks);
                (stream, map)
            };
            let (text, text_map) = layout(Engine::Text(200));
            let (page, page_map) = layout(Engine::Reading(200, Wide::Scroll));
            for range in samples(&texts) {
                let one = pieces(text_map.locate(&text.lines, &range));
                let other = pieces(page_map.locate(&page.lines, &range));
                compared += 1;
                let said = texts.text(&range);
                if one != other && known(&said, &one, &other) {
                    breaks += 1;
                } else if one != other {
                    differing.push(format!(
                        "{name} {style:?} {:?} {said:?}: {one:?} and {other:?}",
                        range.start
                    ));
                }
            }
        }
    }
    assert!(compared > 5000, "{compared}");
    assert!(
        differing.is_empty(),
        "{} differ:\n{}",
        differing.len(),
        differing.join("\n")
    );
    // A known difference that no text shows any more is not known.
    assert!(breaks > 0, "the spaces at line breaks differ no more");
}

#[test]
fn what_is_at_a_cell_of_a_page_is_a_place_that_is_shown_on_that_row() {
    let (mut cells, mut files) = (0, 0);
    for path in crate::testing::documents() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let document = parse(&std::fs::read_to_string(&path).unwrap());
        let texts = DocumentText::new([&document]);
        for engine in [
            Engine::Reading(200, Wide::Scroll),
            Engine::Reading(60, Wide::Wrap),
        ] {
            for style in STYLES {
                let stream = content(Content::Document(&document), &inputs(engine, style));
                let map = Map::new(&stream.lines, &stream.blocks);
                for (index, line) in stream.lines.iter().enumerate() {
                    if line.image.as_ref().is_none_or(|image| image.page.is_none()) {
                        continue;
                    }
                    for column in 0..usize::from(Capabilities::kitty(200).width) {
                        let Some(pos) = map.hit(&stream.lines, index, column) else {
                            continue;
                        };
                        let Within::Text { text, offset } = pos.within else {
                            files += 1;
                            continue;
                        };
                        let pieces = texts.texts(pos.file, &pos.path);
                        let doc = &pieces
                            .get(text)
                            .unwrap_or_else(|| {
                                panic!("{pos:?} is in text {text}, which the model does not have")
                            })
                            .text;
                        let end = doc[offset..]
                            .chars()
                            .next()
                            .map_or(offset, |c| offset + c.len_utf8());
                        let range = pos.clone()..Pos::text(pos.file, &pos.path, text, end);
                        let found = map.locate(&stream.lines, &range);
                        let label = format!(
                            "{name} {engine:?} {style:?}: line {index} column {column} {range:?}"
                        );
                        match found {
                            Ok(parts) => {
                                assert!(has(&map, &stream.lines, &parts, index), "{label}")
                            }
                            Err(why) => assert_eq!(why, Hidden::Picture, "{label}"),
                        }
                        cells += 1;
                    }
                }
            }
        }
    }
    assert!(cells > 5000, "{cells} {files}");
}

#[test]
fn a_range_in_a_band_tells_the_pixels_across_it_and_each_band_has_its_own_part() {
    let long = "a".repeat(200);
    let source = format!(
        "> ```\n> {long}\n> ```\n>\n> text\n>\n> ```\n> {long} needle\n> ```\n\n```\n{long} other needle\n```\n"
    );
    let document = parse(&source);
    let texts = DocumentText::new([&document]);
    let stream = content(
        Content::Document(&document),
        &inputs(Engine::Reading(80, Wide::Scroll), FrontMatterStyle::Table),
    );
    let map = Map::new(&stream.lines, &stream.blocks);
    let bands: Vec<_> = (engine::bands(&stream.lines).into_iter())
        .map(|band| band.lines)
        .collect();
    assert_eq!(bands.len(), 3);
    let found = texts.find("needle", false);
    assert_eq!(found.len(), 2);
    for (found, lines) in found.iter().zip(&bands[1..]) {
        let parts = map.locate(&stream.lines, found).unwrap();
        let [part] = &parts[..] else {
            panic!("{parts:?}")
        };
        let across = part.band.as_ref().expect("in a band");
        assert!(
            across.pixels.start > 1000 && across.pixels.end > across.pixels.start,
            "{across:?}"
        );
        let start = map.start(&part.piece).unwrap();
        assert!(
            lines.contains(&(start + part.rows.start)),
            "{lines:?} {part:?}"
        );
    }
    // Text that is in no band says none.
    let text = texts.find("text", false);
    let parts = map.locate(&stream.lines, &text[0]).unwrap();
    assert!(parts.iter().all(|part| part.band.is_none()));
}

#[test]
fn what_the_layout_makes_up_is_no_place() {
    let document = parse("- item\n\n1. one\n\n> quoted\n\n```rust\nlet a = 1;\n```\n");
    let stream = content(
        Content::Document(&document),
        &inputs(Engine::Text(60), FrontMatterStyle::Table),
    );
    let map = Map::new(&stream.lines, &stream.blocks);
    let line_of = |text: &str| {
        stream
            .lines
            .iter()
            .position(|line| crate::typeset::style::text(&line.spans).contains(text))
            .unwrap()
    };
    // The marker and the number are not in the document.
    let item = line_of("item");
    let marker = wrap::width(&stream.lines[item].lead);
    assert_eq!(map.hit(&stream.lines, item, marker), None);
    assert!(
        map.hit(&stream.lines, item, marker + 3).is_some()
            || map.hit(&stream.lines, item, marker + 2).is_some()
    );
    let one = line_of("one");
    assert_eq!(map.hit(&stream.lines, one, 0), None);
    // Nor is the name of the language of a code block.
    let language = line_of("rust");
    let found = (0..60).filter_map(|column| map.hit(&stream.lines, language, column));
    assert_eq!(found.count(), 0);
}

#[test]
fn a_drawn_formula_and_diagram_are_found_at_the_start_of_their_source() {
    use crate::typeset::images::{ImageKey, Source};
    let code = "flowchart LR\n  a --> b\n";
    let document = parse(&format!("$$x$$\n\n```mermaid\n{code}```\n"));
    for engine in [Engine::Text(60), Engine::Reading(60, Wide::Wrap)] {
        let mut with = inputs(engine, FrontMatterStyle::Table);
        for key in [
            ImageKey::rendered("math", "x\n"),
            ImageKey::rendered("mermaid", code),
        ] {
            let key = with.drawn(key);
            with.images
                .insert(key, Ok(Source::Raster(::image::RgbaImage::new(40, 40))));
        }
        let stream = content(Content::Document(&document), &with);
        let map = Map::new(&stream.lines, &stream.blocks);
        for (path, source) in [([0], 0..1), ([1], 0..22)] {
            let block = Pos::block(0, &path);
            let whole = block..Pos::end(0, &path);
            let rows: Vec<usize> = (0..stream.lines.len())
                .filter(|&line| {
                    let row = stream.lines[line]
                        .inner
                        .map(|id| stream.blocks.get(id).path());
                    row == Some(&path[..]) && stream.lines[line].image.is_some()
                })
                .collect();
            assert!(!rows.is_empty(), "{engine:?} {path:?} is drawn");
            let middle = rows[rows.len() / 2];
            let shown = (0..40).find_map(|column| map.hit(&stream.lines, middle, column));
            let start = Pos::text(0, &path, 0, source.start);
            assert_eq!(shown, Some(start), "{engine:?} {path:?}");
            // The text behind the picture is not shown, and the block is.
            let text = Pos::text(0, &path, 0, 0)..Pos::text(0, &path, 0, source.end);
            assert_eq!(
                map.locate(&stream.lines, &text).map(|_| ()),
                Err(Hidden::Picture)
            );
            assert!(
                map.locate(&stream.lines, &whole)
                    .is_ok_and(|parts| !parts.is_empty())
            );
        }
    }
}

#[test]
fn a_row_of_front_matter_shown_as_its_source_is_at_the_block() {
    let document = parse(FRONT_MATTER);
    for engine in ENGINES {
        let stream = content(
            Content::Document(&document),
            &inputs(engine, FrontMatterStyle::Code),
        );
        let map = Map::new(&stream.lines, &stream.blocks);
        let block = Pos::block(0, &[0]);
        let rows: Vec<usize> = (0..stream.lines.len())
            .filter(|&line| {
                let inner = stream.lines[line].inner;
                inner.is_some_and(|id| stream.blocks.get(id).path() == [0])
            })
            .collect();
        assert!(!rows.is_empty(), "{engine:?}: the source is shown");
        let columns = usize::from(Capabilities::kitty(200).width);
        for &row in &rows {
            let found: Vec<Pos> = (0..columns)
                .filter_map(|column| map.hit(&stream.lines, row, column))
                .collect();
            let lead = wrap::width(&stream.lines[row].lead);
            for column in 0..lead {
                assert_eq!(
                    map.hit(&stream.lines, row, column),
                    None,
                    "{engine:?}: column {column} of row {row} is in the lead"
                );
            }
            assert!(!found.is_empty(), "{engine:?}: row {row} is at no place");
            assert!(
                found.iter().all(|pos| *pos == block),
                "{engine:?}: {found:?}"
            );
        }
        // The body below it is still at its text.
        let body = stream.lines.iter().position(|line| {
            line.inner
                .is_some_and(|id| stream.blocks.get(id).path() == [1])
        });
        let body = body.expect("the body is shown");
        let found = (0..columns).find_map(|column| map.hit(&stream.lines, body, column));
        assert!(
            matches!(
                found,
                Some(Pos {
                    within: Within::Text { .. },
                    ..
                })
            ),
            "{engine:?}"
        );
    }
}

/// The lines the parts are on, in order.
fn lines_of(map: &Map, parts: &[Part]) -> Vec<usize> {
    let mut lines = Vec::new();
    for part in parts {
        let start = map.start(&part.piece).unwrap();
        lines.extend(start + part.rows.start..start + part.rows.end);
    }
    lines
}

/// `p.png`, drawn 20 × 20 pixels, in `with`.
fn with_picture(with: &mut Inputs) {
    use crate::typeset::images::{ImageKey, Source};
    let key = with.drawn(ImageKey::from("p.png"));
    with.images
        .insert(key, Ok(Source::Raster(::image::RgbaImage::new(20, 20))));
}

#[test]
fn a_range_whose_picture_is_drawn_is_shown_on_the_lines_of_the_picture() {
    let document = parse("before ![the alt](p.png) after\n\n![lone alt](p.png)\n");
    let texts = DocumentText::new([&document]);
    for engine in [Engine::Text(60), Engine::Reading(60, Wide::Wrap)] {
        let mut with = inputs(engine, FrontMatterStyle::Table);
        with_picture(&mut with);
        let stream = content(Content::Document(&document), &with);
        let map = Map::new(&stream.lines, &stream.blocks);
        for (term, path) in [("the alt", [0]), ("lone alt", [1])] {
            let found = &texts.find(term, false)[0];
            // `locate` says it is hidden, and `drawn` where the picture is.
            assert_eq!(
                map.locate(&stream.lines, found).map(|_| ()),
                Err(Hidden::Picture),
                "{engine:?} {term}"
            );
            let parts = map.drawn(&stream.lines, found).expect("shown");
            assert_eq!(parts.len(), 1, "{engine:?} {term}: {parts:?}");
            assert_eq!(parts[0].piece.path(), path, "{engine:?} {term}");
            let lines = lines_of(&map, &parts);
            assert!(!lines.is_empty(), "{engine:?} {term}");
            for line in lines {
                let laid = &stream.lines[line];
                let picture = laid.image.is_some() || laid.spans.iter().any(|s| s.embed.is_some());
                assert!(picture, "{engine:?} {term}: line {line} is no picture's");
            }
        }
    }
}

#[test]
fn a_range_that_is_shown_as_text_is_where_locate_says_it_is_for_drawn_too() {
    let document = parse("before ![the alt](p.png) after\n\n![lone alt](p.png)\n");
    let texts = DocumentText::new([&document]);
    for engine in [Engine::Text(60), Engine::Reading(60, Wide::Wrap)] {
        // The picture is drawn: the text around it is text.
        let mut with = inputs(engine, FrontMatterStyle::Table);
        with_picture(&mut with);
        let stream = content(Content::Document(&document), &with);
        let map = Map::new(&stream.lines, &stream.blocks);
        let found = &texts.find("after", false)[0];
        let said = map.locate(&stream.lines, found);
        assert!(said.as_ref().is_ok_and(|p| !p.is_empty()), "{engine:?}");
        assert_eq!(map.drawn(&stream.lines, found), said, "{engine:?}");
        // The picture is not drawn: its alt text is text.
        let stream = content(
            Content::Document(&document),
            &inputs(engine, FrontMatterStyle::Table),
        );
        let map = Map::new(&stream.lines, &stream.blocks);
        for term in ["the alt", "lone alt"] {
            let found = &texts.find(term, false)[0];
            let said = map.locate(&stream.lines, found);
            assert!(said.as_ref().is_ok_and(|p| !p.is_empty()), "{engine:?}");
            assert_eq!(map.drawn(&stream.lines, found), said, "{engine:?} {term}");
        }
    }
}

/// The key of the picture or the image rows of the first line of
/// `piece` that has one.
fn key_on(stream: &Stream, lines: &[usize]) -> Option<crate::typeset::images::ImageKey> {
    lines.iter().find_map(|&line| {
        let laid = &stream.lines[line];
        let embed = laid.spans.iter().find_map(|span| span.embed.as_ref());
        let key = laid.image.as_ref().map(|image| &image.placement.key);
        key.or(embed.map(|embed| &embed.placement.key)).cloned()
    })
}

#[test]
fn the_picture_in_the_place_of_a_text_is_the_key_of_what_is_drawn_there() {
    use crate::typeset::images::{ImageKey, Source};
    let code = "flowchart LR\n  a --> b\n";
    let document = parse(&format!(
        "before ![the alt](p.png) after\n\n![lone alt](p.png)\n\n$$x$$\n\n```mermaid\n{code}```\n"
    ));
    let texts = DocumentText::new([&document]);
    let mut with = inputs(Engine::Text(60), FrontMatterStyle::Table);
    with_picture(&mut with);
    for key in [
        ImageKey::rendered("math", "x\n"),
        ImageKey::rendered("mermaid", code),
    ] {
        let key = with.drawn(key);
        with.images
            .insert(key, Ok(Source::Raster(::image::RgbaImage::new(40, 40))));
    }
    let stream = content(Content::Document(&document), &with);
    let map = Map::new(&stream.lines, &stream.blocks);
    let ranges = [
        texts.find("the alt", false)[0].clone(),
        texts.find("lone alt", false)[0].clone(),
        Pos::text(0, &[2], 0, 0)..Pos::text(0, &[2], 0, 1),
        Pos::text(0, &[3], 0, 0)..Pos::text(0, &[3], 0, 22),
    ];
    let mut keys = Vec::new();
    for range in &ranges {
        let parts = map.drawn(&stream.lines, range).expect("drawn");
        let on = key_on(&stream, &lines_of(&map, &parts)).expect("a picture on its lines");
        assert_eq!(
            map.picture(&stream.lines, range, &[]),
            Some(on.clone()),
            "{range:?}"
        );
        keys.push(on);
    }
    assert_eq!(keys.len(), 4);
    // Text that is no picture's has none, and neither has text that is
    // not drawn.
    let after = &texts.find("after", false)[0];
    assert_eq!(map.picture(&stream.lines, after, &[]), None);
    let bare = content(
        Content::Document(&document),
        &inputs(Engine::Text(60), FrontMatterStyle::Table),
    );
    let map = Map::new(&bare.lines, &bare.blocks);
    assert_eq!(map.picture(&bare.lines, &ranges[0], &[]), None);
}

#[test]
fn the_picture_of_an_image_file_is_the_key_of_its_rows() {
    use crate::typeset::images::{ImageKey, Source};
    use crate::typeset::model::Block;
    let document = Document::new(vec![Block::ImageFile("a.png".to_owned())]);
    let mut inputs = inputs(Engine::Text(60), FrontMatterStyle::Table);
    inputs.caps.cell_pixels = Some((10, 20));
    let key = inputs.drawn(ImageKey::from("a.png"));
    inputs
        .images
        .insert(key, Ok(Source::Raster(::image::RgbaImage::new(40, 40))));
    let stream = content(Content::Document(&document), &inputs);
    let map = Map::new(&stream.lines, &stream.blocks);
    let whole = Pos::block(0, &[0])..Pos::end(0, &[0]);
    let parts = map.drawn(&stream.lines, &whole).expect("drawn");
    let on = key_on(&stream, &lines_of(&map, &parts)).expect("rows of an image");
    assert_eq!(map.picture(&stream.lines, &whole, &[]), Some(on));
}

#[test]
fn a_picture_of_a_page_is_told_among_the_candidates_by_its_pixels() {
    use crate::typeset::images::animation::{Animation, Frame, Plays};
    use crate::typeset::images::{ImageKey, Source};
    let frame = Frame {
        image: ::image::RgbaImage::from_pixel(20, 20, ::image::Rgba([255, 0, 0, 255])),
        gap: std::time::Duration::from_millis(100),
    };
    let animation = Animation {
        frames: vec![frame; 2],
        plays: Plays::Forever,
        over_budget: false,
    };
    // An animation in a line of text stays on its first frame in reading
    // mode, so its hole does not carry its mark and it is not told.
    for (markdown, told) in [
        ("![the alt](a.gif)\n", true),
        ("text ![the alt](a.gif) text\n", false),
    ] {
        let document = parse(markdown);
        let texts = DocumentText::new([&document]);
        let mut with = inputs(Engine::Reading(60, Wide::Wrap), FrontMatterStyle::Table);
        with.images.insert(
            "a.gif",
            Ok(Source::Animated(std::sync::Arc::new(animation.clone()))),
        );
        let stream = content(Content::Document(&document), &with);
        let map = Map::new(&stream.lines, &stream.blocks);
        let alt = &texts.find("the alt", false)[0];
        let key = ImageKey::from("a.gif");
        let other = ImageKey::from("b.gif");
        assert_eq!(
            map.picture(&stream.lines, alt, &[other.clone(), key.clone()]),
            told.then_some(key),
            "{markdown}"
        );
        assert_eq!(
            map.picture(&stream.lines, alt, &[other]),
            None,
            "{markdown}"
        );
        assert_eq!(map.picture(&stream.lines, alt, &[]), None, "{markdown}");
        // And the lines of the picture are there to be shown.
        let parts = map.drawn(&stream.lines, alt).expect("drawn");
        assert!(!lines_of(&map, &parts).is_empty(), "{markdown}");
    }
}

#[test]
fn the_map_is_the_same_after_every_band_is_scrolled_and_selected() {
    // The pager makes the map before it shows the bands as scrolled, and
    // keeps it while the lines stay the same.
    let mut scrolled = 0;
    for path in crate::testing::documents() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let document = parse(&std::fs::read_to_string(&path).unwrap());
        for setting in [
            Engine::Reading(200, Wide::Scroll),
            Engine::Reading(60, Wide::Scroll),
        ] {
            for style in STYLES {
                let mut stream = content(Content::Document(&document), &inputs(setting, style));
                let map = Map::new(&stream.lines, &stream.blocks);
                let before = format!("{map:?}");
                let rows: Vec<_> = map.bands().iter().map(|band| band.lines.clone()).collect();
                let found = engine::bands(&stream.lines).into_iter();
                let found: Vec<_> = found.map(|band| band.lines).collect();
                assert_eq!(rows, found, "{name}");
                for band in map.bands() {
                    let offset = band.most() / 2 + 1;
                    engine::set_band(&mut stream.lines[band.lines.clone()], offset, true);
                    assert_eq!(band.scrolled(&stream.lines), (offset, true), "{name}");
                    scrolled += 1;
                }
                let after = format!("{:?}", Map::new(&stream.lines, &stream.blocks));
                assert_eq!(before, after, "{name} {setting:?} {style:?}");
            }
        }
    }
    assert!(scrolled > 50, "{scrolled} bands scrolled");
}

#[test]
fn the_first_place_on_a_line_and_the_first_line_of_a_place_agree() {
    let (mut checked, mut empty) = (0, 0);
    each_layout(|name, document, stream, map, _| {
        for line in 0..stream.lines.len() {
            let Some(first) = map.first_on(line) else {
                empty += 1;
                continue;
            };
            let found = map.line_of(first);
            assert!(
                found.is_some_and(|at| at <= line),
                "{name}: line {line} is first at {first:?}, which is at {found:?}"
            );
            checked += 1;
        }
        // A place that is shown is first on the line that shows it or an
        // earlier one, and that line starts at no later place.
        let texts = DocumentText::new([document]);
        for range in samples(&texts) {
            let Ok(parts) = map.locate(&stream.lines, &range) else {
                continue;
            };
            let Some(first) = lines_of(map, &parts).first().copied() else {
                continue;
            };
            let at = map.line_of(&range.start);
            assert!(
                at.is_some_and(|at| at <= first),
                "{name}: {range:?} at {at:?}, shown at {first}"
            );
            let first_there = map.first_on(at.unwrap());
            assert!(
                first_there.is_some_and(|there| *there <= range.start),
                "{name}: {range:?} is on line {at:?}, which starts at {first_there:?}"
            );
        }
    });
    assert!(
        checked > 1_000,
        "{checked} lines checked, {empty} without a place"
    );
}

#[test]
fn a_line_that_shows_nothing_of_the_document_has_no_first_place_and_the_end_has_no_line() {
    let document = parse("a\n\nb\n");
    let texts = DocumentText::new([&document]);
    for engine in [Engine::Text(60), Engine::Reading(60, Wide::Wrap)] {
        let stream = content(
            Content::Document(&document),
            &inputs(engine, FrontMatterStyle::Table),
        );
        let map = Map::new(&stream.lines, &stream.blocks);
        // The blank line between the two blocks, and a line past the end.
        let blank = (0..stream.lines.len())
            .find(|&line| stream.lines[line].inner.is_none())
            .expect("a blank line");
        // In reading mode a text's pixels may touch the row of the gap
        // below it, and then the row shows its start, as a search does.
        if matches!(engine, Engine::Text(_)) {
            assert_eq!(map.first_on(blank), None, "{engine:?}");
        }
        assert_eq!(map.first_on(stream.lines.len() + 5), None, "{engine:?}");
        let last = texts.positions().last().unwrap();
        assert_eq!(map.line_of(&last), None, "{engine:?}");
        let start = Pos::block(0, &[]);
        let first = map.line_of(&start).expect("a first line");
        let text = Pos::text(0, &[0], 0, 0);
        assert_eq!(map.first_on(first), Some(&text), "{engine:?}");
    }
}

#[test]
fn a_centred_wrapper_is_found_by_its_own_path_in_both_engines() {
    let source =
        "Before\n\n<div align=\"center\">\n\n# Title\n\nText in the wrapper.\n\n</div>\n\nAfter\n";
    let document = parse(source);
    let targets = crate::typeset::targets::targets([&document]);
    let mut tables = Vec::new();
    for engine in [Engine::Text(80), Engine::Reading(80, Wide::Scroll)] {
        let inputs = inputs(engine, FrontMatterStyle::Table);
        let stream = content(Content::Document(&document), &inputs);
        let map = Map::new(&stream.lines, &stream.blocks);
        let table: Vec<Vec<usize>> = (stream.blocks.ids())
            .map(|id| stream.blocks.path(id).to_vec())
            .collect();
        tables.push(table);
        let wrapper = Pos::block(0, &[1])..Pos::end(0, &[1]);
        let line = |pos: &Pos| map.line_of(pos);
        // It starts where its first block does and ends where the next
        // block starts, as a jump, a saved place or a selection takes it.
        assert_eq!(
            line(&wrapper.start),
            line(&Pos::block(0, &[1, 0])),
            "{engine:?}"
        );
        assert_eq!(line(&wrapper.end), line(&Pos::block(0, &[2])), "{engine:?}");
        let title = targets.named(0, "title").expect("the heading's place");
        assert_eq!(line(&title.range.start), line(&wrapper.start), "{engine:?}");
        // Its range, as a search hit or a selection over it, is laid out
        // and found on the lines of its blocks.
        assert!(!map.not_laid_out(&wrapper), "{engine:?}");
        let parts = map.locate(&stream.lines, &wrapper).expect("laid out");
        assert!(!parts.is_empty(), "{engine:?}");
        assert!(
            parts.iter().all(|part| part.piece.path().starts_with(&[1])),
            "{engine:?}: {parts:?}"
        );
    }
    assert_eq!(tables[0], tables[1]);
    assert!(tables[0].contains(&vec![1]), "{:?}", tables[0]);
}
