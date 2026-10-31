//! Lists: their items, markers and task boxes.

use crate::typeset::model::List;
use crate::typeset::reading::page::{
    Flow, ITEM_GAP, LINE_HEIGHT, LIST_INDENT, Look, MARKER_GAP, Shape, block_margins,
};
use crate::typeset::reading::paint::Canvas;
use crate::typeset::reading::text::{self, Run};

impl Flow<'_> {
    pub(super) fn list(&mut self, list: &List, x: f32, width: f32, depth: usize) {
        let look = *self.look;
        let size = look.size;
        let indent = LIST_INDENT * size;
        let gap = if list.tight { ITEM_GAP * size } else { size };
        for (number, item) in list.items.iter().enumerate() {
            if number > 0 {
                self.y += gap;
            }
            let top = self.y;
            let mut inner = self.child(self.look, top);
            for block in &item.blocks {
                // A tight item's blocks follow one another without
                // margins, as a nested list follows its item's text.
                if list.tight {
                    inner.margin = None;
                }
                inner.top(block_margins(block, &look).0);
                inner.block(block, x + indent, width - indent, depth + 1);
            }
            // The marker sits on the baseline of the item's first line.
            let baseline = (inner.items.iter())
                .find_map(|item| match &item.shape {
                    Shape::Text { paragraph, y, .. } => Some(y + paragraph.first_baseline()),
                    _ => None,
                })
                .unwrap_or(top + size);
            let bottom = inner.y;
            self.take(&mut inner);
            self.y = bottom.max(top + LINE_HEIGHT * size);
            let marker_right = x + indent - MARKER_GAP * size;
            match item.checked {
                Some(checked) => {
                    let box_size = (0.8 * size).round();
                    let (box_x, box_y) =
                        (marker_right - box_size, baseline - box_size + 0.1 * size);
                    self.push(
                        box_y..box_y + box_size,
                        Shape::Task {
                            x: box_x,
                            y: box_y,
                            size: box_size,
                            checked,
                            look,
                        },
                    );
                }
                None => {
                    let marker = match list.start {
                        Some(start) => format!("{}.", start + number as u64),
                        None => ["•", "◦", "▪"][depth.min(2)].to_owned(),
                    };
                    let style = text::Look {
                        size,
                        line_height: LINE_HEIGHT,
                        weight: 400.0,
                        color: look.marker,
                        ..text::Look::PLAIN
                    };
                    let marker = text::lay_out(self.fonts, &[Run::new(&marker)], &style, width);
                    // Right-aligned, as GitHub sets markers.
                    let (marker_x, marker_y) = (
                        marker_right - marker.width(),
                        baseline - marker.first_baseline(),
                    );
                    let height = marker.height();
                    self.push(
                        marker_y..marker_y + height,
                        Shape::Text {
                            paragraph: Box::new(marker),
                            x: marker_x,
                            y: marker_y,
                            chip: None,
                        },
                    );
                }
            }
        }
    }
}

/// Draws the box of a task list item, `size` pixels square, with its top
/// left at (`x`, `y`): an outline, or filled in the link colour with a
/// tick when it is checked.
pub(super) fn task(canvas: &mut Canvas, x: f32, y: f32, size: f32, checked: bool, look: &Look) {
    if checked {
        canvas.round_rect(x, y, size, size, (size * 0.2).round(), look.link);
        // A tick of two strokes, as squares along each, white on a dark
        // box and black on a light one.
        let stroke = (size / 8.0).max(1.0);
        let [red, green, blue] = look.link.map(f32::from);
        let light = 0.299 * red + 0.587 * green + 0.114 * blue > 160.0;
        let ink = if light { [0; 3] } else { [0xff; 3] };
        let tick = [(0.2, 0.5), (0.4, 0.72), (0.8, 0.28)];
        for pair in tick.windows(2) {
            let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
            for step in 0..=12 {
                let t = step as f32 / 12.0;
                let (px, py) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
                canvas.rect(
                    x + px * size - stroke / 2.0,
                    y + py * size - stroke / 2.0,
                    stroke,
                    stroke,
                    ink,
                );
            }
        }
    } else {
        // An outline one pixel wide.
        canvas.rect(x, y, size, 1.0, look.border);
        canvas.rect(x, y + size - 1.0, size, 1.0, look.border);
        canvas.rect(x, y, 1.0, size, look.border);
        canvas.rect(x + size - 1.0, y, 1.0, size, look.border);
    }
}
