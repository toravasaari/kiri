//! Where a search term is in the laid-out lines of a document. A line's
//! text is the text of its spans one after another, so a word whose style
//! changes in the middle is found too.

use std::ops::Range;
use std::rc::Rc;

use crate::session::settings::SearchCase;
use crate::typeset::caps::Capabilities;
use crate::typeset::reading::engine;
use crate::typeset::reading::page::ItemRef;
use crate::typeset::style::{self, ImageRow, Line, Style};
use crate::typeset::theme::{SearchColor, Theme};

/// Where a search term is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// The line it is on.
    pub line: usize,
    /// Its bytes in the line's text: the text of its spans one after
    /// another. Empty for a match in a page of reading mode, which `place`
    /// tells.
    pub bytes: Range<usize>,
    /// Where it is in a page of reading mode, when it is in one.
    pub place: Option<PageMatch>,
}

/// A match in a page of reading mode: which page, which of its texts and
/// the bytes of it, and where in a band, if it is in one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageMatch {
    /// The page, by the address of its parts, which is the same for the
    /// rows of one layout.
    pub page: usize,
    pub item: ItemRef,
    pub range: Range<usize>,
    /// The band it is in, and the pixels of it across the band's content.
    pub band: Option<(u32, Range<u32>)>,
}

/// A search: its term, where it is in the document and which place the
/// reader is at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    term: String,
    matches: Vec<Match>,
    /// The index of the current match; `None` when there is none.
    current: Option<usize>,
}

impl Search {
    /// A search for `term` in `lines`, at its first match on line `top` or
    /// below it, or at its first match when there is none there.
    pub fn new(lines: &[Line], term: &str, case: SearchCase, top: usize) -> Self {
        let matches = find(lines, term, case);
        let current = matches
            .iter()
            .position(|found| found.line >= top)
            .or((!matches.is_empty()).then_some(0));
        Self {
            term: term.to_owned(),
            matches,
            current,
        }
    }

    /// This search again in `lines`, laid out anew: at the match it was at
    /// when that is still among them, as when only a band was shown
    /// anew, and otherwise at the first on line `top` or below.
    pub fn again(&self, lines: &[Line], case: SearchCase, top: usize) -> Self {
        let mut search = Self::new(lines, &self.term, case, top);
        // Only a match in a page is known again so, and only when all of it
        // is the same, its line and its place in the page and a band, as
        // when the lines are the same ones with a band shown anew; after a
        // resize the lines differ and the search starts from the top.
        let kept = (self.current())
            .filter(|current| current.place.is_some())
            .and_then(|current| search.matches.iter().position(|found| found == current));
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

    /// The bytes of the matches on line `line`, from left to right, each
    /// with whether it is the current match. Matches in pages of reading
    /// mode are not among them.
    pub fn on(&self, line: usize) -> Vec<(Range<usize>, bool)> {
        let start = self.matches.partition_point(|found| found.line < line);
        self.matches[start..]
            .iter()
            .enumerate()
            .take_while(|(_, found)| found.line == line)
            .filter(|(_, found)| found.place.is_none())
            .map(|(index, found)| (found.bytes.clone(), Some(start + index) == self.current))
            .collect()
    }

    /// The matches in the page `page`, each with whether it is the current
    /// match.
    pub fn in_page(&self, page: usize) -> Vec<(ItemRef, Range<usize>, bool)> {
        (self.matches.iter().enumerate())
            .filter_map(|(index, found)| {
                let place = found.place.as_ref().filter(|place| place.page == page)?;
                Some((place.item, place.range.clone(), Some(index) == self.current))
            })
            .collect()
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

/// The places of `term` in `lines`, from the top and from left to right,
/// none overlapping another. A no-break space matches a space. With
/// `SearchCase::Smart`, a term with no upper-case letter ignores case.
fn find(lines: &[Line], term: &str, case: SearchCase) -> Vec<Match> {
    let ignore_case = match case {
        SearchCase::Smart => !term.chars().any(char::is_uppercase),
        SearchCase::Literal => false,
    };
    let term: Vec<char> = term.chars().collect();
    let mut found: Vec<Match> = lines
        .iter()
        .enumerate()
        .flat_map(|(index, line)| {
            places(&searched(line), &term, ignore_case)
                .into_iter()
                .map(move |bytes| Match {
                    line: index,
                    bytes,
                    place: None,
                })
        })
        .chain(page_matches(lines, &term, ignore_case))
        .collect();
    // Lines in order; the matches of a page, which are on its rows, come
    // after the text matches of the same line.
    found.sort_by_key(|found| found.line);
    found
}

/// The matches in the texts of the pages of reading mode on `lines`,
/// each on the row that its first line of text is on.
fn page_matches(lines: &[Line], term: &[char], ignore_case: bool) -> Vec<Match> {
    let mut found = Vec::new();
    for page in engine::pages(lines) {
        let (_, cell) = page.parts.cells();
        let address = Rc::as_ptr(&page.parts) as usize;
        let hits = page
            .parts
            .find(&|text: &str| places(text, term, ignore_case));
        for hit in hits {
            let row = (hit.top / f32::from(cell.1)).floor().max(0.0) as usize;
            let line = (page.lines.start + row).min(page.lines.end - 1);
            let band = hit.item.band.map(|band| {
                let first = hit.pieces[0];
                (
                    band as u32,
                    first.x.floor().max(0.0) as u32..(first.x + first.width).ceil() as u32,
                )
            });
            found.push(Match {
                line,
                bytes: 0..0,
                place: Some(PageMatch {
                    page: address,
                    item: hit.item,
                    range: hit.range,
                    band,
                }),
            });
        }
    }
    found
}

/// The text a search looks in on `line`: its spans', and after them the
/// text that the image on its first row shows, such as a heading's, or
/// the text of a block of reading mode's page that starts on it. A match
/// there has bytes past the spans, so it is not marked.
fn searched(line: &Line) -> String {
    let mut text = style::text(&line.spans);
    // The rows of a page of reading mode have no text to find on them:
    // its texts are found where they are.
    let first = |image: &&ImageRow| image.row == 0 && image.page.is_none();
    if let Some(image) = line.image.as_ref().filter(first) {
        text.push_str(image.text.as_deref().unwrap_or_default());
    }
    text
}

/// The bytes of each place of `term` in `text`, from left to right and
/// none overlapping another.
fn places(text: &str, term: &[char], ignore_case: bool) -> Vec<Range<usize>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::color::Color;
    use crate::typeset::style::Span;
    use crate::typeset::theme::{Paper, SearchTheme};

    fn line(text: &str) -> Line {
        Line::new(vec![Span::new(text, Style::default())])
    }

    fn at(line: usize, bytes: Range<usize>) -> Match {
        Match {
            line,
            bytes,
            place: None,
        }
    }

    /// Row `row` of a heading `text` shown as an image of two rows.
    fn heading_row(text: &str, row: u16) -> Line {
        let placement = crate::typeset::images::size::Placement {
            key: crate::typeset::images::ImageKey::heading(1, text, (10, 20)),
            size: crate::typeset::images::size::Size {
                width: 100,
                height: 40,
                columns: 10,
                rows: 2,
            },
            shows: crate::typeset::images::size::Shows::Image,
        };
        let mut line = Line::image(std::rc::Rc::new(placement), row);
        if let Some(image) = &mut line.image {
            image.text = Some(text.to_owned());
        }
        line
    }

    /// `source` laid out in reading mode, in a window of 80 cells.
    fn reading_lines(source: &str) -> Vec<Line> {
        let document = crate::typeset::parse::parse(source);
        let inputs = crate::render::Inputs {
            reading: Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Scroll,
            }),
            ..crate::render::Inputs::new(Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            })
        };
        crate::typeset::layout::content(
            crate::typeset::layout::Content::Document(&document),
            &inputs,
        )
        .lines
    }

    #[test]
    fn a_match_in_a_page_is_on_the_row_of_its_line_of_text() {
        let filler = "words that fill a line ".repeat(20);
        let source = format!(
            "{filler} needle at the end of a long paragraph.\n\nShort.\n\n| a |\n|---|\n| needle |\n\n> [!NOTE]\n> Another needle.\n"
        );
        let lines = reading_lines(&source);
        let search = Search::new(&lines, "needle", SearchCase::Smart, 0);
        let (current, total) = search.count().unwrap();
        assert_eq!((current, total), (1, 3));
        let rows: Vec<usize> = search.matches.iter().map(|found| found.line).collect();
        // Not on the first row of the long paragraph, but on the row of
        // the line of text the word is on, and the others below in order.
        assert!(rows[0] >= 3, "{rows:?}");
        assert!(rows[0] < rows[1] && rows[1] < rows[2], "{rows:?}");
        assert!(search.matches.iter().all(|found| found.place.is_some()));
        // None is a match in the spans of a line, which are marked there.
        assert!(rows.iter().all(|&row| search.on(row).is_empty()));
        // The page's marks: all three, the first the current.
        let page = search.matches[0].place.as_ref().unwrap().page;
        let marks = search.in_page(page);
        assert_eq!(marks.len(), 3);
        assert_eq!(
            marks.iter().map(|mark| mark.2).collect::<Vec<_>>(),
            [true, false, false]
        );
    }

    #[test]
    fn a_match_in_a_scrolling_band_tells_where_in_the_band() {
        let long = "a".repeat(200);
        let lines = reading_lines(&format!("Text.\n\n```\n{long} needle\n```\n"));
        let search = Search::new(&lines, "needle", SearchCase::Smart, 0);
        let place = search
            .current()
            .and_then(|found| found.place.as_ref())
            .unwrap();
        let (band, x) = place.band.clone().expect("in a band");
        assert_eq!(band, 0);
        assert!(x.start > 1000 && x.end > x.start, "{x:?}");
    }

    #[test]
    fn search_finds_a_heading_shown_as_an_image() {
        let lines = [
            line("a"),
            heading_row("Kiri heading", 0),
            heading_row("Kiri heading", 1),
            line("kiri"),
        ];
        // Once, on the image's first row.
        assert_eq!(
            find(&lines, "kiri", SearchCase::Smart),
            [at(1, 0..4), at(3, 0..4)]
        );
        assert_eq!(find(&lines, "heading", SearchCase::Smart), [at(1, 5..12)]);
    }

    #[test]
    fn search_finds_the_words_of_each_line_of_a_wrapped_heading() {
        let mut inputs = crate::render::Inputs::new(Capabilities {
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
        let document = crate::typeset::parse::parse("# One two three\n");
        let lines = crate::typeset::layout::layout(&document, &inputs);
        // Two rows for each of three lines of the image.
        assert_eq!(lines.len(), 6, "{lines:?}");
        let found = |term| find(&lines, term, SearchCase::Smart);
        assert_eq!(found("one"), [at(0, 0..3)]);
        assert_eq!(found("two"), [at(2, 0..3)]);
        assert_eq!(found("three"), [at(4, 0..5)]);
        // A term across the end of a line is not found, as in text.
        assert_eq!(found("two three"), []);
    }

    #[test]
    fn finds_every_place_of_the_term_line_by_line() {
        let lines = [line("kiri and kiri"), Line::blank(), line("a kiri")];
        assert_eq!(
            find(&lines, "kiri", SearchCase::Smart),
            [at(0, 0..4), at(0, 9..13), at(2, 2..6)]
        );
    }

    #[test]
    fn finds_a_term_whose_style_changes_in_the_middle() {
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let lines = [Line::new(vec![
            Span::new("a ", Style::default()),
            Span::new("ki", bold),
            Span::new("ri", Style::default()),
        ])];
        assert_eq!(find(&lines, "kiri", SearchCase::Smart), [at(0, 2..6)]);
    }

    #[test]
    fn places_do_not_overlap() {
        assert_eq!(
            find(&[line("aaaa")], "aa", SearchCase::Smart),
            [at(0, 0..2), at(0, 2..4)]
        );
    }

    #[test]
    fn a_term_without_upper_case_ignores_case_by_default() {
        let lines = [line("Kiri KIRI kiri")];
        assert_eq!(
            find(&lines, "kiri", SearchCase::Smart),
            [at(0, 0..4), at(0, 5..9), at(0, 10..14)]
        );
    }

    #[test]
    fn a_term_with_upper_case_matches_case_exactly() {
        let lines = [line("Kiri KIRI kiri")];
        assert_eq!(find(&lines, "Kiri", SearchCase::Smart), [at(0, 0..4)]);
    }

    #[test]
    fn literal_case_matches_case_exactly() {
        let lines = [line("Kiri KIRI kiri")];
        assert_eq!(find(&lines, "kiri", SearchCase::Literal), [at(0, 10..14)]);
    }

    #[test]
    fn a_no_break_space_matches_a_space() {
        let lines = [line("run\u{a0}make\u{a0}check")];
        assert_eq!(
            find(&lines, "make check", SearchCase::Smart),
            [at(0, 5..16)]
        );
    }

    #[test]
    fn places_are_in_bytes_of_the_original_text() {
        // `İ` is two bytes and lower-cases to two characters.
        let lines = [line("İ Äiti äiti")];
        assert_eq!(
            find(&lines, "äiti", SearchCase::Smart),
            [at(0, 3..8), at(0, 9..14)]
        );
    }

    #[test]
    fn a_search_starts_at_the_first_match_from_the_top_line() {
        let lines = [line("kiri"), line("a"), line("kiri kiri")];
        let search = Search::new(&lines, "kiri", SearchCase::Smart, 1);
        assert_eq!(search.term(), "kiri");
        assert_eq!(search.current(), Some(&at(2, 0..4)));
        assert_eq!(search.count(), Some((2, 3)));
        let search = Search::new(&lines, "kiri", SearchCase::Smart, 0);
        assert_eq!(search.current(), Some(&at(0, 0..4)));
    }

    #[test]
    fn a_search_with_no_match_below_the_top_starts_at_the_first() {
        let lines = [line("kiri"), line("a")];
        let search = Search::new(&lines, "kiri", SearchCase::Smart, 1);
        assert_eq!(search.current(), Some(&at(0, 0..4)));
        let search = Search::new(&lines, "xyz", SearchCase::Smart, 0);
        assert_eq!(search.current(), None);
        assert_eq!(search.count(), None);
    }

    #[test]
    fn next_and_previous_go_around_the_ends() {
        let lines = [line("kiri"), line("kiri kiri")];
        let mut search = Search::new(&lines, "kiri", SearchCase::Smart, 1);
        assert_eq!(search.count(), Some((2, 3)));
        assert!(!search.forward(0..2));
        assert_eq!(search.current(), Some(&at(1, 5..9)));
        assert!(search.forward(0..2));
        assert_eq!(search.count(), Some((1, 3)));
        assert!(search.backward(0..2));
        assert_eq!(search.count(), Some((3, 3)));
        assert!(!search.backward(0..2));
        assert_eq!(search.count(), Some((2, 3)));
    }

    #[test]
    fn next_and_previous_start_from_the_view_when_the_current_match_is_out_of_it() {
        let lines = [
            line("kiri"),
            line("a"),
            line("kiri"),
            line("a"),
            line("kiri"),
            line("a"),
        ];
        let at_line = |top| Search::new(&lines, "kiri", SearchCase::Smart, top);
        // Scrolled away from the current match to lines 2 and 3: the next
        // match is the first in the view, the previous the last above it.
        let mut search = at_line(0);
        assert!(!search.forward(2..4));
        assert_eq!(search.current(), Some(&at(2, 0..4)));
        let mut search = at_line(4);
        assert!(!search.backward(2..4));
        assert_eq!(search.current(), Some(&at(0, 0..4)));
        // With no match from the view down, or above it, around the end.
        let mut search = at_line(0);
        assert!(search.forward(5..6));
        assert_eq!(search.current(), Some(&at(0, 0..4)));
        let mut search = at_line(4);
        assert!(search.backward(0..1));
        assert_eq!(search.current(), Some(&at(4, 0..4)));
    }

    #[test]
    fn next_and_previous_without_matches_stay() {
        let mut search = Search::new(&[line("a")], "kiri", SearchCase::Smart, 0);
        assert!(!search.forward(0..2));
        assert!(!search.backward(0..2));
        assert_eq!(search.current(), None);
    }

    #[test]
    fn an_empty_term_finds_nothing() {
        assert_eq!(find(&[line("kiri")], "", SearchCase::Smart), []);
    }

    #[test]
    fn the_matches_on_a_line_tell_the_current_one() {
        let lines = [line("kiri"), line("kiri a kiri"), line("kiri")];
        let mut search = Search::new(&lines, "kiri", SearchCase::Smart, 1);
        assert_eq!(search.on(1), [(0..4, true), (7..11, false)]);
        search.forward(0..2);
        assert_eq!(search.on(1), [(0..4, false), (7..11, true)]);
        assert_eq!(search.on(0), [(0..4, false)]);
        assert_eq!(search.on(3), []);
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
