//! Headings in reading mode's page: their text in the heading's size and
//! colour, with a rule below the first two levels. `snap_to_rows` is for the
//! rows they start on.

use crate::typeset::model::{Block, Inline};
use crate::typeset::reading::page::{Flow, runs};
use crate::typeset::reading::text;

impl Flow<'_> {
    /// Lays a heading of `level` out at `x`, `width` pixels wide.
    pub(super) fn heading(&mut self, level: &u8, content: &[Inline], x: f32, width: f32) {
        let look = *self.look;
        let size = look.size;
        let index = usize::from(level.clamp(&1, &6) - 1);
        let style = text::Look {
            size: size * look.spacing.heading_sizes[index],
            line_height: look.spacing.heading_line_height,
            weight: look.spacing.bold,
            color: look.headings[index],
            align: self.align(),
            ..text::Look::PLAIN
        };
        let ink = self.ink(width);
        let paragraph = text::lay_out(self.fonts, &runs(content, &look, ink), &style, width);
        self.y += self.text(paragraph, x);
        if *level <= look.spacing.heading_rules {
            self.y += (look.spacing.rule_below * style.size).round();
            self.rect(x, self.y, width, 1.0, look.border);
            self.y += 1.0;
        }
    }
}

/// The deepest level of the headings that start on whole rows with
/// `snap_to_rows`; the deeper ones keep their place.
const SNAPPED_LEVELS: u8 = 4;

/// How close to a row's edge a block counts as on it, in pixels.
const SNAP_SLACK: f32 = 0.01;

/// Whether `block` starts on a whole row with `snap_to_rows`: a heading
/// of the levels that are places to jump to. A heading in a centred
/// wrapper is one too, as every block of a wrapper comes to the page as a
/// piece of its own.
pub(super) fn snaps(block: &Block) -> bool {
    matches!(block, Block::Heading { level, .. } if *level <= SNAPPED_LEVELS)
}

impl Flow<'_> {
    /// Moves the block that starts at `self.y` to the nearest whole row,
    /// so that a jump to it shows it from the top row of the screen, and
    /// returns where it starts now. It moves up only as far as leaves a
    /// font size, the gap between two paragraphs, below `above`, the end
    /// of the block above; otherwise it moves down to the next row.
    pub(super) fn snap(&mut self, above: f32) -> f32 {
        let row = self.row;
        let into = self.y - row * (self.y / row).floor();
        if into > SNAP_SLACK && into < row - SNAP_SLACK {
            let up = self.y - into;
            self.y = match into <= row / 2.0 && up - above >= self.look.size - SNAP_SLACK {
                true => up,
                false => up + row,
            };
        }
        self.y
    }
}
