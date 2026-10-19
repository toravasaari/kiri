//! The interactive pager: the document on a screen of its own, scrolled
//! with keys.

pub mod keys;
pub mod view;

use std::io::{self, Write};

use crate::render::output::Writer;
use crate::render::style::Line;
use view::View;

/// Draws the lines `view` shows from the top of the screen, with the rows
/// below them empty, as one update so the screen does not flicker.
pub fn draw(
    lines: &[Line],
    view: &View,
    writer: &mut Writer,
    out: &mut impl Write,
) -> io::Result<()> {
    out.write_all(BEGIN_UPDATE)?;
    writer.clear(out)?;
    let mut top = 1;
    for line in &lines[view.visible()] {
        for row in 0..writer.rows(line) {
            write!(out, "\x1b[{};1H", top + usize::from(row))?;
            writer.row(line, row, out)?;
        }
        top += usize::from(writer.rows(line));
    }
    out.write_all(END_UPDATE)
}

/// Synchronized output: the terminal shows what comes between these at
/// once. Terminals without it ignore them.
const BEGIN_UPDATE: &[u8] = b"\x1b[?2026h";
const END_UPDATE: &[u8] = b"\x1b[?2026l";
