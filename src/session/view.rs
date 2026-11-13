//! Which lines of a document fill the window, and how scrolling moves
//! them. A line with a larger scale is shown whole or not at all: the text
//! sizing protocol draws it only from its top row.

use std::ops::Range;

use crate::session::edition::{Aligned, Moves};
use crate::typeset::block::BlockInfo;
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
    /// The block each line comes from, as [`View::resize`] finds the place
    /// again in lines laid out anew. Empty when not known.
    blocks: Vec<Option<BlockInfo>>,
    rows: usize,
    top: usize,
}

impl View {
    /// The start of a document whose lines are `heights` rows high, in a
    /// window of `rows` rows.
    pub fn new(heights: Vec<usize>, rows: usize) -> Self {
        Self {
            heights,
            blocks: Vec::new(),
            rows,
            top: 0,
        }
    }

    /// This view for lines that come from `blocks`, one for each line.
    pub fn with_blocks(mut self, blocks: Vec<Option<BlockInfo>>) -> Self {
        self.blocks = blocks;
        self
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

    /// The window's rows.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// The lines shown whole, from the top: a line that does not fit below
    /// the others is left out, and its rows stay empty.
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

    /// Where the view is, as `map` of its lines says.
    pub(crate) fn anchor(&self, map: &Map) -> Anchor {
        if self.top == 0 {
            return Anchor::Start;
        }
        if self.top == self.last_top() {
            return Anchor::End;
        }
        // The first place at or below the top, or else the last above it.
        let below = (self.top..self.heights.len()).find_map(|line| map.first_on(line));
        let place = below.or_else(|| (0..self.top).rev().find_map(|line| map.first_on(line)));
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

    /// The same view for lines laid out again, `heights` high, in a window
    /// of `rows` rows, and coming from `blocks`: the top stays in the same
    /// block, at the same share of its lines. A blank top line moves to the
    /// start of the next block, and a block that is gone to the first one
    /// after it. Without blocks to go by, the top keeps the same share of
    /// the document's rows above it.
    pub fn resize(&mut self, heights: Vec<usize>, blocks: Vec<Option<BlockInfo>>, rows: usize) {
        self.resize_by(heights, blocks, rows, BlockInfo::same_place);
    }

    /// Like [`View::resize`], for the same document laid out again with an
    /// image of another height, as when an image that loaded takes the
    /// place of its placeholder: the top line stays the same line, the
    /// one as far into the same block, or the blank line as far above
    /// the same block below it, also at the end of the document.
    pub fn keep(&mut self, heights: Vec<usize>, blocks: Vec<Option<BlockInfo>>, rows: usize) {
        let old = std::mem::replace(&mut self.blocks, blocks);
        let (old_lines, top) = (old.len(), self.top);
        self.heights = heights;
        self.rows = rows;
        // The new start of the run of lines of the block `block`.
        let start_of = |block: &BlockInfo| {
            (self.blocks.iter())
                .position(|new| new.as_ref().is_some_and(|new| new.same_place(block)))
        };
        let kept = match old.get(top) {
            Some(Some(block)) => start_of(block).map(|found| {
                let offset = top - run_start(&old, top);
                found + offset.min(run_length(&self.blocks, found) - 1)
            }),
            _ => match (top..old_lines).find(|&line| old[line].is_some()) {
                Some(below) => {
                    let block = old[below].as_ref().expect("a line with a block");
                    start_of(block).map(|found| found.saturating_sub(below - top))
                }
                None => Some(self.heights.len().saturating_sub(old_lines - top)),
            },
        };
        self.top = kept.unwrap_or(top).min(self.last_top());
    }

    /// Like [`View::resize`], for lines that may come from another version
    /// of the document: `same` says whether a block of the new lines, the
    /// second, is the one of the old lines that the first is. A block with
    /// no counterpart is as good as gone: the top goes to the next block
    /// that has one, or to the end when none is left.
    pub fn resize_by(
        &mut self,
        heights: Vec<usize>,
        blocks: Vec<Option<BlockInfo>>,
        rows: usize,
        same: impl Fn(&BlockInfo, &BlockInfo) -> bool,
    ) {
        let at_end = self.top > 0 && self.top == self.last_top();
        let above: usize = self.heights[..self.top].iter().sum();
        let total: usize = self.heights.iter().sum();
        let old = std::mem::replace(&mut self.blocks, blocks);
        let old_top = self.top;
        self.heights = heights;
        self.rows = rows;
        if at_end {
            self.top = self.last_top();
            return;
        }
        let top = if old_top == 0 {
            0
        } else if old
            .get(old_top..)
            .is_some_and(|rest| rest.iter().any(Option::is_some))
        {
            self.line_following(&old, old_top, same)
        } else {
            self.line_at_share(above, total)
        };
        self.top = top.min(self.last_top());
    }

    /// The line of the new lines that carries on from the top line `top`
    /// of the `old` ones: the same place in the top block, or the start of
    /// the first block below it that is still there, or the end. A blank
    /// top line counts as the start of the next block.
    fn line_following(
        &self,
        old: &[Option<BlockInfo>],
        top: usize,
        same: impl Fn(&BlockInfo, &BlockInfo) -> bool,
    ) -> usize {
        let mut line = top;
        while let Some(first) = (line..old.len()).find(|&line| old[line].is_some()) {
            let block = old[first].as_ref().expect("a line with a block");
            let start = run_start(old, first);
            let length = run_length(old, start);
            // Only the top line's own block is entered part of the way.
            let offset = if first == top { first - start } else { 0 };
            let found = self
                .blocks
                .iter()
                .position(|new| new.as_ref().is_some_and(|new| same(block, new)));
            if let Some(found) = found {
                let new_length = run_length(&self.blocks, found);
                return found + (offset * new_length / length).min(new_length - 1);
            }
            line = start + length;
        }
        self.heights.len()
    }

    /// The first line below `above` of `total` rows, scaled to the rows of
    /// the lines now.
    fn line_at_share(&self, above: usize, total: usize) -> usize {
        let new_total: usize = self.heights.iter().sum();
        let target = (above * new_total).checked_div(total).unwrap_or(0);
        let mut used = 0;
        self.heights
            .iter()
            .take_while(|&&height| {
                used += height;
                used <= target
            })
            .count()
    }
}

/// The empty rows at the end of the last page, so that the end of the
/// document does not touch the status line below the window.
const BOTTOM_MARGIN: usize = 2;

/// The rows of the lines before a change that [`View::show_change`] shows
/// above it, in a window of at least four times as many rows.
const ABOVE_CHANGE: usize = 3;

/// The first line of the run of lines of one block that `line` is in.
fn run_start(blocks: &[Option<BlockInfo>], line: usize) -> usize {
    (0..line)
        .rev()
        .take_while(|&above| blocks[above] == blocks[line])
        .last()
        .unwrap_or(line)
}

/// How many lines the run of one block that starts at `start` has.
fn run_length(blocks: &[Option<BlockInfo>], start: usize) -> usize {
    blocks[start..]
        .iter()
        .take_while(|&block| *block == blocks[start])
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A view of `count` lines one row high in a window of `rows` rows.
    fn plain(count: usize, rows: usize) -> View {
        View::new(vec![1; count], rows)
    }

    fn scrolled(mut view: View, motions: &[Motion]) -> View {
        for &motion in motions {
            view.scroll(motion);
        }
        view
    }

    #[test]
    fn shows_the_lines_that_fit_from_the_top() {
        assert_eq!(plain(10, 4).visible(), 0..4);
        assert_eq!(plain(3, 4).visible(), 0..3);
    }

    #[test]
    fn scrolls_by_lines_no_further_than_the_last_page() {
        let view = scrolled(plain(10, 4), &[Motion::Down(1)]);
        assert_eq!(view.visible(), 1..5);
        let view = scrolled(view, &[Motion::Down(100)]);
        assert_eq!(view.visible(), 8..10);
        let view = scrolled(view, &[Motion::Up(2)]);
        assert_eq!(view.visible(), 6..10);
        let view = scrolled(view, &[Motion::Up(100)]);
        assert_eq!(view.visible(), 0..4);
    }

    #[test]
    fn a_document_that_fits_does_not_scroll() {
        let view = scrolled(plain(3, 4), &[Motion::Down(1), Motion::End]);
        assert_eq!(view.top(), 0);
        // Not even to make room for the margin at the end.
        let view = scrolled(plain(4, 4), &[Motion::Down(1), Motion::End]);
        assert_eq!(view.top(), 0);
    }

    #[test]
    fn the_last_page_ends_in_two_empty_rows() {
        let view = scrolled(plain(10, 6), &[Motion::End]);
        assert_eq!(view.visible(), 6..10);
        assert_eq!(view.line_at(3), Some(9));
        assert_eq!(view.line_at(4), None);
        assert_eq!(view.position().percent, 100);
    }

    #[test]
    fn a_last_line_too_tall_for_the_margin_gets_less_of_it() {
        let view = scrolled(View::new(vec![1, 1, 1, 1, 1, 3], 4), &[Motion::End]);
        assert_eq!(view.visible(), 5..6);
        assert_eq!(view.line_at(2), Some(5));
        assert_eq!(view.line_at(3), None);
    }

    #[test]
    fn pages_move_by_the_lines_shown() {
        let view = scrolled(plain(10, 4), &[Motion::PageDown]);
        assert_eq!(view.visible(), 4..8);
        let view = scrolled(view, &[Motion::PageDown]);
        assert_eq!(view.visible(), 8..10);
        let view = scrolled(view, &[Motion::PageUp]);
        assert_eq!(view.visible(), 4..8);
        let view = scrolled(view, &[Motion::PageUp]);
        assert_eq!(view.visible(), 0..4);
    }

    #[test]
    fn half_pages_move_by_half_the_window() {
        let view = scrolled(plain(20, 6), &[Motion::HalfPageDown]);
        assert_eq!(view.top(), 3);
        let view = scrolled(view, &[Motion::HalfPageUp]);
        assert_eq!(view.top(), 0);
        // A window of one row still moves.
        assert_eq!(scrolled(plain(5, 1), &[Motion::HalfPageDown]).top(), 1);
    }

    #[test]
    fn start_and_end_jump_to_the_first_and_the_last_page() {
        let view = scrolled(plain(10, 4), &[Motion::End]);
        assert_eq!(view.visible(), 8..10);
        let view = scrolled(view, &[Motion::Start]);
        assert_eq!(view.visible(), 0..4);
    }

    #[test]
    fn a_tall_line_scrolls_away_whole() {
        // A heading two rows high, then plain lines.
        let view = View::new(vec![2, 1, 1, 1, 1, 1], 3);
        assert_eq!(view.visible(), 0..2);
        let view = scrolled(view, &[Motion::Down(1)]);
        assert_eq!(view.visible(), 1..4);
        let view = scrolled(view, &[Motion::Up(1)]);
        assert_eq!(view.visible(), 0..2);
    }

    #[test]
    fn a_tall_line_that_does_not_fit_at_the_bottom_is_left_out() {
        let view = View::new(vec![1, 1, 2, 1, 1], 3);
        assert_eq!(view.visible(), 0..2);
        let view = scrolled(view, &[Motion::Down(1)]);
        assert_eq!(view.visible(), 1..3);
    }

    #[test]
    fn the_last_page_shows_the_last_line_whole() {
        let view = scrolled(View::new(vec![1, 1, 1, 1, 2], 4), &[Motion::End]);
        assert_eq!(view.visible(), 4..5);
        let view = scrolled(View::new(vec![1, 1, 1, 3, 1], 3), &[Motion::End]);
        assert_eq!(view.visible(), 4..5);
    }

    #[test]
    fn paging_past_a_tall_line_moves_at_least_one_line() {
        // A heading taller than the window cannot be shown, but paging
        // still gets past it.
        let view = View::new(vec![1, 4, 1, 1, 1], 3);
        assert_eq!(view.visible(), 0..1);
        let view = scrolled(view, &[Motion::PageDown]);
        assert_eq!(view.top(), 1);
        let view = scrolled(view, &[Motion::PageDown]);
        assert_eq!(view.top(), 2);
        let view = scrolled(view, &[Motion::PageUp]);
        assert_eq!(view.top(), 1);
    }

    #[test]
    fn a_page_up_shows_the_lines_above_whole() {
        // From line 4, a page up of three rows fits the heading of two
        // rows and one line, not the line above them.
        let view = View::new(vec![1, 1, 2, 1, 1, 1, 1], 3);
        let view = scrolled(view, &[Motion::Down(4), Motion::PageUp]);
        assert_eq!(view.top(), 2);
    }

    #[test]
    fn an_empty_document_stays_at_the_start() {
        let mut view = View::new(Vec::new(), 4);
        for motion in [Motion::Down(1), Motion::PageDown, Motion::End] {
            view.scroll(motion);
            assert_eq!(view.visible(), 0..0, "{motion:?}");
        }
        view.resize(Vec::new(), Vec::new(), 2);
        assert_eq!(view.top(), 0);
    }

    #[test]
    fn the_position_is_the_share_of_rows_down_to_the_bottom_of_the_view() {
        let position = |percent| Position { percent };
        assert_eq!(plain(10, 4).position(), position(40));
        let view = scrolled(plain(10, 4), &[Motion::End]);
        assert_eq!(view.position(), position(100));
        assert_eq!(plain(3, 4).position(), position(100));
        assert_eq!(plain(0, 4).position(), position(100));
        // A tall line that does not fit is not shown, so not counted.
        assert_eq!(View::new(vec![1, 1, 3], 4).position(), position(40));
    }

    #[test]
    fn revealing_a_line_moves_only_when_it_is_not_shown() {
        let mut view = plain(10, 4);
        view.reveal(3);
        assert_eq!(view.top(), 0);
        view.reveal(5);
        assert_eq!(view.top(), 5);
        view.reveal(9);
        assert_eq!(view.top(), 8);
        view.reveal(1);
        assert_eq!(view.top(), 1);
        // A heading of two rows is shown whole.
        let mut view = View::new(vec![1, 1, 1, 2, 1], 3);
        view.reveal(3);
        assert_eq!(view.visible(), 3..5);
    }

    #[test]
    fn going_to_a_line_puts_it_at_the_top_even_when_it_is_shown() {
        let mut view = plain(10, 4);
        view.go_to(2);
        assert_eq!(view.top(), 2);
        view.go_to(0);
        assert_eq!(view.top(), 0);
        // No further than the last page.
        view.go_to(9);
        assert_eq!(view.top(), 8);
    }

    #[test]
    fn resizing_keeps_the_place_in_the_document() {
        let mut view = scrolled(plain(100, 10), &[Motion::Down(50)]);
        // Lines rewrapped to half as many, in a taller window.
        view.resize(vec![1; 50], Vec::new(), 20);
        assert_eq!(view.top(), 25);
        // At the end, the view stays on the last page.
        let mut view = scrolled(plain(100, 10), &[Motion::End]);
        view.resize(vec![1; 100], Vec::new(), 20);
        assert_eq!(view.visible(), 82..100);
        view.resize(vec![1; 100], Vec::new(), 5);
        assert_eq!(view.visible(), 97..100);
    }

    /// The heights and blocks of lines.
    type Laid = (Vec<usize>, Vec<Option<BlockInfo>>);

    /// A run of lines: the file and path of their block, or none for the
    /// blank lines between blocks, and how many lines.
    type Run<'a> = (Option<(usize, &'a [usize])>, usize);

    /// Lines in runs of blocks: each run is `(file, path, lines)`, one row
    /// high each, and `None` for a blank line between blocks.
    fn blocked(runs: &[Run]) -> Laid {
        let mut table = crate::typeset::block::Blocks::default();
        let mut blocks = Vec::new();
        for &(block, count) in runs {
            let block = block.map(|(file, path)| {
                let id = table.add(file, path.to_vec());
                table.get(id).clone()
            });
            blocks.extend(std::iter::repeat_n(block, count));
        }
        (vec![1; blocks.len()], blocks)
    }

    /// Three blocks of 20 lines with a blank line between them, and the
    /// same when they are laid out again as 4, 10 and 10 lines.
    fn three_blocks() -> (Laid, Laid) {
        let runs = |a, b, c| {
            blocked(&[
                (Some((0, &[0][..])), a),
                (None, 1),
                (Some((0, &[1][..])), b),
                (None, 1),
                (Some((0, &[2][..])), c),
            ])
        };
        (runs(20, 20, 20), runs(4, 10, 10))
    }

    #[test]
    fn resizing_at_the_end_stays_at_the_end_with_blocks() {
        let ((heights, blocks), (new_heights, new_blocks)) = three_blocks();
        let mut view = View::new(heights, 5).with_blocks(blocks);
        view.scroll(Motion::End);
        view.resize(new_heights, new_blocks, 5);
        assert_eq!(view.visible(), 23..26);
    }

    #[test]
    fn resizing_keeps_the_same_share_of_the_top_block_above_the_top() {
        let ((heights, blocks), (new_heights, new_blocks)) = three_blocks();
        let mut view = View::new(heights, 5).with_blocks(blocks);
        // Half way into the second block.
        view.scroll(Motion::Down(31));
        view.resize(new_heights, new_blocks, 5);
        // The second block now starts on line 5 and has 10 lines.
        assert_eq!(view.top(), 10);
    }

    #[test]
    fn a_blank_top_line_goes_to_the_start_of_the_next_block() {
        let ((heights, blocks), (new_heights, new_blocks)) = three_blocks();
        let mut view = View::new(heights, 5).with_blocks(blocks);
        // The blank line between the first and the second block.
        view.scroll(Motion::Down(20));
        view.resize(new_heights, new_blocks, 5);
        assert_eq!(view.top(), 5);
    }

    /// Three blocks of 20 lines with a blank line between them, and the
    /// same when the first is 30 lines.
    fn taller_first() -> (Laid, Laid) {
        let runs = |a| {
            blocked(&[
                (Some((0, &[0][..])), a),
                (None, 1),
                (Some((0, &[1][..])), 20),
                (None, 1),
                (Some((0, &[2][..])), 20),
            ])
        };
        (runs(20), runs(30))
    }

    #[test]
    fn keeping_the_top_line_follows_it_down_when_a_block_above_grows() {
        for (top, kept) in [
            // In the second block, and on the blank line above it.
            (31, 41),
            (20, 30),
            // In the first block, which grew, as far into it.
            (5, 5),
            // At the end, which moved down.
            (57, 67),
        ] {
            let ((heights, blocks), (new_heights, new_blocks)) = taller_first();
            let mut view = View::new(heights, 5).with_blocks(blocks);
            view.scroll(Motion::Down(top));
            assert_eq!(view.top(), top);
            view.keep(new_heights, new_blocks, 5);
            assert_eq!(view.top(), kept, "from {top}");
        }
    }

    #[test]
    fn keeping_the_top_line_follows_it_up_when_a_block_above_shrinks() {
        for (top, kept) in [(41, 31), (67, 57)] {
            let ((new_heights, new_blocks), (heights, blocks)) = taller_first();
            let mut view = View::new(heights, 5).with_blocks(blocks);
            view.scroll(Motion::Down(top));
            view.keep(new_heights, new_blocks, 5);
            assert_eq!(view.top(), kept, "from {top}");
        }
    }

    #[test]
    fn keeping_the_top_line_inside_a_block_that_shrinks_stays_in_it() {
        let ((new_heights, new_blocks), (heights, blocks)) = taller_first();
        let mut view = View::new(heights, 5).with_blocks(blocks);
        view.scroll(Motion::Down(25));
        view.keep(new_heights, new_blocks, 5);
        assert_eq!(view.top(), 19);
    }

    #[test]
    fn a_change_shown_whole_does_not_move_the_view() {
        let mut view = scrolled(plain(40, 10), &[Motion::Down(5)]);
        view.show_change(5..15);
        assert_eq!(view.top(), 5);
        view.show_change(9..9);
        assert_eq!(view.top(), 5);
        // At the bottom of the view, after its last line.
        view.show_change(15..15);
        assert_eq!(view.top(), 5);
    }

    #[test]
    fn a_change_whose_start_is_shown_does_not_move_the_view() {
        let mut view = plain(60, 10);
        // Its first line on the last row, the rest below.
        view.show_change(9..20);
        assert_eq!(view.top(), 0);
    }

    #[test]
    fn a_view_inside_a_change_taller_than_the_window_stays() {
        let mut view = scrolled(plain(60, 10), &[Motion::Down(25)]);
        view.show_change(20..45);
        assert_eq!(view.top(), 25);
        // Below its end, the view goes to its start.
        let mut view = scrolled(plain(60, 10), &[Motion::Down(45)]);
        view.show_change(20..45);
        // Two rows above it: a quarter of the window's ten.
        assert_eq!(view.top(), 18);
    }

    #[test]
    fn a_change_away_from_the_view_goes_near_the_top() {
        let mut view = plain(60, 20);
        view.show_change(30..32);
        assert_eq!(view.top(), 27);
        // Above the view too, and cut off at the bottom.
        view.show_change(5..6);
        assert_eq!(view.top(), 2);
        view.show_change(5..30);
        assert_eq!(view.top(), 2);
        // A change without lines, as a removal is.
        view.show_change(25..25);
        assert_eq!(view.top(), 22);
        // Near the start, as many rows as there are.
        view.show_change(1..2);
        assert_eq!(view.top(), 0);
    }

    #[test]
    fn a_short_window_shows_a_quarter_of_its_rows_above_a_change() {
        let mut view = plain(40, 8);
        view.show_change(20..21);
        assert_eq!(view.top(), 18);
        let mut view = plain(40, 3);
        view.show_change(20..21);
        assert_eq!(view.top(), 20);
    }

    #[test]
    fn rows_above_a_change_count_by_height() {
        // A line of one row and a line of two above it.
        let mut heights = vec![1; 40];
        heights[12] = 2;
        let mut view = View::new(heights, 12);
        view.show_change(14..15);
        assert_eq!(view.top(), 12);
        // A tall line that does not fit in the rows above stays out.
        let mut heights = vec![1; 60];
        heights[30] = 4;
        let mut view = View::new(heights, 12);
        view.show_change(31..32);
        assert_eq!(view.top(), 31);
    }

    #[test]
    fn a_change_at_the_end_shows_the_last_page() {
        let mut view = plain(40, 10);
        view.show_change(38..40);
        assert_eq!(view.visible(), 32..40);
        view = plain(40, 10);
        view.show_change(40..40);
        assert_eq!(view.visible(), 32..40);
    }

    #[test]
    fn a_vanished_top_block_goes_to_the_next_that_is_left() {
        let (heights, blocks) = three_blocks().0;
        let mut view = View::new(heights, 5).with_blocks(blocks);
        view.scroll(Motion::Down(25));
        // The second block is gone.
        let (new_heights, new_blocks) = blocked(&[
            (Some((0, &[0][..])), 4),
            (None, 1),
            (Some((0, &[2][..])), 10),
        ]);
        view.resize(new_heights, new_blocks, 5);
        assert_eq!(view.top(), 5);
    }

    #[test]
    fn a_header_at_the_top_stays_there() {
        let runs = |a, b| {
            blocked(&[
                (Some((0, &[][..])), 3),
                (None, 1),
                (Some((0, &[0][..])), a),
                (None, 1),
                (Some((1, &[][..])), 3),
                (None, 1),
                (Some((1, &[0][..])), b),
            ])
        };
        let (heights, blocks) = runs(30, 30);
        let mut view = View::new(heights, 5).with_blocks(blocks);
        // The second file's header is on line 35.
        view.go_to(35);
        let (new_heights, new_blocks) = runs(10, 30);
        view.resize(new_heights, new_blocks, 5);
        assert_eq!(view.top(), 15);
    }

    #[test]
    fn resizing_counts_the_place_in_a_block_in_lines_not_rows() {
        // A heading, a blank line and a block of `count` lines, with the
        // heading `scale` rows high.
        let laid = |scale, count| {
            let (mut heights, blocks) = blocked(&[
                (Some((0, &[0][..])), 1),
                (None, 1),
                (Some((0, &[1][..])), count),
            ]);
            heights[0] = scale;
            (heights, blocks)
        };
        let (heights, blocks) = laid(2, 10);
        let mut view = View::new(heights, 5).with_blocks(blocks);
        // Two lines into the block of ten.
        view.scroll(Motion::Down(4));
        let (new_heights, new_blocks) = laid(3, 20);
        view.resize(new_heights, new_blocks, 5);
        // Four lines into the block of twenty; the same share of the rows
        // would be line 7.
        assert_eq!(view.top(), 6);
    }

    /// A view of 40 lines in a window of 10 rows, with its top on `top`.
    fn at(top: usize) -> View {
        let mut view = plain(40, 10);
        view.go_to(top);
        view
    }

    #[test]
    fn a_picture_partly_below_scrolls_down_until_its_bottom_edge_is_at_the_bottom() {
        // Lines 14 to 19 start at row 6 of the window and end below it.
        let mut view = at(8);
        view.show_whole(14..20);
        assert_eq!(view.visible(), 10..20);
        view.show_whole(14..20);
        assert_eq!(view.top(), 10, "shown whole: no move");
        let mut view = at(8);
        view.show_whole(10..14);
        assert_eq!(view.top(), 8, "shown whole: no move");
        // The same with rows of other heights: a heading of two rows.
        let mut view = View::new(vec![1, 1, 1, 2, 1, 1, 1, 1], 5);
        view.go_to(2);
        view.show_whole(5..8);
        assert_eq!(view.visible(), 4..8);
    }

    #[test]
    fn a_picture_partly_above_scrolls_up_until_its_top_edge_is_at_the_top() {
        let mut view = at(14);
        view.show_whole(12..16);
        assert_eq!(view.top(), 12);
    }

    #[test]
    fn a_picture_wholly_outside_the_view_is_put_in_the_middle() {
        // Four lines in ten rows: three rows above, three below.
        let mut view = at(0);
        view.show_whole(20..24);
        assert_eq!(view.top(), 17);
        let mut view = at(30);
        view.show_whole(10..14);
        assert_eq!(view.top(), 7);
        // Six lines: two rows above, two below.
        let mut view = at(0);
        view.show_whole(20..26);
        assert_eq!(view.top(), 18);
    }

    #[test]
    fn a_picture_near_an_end_of_the_document_is_centred_as_far_as_the_end_lets() {
        let mut view = at(20);
        view.show_whole(1..3);
        assert_eq!(view.top(), 0);
        let mut view = at(0);
        view.show_whole(37..40);
        assert_eq!(view.top(), view.last_top());
        assert!(view.visible().contains(&39), "{:?}", view.visible());
    }

    #[test]
    fn a_picture_taller_than_the_view_has_its_top_at_the_top() {
        let mut view = at(0);
        view.show_whole(5..25);
        assert_eq!(view.top(), 5);
        // Partly shown below: its top goes to the top of the view.
        let mut view = at(0);
        view.show_whole(8..28);
        assert_eq!(view.top(), 8);
        // The reader is inside it: it does not scroll up so that the top
        // goes out.
        let mut view = at(12);
        view.show_whole(5..25);
        assert_eq!(view.top(), 12);
    }

    /// The lines of `source` laid out as the engine and window say, and the
    /// map of them.
    fn laid(source: &str, reading: bool, width: u16) -> (crate::typeset::layout::Stream, Map) {
        use crate::render::Inputs;
        use crate::typeset::caps::Capabilities;
        use crate::typeset::layout::{Content, content};
        let inputs = match reading {
            false => Inputs::new(Capabilities::kitty(width)),
            true => Inputs {
                reading: Some(crate::typeset::reading::engine::Reading {
                    pictures: Default::default(),
                    fonts: std::rc::Rc::new(crate::typeset::reading::Fonts::fixture()),
                    wide: crate::typeset::reading::page::Wide::Wrap,
                }),
                ..Inputs::new(Capabilities {
                    cell_pixels: Some((10, 20)),
                    ..Capabilities::kitty(width)
                })
            },
        };
        let document = crate::typeset::parse::parse(source);
        let stream = content(Content::Document(&document), &inputs);
        let map = Map::new(&stream.lines, &stream.blocks);
        (stream, map)
    }

    const SOURCE: &str = "# Title\n\nFirst paragraph.\n\nSecond one, with more words.\n\n- a\n- b\n\n```\ncode\n```\n\nLast.\n";

    #[test]
    fn a_view_is_put_back_where_its_anchor_was_taken_in_the_same_lines() {
        for (reading, width) in [(false, 40), (false, 10), (true, 40)] {
            let (stream, map) = laid(SOURCE, reading, width);
            let heights = vec![1; stream.lines.len()];
            for top in 0..heights.len() {
                let mut view = View::new(heights.clone(), 5);
                view.go_to(top);
                let anchor = view.anchor(&map);
                let mut again = View::new(heights.clone(), 5);
                again.restore(&anchor, &map, true);
                assert_eq!(
                    again.top(),
                    view.top(),
                    "{reading} {width} top {top}: {anchor:?}"
                );
            }
        }
    }

    #[test]
    fn the_start_and_the_end_stay_where_they_are_and_a_blank_top_line_may_move_to_the_next_block() {
        let (stream, map) = laid(SOURCE, false, 40);
        let heights = vec![1; stream.lines.len()];
        let mut view = View::new(heights.clone(), 5);
        assert_eq!(view.anchor(&map), Anchor::Start);
        view.scroll(Motion::End);
        assert_eq!(view.anchor(&map), Anchor::End);
        // A blank line above a block has a place below it: a negative offset.
        let blank = (1..heights.len())
            .find(|&line| map.first_on(line).is_none())
            .unwrap();
        view.go_to(blank);
        let Anchor::At { lines, .. } = view.anchor(&map) else {
            panic!("an anchor at a place")
        };
        assert!(lines < 0, "{lines}");
    }

    #[test]
    fn the_same_place_is_the_top_in_the_other_engine() {
        let (text, text_map) = laid(SOURCE, false, 40);
        let (page, page_map) = laid(SOURCE, true, 40);
        let heights = |count: usize| vec![1; count];
        for top in [3, 5, 8] {
            let mut view = View::new(heights(text.lines.len()), 5);
            view.go_to(top);
            let Anchor::At { pos, .. } = view.anchor(&text_map) else {
                panic!("an anchor at a place")
            };
            let mut other = View::new(heights(page.lines.len()), 5);
            other.restore(&view.anchor(&text_map), &page_map, false);
            // What the page shows at the top is of the same block as what
            // the text had there, or the next one.
            let first = (other.top()..page.lines.len()).find_map(|line| page_map.first_on(line));
            assert!(
                first.clone().is_some_and(|first| first.path >= pos.path),
                "{top}: {first:?} {pos:?}"
            );
        }
    }
}
