//! Where a search term is in the laid-out lines of a document. A line's
//! text is the text of its spans one after another, so a word whose style
//! changes in the middle is found too.

use std::ops::Range;

use crate::config::SearchCase;
use crate::render::style::Line;

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
    /// it went around.
    pub fn next(&mut self) -> bool {
        let Some(current) = self.current else {
            return false;
        };
        let around = current + 1 == self.matches.len();
        self.current = Some(if around { 0 } else { current + 1 });
        around
    }

    /// Moves to the previous match, or from the first to the last; true
    /// when it went around.
    pub fn previous(&mut self) -> bool {
        let Some(current) = self.current else {
            return false;
        };
        let around = current == 0;
        self.current = Some(current.checked_sub(1).unwrap_or(self.matches.len() - 1));
        around
    }

    /// The place of the current match among all of them, counted from 1,
    /// and how many there are; `None` when there is none.
    pub fn count(&self) -> Option<(usize, usize)> {
        Some((self.current? + 1, self.matches.len()))
    }
}

/// The places of `term` in `lines`, from the top and from left to right,
/// none overlapping another. A no-break space matches a space. With
/// `SearchCase::Smart`, a term with no upper-case letter ignores case.
pub fn find(lines: &[Line], term: &str, case: SearchCase) -> Vec<Match> {
    let ignore_case = match case {
        SearchCase::Smart => !term.chars().any(char::is_uppercase),
        SearchCase::Literal => false,
    };
    let term: Vec<char> = term.chars().collect();
    lines
        .iter()
        .enumerate()
        .flat_map(|(index, line)| {
            places(&text(line), &term, ignore_case)
                .into_iter()
                .map(move |bytes| Match { line: index, bytes })
        })
        .collect()
}

/// The text of `line`'s spans one after another.
fn text(line: &Line) -> String {
    line.spans.iter().map(|span| span.text.as_str()).collect()
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
    use crate::render::style::{Span, Style};

    fn line(text: &str) -> Line {
        Line::new(vec![Span::new(text, Style::default())])
    }

    fn at(line: usize, bytes: Range<usize>) -> Match {
        Match { line, bytes }
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
        assert!(!search.next());
        assert_eq!(search.current(), Some(&at(1, 5..9)));
        assert!(search.next());
        assert_eq!(search.count(), Some((1, 3)));
        assert!(search.previous());
        assert_eq!(search.count(), Some((3, 3)));
        assert!(!search.previous());
        assert_eq!(search.count(), Some((2, 3)));
    }

    #[test]
    fn next_and_previous_without_matches_stay() {
        let mut search = Search::new(&[line("a")], "kiri", SearchCase::Smart, 0);
        assert!(!search.next());
        assert!(!search.previous());
        assert_eq!(search.current(), None);
    }

    #[test]
    fn an_empty_term_finds_nothing() {
        assert_eq!(find(&[line("kiri")], "", SearchCase::Smart), []);
    }
}
