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
mod tests {
    use super::*;
    use crate::render::{FrontMatterStyle, Inputs};
    use crate::session::layers::{Highlights, stack};
    use crate::typeset::caps::Capabilities;
    use crate::typeset::driver::{Content, Stream, content};
    use crate::typeset::layers::{Look, paint};
    use crate::typeset::layout::layers::Marks;
    use crate::typeset::locate::Map;
    use crate::typeset::model::{Block, Document, File};
    use crate::typeset::parse::parse;
    use crate::typeset::reading::engine::Reading;
    use crate::typeset::reading::page::Wide;
    use crate::typeset::theme::Theme;
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
                    glyphs: Default::default(),
                    fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                    wide,
                    snap: false,
                    zoom: 100,
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
        let map = Map::new(&stream.lines, &stream.blocks);
        let text = text_of(content_of);
        let search = Search::new((&map, &stream.lines), &text, term, case, top);
        (search, stream)
    }

    /// The bytes of the text of line `line` that the matches of `search`
    /// mark, left to right, each with whether it is the current one: what the
    /// map says of where each match is shown.
    fn on(search: &Search, stream: &Stream, line: usize) -> Vec<(Range<usize>, bool)> {
        let map = Map::new(&stream.lines, &stream.blocks);
        let mut found = Vec::new();
        for (range, current) in search.ranges() {
            for (at, bytes) in map.line_bytes(&range) {
                if at == line {
                    found.push((bytes, current));
                }
            }
        }
        found.sort_by_key(|(bytes, _)| bytes.start);
        found
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
            .flat_map(|line| {
                let on = on(&search, &stream, line);
                on.into_iter().map(move |(b, _)| (line, b))
            })
            .collect()
    }

    #[test]
    fn a_match_is_kept_in_a_few_numbers_and_not_in_two_places_with_paths() {
        // A common term in a log of a million lines has millions of matches.
        assert!(
            std::mem::size_of::<Found>() <= 40,
            "{}",
            std::mem::size_of::<Found>()
        );
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
            .filter(|&line| !on(&search, &stream, line).is_empty())
            .collect();
        assert!(lines.len() >= 2, "{lines:?}");
        // The break between the lines is no part of either mark, and the
        // text each marks is the term's words.
        let marked: Vec<String> = lines
            .iter()
            .flat_map(|&line| {
                let text = crate::typeset::style::text(&stream.lines[line].spans);
                on(&search, &stream, line)
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
            fade: None,
            selected: None,
        };
        let map = Map::new(&stream.lines, &stream.blocks);
        let all = 0..stream.lines.len();
        let layers = stack(&highlights, &map, all.clone());
        let look = Look {
            marks: &marks,
            rgb: None,
            columns: 22,
        };
        let painted = paint(&stream.lines, all, &map, &layers, &look);
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
            assert!(current.line.is_some_and(|line| line < stream.lines.len()));
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
        let rows: Vec<usize> = search
            .matches()
            .iter()
            .filter_map(|found| found.line)
            .collect();
        // Not on the first row of the long paragraph, but on the row of
        // the line of text the word is on, and the others below in order.
        assert!(rows[0] >= 3, "{rows:?}");
        assert!(rows[0] < rows[1] && rows[1] < rows[2], "{rows:?}");
        // None is a match in the spans of a line, which are marked there.
        assert!((0..stream.lines.len()).all(|row| on(&search, &stream, row).is_empty()));
        // The page's marks: all three, the first the current.
        let map = Map::new(&stream.lines, &stream.blocks);
        let page = crate::typeset::reading::engine::pages(&stream.lines);
        let marks: Vec<bool> = (search.ranges())
            .filter(|(range, _)| !map.page_spots(&page[0].parts, range).is_empty())
            .map(|(_, current)| current)
            .collect();
        assert_eq!(marks, [true, false, false]);
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
            let map = Map::new(&stream.lines, &stream.blocks);
            let text = text_of(Content::Document(&document));
            let shown = (&map, &stream.lines[..]);
            let again = search.again(shown, &text, (SearchCase::Smart, 0), None);
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
        inputs.theme = crate::typeset::theme::Theme::large_headings();
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
                .filter_map(|found| found.line)
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
        text_laid(source, term, top).0
    }

    /// As `text_search`, and the lines.
    fn text_laid(source: &str, term: &str, top: usize) -> (Search, Stream) {
        let document = parse(source);
        search_in(
            Content::Document(&document),
            &inputs(Engine::Text(80)),
            (term, SearchCase::Smart, top),
        )
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
        assert_eq!(search.current().unwrap().line, Some(4));
        let search = text_search(PARAGRAPHS, "kiri", 0);
        assert_eq!(search.current().unwrap().line, Some(0));
    }

    #[test]
    fn a_search_with_no_match_below_the_top_starts_at_the_first() {
        let search = text_search("kiri\n\na\n", "kiri", 2);
        assert_eq!(search.current().unwrap().line, Some(0));
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
        assert_eq!(line(&search), Some(4));
        let mut search = text_search(source, "kiri", 8);
        assert!(!search.backward(3..6));
        assert_eq!(line(&search), Some(0));
        // With no match from the view down, or above it, around the end.
        let mut search = text_search(source, "kiri", 0);
        assert!(search.forward(9..11));
        assert_eq!(line(&search), Some(0));
        let mut search = text_search(source, "kiri", 8);
        assert!(search.backward(0..1));
        assert_eq!(line(&search), Some(8));
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
        let (mut search, stream) = text_laid("kiri\n\nkiri a kiri\n\nkiri\n", "kiri", 2);
        assert_eq!(on(&search, &stream, 2), [(0..4, true), (7..11, false)]);
        search.forward(0..5);
        assert_eq!(on(&search, &stream, 2), [(0..4, false), (7..11, true)]);
        assert_eq!(on(&search, &stream, 0), [(0..4, false)]);
        assert_eq!(on(&search, &stream, 1), []);
    }
}
