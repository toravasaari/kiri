//! Which lines of a document fill the window, and how scrolling moves
//! them. A line with a larger scale is shown whole or not at all: the text
//! sizing protocol draws it only from its top row.

use std::ops::Range;

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
        Self {
            heights,
            rows,
            top: 0,
        }
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

    /// The top of the last page: the first line from which the rest of
    /// the document fits in the window.
    fn last_top(&self) -> usize {
        let mut used = 0;
        let fitting = self
            .heights
            .iter()
            .rev()
            .take_while(|&&height| {
                used += height;
                used <= self.rows
            })
            .count();
        // When not even the last line fits, the last page starts at it and
        // shows nothing; the pager gives no line taller than the window.
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

    /// The same view for lines laid out again, `heights` high, in a window
    /// of `rows` rows: the top stays at about the same place in the
    /// document.
    pub fn resize(&mut self, heights: Vec<usize>, rows: usize) {
        let at_end = self.top > 0 && self.top == self.last_top();
        let above: usize = self.heights[..self.top].iter().sum();
        let total: usize = self.heights.iter().sum();
        self.heights = heights;
        self.rows = rows;
        if at_end {
            self.top = self.last_top();
            return;
        }
        // The same share of the document's rows above the top.
        let new_total: usize = self.heights.iter().sum();
        let target = (above * new_total).checked_div(total).unwrap_or(0);
        let mut used = 0;
        let top = self
            .heights
            .iter()
            .take_while(|&&height| {
                used += height;
                used <= target
            })
            .count();
        self.top = top.min(self.last_top());
    }
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
        assert_eq!(view.visible(), 6..10);
        let view = scrolled(view, &[Motion::Up(2)]);
        assert_eq!(view.visible(), 4..8);
        let view = scrolled(view, &[Motion::Up(100)]);
        assert_eq!(view.visible(), 0..4);
    }

    #[test]
    fn a_document_that_fits_does_not_scroll() {
        let view = scrolled(plain(3, 4), &[Motion::Down(1), Motion::End]);
        assert_eq!(view.top(), 0);
    }

    #[test]
    fn pages_move_by_the_lines_shown() {
        let view = scrolled(plain(10, 4), &[Motion::PageDown]);
        assert_eq!(view.visible(), 4..8);
        let view = scrolled(view, &[Motion::PageDown]);
        assert_eq!(view.visible(), 6..10);
        let view = scrolled(view, &[Motion::PageUp]);
        assert_eq!(view.visible(), 2..6);
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
        assert_eq!(view.visible(), 6..10);
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
        let view = scrolled(View::new(vec![1, 1, 1, 1, 2], 3), &[Motion::End]);
        assert_eq!(view.visible(), 3..5);
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
        view.resize(Vec::new(), 2);
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
    fn resizing_keeps_the_place_in_the_document() {
        let mut view = scrolled(plain(100, 10), &[Motion::Down(50)]);
        // Lines rewrapped to half as many, in a taller window.
        view.resize(vec![1; 50], 20);
        assert_eq!(view.top(), 25);
        // At the end, the view stays on the last page.
        let mut view = scrolled(plain(100, 10), &[Motion::End]);
        view.resize(vec![1; 100], 20);
        assert_eq!(view.visible(), 80..100);
        view.resize(vec![1; 100], 5);
        assert_eq!(view.visible(), 95..100);
    }
}
