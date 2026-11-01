//! Code blocks, diagrams and the bands that a long one is cut into.

use crate::typeset::reading::page::{
    BAR_ROOM, Band, FITS, Flow, Label, Picture, Shape, UNBOUNDED, Wide, push,
};
use crate::typeset::reading::text::{self, Run};

impl Flow<'_> {
    /// Lays a code block out, as the picture of a diagram or formula it is
    /// drawn as, centred as the diagram's language says, or as code with
    /// why it could not be drawn below it.
    pub(super) fn code_or_diagram(
        &mut self,
        language: Option<&str>,
        code: &str,
        x: f32,
        width: f32,
    ) {
        let picture = match language {
            Some(language) => self.pictures.diagram(language, code, (width, f32::MAX)),
            None => Picture::None,
        };
        let centred = language.is_some_and(crate::typeset::diagrams::names::centred);
        let failed = match &picture {
            Picture::Failed(message) => message.clone(),
            _ => None,
        };
        if self.place(picture, None, x, width, centred) {
            return;
        }
        self.code(language, code, x, width);
        if let Some(message) = failed {
            self.y += 0.5 * self.look.size;
            let style = text::Look {
                size: self.look.size,
                line_height: self.look.spacing.line_height,
                color: self.look.error,
                ..text::Look::PLAIN
            };
            let paragraph = text::lay_out(self.fonts, &[Run::new(&message)], &style, width);
            self.y += self.text(paragraph, x);
        }
    }

    /// Lays a code block out at `x`: at least `width` pixels wide, as wide
    /// as its longest line up to the area's edge, and with lines longer
    /// than that wrapped at the edge.
    pub(super) fn code(&mut self, language: Option<&str>, code: &str, x: f32, width: f32) {
        let look = *self.look;
        let size = look.size;
        let padding = look.spacing.code_padding * size;
        let style = text::Look {
            size: size * text::CODE_SIZE,
            line_height: look.spacing.code_line_height,
            color: look.code_text,
            mono: true,
            wrap: text::Wrap::Anywhere,
            ..text::Look::PLAIN
        };
        let mut runs = Vec::new();
        for (index, line) in self.highlight.lines(language, code).into_iter().enumerate() {
            if index > 0 {
                push(&mut runs, "\n", &Run::default());
            }
            for run in line {
                push(&mut runs, &run.text.clone(), &run);
            }
        }
        let room = (self.area - x).max(width);
        // Scrolling, the lines stay whole; wrapping, they break at the
        // area's edge.
        let scroll = self.wide == Wide::Scroll;
        let line_width = if scroll {
            UNBOUNDED
        } else {
            room - 2.0 * padding
        };
        let paragraph = text::lay_out(self.fonts, &runs, &style, line_width);
        let (_, natural) = paragraph.content_widths();
        // A line that reaches the area's edge to within rounding fits.
        let banded = scroll && natural + 2.0 * padding > room + FITS;
        let block_width = match banded {
            true => natural + 2.0 * padding,
            false => (natural + 2.0 * padding).clamp(width, room),
        };
        let fonts = self.fonts;
        let draw = move |flow: &mut Self| {
            let height = paragraph.height() + 2.0 * padding;
            let top = flow.y;
            if let Some(color) = look.code_block {
                flow.push(
                    top..top + height,
                    Shape::RoundRect {
                        x,
                        y: top,
                        width: block_width,
                        height,
                        radius: look.spacing.code_radius * size,
                        color,
                    },
                );
            }
            flow.y += padding;
            flow.text(paragraph, x + padding);
            if let (Some(language), Some(color)) =
                (language.filter(|name| !name.is_empty()), look.label)
            {
                // In the padding above the code, one line of its own
                // height, so that a long first line never runs under it.
                let style = text::Look {
                    size: size * look.spacing.label_size,
                    line_height: 1.0,
                    color,
                    ..text::Look::PLAIN
                };
                let label = text::lay_out(fonts, &[Run::new(language)], &style, block_width);
                let label_y = top + ((padding - label.height()) / 2.0).max(0.0);
                if banded {
                    // The band draws it at its own corner, whatever the
                    // offset, and not scrolled with the code.
                    flow.y = top + height;
                    return Some(Label {
                        paragraph: Box::new(label),
                        y: label_y,
                    });
                }
                let label_x = x + block_width - 0.6 * size - label.width();
                let label_height = label.height();
                flow.push(
                    label_y..label_y + label_height,
                    Shape::Text {
                        paragraph: Box::new(label),
                        x: label_x,
                        y: label_y,
                        chip: None,
                    },
                );
            }
            flow.y = top + height;
            None
        };
        match banded {
            true => self.band(x, block_width, draw),
            false => {
                draw(self);
            }
        }
    }

    /// Lays out what `lay_out` lays out from the next whole row, `width`
    /// pixels wide from `x`, as a band of its own, which scrolls sideways
    /// in the room the area leaves it, above a scroll bar.
    pub(super) fn band(
        &mut self,
        x: f32,
        width: f32,
        lay_out: impl FnOnce(&mut Self) -> Option<Label>,
    ) {
        let row = self.row;
        let top = (self.y / row).ceil() * row;
        self.y = top;
        let start = self.items.len();
        let label = lay_out(self);
        let content = self.y - top;
        let items = (self.items.drain(start..))
            .map(|item| item.shifted(-x, -top))
            .collect();
        let rows = ((content + BAR_ROOM * self.look.size) / row)
            .ceil()
            .max(1.0);
        self.bands.push(Band {
            top,
            height: rows * row,
            x,
            width,
            visible: (self.area - x).max(1.0),
            content,
            label: label.map(|label| Label {
                y: label.y - top,
                ..label
            }),
            items,
        });
        self.y = top + rows * row;
    }
}
