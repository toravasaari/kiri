//! Where a search term is in the laid-out lines of a document. A line's
//! text is the text of its spans one after another, so a word whose style
//! changes in the middle is found too.

use std::ops::Range;

use super::fade::Fade;
use crate::caps::Capabilities;
use crate::color::Underline;
use crate::config::SearchCase;
use crate::render::style::{self, Line, Span, Style};
use crate::theme::{SearchColor, Theme};

/// Where a search term is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// The line it is on.
    pub line: usize,
    /// Its bytes in the line's text: the text of its spans one after
    /// another.
    pub bytes: Range<usize>,
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
    /// with whether it is the current match.
    pub fn on(&self, line: usize) -> Vec<(Range<usize>, bool)> {
        let start = self.matches.partition_point(|found| found.line < line);
        self.matches[start..]
            .iter()
            .take_while(|found| found.line == line)
            .enumerate()
            .map(|(index, found)| (found.bytes.clone(), Some(start + index) == self.current))
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

/// `line` with the bytes of each of `matches` in its text, from left to
/// right, styled with `marks`: as the current match where its flag is
/// true, faded from the text's own style while a `fade` is under way. A span a
/// match starts or ends in is split there. In text at a fraction the split
/// moves out to the ends of the packed groups, so that the groups and the
/// cells they take stay as they are; a group in two matches takes the
/// first one's style.
pub fn mark(
    line: &Line,
    matches: &[(Range<usize>, bool)],
    marks: &Marks,
    fade: Option<Fade>,
) -> Line {
    let mut spans = Vec::new();
    // Where the span starts in the line's text.
    let mut start = 0;
    for span in &line.spans {
        let end = start + span.text.len();
        // Where the span may be cut, from its start: found once a match
        // is in it.
        let mut edges = None;
        // The bytes of the span cut so far.
        let mut cut = start;
        let inside = matches
            .iter()
            .filter(|(bytes, _)| bytes.start < end && start < bytes.end);
        for (bytes, current) in inside {
            let (mut from, mut to) = (bytes.start.max(start) - start, bytes.end.min(end) - start);
            let edges = edges.get_or_insert_with(|| self::edges(span));
            if !edges.is_empty() {
                from = edges
                    .iter()
                    .rev()
                    .copied()
                    .find(|&edge| edge <= from)
                    .unwrap_or(0);
                to = (edges.iter().copied().find(|&edge| edge >= to)).unwrap_or(span.text.len());
            }
            let (from, to) = ((start + from).max(cut), start + to);
            if to <= from {
                continue;
            }
            let style = match (current, fade) {
                (false, _) => laid_over(&span.style, &marks.found),
                (true, None) => laid_over(&span.style, &marks.current),
                (true, Some(fade)) => {
                    fade.step(&span.style, &laid_over(&span.style, &marks.current))
                }
            };
            spans.extend(pieces(span, cut - start..from - start, &span.style));
            spans.extend(pieces(span, from - start..to - start, &style));
            cut = to;
        }
        spans.extend(pieces(span, cut - start..end - start, &span.style));
        start = end;
    }
    spans.retain(|span| !span.text.is_empty());
    Line {
        spans,
        ..line.clone()
    }
}

/// Where `span` can be cut without changing how it is drawn, as bytes of
/// its text: anywhere at a whole size, which has no edges, and only between
/// packed groups at a fraction.
fn edges(span: &Span) -> Vec<usize> {
    if span.style.size.fraction().is_none() {
        return Vec::new();
    }
    (span.style.size.groups(&span.text).iter())
        .map(|group| at(span, group.text))
        .chain([span.text.len()])
        .collect()
}

/// Where `text`, a slice of `span`'s text, starts in it.
fn at(span: &Span, text: &str) -> usize {
    text.as_ptr() as usize - span.text.as_ptr() as usize
}

/// The bytes `bytes` of `span`'s text in `style`. A part of text at a
/// fraction is one span for each of its groups, packed as it was in the
/// whole text.
fn pieces(span: &Span, bytes: Range<usize>, style: &Style) -> Vec<Span> {
    if bytes == (0..span.text.len()) || span.style.size.fraction().is_none() {
        let text = span.text[bytes].to_owned();
        return vec![Span {
            text,
            style: style.clone(),
        }];
    }
    (span.style.size.groups(&span.text).into_iter())
        .filter(|group| bytes.contains(&at(span, group.text)))
        .map(|group| Span {
            text: group.text.to_owned(),
            style: Style {
                size: style.size.as_one_group(group.align),
                ..style.clone()
            },
        })
        .collect()
}

/// `style` with `mark` laid over it: its colours where it has them, its
/// underline where it has one, and its bold and reverse video.
fn laid_over(style: &Style, mark: &Style) -> Style {
    Style {
        bold: style.bold || mark.bold,
        reverse: style.reverse || mark.reverse,
        underline: match mark.underline {
            Underline::None => style.underline,
            underline => underline,
        },
        fg: mark.fg.or(style.fg),
        bg: mark.bg.or(style.bg),
        ..style.clone()
    }
}

/// The colour `color` gives a match: none for the text's own.
fn color(color: SearchColor) -> Option<crate::color::Color> {
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
    lines
        .iter()
        .enumerate()
        .flat_map(|(index, line)| {
            places(&searched(line), &term, ignore_case)
                .into_iter()
                .map(move |bytes| Match { line: index, bytes })
        })
        .collect()
}

/// The text a search looks in on `line`: its spans', and after them the
/// text that the image on its first row shows, such as a heading's. A
/// match there has bytes past the spans, so it is not marked.
fn searched(line: &Line) -> String {
    let mut text = style::text(&line.spans);
    if let Some(image) = line.image.as_ref().filter(|image| image.row == 0) {
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
    use crate::color::Color;
    use crate::pager::fade::Rgb;
    use crate::size::{Fraction, HorizontalAlign, TextSize, VerticalAlign};
    use crate::theme::{Paper, SearchTheme};

    fn line(text: &str) -> Line {
        Line::new(vec![Span::new(text, Style::default())])
    }

    fn at(line: usize, bytes: Range<usize>) -> Match {
        Match { line, bytes }
    }

    /// Row `row` of a heading `text` shown as an image of two rows.
    fn heading_row(text: &str, row: u16) -> Line {
        let placement = crate::render::image::Placement {
            key: crate::images::ImageKey::heading(1, text, (10, 20)),
            size: crate::render::image::Size {
                width: 100,
                height: 40,
                columns: 10,
                rows: 2,
            },
            shows: crate::render::image::Shows::Image,
        };
        let mut line = Line::image(std::rc::Rc::new(placement), row);
        if let Some(image) = &mut line.image {
            image.text = Some(text.to_owned());
        }
        line
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
        let text = crate::images::Source::Text {
            image,
            breaks: vec![30, 60],
            ends: vec![28, 58, 90],
        };
        let key = crate::images::ImageKey::heading(1, "One two three", (10, 20));
        inputs.images.insert(key, Ok(text));
        let document = crate::parse::parse("# One two three\n");
        let lines = crate::render::layout::layout(&document, &inputs);
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
    fn search_highlight_skips_an_image_match() {
        let marks = Marks::new(&Theme::default(), &Capabilities::kitty(80));
        let row = heading_row("Kiri heading", 0);
        assert_eq!(mark(&row, &[(0..4, true)], &marks, None), row);
        // Beside text before the image, such as a quote's bar.
        let mut row = heading_row("Kiri", 0);
        row.spans = vec![Span::new("> ", Style::default())];
        assert_eq!(mark(&row, &[(2..6, false)], &marks, None), row);
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

    fn marks() -> Marks {
        Marks {
            found: Style {
                reverse: true,
                ..Style::default()
            },
            current: Style {
                bold: true,
                bg: Some(Color::Palette(3)),
                ..Style::default()
            },
        }
    }

    #[test]
    fn a_match_on_a_striped_table_row_stays_marked() {
        // The stripe is a background; reverse video swaps it with the
        // text colour, and a match colour takes its place.
        let striped = Style {
            bg: Some(Color::Rgb(16, 16, 16)),
            ..Style::default()
        };
        let found = laid_over(&striped, &marks().found);
        assert!(found.reverse);
        assert_eq!(found.bg, striped.bg);
        let current = laid_over(&striped, &marks().current);
        assert_eq!(current.bg, Some(Color::Palette(3)));
    }

    #[test]
    fn marking_splits_spans_where_matches_start_and_end() {
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let source = Line::new(vec![
            Span::new("a ki", Style::default()),
            Span::new("ri b", bold.clone()),
        ]);
        let marked = mark(&source, &[(2..6, false)], &marks(), None);
        let reverse = |style: &Style| Style {
            reverse: true,
            ..style.clone()
        };
        assert_eq!(
            marked.spans,
            [
                Span::new("a ", Style::default()),
                Span::new("ki", reverse(&Style::default())),
                Span::new("ri", reverse(&bold)),
                Span::new(" b", bold),
            ]
        );
    }

    #[test]
    fn the_current_match_is_marked_apart_and_keeps_the_text_s_style() {
        let link = Style {
            italic: true,
            fg: Some(Color::Palette(4)),
            link: Some("https://example.com".into()),
            ..Style::default()
        };
        let source = Line::new(vec![Span::new("kiri, kiri", link.clone())]);
        let marked = mark(&source, &[(0..4, false), (6..10, true)], &marks(), None);
        assert_eq!(
            marked.spans,
            [
                Span::new(
                    "kiri",
                    Style {
                        reverse: true,
                        ..link.clone()
                    }
                ),
                Span::new(", ", link.clone()),
                Span::new(
                    "kiri",
                    Style {
                        bold: true,
                        bg: Some(Color::Palette(3)),
                        ..link
                    }
                ),
            ]
        );
    }

    #[test]
    fn a_mark_in_text_at_a_fraction_keeps_its_groups() {
        // Packing looks at the words around a group, so a piece cut out of
        // the text could be packed apart from the rest.
        let two_thirds =
            TextSize::NORMAL.with_fraction(Fraction::new(2, 3).unwrap(), VerticalAlign::Top);
        let style = Style {
            size: two_thirds,
            ..Style::default()
        };
        let text = "dceceddaae ba";
        let source = Line::new(vec![Span::new(text, style.clone())]);
        let groups = |line: &Line| -> Vec<(String, u8, HorizontalAlign)> {
            (line.spans.iter())
                .flat_map(|span| span.style.size.groups(&span.text))
                .map(|group| (group.text.to_owned(), group.cells, group.align))
                .collect()
        };
        for start in 0..text.len() {
            for end in start + 1..=text.len() {
                let marked = mark(&source, &[(start..end, false)], &marks(), None);
                assert_eq!(groups(&marked), groups(&source), "{start}..{end}");
            }
        }
    }

    #[test]
    fn a_mark_in_text_at_a_fraction_covers_whole_groups() {
        // Half-size text is packed two cells to a group: `ab`, `cd`, `ef`.
        // Cutting a group would pack its pieces apart, each rounded up, and
        // move the cells after them.
        let half = TextSize::NORMAL.with_fraction(Fraction::new(1, 2).unwrap(), VerticalAlign::Top);
        let style = Style {
            size: half,
            ..Style::default()
        };
        let source = Line::new(vec![Span::new("abcdef", style.clone())]);
        let marked = mark(&source, &[(3..4, false)], &marks(), None);
        let found: Vec<(&str, bool)> = (marked.spans.iter())
            .map(|span| (span.text.as_str(), span.style.reverse))
            .collect();
        assert_eq!(found, [("ab", false), ("cd", true), ("ef", false)]);
        assert_eq!(
            crate::render::wrap::width(&marked.spans),
            crate::render::wrap::width(&source.spans)
        );
        // Two matches in one group mark it once; one across groups marks
        // both.
        let marked = mark(&source, &[(0..1, true), (1..3, false)], &marks(), None);
        let texts: Vec<&str> = marked.spans.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(texts, ["ab", "cd", "ef"]);
        assert!(marked.spans[0].style.bold && marked.spans[1].style.reverse);
    }

    #[test]
    fn marking_keeps_the_rest_of_the_line() {
        let source = Line {
            lead: vec![Span::new("> ", Style::default())],
            trail: vec![Span::new("  ", Style::default())],
            ..Line::new(vec![Span::new(
                "Ä kiri",
                Style {
                    size: TextSize::new(2).unwrap(),
                    ..Style::default()
                },
            )])
        };
        let marked = mark(&source, &[(3..7, true)], &marks(), None);
        assert_eq!(
            Line {
                spans: Vec::new(),
                ..marked.clone()
            },
            Line {
                spans: Vec::new(),
                ..source
            }
        );
        let texts: Vec<&str> = marked.spans.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(texts, ["Ä ", "kiri"]);
        assert_eq!(mark(&line("kiri"), &[], &marks(), None), line("kiri"));
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
    fn the_current_match_fades_from_the_text_s_own_style() {
        let source = Line::new(vec![Span::new("kiri", Style::default())]);
        let mut caps = Capabilities::kitty(80);
        caps.foreground = Some(Color::Rgb(200, 200, 200));
        caps.background = Some(Color::Rgb(0, 0, 0));
        caps.palette[3] = Some(Color::Rgb(200, 200, 0));
        let known = Rgb::new(&Theme::default(), &caps).unwrap();
        let at = |millis, rgb| {
            let fade = Fade {
                elapsed: std::time::Duration::from_millis(millis),
                rgb,
            };
            let marked = mark(&source, &[(0..4, true)], &marks(), Some(fade));
            let style = &marked.spans[0].style;
            (style.fg, style.bg)
        };
        // From grey on black into grey on yellow.
        let grey = Some(Color::Rgb(200, 200, 200));
        assert_eq!(at(0, Some(&known)), (grey, Some(Color::Rgb(0, 0, 0))));
        assert_eq!(at(375, Some(&known)), (grey, Some(Color::Rgb(100, 100, 0))));
        assert_eq!(at(1000, Some(&known)), (None, Some(Color::Palette(3))));
        // Without colours known as RGB, the mark at once.
        assert_eq!(at(0, None), (None, Some(Color::Palette(3))));
        // Other matches do not fade.
        let fade = Fade {
            elapsed: std::time::Duration::ZERO,
            rgb: Some(&known),
        };
        let other = mark(&source, &[(0..4, false)], &marks(), Some(fade));
        assert!(other.spans[0].style.reverse);
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
