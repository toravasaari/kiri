//! Bringing a range of the document into view: the lines that show it,
//! the band it is in, and why it is not shown when it is not.
//!
//! One function does it for everyone who has a range to show: a search that
//! jumps to a match, Tab that goes to a link, a picture the pill tells of and
//! the follow that goes to a change. How the range is brought in is `Show`,
//! because a match, a link and a change are shown differently (the view's
//! own rules for lines).

use std::ops::Range;

use crate::session::bands::Bands;
use crate::session::edition::Edition;
use crate::session::view::View;
use crate::typeset::block::Blocks;
use crate::typeset::locate::{Hidden, Map, Part};
use crate::typeset::position::Pos;
use crate::typeset::style::Line;

/// How a range is brought into view: by the rules of the view for what it
/// shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Show {
    /// Its first line at the top, unless it is in view already: a search
    /// match (`View::reveal`).
    Line,
    /// All its lines in view when they fit, as a link or a picture Tab goes
    /// to (`View::show_whole`).
    Whole,
    /// A change a few lines below the top, unless its start is shown: the
    /// follow (`View::show_change`). The bands stay as they are.
    Change,
}

/// What the lines of a layout say about where a range is: the map of them,
/// the lines, and the table of the blocks they come from.
#[derive(Clone, Copy)]
pub(crate) struct Seen<'a> {
    pub(crate) map: &'a Map,
    pub(crate) lines: &'a [Line],
    pub(crate) blocks: &'a Blocks,
}

impl Seen<'_> {
    /// The parts of the lines that show `range`, and the lines from the
    /// first to the last: for a range that has none, such as a block that is
    /// gone, the place between the lines where it would be, as an empty
    /// range, which at the end of the document is after the last line. Why
    /// not, when the range is not shown.
    pub(crate) fn shown(&self, range: &Range<Pos>) -> Result<(Vec<Part>, Range<usize>), Hidden> {
        self.map.drawn_rows(self.lines, range)
    }
}

/// Brings `range` into `view` as `show` says, and scrolls the band that
/// shows it, when it is in one, to it. Whether a band was scrolled or
/// selected, or why `range` is not shown, and then the view stays.
pub(crate) fn reveal(
    range: &Range<Pos>,
    show: Show,
    at: Seen,
    view: &mut View,
    (bands, edition): (&mut Bands, &Edition),
) -> Result<bool, Hidden> {
    let (parts, lines) = at.shown(range)?;
    match show {
        Show::Line => view.reveal(lines.start.min(at.lines.len().saturating_sub(1))),
        Show::Whole => view.show_whole(lines.clone()),
        Show::Change => {
            view.show_change(lines);
            return Ok(false);
        }
    }
    // A range in a band scrolls the band to it, and selects it.
    let band = parts.iter().find_map(|part| part.band.as_ref());
    let scrolled = band.and_then(|across| {
        let shown = (at.lines, at.blocks);
        bands.reveal(edition, shown, lines.start, across.pixels.clone())
    });
    Ok(scrolled == Some(true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{FrontMatterStyle, Inputs};
    use crate::session::sources::Session;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::layout::{Content, Stream, content};
    use crate::typeset::parse::parse;
    use crate::typeset::text::DocumentText;

    /// 30 paragraphs, "Line 1" to "Line 30", laid out in 40 columns.
    fn laid(style: FrontMatterStyle) -> (Stream, Map, DocumentText, Edition) {
        let body: String = (1..=30).map(|n| format!("Line {n}\n\n")).collect();
        let document = parse(&format!("---\ntitle: Secret\n---\n\n{body}"));
        let inputs = Inputs {
            front_matter: style,
            ..Inputs::new(Capabilities::kitty(40))
        };
        let stream = content(Content::Document(&document), &inputs);
        let map = Map::new(&stream.lines, &stream.blocks);
        let text = DocumentText::new([&document]);
        let edition = Edition::new(Content::Document(&document), &Session::new(["a.md"]), None);
        (stream, map, text, edition)
    }

    fn view(stream: &Stream) -> View {
        View::new(vec![1; stream.lines.len()], 5)
    }

    #[test]
    fn a_range_that_is_not_shown_says_why_and_the_view_stays() {
        let (stream, map, text, edition) = laid(FrontMatterStyle::Hide);
        let seen = Seen {
            map: &map,
            lines: &stream.lines,
            blocks: &stream.blocks,
        };
        let mut shown = view(&stream);
        shown.go_to(10);
        let secret = &text.find("Secret", false)[0];
        let result = reveal(
            secret,
            Show::Line,
            seen,
            &mut shown,
            (&mut Bands::default(), &edition),
        );
        assert_eq!(result, Err(Hidden::FrontMatter));
        assert_eq!(shown.top(), 10);
    }

    #[test]
    fn a_range_is_shown_as_the_view_shows_its_first_line() {
        let (stream, map, text, edition) = laid(FrontMatterStyle::Table);
        let seen = Seen {
            map: &map,
            lines: &stream.lines,
            blocks: &stream.blocks,
        };
        for term in ["Line 5", "Line 17", "Line 30"] {
            let range = &text.find(term, false)[0];
            let parts = map.drawn(&stream.lines, range).unwrap();
            let line = map.start(&parts[0].piece).unwrap() + parts[0].rows.start;
            let mut revealed = view(&stream);
            let result = reveal(
                range,
                Show::Line,
                seen,
                &mut revealed,
                (&mut Bands::default(), &edition),
            );
            assert_eq!(result, Ok(false), "{term}");
            let mut direct = view(&stream);
            direct.reveal(line);
            assert_eq!(revealed.top(), direct.top(), "{term}");
            // In view already, the view stays.
            let result = reveal(
                range,
                Show::Line,
                seen,
                &mut revealed,
                (&mut Bands::default(), &edition),
            );
            assert_eq!(result, Ok(false));
            assert_eq!(revealed.top(), direct.top(), "{term}");
        }
    }

    #[test]
    fn an_empty_range_at_the_end_of_the_document_shows_the_last_page() {
        let (stream, map, text, edition) = laid(FrontMatterStyle::Table);
        let seen = Seen {
            map: &map,
            lines: &stream.lines,
            blocks: &stream.blocks,
        };
        let end = text.positions().last().unwrap();
        let mut shown = view(&stream);
        let result = reveal(
            &(end.clone()..end),
            Show::Line,
            seen,
            &mut shown,
            (&mut Bands::default(), &edition),
        );
        assert_eq!(result, Ok(false));
        let mut last = view(&stream);
        last.scroll(crate::session::view::Motion::End);
        assert_eq!(shown.top(), last.top());
    }
}
