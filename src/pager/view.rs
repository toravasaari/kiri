//! Which lines of a document fill the window, and how scrolling moves
//! them. A line with a larger scale is shown whole or not at all: the text
//! sizing protocol draws it only from its top row.

use std::ops::Range;

use crate::render::block::BlockInfo;

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

    /// The same view for lines laid out again, `heights` high, in a window
    /// of `rows` rows, and coming from `blocks`: the top stays in the same
    /// block, at the same share of its lines. A blank top line moves to the
    /// start of the next block, and a block that is gone to the first one
    /// after it. Without blocks to go by, the top keeps the same share of
    /// the document's rows above it.
    pub fn resize(&mut self, heights: Vec<usize>, blocks: Vec<Option<BlockInfo>>, rows: usize) {
        self.resize_by(heights, blocks, rows, BlockInfo::same_place);
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
        assert_eq!(view.top(), 6);
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
        view.go_to(8);
        assert_eq!(view.top(), 6);
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
        assert_eq!(view.visible(), 80..100);
        view.resize(vec![1; 100], Vec::new(), 5);
        assert_eq!(view.visible(), 95..100);
    }

    /// The heights and blocks of lines.
    type Laid = (Vec<usize>, Vec<Option<BlockInfo>>);

    /// A run of lines: the file and path of their block, or none for the
    /// blank lines between blocks, and how many lines.
    type Run<'a> = (Option<(usize, &'a [usize])>, usize);

    /// Lines in runs of blocks: each run is `(file, path, lines)`, one row
    /// high each, and `None` for a blank line between blocks.
    fn blocked(runs: &[Run]) -> Laid {
        let mut table = crate::render::block::Blocks::default();
        let mut blocks = Vec::new();
        for &(block, count) in runs {
            let block = block.map(|(file, path)| {
                let id = table.add(file, path.to_vec(), None);
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
        assert_eq!(view.visible(), 21..26);
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
}
