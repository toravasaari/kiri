//! Where a search term is in the document, and where that is shown: the term
//! is looked for in the document's text, each place is a range of it, and
//! the layout says on which lines the range is shown, or that it is not.
//! What the layout does not show, such as the text behind a picture or
//! hidden front matter, is not found, as in a browser. What it has not laid
//! out yet is not known not to be shown: such a match is kept without a line,
//! in the order of the document after those that have lines, and gets its
//! line when the layout reaches it ([`Search::extend`]), without the text
//! being searched again.

use std::cmp::Ordering;
use std::ops::Range;

use crate::session::settings::SearchCase;
use crate::typeset::locate::{Across, Hidden, Map};
use crate::typeset::position::{Pos, Within};
use crate::typeset::style::Line;
use crate::typeset::text::DocumentText;

/// Where a search term is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// The range of the document's text it is.
    pub range: Range<Pos>,
    /// The line it starts on; `None` while the layout has not laid it out.
    pub line: Option<usize>,
    /// Where it is in a band of reading mode, when it is in one.
    pub band: Option<Across>,
}

/// A match as the search keeps it: the bytes of one text of a block, and
/// the block by its number among the blocks of the search, rather than two
/// places that each hold the block's path. A common term in a log of a
/// million lines has millions of matches.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    /// The number of the block in [`Search::blocks`].
    in_block: u32,
    /// The number of the text in the block.
    text: u32,
    /// The bytes of the text.
    start: u32,
    end: u32,
    /// The line it starts on; `None` while the layout has not laid it out.
    line: Option<u32>,
    /// Where it is in a band of reading mode, when it is in one.
    band: Option<Across>,
}

impl Found {
    /// Where it starts, in the order of the document: the blocks of a search
    /// are in that order.
    fn start(&self) -> (u32, u32, u32) {
        (self.in_block, self.text, self.start)
    }

    /// The line it starts on.
    fn line(&self) -> Option<usize> {
        self.line.map(|line| line as usize)
    }
}

/// `n` as kept in a [`Found`]. Texts, their bytes and lines are counted in
/// `u32` by the layouts too.
fn small(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
thread_local! {
    static FINDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times the text of a document was searched on this thread.
#[cfg(test)]
pub(crate) fn finds() -> usize {
    FINDS.with(std::cell::Cell::get)
}

/// A search: its term, where it is in the document and which place the
/// reader is at. Two are equal when these are, whichever blocks they list
/// that no match is in any more.
#[derive(Debug, Clone)]
pub struct Search {
    term: String,
    /// The blocks with a place of the term, each by its file and path, in
    /// the order of the documents; a match names its block by its number
    /// here.
    blocks: Vec<(usize, Vec<usize>)>,
    /// In the order of the lines they start on, and then of the document;
    /// those with no line yet after them, in the order of the document.
    matches: Vec<Found>,
    /// The index of the current match; `None` when there is none.
    current: Option<usize>,
}

impl Search {
    /// A search for `term` in `text`, the text of the documents the lines
    /// `lines` show, of which `map` is the map, at its first match on line
    /// `top` or below it, or at its first match when there is none there.
    pub(crate) fn new(
        (map, lines): (&Map, &[Line]),
        text: &DocumentText,
        term: &str,
        case: SearchCase,
        top: usize,
    ) -> Self {
        let (blocks, matches) = find((map, lines), text, term, case);
        let current = matches
            .iter()
            .position(|found| found.line().is_none_or(|line| line >= top))
            .or((!matches.is_empty()).then_some(0));
        Self {
            term: term.to_owned(),
            blocks,
            matches,
            current,
        }
    }

    /// This search again in `text`, the text of the documents the lines
    /// `lines` show, of which `map` is the map, laid out anew: at the match it was at
    /// (`current` when that is given) when that range is still found, as
    /// after a resize or when only a band was shown anew, and otherwise at
    /// the first on line `top` or below.
    pub(crate) fn again(
        &self,
        shown: (&Map, &[Line]),
        text: &DocumentText,
        (case, top): (SearchCase, usize),
        current: Option<&Range<Pos>>,
    ) -> Self {
        let mut search = Self::new(shown, text, &self.term, case, top);
        // The match it was at, which the caller gives when the document is
        // another version and it moved with it.
        let current = current
            .cloned()
            .or_else(|| self.current().map(|current| current.range));
        let kept = current.and_then(|current| {
            (search.matches.iter()).position(|found| search.is(found, &current))
        });
        if kept.is_some() {
            search.current = kept;
        }
        search
    }

    /// Gives the matches that had no line their lines, now that the layout
    /// `shown`, which the search was made of, is laid out further. The text
    /// is not searched again: a match with no line is after all those that
    /// have one, in the order of the document, so those the layout reaches
    /// are the first of them; they take their places among the matches by
    /// their lines, as in a whole layout, and one that turns out not to be
    /// shown, as the text behind a picture, is gone. The match the reader is
    /// at stays the one.
    pub fn extend(&mut self, (map, lines): (&Map, &[Line])) {
        let from = self.matches.partition_point(|found| found.line.is_some());
        let at = (self.current.filter(|&at| at >= from)).map(|at| self.matches[at].start());
        let mut left = self.matches.split_off(from).into_iter();
        let mut laid = Vec::new();
        let mut rest = Vec::new();
        // The ranges end later the later they start, so what is not laid out
        // is where the first of them is.
        for found in left.by_ref() {
            match place(map, lines, &self.range(&found)) {
                Placed::Laid(line, band) => laid.push(Found {
                    line: Some(small(line)),
                    band,
                    ..found
                }),
                Placed::Hidden => {}
                Placed::NotLaid => {
                    rest.push(found);
                    break;
                }
            }
        }
        rest.extend(left);
        laid.sort_by(by_line);
        self.matches.extend(laid);
        self.matches.extend(rest);
        if let Some(at) = at {
            let moved = self.matches[from..]
                .iter()
                .position(|found| found.start() == at);
            // The match the reader was at turned out not to be shown: the
            // first match that starts after it in the document, or the last
            // when none does.
            let after = self.matches[from..]
                .iter()
                .position(|found| found.start() > at);
            let last = self.matches.len().checked_sub(1);
            self.current = moved.or(after).map(|at| from + at).or(last);
        }
    }

    /// The range of the document's text that `found` is.
    fn range(&self, found: &Found) -> Range<Pos> {
        let (file, path) = &self.blocks[found.in_block as usize];
        let at = |offset: u32| Pos::text(*file, path, found.text as usize, offset as usize);
        at(found.start)..at(found.end)
    }

    /// Whether `found` is the range `range`.
    fn is(&self, found: &Found, range: &Range<Pos>) -> bool {
        let (file, path) = &self.blocks[found.in_block as usize];
        let is = |at: &Pos, offset: u32| {
            (at.file, at.path.as_slice()) == (*file, path.as_slice())
                && at.within
                    == Within::Text {
                        text: found.text as usize,
                        offset: offset as usize,
                    }
        };
        is(&range.start, found.start) && is(&range.end, found.end)
    }

    /// `found` as a [`Match`].
    fn as_match(&self, found: &Found) -> Match {
        Match {
            range: self.range(found),
            line: found.line(),
            band: found.band.clone(),
        }
    }

    /// The matches, in order.
    #[cfg(test)]
    pub(crate) fn matches(&self) -> Vec<Match> {
        (self.matches.iter())
            .map(|found| self.as_match(found))
            .collect()
    }

    /// The term searched for.
    pub fn term(&self) -> &str {
        &self.term
    }

    /// The match the reader is at.
    pub fn current(&self) -> Option<Match> {
        Some(self.as_match(self.matches.get(self.current?)?))
    }

    /// Moves to the next match, or from the last to the first; true when
    /// it went around. When the current match is out of the lines
    /// `shown`, the next is the first from the top of them, as in `less`.
    pub fn forward(&mut self, shown: Range<usize>) -> bool {
        let Some(current) = self.current else {
            return false;
        };
        let next = if self.is_in(current, &shown) {
            current + 1
        } else {
            self.before(&shown)
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
        let after = if self.is_in(current, &shown) {
            current
        } else {
            self.before(&shown)
        };
        let around = after == 0;
        self.current = Some(after.checked_sub(1).unwrap_or(self.matches.len() - 1));
        around
    }

    /// Whether the match `at` is on one of the lines `shown`.
    fn is_in(&self, at: usize, shown: &Range<usize>) -> bool {
        self.matches[at]
            .line()
            .is_some_and(|line| shown.contains(&line))
    }

    /// How many matches are on the lines before `shown`: those without a
    /// line are after all that have one.
    fn before(&self, shown: &Range<usize>) -> usize {
        (self.matches).partition_point(|found| found.line().is_some_and(|line| line < shown.start))
    }

    /// The place of the current match among all of them, counted from 1,
    /// and how many there are; `None` when there is none.
    pub fn count(&self) -> Option<(usize, usize)> {
        Some((self.current? + 1, self.matches.len()))
    }

    /// The range of each match, in order, with whether it is the current
    /// match.
    #[cfg(test)]
    pub(crate) fn ranges(&self) -> impl Iterator<Item = (Range<Pos>, bool)> + '_ {
        (self.matches.iter().enumerate())
            .map(|(index, found)| (self.range(found), Some(index) == self.current))
    }

    /// The range of each match that may be on lines that show the places
    /// `shown`, as [`Map::near`] tells it, in order, with whether it is the
    /// current match. Only those are made into ranges.
    pub fn near(&self, shown: &Range<Pos>) -> Vec<(Range<Pos>, bool)> {
        let mut near = Vec::new();
        // A place in the block of the match before, whose text and byte are
        // set for each match: no path is copied for a match.
        let mut place: Option<(u32, Pos)> = None;
        for (index, found) in self.matches.iter().enumerate() {
            let at = match &mut place {
                Some((block, at)) if *block == found.in_block => at,
                _ => {
                    let (file, path) = &self.blocks[found.in_block as usize];
                    &mut place.insert((found.in_block, Pos::block(*file, path))).1
                }
            };
            let within = |offset: u32| Within::Text {
                text: found.text as usize,
                offset: offset as usize,
            };
            // As `Map::near` has it, without the ranges.
            at.within = within(found.end);
            let ends_after = *at > shown.start;
            at.within = within(found.start);
            if ends_after && *at <= shown.end {
                near.push((self.range(found), Some(index) == self.current));
            }
        }
        near
    }
}

impl PartialEq for Search {
    fn eq(&self, other: &Self) -> bool {
        let same = |a: &Found, b: &Found| {
            self.blocks[a.in_block as usize] == other.blocks[b.in_block as usize]
                && Found {
                    in_block: b.in_block,
                    ..a.clone()
                } == *b
        };
        (&self.term, self.current, self.matches.len())
            == (&other.term, other.current, other.matches.len())
            && (self.matches.iter().zip(&other.matches)).all(|(a, b)| same(a, b))
    }
}

impl Eq for Search {}

/// The blocks in which `term` is in the document, and its places there, from
/// the top and from left to right, none overlapping another, as far as
/// `map`, the map of `lines`, shows them. A no-break space matches a space.
/// With `SearchCase::Smart`, a term with no upper-case letter ignores case.
fn find(
    (map, lines): (&Map, &[Line]),
    text: &DocumentText,
    term: &str,
    case: SearchCase,
) -> (Vec<(usize, Vec<usize>)>, Vec<Found>) {
    let ignore_case = match case {
        SearchCase::Smart => !term.chars().any(char::is_uppercase),
        SearchCase::Literal => false,
    };
    #[cfg(test)]
    FINDS.with(|finds| finds.set(finds.get() + 1));
    let mut blocks: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut found = Vec::new();
    text.find_each(term, ignore_case, |file, path, text, bytes| {
        let range =
            Pos::text(file, path, text, bytes.start)..Pos::text(file, path, text, bytes.end);
        let line = match place(map, lines, &range) {
            Placed::Laid(line, band) => Some((small(line), band)),
            Placed::NotLaid => None,
            Placed::Hidden => return,
        };
        if blocks
            .last()
            .is_none_or(|(at, on)| (*at, on.as_slice()) != (file, path))
        {
            blocks.push((file, path.to_vec()));
        }
        let (line, band) = line.map_or((None, None), |(line, band)| (Some(line), band));
        found.push(Found {
            in_block: small(blocks.len() - 1),
            text: small(text),
            start: small(bytes.start),
            end: small(bytes.end),
            line,
            band,
        });
    });
    found.sort_by(by_line);
    (blocks, found)
}

/// Where a range is: on lines of the layout, or not laid out yet, or not
/// shown.
enum Placed {
    /// On a line, the one it starts on, and in a band of reading mode when
    /// it is in one.
    Laid(usize, Option<Across>),
    /// Where the layout has not reached: no line yet.
    NotLaid,
    /// What no line shows is not a match, as in a browser.
    Hidden,
}

/// Where `range` is, as the layout `map` of `lines` shows it.
fn place(map: &Map, lines: &[Line], range: &Range<Pos>) -> Placed {
    let parts = match map.locate(lines, range) {
        Ok(parts) => parts,
        Err(Hidden::NotLaidOut) => return Placed::NotLaid,
        Err(_) => return Placed::Hidden,
    };
    let Some(first) = parts.first() else {
        return Placed::Hidden;
    };
    let line = map.start(&first.piece).unwrap_or(0) + first.rows.start;
    Placed::Laid(line, first.band.clone())
}

/// The order of the matches: by the line each starts on and then by place,
/// those without a line after all the others, by place.
fn by_line(a: &Found, b: &Found) -> Ordering {
    (a.line.is_none(), a.line, a.start()).cmp(&(b.line.is_none(), b.line, b.start()))
}

#[cfg(test)]
mod tests;
