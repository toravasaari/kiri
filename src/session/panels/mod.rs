//! A panel as a document: its look, its frame and its scroll.
//!
//! A panel is text that the loop lays out like any document, at the width
//! [`width`] gives, and that the state engine draws in a frame over the
//! middle of the screen, from the line its [`Scroll`] is at. The help is
//! the one there is.
//!
//! `Panel` holds the lines the loop laid out, the width they were laid out in
//! and where each topic starts. `Help` is the help card's scroll, which opens
//! at a topic and stops at the ends, and the press, drag and release of the
//! left button on its bar. `Shape` says where the frame is in the window,
//! which the drawing and the mouse both ask. `scrollbar` and `bar_cell` give
//! the bar's colours and its cells in half rows, `width` and `visible_rows`
//! the text's width and the rows inside the frame, and `Placed` is spans
//! to write at a place in the window.

pub(crate) mod help;

use crate::session::commands::Topic;
use crate::session::view::Motion;
use crate::typeset::caps::Capabilities;
use crate::typeset::color::{Color, Rgb};
use crate::typeset::style::{Line, Span, Style};
use crate::typeset::theme::Theme;

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

/// Where a panel's frame is in the window, and what is in it: the one
/// place that the drawing and the mouse both ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shape {
    /// The window's columns and the rows the frame may use.
    pub(crate) window: (usize, usize),
    /// The column and the row of the frame's top left corner, from 1.
    pub(crate) left: usize,
    pub(crate) top: usize,
    /// The frame's cells across and down, the edges included.
    pub(crate) width: usize,
    pub(crate) height: usize,
    /// The cells the text is as wide as.
    pub(crate) text: usize,
    /// The lines that show at once, and the lines there are.
    pub(crate) shown: usize,
    pub(crate) total: usize,
}

impl Shape {
    /// The frame of a panel of `total` lines laid out `text` cells wide,
    /// in the middle of the top `rows` rows of a window `columns` wide.
    pub(crate) fn new((columns, rows): (usize, usize), text: usize, total: usize) -> Self {
        let shown = visible_rows(rows).min(total.max(1));
        let mut shape = Self {
            window: (columns, rows),
            left: 1,
            top: 1,
            width: 0,
            height: shown + 2,
            text,
            shown,
            total,
        };
        shape.width = text + 4 + usize::from(shape.has_bar());
        shape.left = columns.saturating_sub(shape.width) / 2 + 1;
        shape.top = rows.saturating_sub(shape.height) / 2 + 1;
        shape
    }

    /// Whether the panel has more lines than show, so a bar tells where
    /// they are.
    pub(crate) fn has_bar(&self) -> bool {
        self.total > visible_rows(self.window.1)
    }

    /// Whether the cell at `column` and `row` of the window, from 1, is
    /// in the frame.
    pub(crate) fn contains(&self, column: usize, row: usize) -> bool {
        (self.left..self.left + self.width).contains(&column)
            && (self.top..self.top + self.height).contains(&row)
    }

    /// The column of the window the bar is in.
    pub(crate) fn bar_column(&self) -> usize {
        self.left + self.width - 2
    }

    /// The row of the window where the lines of the frame start.
    pub(crate) fn first_row(&self) -> usize {
        self.top + 1
    }

    /// The bar for a panel scrolled to line `at`: where it starts and how
    /// long it is, in half rows, or none when everything shows.
    pub(crate) fn thumb(&self, at: usize) -> Option<(usize, usize)> {
        self.has_bar()
            .then(|| half_thumb(self.total, self.shown, at))
    }

    /// The line at the top when the bar's first half row is at `half`:
    /// the other way round from [`Shape::thumb`], for dragging.
    pub(crate) fn line_at(&self, half: usize) -> usize {
        top_at(self.total, self.shown, half)
    }
}

/// The line at the top of `shown` lines out of `total` when the bar's first
/// half row is at `half`: the other way round from [`half_thumb`].
pub(crate) fn top_at(total: usize, shown: usize, half: usize) -> usize {
    let (_, size) = half_thumb(total, shown, 0);
    let room = (2 * shown).saturating_sub(size).max(1);
    let lines = total.saturating_sub(shown);
    let at = (half.min(room) * lines + room / 2) / room;
    at.min(lines)
}

/// The bar of `shown` lines out of `total`, with the top at line `at`, in
/// half rows: where it starts, and how long it is, at least two halves.
pub(crate) fn half_thumb(total: usize, shown: usize, at: usize) -> (usize, usize) {
    let halves = 2 * shown;
    let lines = total.saturating_sub(shown).max(1);
    let size = ((halves * shown * 2 + total) / (2 * total.max(1))).clamp(2, halves.max(2));
    let room = halves.saturating_sub(size);
    (((room * at.min(lines)) * 2 + lines) / (2 * lines), size)
}

/// How much of the way from the page's colour to the text's the bar's
/// track and the bar are.
pub(crate) const TRACK: f32 = 0.14;
pub(crate) const THUMB: f32 = 0.45;

/// The style of the bar's cells under `theme`: the track as the
/// background, a little lighter than the page where the page is dark, and
/// darker where it is light, and the bar as the foreground, further the
/// same way. Where neither the theme nor the terminal gave the page's colours,
/// the palette's greys.
pub(crate) fn scrollbar(theme: &Theme, caps: &Capabilities) -> Style {
    let page = Rgb::new(theme, caps).and_then(|rgb| rgb.page());
    let mixed =
        page.and_then(|(text, page)| Some((page.mix(text, TRACK)?, page.mix(text, THUMB)?)));
    let (track, thumb) = mixed.unwrap_or((Color::Palette(237), Color::Palette(250)));
    Style {
        fg: Some(thumb),
        bg: Some(track),
        ..Style::default()
    }
}

/// The cell of the bar in the row `row` of the frame's lines, counted
/// from 0: a full block, the upper or the lower half of one, or none,
/// for a bar that is `thumb` half rows.
pub(crate) fn bar_cell(row: usize, (start, size): (usize, usize)) -> &'static str {
    let inside = |half: usize| (start..start + size).contains(&half);
    match (inside(2 * row), inside(2 * row + 1)) {
        (true, true) => "█",
        (true, false) => "▀",
        (false, true) => "▄",
        (false, false) => " ",
    }
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

/// The help panel on the deck: where it is scrolled to, and the bar's
/// grip while the left button holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Help {
    pub(crate) scroll: Scroll,
    /// The half rows between the bar's start and the place that was
    /// pressed, while the bar is dragged.
    grip: Option<usize>,
}

/// What a press of the left button did to a panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pressed {
    /// The press was outside the frame: the panel is to close.
    Outside,
    /// The press was inside it, and did what it does there, which may be
    /// nothing.
    Inside,
}

impl Help {
    /// A help that opens at the heading of `topic`.
    pub(crate) fn at(topic: Topic) -> Self {
        Self {
            scroll: Scroll::Topic(topic),
            grip: None,
        }
    }

    /// The left button pressed at `column` and `row` of the window, from
    /// 1, over the frame `shape`: on the bar's track it pages towards the
    /// press, on the bar it takes hold of it, anywhere else inside the
    /// frame it does nothing, and outside the frame it asks the panel to
    /// close.
    pub(crate) fn press(&mut self, shape: &Shape, (column, row): (usize, usize)) -> Pressed {
        self.grip = None;
        if !shape.contains(column, row) {
            return Pressed::Outside;
        }
        let first = shape.first_row();
        let on_track = column == shape.bar_column() && (first..first + shape.shown).contains(&row);
        let Some(thumb) = shape.thumb(self.top()).filter(|_| on_track) else {
            return Pressed::Inside;
        };
        let (start, size) = thumb;
        let (upper, lower) = (2 * (row - first), 2 * (row - first) + 1);
        let rows = shape.window.1;
        if lower < start {
            self.scroll(Motion::PageUp, shape.total, rows);
        } else if upper >= start + size {
            self.scroll(Motion::PageDown, shape.total, rows);
        } else {
            self.grip = Some(upper.saturating_sub(start));
        }
        Pressed::Inside
    }

    /// The mouse moved to `row` with the button held: the bar goes with
    /// it if it was taken hold of.
    pub(crate) fn drag(&mut self, shape: &Shape, row: usize) {
        let Some(grip) = self.grip else {
            return;
        };
        let half = 2 * row.saturating_sub(shape.first_row());
        self.scroll = Scroll::Line(shape.line_at(half.saturating_sub(grip)));
    }

    /// The button let go: the bar is let go too.
    pub(crate) fn release(&mut self) {
        self.grip = None;
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
    fn the_frame_is_as_wide_as_the_text_and_the_bar_and_in_the_middle() {
        // Everything shows: four cells of frame and space, and no bar.
        let shape = Shape::new((80, 30), 40, 10);
        assert_eq!((shape.width, shape.height), (44, 12));
        assert_eq!((shape.left, shape.top), (19, 10));
        assert!(!shape.has_bar());
        assert_eq!(shape.thumb(0), None);
        // More lines than rows: the bar's column too.
        let shape = Shape::new((80, 12), 40, 30);
        assert_eq!((shape.width, shape.height), (45, 12));
        assert_eq!((shape.left, shape.top), (18, 1));
        assert!(shape.has_bar());
        assert_eq!(shape.bar_column(), 18 + 45 - 2);
        assert_eq!(shape.first_row(), 2);
        assert!(shape.contains(18, 1) && shape.contains(62, 12));
        assert!(!shape.contains(17, 1) && !shape.contains(63, 1) && !shape.contains(20, 13));
    }

    #[test]
    fn the_bar_is_in_half_rows_at_the_top_the_middle_and_the_end() {
        // Eight lines of 30 show: 16 half rows, a bar of 4 of them.
        let shape = Shape::new((80, 10), 40, 30);
        assert_eq!(shape.thumb(0), Some((0, 4)));
        assert_eq!(shape.thumb(1), Some((1, 4)));
        assert_eq!(shape.thumb(11), Some((6, 4)));
        assert_eq!(shape.thumb(22), Some((12, 4)));
        // The cells of the first rows at the top, a half row down, and the
        // end.
        let cells = |thumb| (0..8).map(|row| bar_cell(row, thumb)).collect::<String>();
        assert_eq!(cells((0, 4)), "██      ");
        assert_eq!(cells((1, 4)), "▄█▀     ");
        assert_eq!(cells((12, 4)), "      ██");
    }

    #[test]
    fn the_bar_is_lighter_than_the_track_which_is_a_little_off_the_page() {
        let theme = Theme {
            foreground: Some(Color::Rgb(200, 200, 200)),
            background: Some(Color::Rgb(0, 0, 0)),
            ..Theme::default()
        };
        let bar = scrollbar(&theme, &Capabilities::ansi(80));
        assert_eq!(bar.bg, Some(Color::Rgb(28, 28, 28)));
        assert_eq!(bar.fg, Some(Color::Rgb(90, 90, 90)));
        // On a light page the same moves the other way.
        let light = Theme {
            foreground: Some(Color::Rgb(0, 0, 0)),
            background: Some(Color::Rgb(255, 255, 255)),
            ..Theme::default()
        };
        let bar = scrollbar(&light, &Capabilities::ansi(80));
        assert_eq!(bar.bg, Some(Color::Rgb(219, 219, 219)));
        assert_eq!(bar.fg, Some(Color::Rgb(140, 140, 140)));
        // With no colours of its own, a theme has the palette's.
        let none = Theme {
            foreground: None,
            background: None,
            ..Theme::default()
        };
        assert_eq!(
            scrollbar(&none, &Capabilities::ansi(80)).bg,
            Some(Color::Palette(237))
        );
    }

    fn probed(fg: u8, bg: u8) -> Capabilities {
        Capabilities {
            foreground: Some(Color::Rgb(fg, fg, fg)),
            background: Some(Color::Rgb(bg, bg, bg)),
            ..Capabilities::ansi(80)
        }
    }

    #[test]
    fn the_built_in_theme_gets_a_bar_for_the_page_the_terminal_told() {
        let theme = Theme::default();
        let dark = scrollbar(&theme, &probed(200, 0));
        assert_eq!(dark.bg, Some(Color::Rgb(28, 28, 28)));
        assert_eq!(dark.fg, Some(Color::Rgb(90, 90, 90)));
        let light = scrollbar(&theme, &probed(0, 255));
        assert_eq!(light.bg, Some(Color::Rgb(219, 219, 219)));
        assert_eq!(light.fg, Some(Color::Rgb(140, 140, 140)));
        // The theme's own colours come before the terminal's.
        let own = Theme {
            foreground: Some(Color::Rgb(0, 0, 0)),
            background: Some(Color::Rgb(255, 255, 255)),
            ..Theme::default()
        };
        assert_eq!(scrollbar(&own, &probed(200, 0)).bg, light.bg);
    }

    #[test]
    fn the_bar_is_never_shorter_than_two_halves() {
        let shape = Shape::new((80, 10), 40, 2000);
        assert_eq!(shape.thumb(0), Some((0, 2)));
        assert_eq!(shape.thumb(1992), Some((14, 2)));
    }

    #[test]
    fn dragging_the_bar_gives_the_line_it_stands_for() {
        let shape = Shape::new((80, 10), 40, 30);
        assert_eq!(shape.line_at(0), 0);
        assert_eq!(shape.line_at(12), 22);
        assert_eq!(shape.line_at(99), 22);
        // Where the bar is for a line is where that line is for the bar.
        for at in 0..=22 {
            let (start, _) = shape.thumb(at).unwrap();
            let back = shape.line_at(start);
            assert_eq!(shape.thumb(back), shape.thumb(at), "{at} {back}");
        }
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
