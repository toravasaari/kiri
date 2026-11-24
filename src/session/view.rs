//! Which lines of a document fill the window, and how scrolling moves
//! them. A line with a larger scale is shown whole, or at a smaller scale
//! where the rows left at the bottom are too few, as the text sizing
//! protocol draws it only from its top row.
//!
//! Where the view is in the document is not a line number, which each
//! layout has its own of: it is an [`Anchor`], a place and the lines from the
//! line that shows it, taken from the lines of one layout and found again in
//! those of the next, whatever made the next (a resize, `M`, an image that
//! came, a reload that moved the place with `Align`).

use std::ops::Range;

use crate::session::edition::{Aligned, Moves};
use crate::typeset::locate::Map;
use crate::typeset::position::Pos;

/// A move through the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// Down by this many lines.
    Down(usize),
    /// Up by this many lines.
    Up(usize),
    PageDown,
    PageUp,
    HalfPageDown,
    HalfPageUp,
    Start,
    End,
}

/// How far down the document a view is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// The document's rows down to the bottom of the view, in percent.
    pub percent: u8,
}

/// Where a view is in the document, as a place and not a line, so that it
/// is the same place in another layout or version of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Anchor {
    /// At the start of the document, which the view stays at.
    Start,
    /// On the last page, which the view stays on.
    End,
    /// The place that is first shown at or below the top line, and the
    /// lines from the line that shows it down to the top line: positive
    /// when the top is inside what shows the place, such as the rows of a
    /// picture, and negative when it is a line with no place above the one
    /// that has it, such as the blank line between two blocks.
    At { pos: Pos, lines: isize },
}

impl Aligned for Anchor {
    fn aligned(&self, moves: &Moves) -> Option<Self> {
        match self {
            Self::At { pos, lines } => Some(Self::At {
                pos: moves.pos(pos)?,
                lines: *lines,
            }),
            other => Some(other.clone()),
        }
    }
}

/// The part of the document in a window of `rows` rows. Each line takes
/// as many rows as its height; the top of the window is always the top
/// of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    heights: Vec<usize>,
    rows: usize,
    top: usize,
}

impl View {
    /// The start of a document whose lines are `heights` rows high, in a
    /// window of `rows` rows.
    pub fn new(heights: Vec<usize>, rows: usize) -> Self {
        Self::at(heights, rows, 0)
    }

    /// A view of lines `heights` rows high in a window of `rows` rows, with
    /// `top` as the top line, or the last page when `top` is further down.
    pub fn at(heights: Vec<usize>, rows: usize, top: usize) -> Self {
        let mut view = Self { heights, rows, top };
        view.top = top.min(view.last_top());
        view
    }

    /// The first line shown.
    pub fn top(&self) -> usize {
        self.top
    }

    /// How far down the document the view is.
    pub fn position(&self) -> Position {
        let total: usize = self.heights.iter().sum();
        let down: usize = self.heights[..self.visible().end].iter().sum();
        let percent = (down * 100).checked_div(total).unwrap_or(100);
        Position {
            percent: u8::try_from(percent).unwrap_or(100),
        }
    }

    /// Whether the view is scrolled to the end of a document that is longer
    /// than the window: the last page, which is no more than where it is
    /// when the document fits.
    pub fn at_end(&self) -> bool {
        self.top > 0 && self.top >= self.last_top()
    }

    /// The window's rows.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// The lines shown whole, from the top: a line that does not fit below
    /// the others is left out, and [`View::left_over`] tells of it.
    pub fn visible(&self) -> Range<usize> {
        let mut used = 0;
        let end = self.heights[self.top..]
            .iter()
            .take_while(|&&height| {
                used += height;
                used <= self.rows
            })
            .count();
        self.top..self.top + end
    }

    /// The first line that does not fit below the lines shown, and the
    /// rows that are left for it; `None` when the lines shown are the last
    /// ones or fill the window.
    ///
    /// The frame draws that line smaller to fit, but it is not one of the
    /// lines [`View::visible`] gives, so it is not clickable and Tab does
    /// not stop at a link in it: the rows were blank before, and a link in
    /// a heading at the edge is one scroll away. Kept as it is.
    pub fn left_over(&self) -> Option<(usize, usize)> {
        let end = self.visible().end;
        let used: usize = self.heights[self.top..end].iter().sum();
        let left = self.rows.saturating_sub(used);
        (left > 0 && end < self.heights.len()).then_some((end, left))
    }

    /// The rows the top line takes.
    pub fn top_rows(&self) -> usize {
        self.heights.get(self.top).copied().unwrap_or(1)
    }

    /// The line shown on row `row` of the window, counted from 0.
    pub fn line_at(&self, row: usize) -> Option<usize> {
        let mut used = 0;
        for line in self.visible() {
            used += self.heights[line];
            if row < used {
                return Some(line);
            }
        }
        None
    }

    /// Where the view is, as `map` of its lines says. For lines that are
    /// made again for the same content, `exact`, a view on the last page
    /// stays on the line it has at the top, which the end of the document
    /// may move down, as when a picture comes in the place of its
    /// placeholder.
    pub(crate) fn anchor(&self, map: &Map, exact: bool) -> Anchor {
        if self.top == 0 {
            return Anchor::Start;
        }
        if self.top == self.last_top() && !exact {
            return Anchor::End;
        }
        // The first place at or below the top, or else the last above it.
        let below = (self.top..self.heights.len()).find_map(|line| map.first_on(line));
        let place =
            (below.or_else(|| (0..self.top).rev().find_map(|line| map.first_on(line)))).cloned();
        let Some(pos) = place else {
            return Anchor::Start;
        };
        let at = map.line_of(&pos).unwrap_or(self.top);
        Anchor::At {
            pos,
            lines: self.top as isize - at as isize,
        }
    }

    /// Puts the view where `anchor` says in the lines `map` is of. A top
    /// above the place is moved down to it, as a blank top line goes to the
    /// start of the next block, unless `exact`, for lines made again for the
    /// same content, which keep the blank line. A place that no line shows,
    /// as at the end of the document, puts the view on the last page.
    pub(crate) fn restore(&mut self, anchor: &Anchor, map: &Map, exact: bool) {
        let top = match anchor {
            Anchor::Start => 0,
            Anchor::End => usize::MAX,
            Anchor::At { pos, lines } => match map.line_of(pos) {
                Some(line) => line.saturating_add_signed(match exact {
                    true => *lines,
                    false => (*lines).max(0),
                }),
                None => usize::MAX,
            },
        };
        self.top = top.min(self.last_top());
    }

    /// The top of the last page: the first line from which the rest of
    /// the document fits in the window above [`BOTTOM_MARGIN`] empty rows.
    /// A document that fits in the window has no other page.
    fn last_top(&self) -> usize {
        if self.heights.iter().sum::<usize>() <= self.rows {
            return 0;
        }
        let room = self.rows.saturating_sub(BOTTOM_MARGIN);
        let mut used = 0;
        let fitting = self
            .heights
            .iter()
            .rev()
            .take_while(|&&height| {
                used += height;
                used <= room
            })
            .count();
        // When not even the last line fits above the margin, the last page
        // starts at it, with less margin or none; the pager gives no line
        // taller than the window.
        (self.heights.len() - fitting).min(self.heights.len().saturating_sub(1))
    }

    /// The top line above `top` from which the lines up to `top` take at
    /// most `rows` rows; at least one line up when there is one.
    fn top_above(&self, top: usize, rows: usize) -> usize {
        let mut used = 0;
        let fitting = self.heights[..top]
            .iter()
            .rev()
            .take_while(|&&height| {
                used += height;
                used <= rows
            })
            .count();
        top - fitting.max(1).min(top)
    }

    /// The top line below `top` after lines of at most `rows` rows; at
    /// least one line down.
    fn top_below(&self, top: usize, rows: usize) -> usize {
        let mut used = 0;
        let fitting = self.heights[top..]
            .iter()
            .take_while(|&&height| {
                used += height;
                used <= rows
            })
            .count();
        top + fitting.max(1)
    }

    /// Moves by `motion`, no further than the start and the last page.
    pub fn scroll(&mut self, motion: Motion) {
        let half = self.rows / 2;
        let top = match motion {
            Motion::Down(lines) => self.top.saturating_add(lines),
            Motion::Up(lines) => self.top.saturating_sub(lines),
            Motion::PageDown => self.top_below(self.top, self.rows),
            Motion::PageUp => self.top_above(self.top, self.rows),
            Motion::HalfPageDown => self.top_below(self.top, half),
            Motion::HalfPageUp => self.top_above(self.top, half),
            Motion::Start => 0,
            Motion::End => usize::MAX,
        };
        self.top = top.min(self.last_top());
    }

    /// Puts `line` at the top of the view, or as near it as the last page
    /// lets it come.
    pub fn go_to(&mut self, line: usize) {
        self.top = line.min(self.last_top());
    }

    /// Brings `line` into the view: when it is not shown, it becomes the
    /// top line, or the last page shows it.
    pub fn reveal(&mut self, line: usize) {
        if !self.visible().contains(&line) {
            self.top = line.min(self.last_top());
        }
    }

    /// Brings `lines`, a picture's or a link's, into the view so that all
    /// of them are shown, when they fit in it:
    /// - shown whole already: the view stays;
    /// - partly below the view: it scrolls down as little as it takes, so
    ///   that the last line is on the last row;
    /// - partly above it: it scrolls up so that the first line is the top;
    /// - none of them shown: they go in the middle of the view, or as near
    ///   it as the ends of the document allow;
    /// - taller than the view: the first line goes to the top, unless the
    ///   top is inside them already, as when the reader scrolled into them.
    pub fn show_whole(&mut self, lines: Range<usize>) {
        if lines.is_empty() || lines.end > self.heights.len() {
            return;
        }
        let height: usize = self.heights[lines.clone()].iter().sum();
        let visible = self.visible();
        let in_view = |line: usize| visible.contains(&line);
        let top = if height > self.rows {
            match lines.contains(&self.top) {
                true => self.top,
                false => lines.start,
            }
        } else if lines.clone().all(in_view) {
            self.top
        } else if lines.start < self.top && lines.end > self.top {
            lines.start
        } else if lines.clone().any(in_view) {
            // The first line of the lowest top from which they end in view.
            let mut used = 0;
            let kept = (self.heights[..lines.end].iter().rev())
                .take_while(|&&line| {
                    used += line;
                    used <= self.rows
                })
                .count();
            lines.end - kept
        } else {
            // The rows to leave above them, half of those they leave free.
            let above = (self.rows - height) / 2;
            let mut used = 0;
            let kept = (self.heights[..lines.start].iter().rev())
                .take_while(|&&line| {
                    used += line;
                    used <= above
                })
                .count();
            lines.start - kept
        };
        self.top = top.min(self.last_top());
    }

    /// Shows the change on `lines`, which is at a place between two lines
    /// when it has none. The view stays when it shows the first line of the
    /// change, or that place, or when its top is inside the change, as in a
    /// change taller than the window that the reader scrolled into.
    /// Otherwise the first line goes near the top, below [`ABOVE_CHANGE`]
    /// rows of the lines before it, at most a quarter of the window's rows;
    /// or as near the top as the last page lets it come.
    pub fn show_change(&mut self, lines: Range<usize>) {
        let visible = self.visible();
        let shown = match lines.is_empty() {
            true => visible.start <= lines.start && lines.start <= visible.end,
            false => visible.contains(&lines.start) || lines.contains(&self.top),
        };
        if shown {
            return;
        }
        let above = ABOVE_CHANGE.min(self.rows / 4);
        let mut used = 0;
        let fitting = self.heights[..lines.start]
            .iter()
            .rev()
            .take_while(|&&height| {
                used += height;
                used <= above
            })
            .count();
        self.top = (lines.start - fitting).min(self.last_top());
    }
}

/// The empty rows at the end of the last page, so that the end of the
/// document does not touch the status line below the window.
const BOTTOM_MARGIN: usize = 2;

/// The rows of the lines before a change that [`View::show_change`] shows
/// above it, in a window of at least four times as many rows.
const ABOVE_CHANGE: usize = 3;

#[cfg(test)]
mod tests;
