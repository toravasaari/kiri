//! A centred wrapper such as `<p align="center">` in the cell layout: its paragraphs, headings and pictures centred in its room.
//!
//! The centring is padding of spaces before the text or the picture. The
//! padding is the layout's own, with no origin in the document's text, so
//! selecting and copying skip it as they skip the indent of a quote.

use crate::typeset::layout::{Context, Layout};
use crate::typeset::model::Block;
use crate::typeset::style::{Line, Span, Style};
use crate::typeset::wrap::width as width_of;

impl Layout<'_> {
    /// The blocks of a centred wrapper, one below another, those that are
    /// centred in the `width` cells they have.
    pub(super) fn centred(&self, blocks: &[Block], width: usize, context: Context) -> Vec<Line> {
        let mut lines = Vec::new();
        for (index, block) in blocks.iter().enumerate() {
            let layout = Layout {
                centre: block.is_centred_in_wrapper(),
                ..*self
            };
            let block_lines = layout.block_at(index, block, width, context);
            super::append_block(&mut lines, block_lines, !context.tight);
        }
        lines
    }

    /// Centres the text lines among `lines`, a paragraph's, in `width`
    /// cells, when this layout centres what it lays out. A row of a
    /// picture is placed by the picture's own centring.
    pub(super) fn centre_text(&self, lines: &mut [Line], width: usize) {
        if !self.centre {
            return;
        }
        for line in lines.iter_mut().filter(|line| line.image.is_none()) {
            let pad = width.saturating_sub(width_of(&line.spans)) / 2;
            if pad > 0 && !line.is_blank() {
                line.indent(Span::new(&" ".repeat(pad), Style::default()));
            }
        }
    }

    /// The cells to leave before something `used` cells wide that is
    /// centred in `room`, when this layout centres what it lays out.
    pub(super) fn centre_pad(&self, room: usize, used: usize) -> usize {
        match self.centre {
            true => room.saturating_sub(used) / 2,
            false => 0,
        }
    }
}

impl Layout<'_> {
    /// The `spans` of a heading's row, in `style`, with the spaces before
    /// them that centre them in `room` cells, when this layout centres what
    /// it lays out. The spaces are as large as the text, so there are as
    /// many whole ones as fit in the half of the spare room.
    pub(super) fn centre_row(&self, mut spans: Vec<Span>, room: usize, style: &Style) -> Vec<Span> {
        let scale = usize::from(style.size.scale().max(1));
        let spaces = self.centre_pad(room, width_of(&spans)) / scale;
        if spaces > 0 {
            spans.insert(0, Span::new(&" ".repeat(spaces), style.clone()));
        }
        spans
    }
}
