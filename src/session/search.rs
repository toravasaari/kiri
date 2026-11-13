//! Where a search term is in the document, and where that is shown: the term
//! is looked for in the document's text, each place is a range of it, and
//! the layout says on which lines the range is shown, or that it is not.
//! What the layout does not show, such as the text behind a picture or
//! hidden front matter, is not found, as in a browser.

use std::ops::Range;

use crate::session::frame::Shown;
use crate::session::settings::SearchCase;
use crate::typeset::caps::Capabilities;
use crate::typeset::locate::bridge::{self, Mark};
use crate::typeset::locate::{Across, Map};
use crate::typeset::position::Pos;
use crate::typeset::reading::page::ItemRef;
use crate::typeset::style::{Line, Style};
use crate::typeset::text::DocumentText;
use crate::typeset::theme::{SearchColor, Theme};

/// Where a search term is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// The range of the document's text it is.
    pub range: Range<Pos>,
    /// The line it starts on.
    pub line: usize,
    /// Where it is in a band of reading mode, when it is in one.
    pub band: Option<Across>,
    /// What shows it of each line or page, as painting finds it.
    pub marks: Vec<Mark>,
}

/// A search: its term, where it is in the document and which place the
/// reader is at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    term: String,
    /// In the order of the lines they start on, and then of the document.
    matches: Vec<Match>,
    /// The bytes of the matches that are marked in the text of a line:
    /// the line, the bytes and the match, in the order of the lines.
    on_lines: Vec<(usize, Range<usize>, usize)>,
    /// The index of the current match; `None` when there is none.
    current: Option<usize>,
}

impl Search {
    /// A search for `term` in `text`, the text of the documents `shown`
    /// shows, which are on `lines`, at its first match on line `top` or
    /// below it, or at its first match when there is none there.
    pub(crate) fn new(
        shown: &Shown,
        text: &DocumentText,
        lines: &[Line],
        term: &str,
        case: SearchCase,
        top: usize,
    ) -> Self {
        let matches = find(shown, text, lines, term, case);
        let current = matches
            .iter()
            .position(|found| found.line >= top)
            .or((!matches.is_empty()).then_some(0));
        let mut on_lines = Vec::new();
        for (index, found) in matches.iter().enumerate() {
            for mark in &found.marks {
                if let Mark::Bytes { line, bytes } = mark {
                    on_lines.push((*line, bytes.clone(), index));
                }
            }
        }
        on_lines.sort_by_key(|(line, bytes, _)| (*line, bytes.start));
        Self {
            term: term.to_owned(),
            matches,
            on_lines,
            current,
        }
    }

    /// This search again in `text`, the text of the documents `shown`
    /// shows, which are on `lines`, laid
    /// out anew: at the match it was at when that range is still found, as
    /// after a resize or when only a band was shown anew, and otherwise at
    /// the first on line `top` or below.
    pub(crate) fn again(
        &self,
        shown: &Shown,
        text: &DocumentText,
        lines: &[Line],
        case: SearchCase,
        top: usize,
    ) -> Self {
        let mut search = Self::new(shown, text, lines, &self.term, case, top);
        let kept = (self.current()).and_then(|current| {
            search
                .matches
                .iter()
                .position(|found| found.range == current.range)
        });
        if kept.is_some() {
            search.current = kept;
        }
        search
    }

    /// The term searched for.
    pub fn term(&self) -> &str {
        &self.term
    }

    /// The match the reader is at.
    pub fn current(&self) -> Option<&Match> {
        self.matches.get(self.current?)
    }

    /// Moves to the next match, or from the last to the first; true when
    /// it went around. When the current match is out of the lines
    /// `shown`, the next is the first from the top of them, as in `less`.
    pub fn forward(&mut self, shown: Range<usize>) -> bool {
        let Some(current) = self.current else {
            return false;
        };
        let next = if shown.contains(&self.matches[current].line) {
            current + 1
        } else {
            self.matches
                .partition_point(|found| found.line < shown.start)
        };
        let around = next == self.matches.len();
        self.current = Some(if around { 0 } else { next });
        around
    }

    /// Moves to the previous match, or from the first to the last; true
    /// when it went around. When the current match is out of the lines
    /// `shown`, the previous is the last above them, as in `less`.
    pub fn backward(&mut self, shown: Range<usize>) -> bool {
        let Some(current) = self.current else {
            return false;
        };
        let after = if shown.contains(&self.matches[current].line) {
            current
        } else {
            self.matches
                .partition_point(|found| found.line < shown.start)
        };
        let around = after == 0;
        self.current = Some(after.checked_sub(1).unwrap_or(self.matches.len() - 1));
        around
    }

    /// The place of the current match among all of them, counted from 1,
    /// and how many there are; `None` when there is none.
    pub fn count(&self) -> Option<(usize, usize)> {
        Some((self.current? + 1, self.matches.len()))
    }

    /// The bytes of the matches in the text of line `line`, from left to
    /// right, each with whether it is the current match. A match that a
    /// line break splits is on both lines. Matches in pages of reading mode
    /// and in images are not among them.
    pub fn on(&self, line: usize) -> Vec<(Range<usize>, bool)> {
        let start = self.on_lines.partition_point(|(at, ..)| *at < line);
        self.on_lines[start..]
            .iter()
            .take_while(|(at, ..)| *at == line)
            .map(|(_, bytes, found)| (bytes.clone(), Some(*found) == self.current))
            .collect()
    }

    /// The matches in the page `page`, each with whether it is the current
    /// match.
    pub fn in_page(&self, page: usize) -> Vec<(ItemRef, Range<usize>, bool)> {
        let mut found = Vec::new();
        for (index, matched) in self.matches.iter().enumerate() {
            for mark in &matched.marks {
                if let Mark::Glyphs {
                    page: at,
                    item,
                    range,
                } = mark
                    && *at == page
                {
                    found.push((*item, range.clone(), Some(index) == self.current));
                }
            }
        }
        found
    }
}

/// How matches are shown: the style laid over a match's text, and over
/// the current match's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marks {
    pub found: Style,
    pub current: Style,
}

impl Marks {
    /// The marks for `theme` on a terminal with `caps`.
    pub fn new(theme: &Theme, caps: &Capabilities) -> Self {
        let search = &theme.search;
        let style = |foreground, background| match (foreground, background) {
            _ if !caps.color => Style {
                reverse: true,
                ..Style::default()
            },
            (SearchColor::Auto, SearchColor::Auto) => Style {
                reverse: true,
                ..Style::default()
            },
            (foreground, background) => Style {
                fg: color(foreground),
                bg: color(background),
                ..Style::default()
            },
        };
        let current = Style {
            bold: true,
            ..style(search.current_foreground, search.current_background)
        };
        Self {
            found: style(search.match_foreground, search.match_background),
            current,
        }
    }
}

/// The colour `color` gives a match: none for the text's own.
fn color(color: SearchColor) -> Option<crate::typeset::color::Color> {
    match color {
        SearchColor::Color(color) => color,
        SearchColor::Auto => None,
    }
}

/// The places of `term` in the document that `shown` shows, from the top and
/// from left to right, none overlapping another, as far as they are shown on
/// `lines`. A no-break space matches a space. With `SearchCase::Smart`, a
/// term with no upper-case letter ignores case.
fn find(
    shown: &Shown,
    text: &DocumentText,
    lines: &[Line],
    term: &str,
    case: SearchCase,
) -> Vec<Match> {
    let ignore_case = match case {
        SearchCase::Smart => !term.chars().any(char::is_uppercase),
        SearchCase::Literal => false,
    };
    let map = Map::new(lines, shown.blocks);
    let mut found = Vec::new();
    for range in text.find(term, ignore_case) {
        // What no line shows is not a match, as in a browser.
        let Ok(parts) = map.locate(lines, &range) else {
            continue;
        };
        let Some(first) = parts.first() else {
            continue;
        };
        found.push(Match {
            line: map.start(&first.piece).unwrap_or(0) + first.rows.start,
            band: first.band.clone(),
            marks: bridge::marks(&map, &range),
            range,
        });
    }
    found.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.range.start.cmp(&b.range.start))
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{FrontMatterStyle, Inputs};
    use crate::session::layers::{Highlights, paint, stack};
    use crate::typeset::color::Color;
    use crate::typeset::layout::{Content, Stream, content};
    use crate::typeset::model::{Block, Document, File};
    use crate::typeset::parse::parse;
    use crate::typeset::reading::engine::Reading;
    use crate::typeset::reading::page::Wide;
    use crate::typeset::theme::{Paper, SearchTheme};
    use std::rc::Rc;

    #[derive(Debug, Clone, Copy)]
    enum Engine {
        Text(u16),
        Reading(u16, Wide),
    }

    const BOTH: [Engine; 3] = [
        Engine::Text(80),
        Engine::Reading(80, Wide::Wrap),
        Engine::Reading(80, Wide::Scroll),
    ];

    fn inputs(engine: Engine) -> Inputs {
        match engine {
            Engine::Text(width) => Inputs::new(Capabilities::kitty(width)),
            Engine::Reading(width, wide) => Inputs {
                reading: Some(Reading {
                    pictures: Default::default(),
                    fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                    wide,
                }),
                ..Inputs::new(Capabilities {
                    cell_pixels: Some((10, 20)),
                    ..Capabilities::kitty(width)
                })
            },
        }
    }

    /// The text of `content`, as the state's edition has it.
    fn text_of(content: Content) -> DocumentText {
        DocumentText::new(crate::session::edition::documents(content))
    }

    /// A search for `term` in `content` laid out with `inputs`, from the top
    /// line `top`, and what it came from.
    fn search_in(
        content_of: Content,
        inputs: &Inputs,
        (term, case, top): (&str, SearchCase, usize),
    ) -> (Search, Stream) {
        let stream = content(content_of, inputs);
        let places = stream.blocks.places(&stream.lines);
        let shown = Shown::new(content_of, &stream.blocks, &places);
        let text = text_of(content_of);
        let search = Search::new(&shown, &text, &stream.lines, term, case, top);
        (search, stream)
    }

    /// How many matches `term` has in `source` in `engine`.
    fn count(source: &str, engine: Engine, term: &str) -> usize {
        let document = parse(source);
        let (search, _) = search_in(
            Content::Document(&document),
            &inputs(engine),
            (term, SearchCase::Smart, 0),
        );
        search.matches.len()
    }

    /// `source` in a text window of `width` columns and `term`'s matches in
    /// it: the line and the bytes of each, as the line's text says.
    fn found_in_text(source: &str, width: u16, term: &str) -> Vec<(usize, Range<usize>)> {
        let document = parse(source);
        let (search, stream) = search_in(
            Content::Document(&document),
            &inputs(Engine::Text(width)),
            (term, SearchCase::Smart, 0),
        );
        (0..stream.lines.len())
            .flat_map(|line| search.on(line).into_iter().map(move |(b, _)| (line, b)))
            .collect()
    }

    #[test]
    fn a_term_is_found_in_a_table_cell_and_in_the_front_matter_that_is_shown() {
        let source =
            "---\ntitle: A needle\n---\n\n| a | b |\n|---|---|\n| one | needle |\n\nA needle.\n";
        for engine in BOTH {
            assert_eq!(count(source, engine, "needle"), 3, "{engine:?}");
        }
        // The cell is on the line of its row.
        let found = found_in_text(source, 80, "needle");
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].0 < found[1].0 && found[1].0 < found[2].0);
    }

    #[test]
    fn a_term_is_not_found_in_front_matter_that_is_hidden_or_shown_as_its_source() {
        let source = "---\ntitle: A needle\n---\n\nA needle.\n";
        let document = parse(source);
        for engine in BOTH {
            for (style, found) in [
                (FrontMatterStyle::Table, 2),
                (FrontMatterStyle::Hide, 1),
                (FrontMatterStyle::Code, 1),
            ] {
                let inputs = Inputs {
                    front_matter: style,
                    ..inputs(engine)
                };
                let (search, _) = search_in(
                    Content::Document(&document),
                    &inputs,
                    ("needle", SearchCase::Smart, 0),
                );
                assert_eq!(search.matches.len(), found, "{engine:?} {style:?}");
                assert_eq!(search.count().map(|(_, total)| total), Some(found));
            }
        }
    }

    #[test]
    fn a_term_across_the_wrap_of_a_line_is_found_in_the_text_layout_and_marked_on_both() {
        let source = "alpha beta gamma delta epsilon zeta eta theta\n";
        let document = parse(source);
        let inputs = inputs(Engine::Text(22));
        let (search, stream) = search_in(
            Content::Document(&document),
            &inputs,
            ("gamma delta epsilon", SearchCase::Smart, 0),
        );
        assert_eq!(search.matches.len(), 1);
        let lines: Vec<usize> = (0..stream.lines.len())
            .filter(|&line| !search.on(line).is_empty())
            .collect();
        assert!(lines.len() >= 2, "{lines:?}");
        // The break between the lines is no part of either mark, and the
        // text each marks is the term's words.
        let marked: Vec<String> = lines
            .iter()
            .flat_map(|&line| {
                let text = crate::typeset::style::text(&stream.lines[line].spans);
                search
                    .on(line)
                    .into_iter()
                    .map(move |(bytes, _)| text[bytes].to_owned())
            })
            .collect();
        assert_eq!(marked.join(" "), "gamma delta epsilon");
        // Both lines are painted.
        let marks = Marks::new(&Theme::default(), &Capabilities::kitty(22));
        let highlights = Highlights {
            tint: None,
            search: Some(&search),
            marks: &marks,
            fade: None,
            link: None,
        };
        let layers = stack(&highlights, &stream.lines, 0);
        let painted = paint(&stream.lines, 0, &layers);
        for &line in &lines {
            assert!(
                painted[line].spans.iter().any(|span| span.style.reverse),
                "line {line} is not painted: {:?}",
                painted[line]
            );
        }
        // The reading mode finds it too.
        assert_eq!(
            count(
                source,
                Engine::Reading(22, Wide::Wrap),
                "gamma delta epsilon"
            ),
            1
        );
    }

    #[test]
    fn a_term_is_not_found_in_the_name_in_the_header_of_a_file() {
        let files = [
            File {
                name: "notes-needle.md".to_owned(),
                document: parse("First file.\n"),
            },
            File {
                name: "second.md".to_owned(),
                document: parse("A needle in the second file.\n"),
            },
        ];
        for engine in [Engine::Text(80), Engine::Reading(80, Wide::Wrap)] {
            let (search, stream) = search_in(
                Content::Files(&files),
                &inputs(engine),
                ("needle", SearchCase::Smart, 0),
            );
            assert_eq!(search.matches.len(), 1, "{engine:?}");
            let current = search.current().unwrap();
            assert_eq!(current.range.start.file, 1);
            assert!(current.line < stream.lines.len());
        }
    }

    #[test]
    fn a_term_is_not_found_in_what_the_layout_makes_up_nor_in_what_kiri_says() {
        let source =
            "1. one\n2. two\n\n- [x] done\n\n```rust\nlet a = 1;\n```\n\n> [!NOTE]\n> Careful.\n";
        for engine in BOTH {
            for term in ["2.", "[x]", "rust", "NOTE", "Note", "[image", "# "] {
                assert_eq!(count(source, engine, term), 0, "{engine:?} {term:?}");
            }
            assert_eq!(count(source, engine, "careful"), 1, "{engine:?}");
        }
        let document = Document::new(vec![
            Block::Note("3 rows are left out".to_owned()),
            Block::Error("could not read".to_owned()),
            Block::Paragraph(vec![crate::typeset::model::Inline::Text("rows".to_owned())]),
        ]);
        for engine in BOTH {
            let (search, _) = search_in(
                Content::Document(&document),
                &inputs(engine),
                ("rows", SearchCase::Smart, 0),
            );
            assert_eq!(search.matches.len(), 1, "{engine:?}");
        }
    }

    #[test]
    fn the_text_behind_a_picture_is_found_when_the_picture_is_not_drawn() {
        use crate::typeset::images::{ImageKey, Source};
        let code = "flowchart LR\n  a --> b\n";
        let source = format!(
            "before ![the alt](p.png) after\n\n$$x + needle$$\n\n```mermaid\n{code}```\n\n![lone alt](p.png)\n"
        );
        let document = parse(&source);
        for engine in [Engine::Text(60), Engine::Reading(60, Wide::Wrap)] {
            let drawn = |term: &str, draw: bool| {
                let mut with = inputs(engine);
                if draw {
                    for key in [
                        ImageKey::from("p.png"),
                        ImageKey::rendered("math", "x + needle\n"),
                        ImageKey::rendered("mermaid", code),
                    ] {
                        let key = with.drawn(key);
                        with.images
                            .insert(key, Ok(Source::Raster(::image::RgbaImage::new(40, 40))));
                    }
                }
                let (search, _) = search_in(
                    Content::Document(&document),
                    &with,
                    (term, SearchCase::Smart, 0),
                );
                search.matches.len()
            };
            for term in ["the alt", "lone alt", "needle", "flowchart"] {
                assert_eq!(drawn(term, true), 0, "{engine:?} {term:?} drawn");
                assert_eq!(drawn(term, false), 1, "{engine:?} {term:?} as text");
            }
            // What is around the picture is found either way.
            assert_eq!(drawn("after", true), 1, "{engine:?}");
        }
    }

    /// Terms that the fixtures and examples have, to find in both engines.
    const TERMS: [&str; 8] = ["the", "e", "a b", "text", "code", "table", "|", "kiri"];

    #[test]
    fn both_engines_find_the_same_number_of_matches_in_every_fixture() {
        let mut total = 0;
        for path in crate::testing::documents() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let source = std::fs::read_to_string(&path).unwrap();
            for term in TERMS {
                let text = count(&source, Engine::Text(120), term);
                let page = count(&source, Engine::Reading(120, Wide::Wrap), term);
                let band = count(&source, Engine::Reading(120, Wide::Scroll), term);
                assert_eq!((text, band), (page, page), "{name} {term:?}");
                total += text;
            }
        }
        assert!(total > 2000, "{total}");
    }

    #[test]
    fn a_match_in_a_page_is_on_the_row_of_its_line_of_text() {
        let filler = "words that fill a line ".repeat(20);
        let source = format!(
            "{filler} needle at the end of a long paragraph.\n\nShort.\n\n| a |\n|---|\n| needle |\n\n> [!NOTE]\n> Another needle.\n"
        );
        let document = parse(&source);
        let (search, stream) = search_in(
            Content::Document(&document),
            &inputs(Engine::Reading(80, Wide::Scroll)),
            ("needle", SearchCase::Smart, 0),
        );
        let (_, total) = search.count().unwrap();
        assert_eq!(total, 3);
        let rows: Vec<usize> = search.matches.iter().map(|found| found.line).collect();
        // Not on the first row of the long paragraph, but on the row of
        // the line of text the word is on, and the others below in order.
        assert!(rows[0] >= 3, "{rows:?}");
        assert!(rows[0] < rows[1] && rows[1] < rows[2], "{rows:?}");
        // None is a match in the spans of a line, which are marked there.
        assert!((0..stream.lines.len()).all(|row| search.on(row).is_empty()));
        // The page's marks: all three, the first the current.
        let page = crate::typeset::reading::engine::pages(&stream.lines);
        let address = Rc::as_ptr(&page[0].parts) as usize;
        let marks = search.in_page(address);
        assert_eq!(marks.len(), 3);
        assert_eq!(
            marks.iter().map(|mark| mark.2).collect::<Vec<_>>(),
            [true, false, false]
        );
    }

    #[test]
    fn a_match_in_a_scrolling_band_tells_where_in_the_band() {
        let long = "a".repeat(200);
        let document = parse(&format!("Text.\n\n```\n{long} needle\n```\n"));
        let (search, _) = search_in(
            Content::Document(&document),
            &inputs(Engine::Reading(80, Wide::Scroll)),
            ("needle", SearchCase::Smart, 0),
        );
        let across = search
            .current()
            .and_then(|found| found.band.clone())
            .expect("in a band");
        assert_eq!(across.band, 0);
        assert!(across.pixels.start > 1000 && across.pixels.end > across.pixels.start);
    }

    #[test]
    fn the_current_match_stays_when_the_window_is_resized() {
        let source = "needle one\n\nneedle two\n\nneedle three\n".repeat(3);
        let document = parse(&source);
        for width in [60, 100, 30] {
            let (mut search, _) = search_in(
                Content::Document(&document),
                &inputs(Engine::Text(80)),
                ("needle", SearchCase::Smart, 0),
            );
            search.forward(0..100);
            search.forward(0..100);
            let range = search.current().unwrap().range.clone();
            assert_eq!(search.count(), Some((3, 9)));
            let narrow = inputs(Engine::Text(width));
            let stream = content(Content::Document(&document), &narrow);
            let places = stream.blocks.places(&stream.lines);
            let shown = Shown::new(Content::Document(&document), &stream.blocks, &places);
            let text = text_of(Content::Document(&document));
            let again = search.again(&shown, &text, &stream.lines, SearchCase::Smart, 0);
            assert_eq!(again.current().unwrap().range, range, "{width}");
        }
    }

    #[test]
    fn a_heading_shown_as_an_image_is_found_on_its_rows_and_a_term_across_its_lines_too() {
        let mut inputs = Inputs::new(Capabilities {
            text_sizing: false,
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(30)
        });
        let image = image::RgbaImage::from_pixel(900, 40, image::Rgba([200, 30, 30, 255]));
        let text = crate::typeset::images::Source::Text {
            image,
            breaks: vec![30, 60],
            ends: vec![28, 58, 90],
        };
        let key = inputs.drawn(crate::typeset::images::ImageKey::heading(
            1,
            "One two three",
            (10, 20),
        ));
        inputs.images.insert(key, Ok(text));
        let document = parse("# One two three\n");
        let found = |term| {
            let (search, stream) = search_in(
                Content::Document(&document),
                &inputs,
                (term, SearchCase::Smart, 0),
            );
            assert_eq!(stream.lines.len(), 6, "{:?}", stream.lines);
            search
                .matches
                .iter()
                .map(|found| found.line)
                .collect::<Vec<_>>()
        };
        // Two rows for each of three lines of the image: a word is on the
        // first row of its line, and a term across two lines starts there.
        assert_eq!(found("one"), [0]);
        assert_eq!(found("two"), [2]);
        assert_eq!(found("three"), [4]);
        assert_eq!(found("two three"), [2]);
    }

    /// The matches `term` has in `source` in the text layout, with the
    /// current one at `top`.
    fn text_search(source: &str, term: &str, top: usize) -> Search {
        let document = parse(source);
        search_in(
            Content::Document(&document),
            &inputs(Engine::Text(80)),
            (term, SearchCase::Smart, top),
        )
        .0
    }

    #[test]
    fn finds_every_place_of_the_term_line_by_line() {
        let found = found_in_text("kiri and kiri\n\na kiri\n", 80, "kiri");
        let bytes: Vec<_> = found.iter().map(|(_, bytes)| bytes.clone()).collect();
        assert_eq!(bytes, [0..4, 9..13, 2..6]);
        assert_eq!(found[0].0, found[1].0);
        assert!(found[1].0 < found[2].0);
    }

    #[test]
    fn finds_a_term_whose_style_changes_in_the_middle() {
        assert_eq!(found_in_text("a **ki**ri\n", 80, "kiri").len(), 1);
        let found = found_in_text("a **ki**ri\n", 80, "kiri");
        assert_eq!(found[0].1, 2..6);
    }

    #[test]
    fn places_do_not_overlap() {
        let found = found_in_text("aaaa\n", 80, "aa");
        let bytes: Vec<_> = found.iter().map(|(_, bytes)| bytes.clone()).collect();
        assert_eq!(bytes, [0..2, 2..4]);
    }

    #[test]
    fn a_term_without_upper_case_ignores_case_by_default() {
        let document = parse("Kiri KIRI kiri\n");
        let case = |term, case| {
            search_in(
                Content::Document(&document),
                &inputs(Engine::Text(80)),
                (term, case, 0),
            )
            .0
            .matches
            .len()
        };
        assert_eq!(case("kiri", SearchCase::Smart), 3);
        assert_eq!(case("Kiri", SearchCase::Smart), 1);
        assert_eq!(case("kiri", SearchCase::Literal), 1);
    }

    #[test]
    fn a_no_break_space_matches_a_space() {
        let found = found_in_text("run\u{a0}make\u{a0}check\n", 80, "make check");
        assert_eq!(found[0].1, 5..16);
    }

    #[test]
    fn places_are_in_bytes_of_the_original_text() {
        // `İ` is two bytes and lower-cases to two characters.
        let found = found_in_text("İ Äiti äiti\n", 80, "äiti");
        let bytes: Vec<_> = found.iter().map(|(_, bytes)| bytes.clone()).collect();
        assert_eq!(bytes, [3..8, 9..14]);
    }

    /// Lines `0..` of `kiri` paragraphs with one blank line between.
    const PARAGRAPHS: &str = "kiri\n\na\n\nkiri kiri\n";

    #[test]
    fn a_search_starts_at_the_first_match_from_the_top_line() {
        let search = text_search(PARAGRAPHS, "kiri", 2);
        assert_eq!(search.term(), "kiri");
        assert_eq!(search.count(), Some((2, 3)));
        assert_eq!(search.current().unwrap().line, 4);
        let search = text_search(PARAGRAPHS, "kiri", 0);
        assert_eq!(search.current().unwrap().line, 0);
    }

    #[test]
    fn a_search_with_no_match_below_the_top_starts_at_the_first() {
        let search = text_search("kiri\n\na\n", "kiri", 2);
        assert_eq!(search.current().unwrap().line, 0);
        let search = text_search("kiri\n\na\n", "xyz", 0);
        assert_eq!(search.current(), None);
        assert_eq!(search.count(), None);
    }

    #[test]
    fn next_and_previous_go_around_the_ends() {
        let mut search = text_search("kiri\n\nkiri kiri\n", "kiri", 2);
        assert_eq!(search.count(), Some((2, 3)));
        assert!(!search.forward(0..4));
        assert_eq!(search.count(), Some((3, 3)));
        assert!(search.forward(0..4));
        assert_eq!(search.count(), Some((1, 3)));
        assert!(search.backward(0..4));
        assert_eq!(search.count(), Some((3, 3)));
        assert!(!search.backward(0..4));
        assert_eq!(search.count(), Some((2, 3)));
    }

    #[test]
    fn next_and_previous_start_from_the_view_when_the_current_match_is_out_of_it() {
        let source = "kiri\n\na\n\nkiri\n\na\n\nkiri\n\na\n";
        let line = |search: &Search| search.current().unwrap().line;
        // Matches on the lines 0, 4 and 8. Scrolled away from the current
        // match to lines 3 to 5: the next match is the first in the view, the
        // previous the last above it.
        let mut search = text_search(source, "kiri", 0);
        assert!(!search.forward(3..6));
        assert_eq!(line(&search), 4);
        let mut search = text_search(source, "kiri", 8);
        assert!(!search.backward(3..6));
        assert_eq!(line(&search), 0);
        // With no match from the view down, or above it, around the end.
        let mut search = text_search(source, "kiri", 0);
        assert!(search.forward(9..11));
        assert_eq!(line(&search), 0);
        let mut search = text_search(source, "kiri", 8);
        assert!(search.backward(0..1));
        assert_eq!(line(&search), 8);
    }

    #[test]
    fn next_and_previous_without_matches_stay() {
        let mut search = text_search("a\n", "kiri", 0);
        assert!(!search.forward(0..2));
        assert!(!search.backward(0..2));
        assert_eq!(search.current(), None);
    }

    #[test]
    fn an_empty_term_finds_nothing() {
        assert_eq!(text_search("kiri\n", "", 0).count(), None);
    }

    #[test]
    fn the_matches_on_a_line_tell_the_current_one() {
        let mut search = text_search("kiri\n\nkiri a kiri\n\nkiri\n", "kiri", 2);
        assert_eq!(search.on(2), [(0..4, true), (7..11, false)]);
        search.forward(0..5);
        assert_eq!(search.on(2), [(0..4, false), (7..11, true)]);
        assert_eq!(search.on(0), [(0..4, false)]);
        assert_eq!(search.on(1), []);
    }

    #[test]
    fn matches_are_in_reverse_video_and_the_current_one_bold_by_default() {
        let marks = Marks::new(&Theme::default(), &Capabilities::kitty(80));
        let current = Style {
            reverse: true,
            bold: true,
            ..Style::default()
        };
        assert_eq!(
            marks,
            Marks {
                found: Style {
                    reverse: true,
                    ..Style::default()
                },
                current,
            }
        );
        // Without colours, the paper themes' greys give way too.
        let mut caps = Capabilities::kitty(80);
        caps.color = false;
        assert_eq!(Marks::new(&Theme::paper(Paper::Light), &caps), marks);
    }

    #[test]
    fn colours_from_the_theme_mark_matches_without_reverse_video() {
        let grey = |level| Some(Color::Rgb(level, level, level));
        let caps = Capabilities::kitty(80);
        let light = Marks::new(&Theme::paper(Paper::Light), &caps);
        assert_eq!((light.found.fg, light.found.bg), (grey(0x00), grey(0xcc)));
        assert!(!light.found.reverse && !light.found.bold);
        assert_eq!(
            (light.current.fg, light.current.bg),
            (grey(0xff), grey(0x1f))
        );
        assert!(!light.current.reverse && light.current.bold);
        // One colour given: the other is the text's own.
        let theme = Theme {
            search: SearchTheme {
                current_background: SearchColor::Color(Some(Color::Palette(3))),
                ..SearchTheme::default()
            },
            ..Theme::default()
        };
        let marks = Marks::new(&theme, &caps);
        assert!(marks.found.reverse);
        assert_eq!(
            (marks.current.fg, marks.current.bg),
            (None, Some(Color::Palette(3)))
        );
        assert!(!marks.current.reverse && marks.current.bold);
    }
}
