//! A panel as a document: its look, its frame and its scroll.
//!
//! A panel is text that the loop lays out like any document, at the width
//! [`width`] gives, and that the state engine draws in a frame over the
//! middle of the screen, from the line its [`Scroll`] is at. The help is
//! the first; a menu and a table of contents will be others.

pub(crate) mod help;

use crate::session::commands::Topic;
use crate::session::view::Motion;
use crate::typeset::style::{Line, Span};

/// Spans to write at a place in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Placed {
    /// The row and the column, from 1 at the top left.
    pub(crate) row: usize,
    pub(crate) column: usize,
    pub(crate) spans: Vec<Span>,
}

/// The cells a panel's text is as wide as in a window with all its
/// `columns`, for text that needs `fit` cells to stay unwrapped: `fit`
/// where the window has room, and otherwise what is left of it after the
/// frame, the space inside it and the column of the scroll bar.
pub(crate) fn width(columns: usize, fit: usize) -> usize {
    fit.min(columns.saturating_sub(5)).max(1)
}

/// The lines of a panel that show inside its frame in a window of `rows`
/// rows: all but the frame's top and bottom, and at least one.
pub(crate) fn visible_rows(rows: usize) -> usize {
    rows.saturating_sub(2).max(1)
}

/// Where a panel's view of its lines is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scroll {
    /// At the heading of this topic, until a layout places it.
    Topic(Topic),
    /// This many lines down from the top.
    Line(usize),
}

/// The help panel on the deck: where it is scrolled to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Help {
    pub(crate) scroll: Scroll,
}

impl Help {
    /// A help that opens at the heading of `topic`.
    pub(crate) fn at(topic: Topic) -> Self {
        Self {
            scroll: Scroll::Topic(topic),
        }
    }

    /// The line at the top of the frame.
    pub(crate) fn top(&self) -> usize {
        match self.scroll {
            Scroll::Topic(_) => 0,
            Scroll::Line(line) => line,
        }
    }

    /// Places a scroll that waits for the lines, and keeps the top line
    /// where the last page of `panel` in a window of `rows` rows lets it
    /// be.
    pub(crate) fn resolve(&mut self, panel: &Panel, rows: usize) {
        let top = match self.scroll {
            Scroll::Topic(topic) => panel.line_of(topic),
            Scroll::Line(line) => line,
        };
        self.scroll = Scroll::Line(top.min(last_top(panel.len(), rows)));
    }

    /// Scrolls by `motion` in `lines` lines shown in a window of `rows`
    /// rows, no further than the last page.
    pub(crate) fn scroll(&mut self, motion: Motion, lines: usize, rows: usize) {
        let shown = visible_rows(rows);
        let top = self.top();
        let top = match motion {
            Motion::Down(n) => top.saturating_add(n),
            Motion::Up(n) => top.saturating_sub(n),
            Motion::PageDown => top.saturating_add(shown),
            Motion::PageUp => top.saturating_sub(shown),
            Motion::HalfPageDown => top.saturating_add(shown / 2),
            Motion::HalfPageUp => top.saturating_sub(shown / 2),
            Motion::Start => 0,
            Motion::End => usize::MAX,
        };
        self.scroll = Scroll::Line(top.min(last_top(lines, rows)));
    }
}

/// The last line that can be at the top of the frame.
fn last_top(lines: usize, rows: usize) -> usize {
    lines.saturating_sub(visible_rows(rows))
}

/// A panel laid out: its lines, and the line each topic of it starts at.
/// The loop makes it, since what a laid-out line shows is for the layout
/// to say.
#[derive(Debug)]
pub(crate) struct Panel {
    pub(crate) lines: Vec<Line>,
    /// The cells the lines were laid out in, which the frame is as wide as.
    pub(crate) width: usize,
    /// The line of each topic's heading, for the topics the panel has.
    pub(crate) topics: Vec<(Topic, usize)>,
}

impl Panel {
    /// The number of lines.
    pub(crate) fn len(&self) -> usize {
        self.lines.len()
    }

    /// The line where `topic` starts, or the first.
    pub(crate) fn line_of(&self, topic: Topic) -> usize {
        let found = self.topics.iter().find(|(listed, _)| *listed == topic);
        found.map_or(0, |(_, line)| *line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(lines: usize) -> Panel {
        let topics = vec![(Topic::Searching, 4), (Topic::Help, lines - 2)];
        Panel {
            lines: (0..lines).map(|_| Line::new(Vec::new())).collect(),
            width: 48,
            topics,
        }
    }

    #[test]
    fn the_text_is_as_wide_as_it_needs_or_what_the_window_leaves() {
        assert_eq!(width(80, 48), 48);
        assert_eq!(width(53, 48), 48);
        assert_eq!(width(52, 48), 47);
        assert_eq!(width(40, 48), 35);
        assert_eq!(width(80, 36), 36);
        assert_eq!(width(2, 48), 1);
        assert_eq!(visible_rows(24), 22);
        assert_eq!(visible_rows(1), 1);
    }

    #[test]
    fn scrolling_stops_at_both_ends() {
        let mut help = Help::at(Topic::Moving);
        help.scroll(Motion::Up(5), 20, 10);
        assert_eq!(help.top(), 0);
        help.scroll(Motion::Down(5), 20, 10);
        assert_eq!(help.top(), 5);
        // Eight lines show in ten rows, so 12 is the last top.
        help.scroll(Motion::End, 20, 10);
        assert_eq!(help.top(), 12);
        help.scroll(Motion::Down(1), 20, 10);
        assert_eq!(help.top(), 12);
        help.scroll(Motion::PageUp, 20, 10);
        assert_eq!(help.top(), 4);
        help.scroll(Motion::HalfPageDown, 20, 10);
        assert_eq!(help.top(), 8);
        help.scroll(Motion::Start, 20, 10);
        assert_eq!(help.top(), 0);
        // What fits does not scroll.
        help.scroll(Motion::End, 5, 10);
        assert_eq!(help.top(), 0);
    }

    #[test]
    fn a_topic_resolves_to_its_heading_line() {
        let panel = panel(40);
        let mut help = Help::at(Topic::Searching);
        assert_eq!(help.top(), 0);
        help.resolve(&panel, 16);
        assert_eq!(help.scroll, Scroll::Line(4));
        // A topic the panel does not have is its top.
        let mut help = Help::at(Topic::Links);
        help.resolve(&panel, 16);
        assert_eq!(help.scroll, Scroll::Line(0));
        // The last page lets no further: the heading is not on top.
        let mut help = Help::at(Topic::Help);
        help.resolve(&panel, 12);
        assert_eq!(help.top(), 30);
        // A larger window has a smaller last top, which pulls the line back.
        help.scroll(Motion::End, 40, 12);
        help.resolve(&panel, 40);
        assert_eq!(help.top(), 2);
    }
}
