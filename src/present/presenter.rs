//! Writes a `Scene` to an `impl Write` through the `Writer` as one synchronized
//! update: the lines row by row, then the bars in the order of the scene.
//!
//! An update deletes the images no longer kept, sends the data of the images the
//! rows show, writes the clear or the erases, then the lines' cells row by row and
//! the bars in the scene's order; a bar below the top row of a scaled line erases
//! its cells first. The presenter keeps what the screen shows and writes only the
//! rows that changed, each at its area's column and erased and ended inside its
//! area, and it forgets the screen when the terminal loses it (`lost`,
//! `lost_some`).
//!
//! It does no I/O of its own. What a scene holds is `scene.rs`'s,
//! and kitty's own sequences are in `kitty/`.

use std::io::{self, Write};
use std::ops::Range;

use crate::present::kitty::graphics;
use crate::present::output::{self, Canvas, Edge, Writer};
use crate::present::scene::{Area, Flat, Scene, Surface, flatten};
use crate::typeset::style::Line;
use crate::typeset::wrap::width;

/// Writes scenes, and only the rows of each that differ from what the
/// screen shows.
#[derive(Debug, Default)]
pub struct Presenter {
    /// What the screen shows as the last frame left it; `None` before the
    /// first frame and after `forget`.
    shown: Option<Shown>,
    /// The areas the shade is placed over on the screen.
    shaded: Vec<Area>,
    /// Whether the terminal may hold the shade's image: it was sent and
    /// not deleted since. A clear takes the placements away, not the image.
    shade_sent: bool,
    /// Whether a frame that is the last one shifted is written as a scroll
    /// of the terminal and the rows that are new.
    scroll: bool,
    /// The shift in rows the pager expects the next frame to be, from
    /// `view_at`; the first one tried.
    hint: Option<isize>,
    /// The line at the top of the view in the last frame, for `view_at`.
    top: Option<usize>,
}

/// The screen as the last frame left it: the window, the canvas it was
/// drawn on, the areas of lines, and each row of them, area after area.
#[derive(Debug)]
struct Shown {
    window: (u16, u16),
    canvas: Canvas,
    areas: Vec<Area>,
    rows: Vec<Written>,
}

/// Where a row of an area is on the screen, and how it is erased and ended.
#[derive(Debug, Clone, Copy)]
struct Place {
    /// The row of the window and the column of the area's first cell, from 1.
    row: usize,
    column: usize,
    /// The area's cells across, which an erase of the row covers.
    columns: usize,
    /// Whether the area covers every column of the window, so that the row
    /// may be erased whole with one `2K`.
    full_width: bool,
    /// Whether the area reaches the window's right edge.
    to_edge: bool,
}

impl Place {
    /// Moves to the first cell of the row.
    fn go(&self, out: &mut impl Write) -> io::Result<()> {
        output::move_to(self.row, self.column, out)
    }

    /// Erases the row's cells in the area, and only those: a row that is
    /// not the screen's whole row is erased with ECH, as `2K` would take
    /// what is beside the area, a scaled heading's lower row or a sixel.
    fn erase(&self, writer: &Writer<'_>, out: &mut impl Write) -> io::Result<()> {
        self.go(out)?;
        match self.full_width {
            true => writer.erase_line(out),
            false => writer.erase_chars(self.columns, out),
        }
    }
}

/// The place of every row of `flat`, in the order of `Flat::rows`.
fn places(flat: &Flat<'_>, window: (u16, u16)) -> Vec<Place> {
    flat.areas
        .iter()
        .flat_map(|flat| {
            let area = flat.area;
            (0..flat.rows.len()).map(move |r| Place {
                row: area.row + r,
                column: area.column,
                columns: area.columns,
                full_width: area.is_full_width(window),
                to_edge: area.reaches_edge(window),
            })
        })
        .collect()
}

/// One row of the screen: the slot it was in, and the bytes written there,
/// the line's cells and then the bars on it in the scene's order. The data
/// of the images the row shows is no part of them, so an image row compares
/// the same whether or not it sent its image. A loading band's transmission
/// is among the cells, but a frame with a band is drawn whole.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Written {
    slot: Range<usize>,
    bytes: Vec<u8>,
}

/// The rendered rows of a frame: what the rows send to the terminal, all
/// of it in row order, each row's cells, whether rendering the row sent
/// image data or deleted a replaced image, and the bars in scene order.
struct Rendered {
    sends: Vec<u8>,
    lines: Vec<Option<Vec<u8>>>,
    sent: Vec<bool>,
    bars: Vec<Bar>,
    written: Vec<Written>,
}

/// A bar rendered: it belongs to its row of the window, over whichever areas
/// it lies on.
struct Bar {
    /// The window's row, and the columns the bar covers, both from 1.
    row: usize,
    columns: Range<usize>,
    bytes: Vec<u8>,
}

impl Bar {
    /// Whether the bar lies over cells of the row of `place`.
    fn over(&self, place: &Place) -> bool {
        self.row == place.row
            && self.columns.start < place.column + place.columns
            && place.column < self.columns.end
    }
}

impl Bar {
    /// Whether the bar is among the bytes of the row of `place`: where it lies
    /// over it, or, if it lies over no row of its window row, on each.
    fn is_in(&self, place: &Place, places: &[Place]) -> bool {
        self.over(place) || (self.row == place.row && !places.iter().any(|other| self.over(other)))
    }
}

impl Presenter {
    /// A presenter that scrolls the terminal for a frame that is the last
    /// one shifted when `scroll` is set, and writes every changed row
    /// when it is not.
    pub fn scrolling(scroll: bool) -> Self {
        Self {
            scroll,
            ..Self::default()
        }
    }

    /// Tells the shift the next frame is expected to be: positive when the
    /// rows move up, negative when they move down.
    pub fn hint(&mut self, rows: isize) {
        self.hint = Some(rows);
    }

    /// Tells where the view's top is in `lines`, the layout's, so that the
    /// next frame's shift is known from the last: the rows `rows` gives
    /// the lines between the two tops.
    pub fn view_at(&mut self, top: usize, lines: &[Line], rows: impl Fn(&Line) -> u8) {
        if !self.scroll {
            return;
        }
        if let Some(last) = self.top.replace(top).filter(|last| *last != top) {
            let (from, to) = (last.min(top), last.max(top));
            let between: usize = (lines.get(from..to).unwrap_or_default().iter())
                .map(|line| usize::from(rows(line).max(1)))
                .sum();
            let between = isize::try_from(between).unwrap_or(isize::MAX);
            self.hint(if top > last { between } else { -between });
        }
    }

    /// The rows the terminal is to scroll the screen by before this frame
    /// is written, positive up, when the frame is the last one shifted and
    /// the switch is on.
    fn shift(
        &self,
        scene: &Scene<'_>,
        shown: &Shown,
        new: &[Written],
        hint: Option<isize>,
    ) -> Option<isize> {
        // The shade's placements are at absolute cells, and the terminal
        // would move them.
        let shade = scene
            .surfaces
            .iter()
            .any(|surface| matches!(surface, Surface::Shade { .. }));
        // A frame without a shade in its scene takes the placed one away
        // after it is written, so what is still placed is no reason.
        if !self.scroll || shade {
            return None;
        }
        best_shift(&shown.rows, new, hint)
    }

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
        // The order is: what the terminal keeps (the delete of images no
        // longer used, then the data of the rows' images), then whatever
        // erases, then the cells. A terminal that draws what has come so far
        // then never shows a row erased and waiting for its image.
        let hint = self.hint.take();
        output::begin_update(out)?;
        writer.delete_unused(out)?;
        let flat = flatten(scene, |line| writer.rows(line));
        let canvas = writer.canvas();
        let areas: Vec<Area> = flat.areas.iter().map(|flat| flat.area).collect();
        let places = places(&flat, scene.window);
        let full = previous.as_ref().is_none_or(|shown| {
            shown.window != scene.window || shown.canvas != canvas || shown.areas != areas
        }) || flat.rows().any(|(_, _, row)| {
            row.line
                .is_some_and(|(line, _)| line.loading.as_ref().is_some_and(|l| l.band.is_some()))
        }) || writer.has_bands();
        if full {
            writer.cleared();
        }
        let rendered = render(scene, &flat, &places, writer)?;
        out.write_all(&rendered.sends)?;
        if full {
            writer.clear_bytes(out)?;
        }
        match previous.as_ref().filter(|_| !full) {
            None => write_whole(&rendered, &places, out)?,
            Some(shown) => {
                // The terminal scrolls its whole screen: only where one
                // area is the whole window.
                let whole = areas == [Area::window(scene.window)];
                let shift = whole
                    .then(|| self.shift(scene, shown, &rendered.written, hint))
                    .flatten();
                let moved = shift.map(|by| moved(&shown.rows, by));
                if let Some(by) = shift {
                    writer.scroll(by, out)?;
                }
                let old = moved.as_deref().unwrap_or(&shown.rows);
                let dirty = dirty_areas(&flat, old, &rendered.written, &rendered.sent);
                // Row by row where no scaled text overlaps the rows: a
                // sixel is a row of its own, and the terminal then shows each
                // row anew as soon as it is written.
                let by_row = writer.sixel() && !flat.rows().any(|(_, _, row)| row.scaled);
                write_changed(&rendered, &places, &dirty, by_row, writer, out)?;
            }
        }
        self.shown = Some(Shown {
            window: scene.window,
            canvas,
            areas,
            rows: rendered.written,
        });
        self.shade(scene, writer, out, full)?;
        output::end_update(out)
    }

    /// The pager quits: the shade's image, which the encoder knows
    /// nothing of, is deleted from the terminal if it was sent.
    pub fn close(&mut self, out: &mut impl Write) -> io::Result<()> {
        self.shaded.clear();
        if std::mem::take(&mut self.shade_sent) {
            graphics::delete_shade(out)?;
        }
        Ok(())
    }

    /// Takes the shade away and places it anew where the scene's differs
    /// from the screen's. A clear, `cleared`, took the placements away
    /// already. Only where the terminal places images, as the bands are.
    fn shade(
        &mut self,
        scene: &Scene<'_>,
        writer: &Writer<'_>,
        out: &mut impl Write,
        cleared: bool,
    ) -> io::Result<()> {
        let wanted: Vec<Area> = (scene.surfaces.iter())
            .filter(|_| writer.shades())
            .flat_map(|surface| match surface {
                Surface::Shade { areas } => areas.to_vec(),
                Surface::Lines { .. } | Surface::Bar { .. } => Vec::new(),
            })
            .collect();
        if cleared {
            self.shaded.clear();
        }
        // Nothing wanted: the image goes if the terminal holds it, also
        // when a clear took the placements, which it leaves the image of.
        if wanted.is_empty() {
            self.shaded.clear();
            if std::mem::take(&mut self.shade_sent) {
                graphics::delete_shade(out)?;
            }
            return Ok(());
        }
        if wanted == self.shaded {
            return Ok(());
        }
        // The image goes with its placements, and is sent anew with the
        // new ones: it is a pixel.
        if std::mem::take(&mut self.shade_sent) {
            graphics::delete_shade(out)?;
        }
        graphics::send_shade(out, SHADE_ALPHA, writer.effort())?;
        self.shade_sent = true;
        for (number, area) in (1..).zip(&wanted) {
            output::move_to(area.row, area.column, out)?;
            graphics::place_shade(out, number, (area.columns, area.rows))?;
        }
        self.shaded = wanted;
        Ok(())
    }
}

/// How opaque the black over what a panel leaves visible is, out of 255.
/// kitty blends in linear light, so this darkens less than it reads: 185
/// leaves white at about half its brightness.
const SHADE_ALPHA: u8 = 185;

/// Renders every row of `flat`, each in its area, and every bar of `scene`
/// into bytes. `places` is the place of each row, as `places` gives it.
fn render(
    scene: &Scene<'_>,
    flat: &Flat<'_>,
    places: &[Place],
    writer: &mut Writer<'_>,
) -> io::Result<Rendered> {
    let total = places.len();
    let mut lines = Vec::with_capacity(total);
    let mut sent = Vec::with_capacity(total);
    let mut sends = Vec::new();
    // The rows bars are drawn over, as the window's rows from 0: their
    // placeholders are written whole, as in the short form the cells after a
    // bar would take their position from its text.
    let barred: Vec<usize> = (scene.surfaces.iter())
        .filter_map(|surface| match surface {
            Surface::Bar { row, .. } => row.checked_sub(1),
            Surface::Lines { .. } | Surface::Shade { .. } => None,
        })
        .collect();
    for ((_, _, row), place) in flat.rows().zip(places) {
        let Some((line, at)) = row.line else {
            lines.push(None);
            sent.push(false);
            continue;
        };
        writer.dim(row.dim);
        writer.whole_placeholders(barred.contains(&(place.row - 1)));
        writer.edge(Some(Edge {
            columns: place.columns,
            window: place.to_edge,
        }));
        let mut bytes = Vec::new();
        writer.row(line, at, &mut sends, &mut bytes)?;
        lines.push(Some(bytes));
        sent.push(writer.sent_image());
    }
    writer.dim(false);
    writer.whole_placeholders(false);
    writer.edge(None);
    let mut bars = Vec::new();
    for surface in &scene.surfaces {
        let Surface::Bar { row, column, spans } = surface else {
            continue;
        };
        // A bar outside the window is not written.
        if *row == 0 || *row > usize::from(scene.window.1) {
            continue;
        }
        // The row of the area the bar's first cell is in, or else of the
        // first area that has the row, tells what is under the bar; a row
        // that no area has has nothing under it.
        let covering = |area: &Area| (area.row..area.row + area.rows).contains(row);
        let under = (flat.areas.iter())
            .position(|flat| flat.area.holds(*row, *column))
            .or_else(|| flat.areas.iter().position(|flat| covering(&flat.area)))
            .map(|a| &flat.areas[a])
            .and_then(|area| {
                let at = row - area.area.row;
                area.rows.get(at).map(|under| (under, at))
            });
        let mut bytes = Vec::new();
        output::move_to(*row, *column, &mut bytes)?;
        // Writing on the top row of a scaled line drops the characters
        // it touches whole; below it, they stay until they are erased.
        if let Some((under, at)) = under
            && under.scaled
            && at != under.slot.start
        {
            writer.erase_chars(width(spans), &mut bytes)?;
        }
        writer.bar(spans, &mut bytes)?;
        bars.push(Bar {
            row: *row,
            columns: *column..column + width(spans).max(1),
            bytes,
        });
    }
    // A bar is among the bytes of every row of an area it lies over, so that
    // a bar that changes makes each of them differ; one beside every area is
    // among those of every area's row.
    let written = flat
        .rows()
        .zip(&lines)
        .zip(places)
        .map(|(((_, _, row), line), place)| {
            let mut bytes = line.clone().unwrap_or_default();
            for bar in bars.iter().filter(|bar| bar.is_in(place, places)) {
                bytes.extend_from_slice(&bar.bytes);
            }
            Written {
                slot: row.slot.clone(),
                bytes,
            }
        })
        .collect();
    Ok(Rendered {
        sends,
        lines,
        sent,
        bars,
        written,
    })
}

/// Writes the lines in row order, each at the column of its area, then the
/// bars in the order of the scene.
fn write_whole(rendered: &Rendered, places: &[Place], out: &mut impl Write) -> io::Result<()> {
    for (line, place) in rendered.lines.iter().zip(places) {
        if let Some(bytes) = line {
            place.go(out)?;
            out.write_all(bytes)?;
        }
    }
    for bar in &rendered.bars {
        out.write_all(&bar.bytes)?;
    }
    Ok(())
}

/// Erases the rows of `dirty`, in scene order, and writes their lines and bars:
/// all the erasing first, or with `by_row` each row erased and written
/// before the next. A bar belongs to its row of the window: it is written
/// after every line of that row, once any area's row there is written, since
/// the erase of an area takes the bar's cells in it. A bar on a row that no
/// area has is written every time.
fn write_changed(
    rendered: &Rendered,
    places: &[Place],
    dirty: &[bool],
    by_row: bool,
    writer: &Writer<'_>,
    out: &mut impl Write,
) -> io::Result<()> {
    let rows_of = |row: usize| (0..dirty.len()).filter(move |r| places[*r].row == row);
    let wanted = |bar: &Bar| {
        let mut rows = rows_of(bar.row).peekable();
        rows.peek().is_none() || rows.any(|r| dirty[r])
    };
    if by_row {
        let mut done = Vec::new();
        for r in (0..dirty.len()).filter(|r| dirty[*r]) {
            let row = places[r].row;
            if done.contains(&row) {
                continue;
            }
            done.push(row);
            for r in rows_of(row).filter(|r| dirty[*r]) {
                places[r].erase(writer, out)?;
                if let Some(bytes) = &rendered.lines[r] {
                    places[r].go(out)?;
                    out.write_all(bytes)?;
                }
            }
            for bar in rendered.bars.iter().filter(|bar| bar.row == row) {
                out.write_all(&bar.bytes)?;
            }
        }
        for bar in rendered
            .bars
            .iter()
            .filter(|bar| rows_of(bar.row).next().is_none())
        {
            out.write_all(&bar.bytes)?;
        }
        return Ok(());
    }
    for r in (0..dirty.len()).filter(|r| dirty[*r]) {
        places[r].erase(writer, out)?;
    }
    for r in (0..dirty.len()).filter(|r| dirty[*r]) {
        if let Some(bytes) = &rendered.lines[r] {
            places[r].go(out)?;
            out.write_all(bytes)?;
        }
    }
    for bar in rendered.bars.iter().filter(|bar| wanted(bar)) {
        out.write_all(&bar.bytes)?;
    }
    Ok(())
}

/// How many non-blank rows of `new` are where `old` shows them after the
/// screen scrolled by `by` rows, up when positive: the same bytes in a slot
/// of the same shape.
fn matching(old: &[Written], new: &[Written], by: isize) -> usize {
    (0..new.len())
        .filter(|&r| {
            let Some(row) = r.checked_add_signed(by).and_then(|from| old.get(from)) else {
                return false;
            };
            let slot = &new[r].slot;
            !new[r].bytes.is_empty()
                && row.bytes == new[r].bytes
                && row.slot.start.checked_add_signed(-by) == Some(slot.start)
                && row.slot.end.checked_add_signed(-by) == Some(slot.end)
        })
        .count()
}

/// The shift to scroll by: the one with the most rows of `new` matching
/// `old`, `hint` first and then the smaller shifts, when it beats not
/// scrolling and more than half of the rows that show something match.
fn best_shift(old: &[Written], new: &[Written], hint: Option<isize>) -> Option<isize> {
    let shown = new.iter().filter(|row| !row.bytes.is_empty()).count();
    let worth = |by| {
        let matches = matching(old, new, by);
        (matches * 2 > shown).then_some(matches)
    };
    let height = isize::try_from(new.len()).ok()?;
    let unmoved = matching(old, new, 0);
    // The shift the pager gave, when it matches every row of text it can
    // reach, is the steady case: one pass. Another shift could match more
    // rows only by reaching rows this one cannot, and a frame written
    // after either is the same screen.
    if let Some(by) = hint.filter(|by| by.abs() < height) {
        let reach = |r: usize| {
            r.checked_add_signed(by)
                .is_some_and(|from| from < new.len())
        };
        let reachable = (0..new.len())
            .filter(|&r| reach(r) && !new[r].bytes.is_empty())
            .count();
        let matches = matching(old, new, by);
        if matches > unmoved && matches == reachable && matches * 2 > shown {
            return Some(by);
        }
    }
    let mut best: Option<(isize, usize)> = None;
    let candidates = (hint.into_iter()).chain((1..height).flat_map(|n| [n, -n]));
    for by in candidates.filter(|by| by.abs() < height) {
        let Some(matches) = worth(by).filter(|m| *m > unmoved) else {
            continue;
        };
        if best.is_none_or(|(_, most)| matches > most) {
            best = Some((by, matches));
        }
    }
    best.map(|(by, _)| by)
}

/// `old` as the screen shows it after it scrolled by `by` rows, up when
/// positive: the rows moved, with their slots. A row that scrolled in, and
/// one whose slot the scroll cut at the top, are not known to show what a
/// row of the new frame does, blank or not: they get an empty slot, which
/// no row of a frame has, so that they are written.
fn moved(old: &[Written], by: isize) -> Vec<Written> {
    let unknown = |r: usize| Written {
        slot: r..r,
        bytes: Vec::new(),
    };
    (0..old.len())
        .map(|r| {
            let row = r.checked_add_signed(by).and_then(|from| old.get(from));
            let Some(row) = row else {
                return unknown(r);
            };
            let Some(start) = row.slot.start.checked_add_signed(-by) else {
                return unknown(r);
            };
            Written {
                slot: start..row.slot.end.saturating_add_signed(-by),
                bytes: row.bytes.clone(),
            }
        })
        .collect()
}

/// The rows to write, as [`dirty`] tells them for each area of `flat` on
/// its own: a row is dirty by what is in its area alone, and a slot is
/// never wider than its area.
fn dirty_areas(flat: &Flat<'_>, old: &[Written], new: &[Written], sent: &[bool]) -> Vec<bool> {
    let mut start = 0;
    let mut all = Vec::with_capacity(new.len());
    for area in &flat.areas {
        let rows = start..start + area.rows.len();
        all.extend(dirty(
            &old[rows.clone()],
            &new[rows.clone()],
            &sent[rows.clone()],
        ));
        start = rows.end;
    }
    all
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

#[cfg(test)]
mod tests;

#[cfg(test)]
mod area_tests;
