//! Writes a `Scene` to an `impl Write` through the `Writer` as one
//! synchronized update: the lines row by row, then the bars in the order of
//! the scene. It does no I/O of its own. What a scene holds is `scene.rs`'s,
//! and kitty's own sequences are in `kitty/`.

use std::io::{self, Write};
use std::ops::Range;

use crate::present::kitty::graphics;
use crate::present::output::{self, Canvas, Writer};
use crate::present::scene::{Flat, Scene, Surface, flatten};
use crate::typeset::wrap::width;

/// Writes scenes, and only the rows of each that differ from what the
/// screen shows.
#[derive(Debug, Default)]
pub struct Presenter {
    /// What the screen shows as the last frame left it; `None` before the
    /// first frame and after `forget`.
    shown: Option<Shown>,
}

/// The screen as the last frame left it: the window, the canvas it was
/// drawn on, and each row.
#[derive(Debug)]
struct Shown {
    window: (u16, u16),
    canvas: Canvas,
    rows: Vec<Written>,
}

/// One row of the screen: the slot it was in, and the bytes written there,
/// the line's row and then the bars on it in the scene's order. For a row
/// that sent an image the bytes are those of the row once the terminal holds
/// the image, so they leave its data out, as `Rendered`'s `written` does.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Written {
    slot: Range<usize>,
    bytes: Vec<u8>,
}

/// The rendered rows of a frame: each row's line bytes, whether rendering
/// it sent image data or deleted a replaced image, and the bars as (row index, bytes) in scene order.
/// `lines` of a row that sent an image hold its data; `written` does not,
/// so that a row compares the same whether or not it sent.
struct Rendered {
    lines: Vec<Option<Vec<u8>>>,
    sent: Vec<bool>,
    bars: Vec<(usize, Vec<u8>)>,
    written: Vec<Written>,
}

impl Presenter {
    /// Forgets what the screen shows: the next frame is drawn whole, as the
    /// terminal may have lost it (tmux showing the pane again, a redraw
    /// the reader asked for).
    pub fn forget(&mut self) {
        self.shown = None;
    }

    /// The terminal may have lost what it showed: the next frame is drawn
    /// whole and every image is sent again as its row is written.
    pub fn lost(&mut self, writer: &mut Writer<'_>) {
        self.forget();
        writer.lost();
    }

    /// As `lost`, but only the images `ids` are sent again.
    pub fn lost_some(&mut self, writer: &mut Writer<'_>, ids: &[graphics::ImageId]) {
        self.forget();
        writer.lost_some(ids);
    }

    /// Writes `scene` through `writer` to `out` as one synchronized update.
    pub fn present(
        &mut self,
        scene: &Scene<'_>,
        writer: &mut Writer<'_>,
        out: &mut impl Write,
    ) -> io::Result<()> {
        // A write that fails partway leaves the screen showing neither the
        // last frame nor this one: forget the last, so the next frame is
        // drawn whole. The encoder has by then marked the images of the
        // rows rendered as sent, though the terminal may not have got them:
        // it is told they are lost, so that the next frame sends them again.
        let previous = self.shown.take();
        let result = self.draw(scene, writer, out, previous);
        if result.is_err() {
            writer.lost();
        }
        result
    }

    /// The writing of [`Presenter::present`], after `previous` was taken.
    fn draw(
        &mut self,
        scene: &Scene<'_>,
        writer: &mut Writer<'_>,
        out: &mut impl Write,
        previous: Option<Shown>,
    ) -> io::Result<()> {
        output::begin_update(out)?;
        writer.delete_unused(out)?;
        let flat = flatten(scene, |line| writer.rows(line));
        let canvas = writer.canvas();
        let full = previous
            .as_ref()
            .is_none_or(|shown| shown.window != scene.window || shown.canvas != canvas)
            || flat
                .rows
                .iter()
                .any(|row| row.line.is_some_and(|(line, _)| line.band.is_some()))
            || writer.has_bands();
        if full {
            writer.clear(out)?;
        }
        let rendered = render(scene, &flat, writer)?;
        match previous.as_ref().filter(|_| !full) {
            None => write_whole(&rendered, out)?,
            Some(shown) => {
                let dirty = dirty(&shown.rows, &rendered.written, &rendered.sent);
                write_changed(&rendered, &dirty, writer, out)?;
            }
        }
        self.shown = Some(Shown {
            window: scene.window,
            canvas,
            rows: rendered.written,
        });
        output::end_update(out)
    }
}

/// Renders every row of `flat` and every bar of `scene` into bytes.
fn render(scene: &Scene<'_>, flat: &Flat<'_>, writer: &mut Writer<'_>) -> io::Result<Rendered> {
    let mut lines = Vec::with_capacity(flat.rows.len());
    let mut sent = Vec::with_capacity(flat.rows.len());
    // For a row that sent an image, the row as it is written once the
    // terminal holds the image: without the image data.
    let mut held: Vec<Option<Vec<u8>>> = Vec::with_capacity(flat.rows.len());
    for row in &flat.rows {
        let Some((line, at)) = row.line else {
            lines.push(None);
            sent.push(false);
            held.push(None);
            continue;
        };
        writer.dim(row.dim);
        let mut bytes = Vec::new();
        writer.row(line, at, &mut bytes)?;
        let did_send = writer.sent_image();
        held.push(if did_send {
            let mut again = Vec::new();
            writer.row(line, at, &mut again)?;
            Some(again)
        } else {
            None
        });
        lines.push(Some(bytes));
        sent.push(did_send);
    }
    writer.dim(false);
    let mut bars = Vec::new();
    for surface in &scene.surfaces {
        let Surface::Bar { row, column, spans } = surface else {
            continue;
        };
        let Some(index) = row.checked_sub(1).filter(|r| *r < flat.rows.len()) else {
            continue;
        };
        let under = &flat.rows[index];
        let mut bytes = Vec::new();
        output::move_to(*row, *column, &mut bytes)?;
        // Writing on the top row of a scaled line drops the characters
        // it touches whole; below it, they stay until they are erased.
        if under.scaled && index != under.slot.start {
            writer.erase_chars(width(spans), &mut bytes)?;
        }
        writer.bar(spans, &mut bytes)?;
        bars.push((index, bytes));
    }
    let written = flat
        .rows
        .iter()
        .zip(lines.iter().zip(held))
        .enumerate()
        .map(|(r, (row, (line, held)))| {
            let mut bytes = held.or_else(|| line.clone()).unwrap_or_default();
            for (_, bar) in bars.iter().filter(|(at, _)| *at == r) {
                bytes.extend_from_slice(bar);
            }
            Written {
                slot: row.slot.clone(),
                bytes,
            }
        })
        .collect();
    Ok(Rendered {
        lines,
        sent,
        bars,
        written,
    })
}

/// Writes the lines in row order, then the bars in the order of the scene.
fn write_whole(rendered: &Rendered, out: &mut impl Write) -> io::Result<()> {
    for (r, line) in rendered.lines.iter().enumerate() {
        if let Some(bytes) = line {
            output::move_to(r + 1, 1, out)?;
            out.write_all(bytes)?;
        }
    }
    for (_, bytes) in &rendered.bars {
        out.write_all(bytes)?;
    }
    Ok(())
}

/// Erases the rows of `dirty`, top first, and writes their lines and bars.
fn write_changed(
    rendered: &Rendered,
    dirty: &[bool],
    writer: &Writer<'_>,
    out: &mut impl Write,
) -> io::Result<()> {
    for r in (0..dirty.len()).filter(|r| dirty[*r]) {
        output::move_to(r + 1, 1, out)?;
        writer.erase_line(out)?;
    }
    for r in (0..dirty.len()).filter(|r| dirty[*r]) {
        if let Some(bytes) = &rendered.lines[r] {
            output::move_to(r + 1, 1, out)?;
            out.write_all(bytes)?;
        }
    }
    for (r, bytes) in &rendered.bars {
        if dirty[*r] {
            out.write_all(bytes)?;
        }
    }
    Ok(())
}

/// The rows to write: those whose `Written` differs or that `sent` image
/// data or an image's deletion, widened to every row of each slot, old or new, they are in.
fn dirty(old: &[Written], new: &[Written], sent: &[bool]) -> Vec<bool> {
    let height = new.len();
    let mut dirty: Vec<bool> = new
        .iter()
        .enumerate()
        .map(|(r, row)| old.get(r) != Some(row) || sent[r])
        .collect();
    loop {
        let mut changed = false;
        for r in 0..dirty.len() {
            if !dirty[r] {
                continue;
            }
            let slots = [old.get(r).map(|w| &w.slot), Some(&new[r].slot)];
            for slot in slots.into_iter().flatten() {
                for covered in &mut dirty[slot.start.min(height)..slot.end.min(height)] {
                    changed |= !*covered;
                    *covered = true;
                }
            }
        }
        if !changed {
            return dirty;
        }
    }
}
