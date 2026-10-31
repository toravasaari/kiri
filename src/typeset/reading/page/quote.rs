//! Block quotes and GitHub alerts: the blocks of a quote indented past a bar,
//! and an alert with its title in the kind's colour.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::typeset::model::{AlertKind, Block};
use crate::typeset::reading::octicons;
use crate::typeset::reading::page::{
    ALERT_PADDING, ALERT_TITLE_GAP, BOLD, Flow, ICON_GAP, LINE_HEIGHT, Look, QUOTE_BAR,
    QUOTE_INDENT, Shape,
};
use crate::typeset::reading::text::{self, Run};

impl Flow<'_> {
    /// Lays a block quote out at `x`: its blocks indented past a bar in the
    /// border colour, in the quote's colour.
    pub(super) fn quote(&mut self, blocks: &[Block], x: f32, width: f32, depth: usize) {
        let look = *self.look;
        let size = look.size;
        let top = self.y;
        let quoted = Look {
            text: look.quote,
            ..look
        };
        let indent = QUOTE_INDENT * size;
        let mut inner = self.child(&quoted, top);
        inner.blocks(blocks, x + indent, width - indent, depth);
        self.take(&mut inner);
        self.y = inner.y;
        let bar = (QUOTE_BAR * size).round().max(1.0);
        self.rect(x, top, bar, self.y - top, look.border);
    }

    /// Lays an alert out: a tint over the room, a bar down its left edge,
    /// the title in the kind's colour after the kind's icon, and the blocks
    /// inside, in the room that is left.
    pub(super) fn alert(
        &mut self,
        kind: AlertKind,
        title: Option<&str>,
        blocks: &[Block],
        x: f32,
        width: f32,
        depth: usize,
    ) {
        let look = *self.look;
        let size = look.size;
        let alert = look.alert(kind);
        let bar = (QUOTE_BAR * size).round().max(1.0);
        let pad = ALERT_PADDING * size;
        let top = self.y;
        let (inner_x, inner_width) = (x + bar + size, width - bar - 2.0 * size);
        // Inline code in it has a chip of its own, if it has one at all.
        let inside = Look {
            chip: look.chip.and(alert.chip),
            ..look
        };
        let mut inner = self.child(&inside, top + pad);
        // The icon is a font size wide and tall, before the title.
        let icon_size = size.round();
        let icon_room = icon_size + ICON_GAP * size;
        let style = text::Look {
            size,
            line_height: LINE_HEIGHT,
            weight: BOLD,
            color: alert.color,
            ..text::Look::PLAIN
        };
        let name = title.unwrap_or(kind.name());
        let name = Run::new(name);
        let title = text::lay_out(self.fonts, &[name], &style, inner_width - icon_room);
        let line = title.lines().first().map_or(0.0, |line| line.height);
        let pixels = Rc::new(octicons::icon(kind, icon_size as u32, alert.color));
        let mut hasher = DefaultHasher::new();
        (kind as u8, alert.color, icon_size as u32).hash(&mut hasher);
        let id = hasher.finish();
        inner.ids.push(id);
        let y = inner.y + ((line - icon_size) / 2.0).max(0.0);
        inner.push(
            y..y + icon_size,
            Shape::Picture {
                x: inner_x,
                y,
                pixels,
            },
        );
        inner.y += inner.text(title, inner_x + icon_room);
        inner.y += ALERT_TITLE_GAP * size;
        inner.blocks(blocks, inner_x, inner_width, depth);
        let bottom = inner.y + pad;
        if let Some(tint) = alert.tint {
            self.rect(x, top, width, bottom - top, tint);
        }
        self.rect(x, top, bar, bottom - top, alert.bar);
        self.take(&mut inner);
        self.y = bottom;
    }
}
