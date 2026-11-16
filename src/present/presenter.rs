//! Writes a `Scene` to an `impl Write` through the `Writer` as one
//! synchronized update: the lines row by row, then the bars in the order of
//! the scene. It does no I/O of its own. What a scene holds is `scene.rs`'s,
//! and kitty's own sequences are in `kitty/`.

use std::io::{self, Write};
use std::ops::Range;

use crate::present::kitty::graphics;
use crate::present::output::{self, Canvas, Writer};
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
/// drawn on, and each row.
#[derive(Debug)]
struct Shown {
    window: (u16, u16),
    canvas: Canvas,
    rows: Vec<Written>,
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
/// image data or deleted a replaced image, and the bars as (row index,
/// bytes) in scene order.
struct Rendered {
    sends: Vec<u8>,
    lines: Vec<Option<Vec<u8>>>,
    sent: Vec<bool>,
    bars: Vec<(usize, Vec<u8>)>,
    written: Vec<Written>,
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
        let full = previous
            .as_ref()
            .is_none_or(|shown| shown.window != scene.window || shown.canvas != canvas)
            || flat.rows.iter().any(|row| {
                row.line.is_some_and(|(line, _)| {
                    line.loading.as_ref().is_some_and(|l| l.band.is_some())
                })
            })
            || writer.has_bands();
        if full {
            writer.cleared();
        }
        let rendered = render(scene, &flat, writer)?;
        out.write_all(&rendered.sends)?;
        if full {
            writer.clear_bytes(out)?;
        }
        match previous.as_ref().filter(|_| !full) {
            None => write_whole(&rendered, out)?,
            Some(shown) => {
                let shift = self.shift(scene, shown, &rendered.written, hint);
                let moved = shift.map(|by| moved(&shown.rows, by));
                if let Some(by) = shift {
                    writer.scroll(by, out)?;
                }
                let old = moved.as_deref().unwrap_or(&shown.rows);
                let dirty = dirty(old, &rendered.written, &rendered.sent);
                // Row by row where no scaled text overlaps the rows: a
                // sixel is a row of its own, and the terminal then shows each
                // row anew as soon as it is written.
                let by_row = writer.sixel() && !flat.rows.iter().any(|row| row.scaled);
                write_changed(&rendered, &dirty, by_row, writer, out)?;
            }
        }
        self.shown = Some(Shown {
            window: scene.window,
            canvas,
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
        graphics::send_shade(out, SHADE_ALPHA)?;
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

/// Renders every row of `flat` and every bar of `scene` into bytes.
fn render(scene: &Scene<'_>, flat: &Flat<'_>, writer: &mut Writer<'_>) -> io::Result<Rendered> {
    let mut lines = Vec::with_capacity(flat.rows.len());
    let mut sent = Vec::with_capacity(flat.rows.len());
    let mut sends = Vec::new();
    // The rows bars are drawn over, from 0: their placeholders are written
    // whole, as in the short form the cells after a bar would take their
    // position from its text.
    let barred: Vec<usize> = (scene.surfaces.iter())
        .filter_map(|surface| match surface {
            Surface::Bar { row, .. } => row.checked_sub(1),
            Surface::Lines { .. } | Surface::Shade { .. } => None,
        })
        .collect();
    for (index, row) in flat.rows.iter().enumerate() {
        let Some((line, at)) = row.line else {
            lines.push(None);
            sent.push(false);
            continue;
        };
        writer.dim(row.dim);
        writer.whole_placeholders(barred.contains(&index));
        let mut bytes = Vec::new();
        writer.row(line, at, &mut sends, &mut bytes)?;
        lines.push(Some(bytes));
        sent.push(writer.sent_image());
    }
    writer.dim(false);
    writer.whole_placeholders(false);
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
        .zip(&lines)
        .enumerate()
        .map(|(r, (row, line))| {
            let mut bytes = line.clone().unwrap_or_default();
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
        sends,
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

/// Erases the rows of `dirty`, top first, and writes their lines and bars:
/// all the erasing first, or with `by_row` each row erased and written
/// before the next.
fn write_changed(
    rendered: &Rendered,
    dirty: &[bool],
    by_row: bool,
    writer: &Writer<'_>,
    out: &mut impl Write,
) -> io::Result<()> {
    if by_row {
        for r in (0..dirty.len()).filter(|r| dirty[*r]) {
            output::move_to(r + 1, 1, out)?;
            writer.erase_line(out)?;
            if let Some(bytes) = &rendered.lines[r] {
                output::move_to(r + 1, 1, out)?;
                out.write_all(bytes)?;
            }
            for (_, bytes) in rendered.bars.iter().filter(|(at, _)| *at == r) {
                out.write_all(bytes)?;
            }
        }
        return Ok(());
    }
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
mod tests {
    use super::*;
    use crate::present::encoder::Encoder;
    use crate::present::output::Page;
    use crate::present::scene::Area;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::color::Color;
    use crate::typeset::images::Images;
    use crate::typeset::style::{Band, Line, Loading, Span, Style};

    fn text(text: &str) -> Line {
        Line::new(vec![Span::new(text, Style::default())])
    }

    fn banded() -> Line {
        Line {
            loading: Some(Loading {
                key: "a.png".into(),
                above: 1,
                band: Some(Band {
                    columns: 4,
                    indent: 0,
                    low: false,
                }),
            }),
            ..Line::blank()
        }
    }

    /// What presenting each of `frames` writes, one after the other, on a
    /// terminal with `caps`, in a window of 20 columns and as many rows as
    /// the most lines of a frame.
    fn presented(caps: &Capabilities, frames: &[&[Line]]) -> Vec<String> {
        presented_by(Presenter::default(), caps, frames)
    }

    /// As [`presented`], by `presenter`.
    fn presented_by(
        mut presenter: Presenter,
        caps: &Capabilities,
        frames: &[&[Line]],
    ) -> Vec<String> {
        let images = Images::new();
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, caps, Page::default(), &mut encoder);
        let rows = frames.iter().map(|lines| lines.len()).max().unwrap_or(0);
        frames
            .iter()
            .map(|lines| {
                let scene = Scene {
                    window: (20, rows as u16),
                    surfaces: vec![Surface::Lines { lines, dim: false }],
                };
                let mut out = Vec::new();
                presenter.present(&scene, &mut writer, &mut out).unwrap();
                String::from_utf8(out).unwrap()
            })
            .collect()
    }

    fn with_cells(caps: Capabilities) -> Capabilities {
        Capabilities {
            cell_pixels: Some((10, 20)),
            ..caps
        }
    }

    #[test]
    fn a_line_with_a_band_redraws_the_screen_in_each_frame() {
        let lines = [banded(), text("below")];
        let kitty = with_cells(Capabilities::kitty(20));
        let frames = presented(&kitty, &[&lines, &lines]);
        assert!(frames[1].contains("\x1b[2J"), "{:?}", frames[1]);
    }

    #[test]
    fn on_sixel_each_dirty_row_is_erased_and_written_before_the_next() {
        let (before, after) = ([text("a1"), text("b1")], [text("a2"), text("b2")]);
        let frames = presented(&Capabilities::sixel(20), &[&before, &after]);
        let expected =
            "\x1b[?2026h\x1b[1;1H\x1b[2K\x1b[1;1Ha2\x1b[2;1H\x1b[2K\x1b[2;1Hb2\x1b[?2026l";
        assert_eq!(frames[1], expected);
        // As before for kitty: every dirty row is erased first.
        let kitty = with_cells(Capabilities::kitty(20));
        let frames = presented(&kitty, &[&before, &after]);
        let expected =
            "\x1b[?2026h\x1b[1;1H\x1b[2K\x1b[2;1H\x1b[2K\x1b[1;1Ha2\x1b[2;1Hb2\x1b[?2026l";
        assert_eq!(frames[1], expected);
    }

    /// What presenting `frames` of image lines on kitty writes, one after
    /// the other: each frame has the lines given, and `images` of two image
    /// rows each are loaded.
    fn sent_frames(frames: &[Vec<Option<usize>>]) -> Vec<String> {
        sent_frames_with(false, frames)
    }

    fn sent_frames_scrolling(frames: &[Vec<Option<usize>>]) -> Vec<String> {
        sent_frames_with(true, frames)
    }

    fn sent_frames_with(scroll: bool, frames: &[Vec<Option<usize>>]) -> Vec<String> {
        use crate::typeset::caps::Graphics;
        use crate::typeset::images::size::{Placement, Shows, Size};
        use crate::typeset::images::{ImageKey, Source};
        let mut images = Images::new();
        let mut placements = Vec::new();
        for n in 0..3u8 {
            let size = Size {
                width: 40,
                height: 20,
                columns: 4,
                rows: 1,
            };
            let url = format!("{n}.png");
            let pixels = image::RgbaImage::from_pixel(40, 20, image::Rgba([n, 9, 9, 255]));
            images.insert(url.as_str(), Ok(Source::Raster(pixels)));
            let key = ImageKey::from(url.as_str());
            placements.push(std::rc::Rc::new(Placement::block(key, size, Shows::Image)));
        }
        let mut caps = with_cells(Capabilities::kitty(20));
        if let Graphics::Kitty(kitty) = &mut caps.graphics {
            kitty.terminal = Some("kitty(0.48.2)".into());
        }
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
        let mut presenter = Presenter::scrolling(scroll);
        let rows = frames.iter().map(|lines| lines.len()).max().unwrap_or(0);
        frames
            .iter()
            .map(|shown| {
                let lines: Vec<Line> = (shown.iter())
                    .map(|line| match line {
                        Some(n) => Line::image(std::rc::Rc::clone(&placements[*n]), 0),
                        None => text("text"),
                    })
                    .collect();
                let scene = Scene {
                    window: (20, rows as u16),
                    surfaces: vec![Surface::Lines {
                        lines: &lines,
                        dim: false,
                    }],
                };
                let mut out = Vec::new();
                presenter.present(&scene, &mut writer, &mut out).unwrap();
                String::from_utf8(out).unwrap()
            })
            .collect()
    }

    /// Where each of the `needles` is first in `out`.
    fn at(out: &str, needle: &str) -> usize {
        out.find(needle)
            .unwrap_or_else(|| panic!("{needle:?} is not in {out:?}"))
    }

    #[test]
    fn a_full_frame_sends_every_image_before_it_clears_the_screen() {
        let frames = sent_frames(&[vec![Some(0), None, Some(1), Some(2)]]);
        let out = &frames[0];
        let clear = at(out, "\x1b[2J");
        assert_eq!(out.matches("a=T").count(), 3, "{out:?}");
        let last = out
            .rmatches("a=T")
            .next()
            .map(|_| out.rfind("a=T").unwrap());
        assert!(last.unwrap() < clear, "{out:?}");
        // The cells come after the clear and hold no data.
        let after = &out[clear..];
        assert!(!after.contains("\x1b_G"), "{after:?}");
        assert!(after.contains(crate::present::kitty::graphics::PLACEHOLDER));
        assert!(out.starts_with("\x1b[?2026h"), "{out:?}");
    }

    #[test]
    fn a_frame_that_changes_rows_sends_every_image_before_the_first_erase() {
        let frames = sent_frames(&[
            vec![None, None, None, None],
            vec![Some(0), None, Some(1), Some(2)],
        ]);
        let out = &frames[1];
        assert!(!out.contains("\x1b[2J"), "{out:?}");
        assert_eq!(out.matches("a=T").count(), 3, "{out:?}");
        let erase = at(out, "\x1b[2K");
        assert!(out.rfind("a=T").unwrap() < erase, "{out:?}");
        assert!(!out[erase..].contains("\x1b_G"), "{out:?}");
    }

    #[test]
    fn the_images_of_a_frame_are_one_run_of_sends() {
        // Nothing but graphics between the first send and the last, so a
        // wrapper that shares a passthrough shares one for all.
        let frames = sent_frames(&[vec![Some(0), None, Some(1), Some(2)]]);
        let out = &frames[0];
        let (first, last) = (at(out, "\x1b_G"), out.rfind("\x1b\\").unwrap());
        let run = &out[first..last + 2];
        assert_eq!(run.matches("\x1b_G").count(), run.matches("\x1b\\").count());
        assert!(
            !run.replace("\x1b_G", "")
                .replace("\x1b\\", "")
                .contains('\x1b')
        );
        assert!(
            !run.contains(crate::present::kitty::graphics::PLACEHOLDER),
            "{run:?}"
        );
    }

    #[test]
    fn a_loading_band_is_sent_after_the_clear_among_the_cells() {
        let lines = [banded(), text("below")];
        let kitty = with_cells(Capabilities::kitty(20));
        let frames = presented(&kitty, &[&lines]);
        let out = &frames[0];
        assert!(at(out, "\x1b_G") > at(out, "\x1b[2J"), "{out:?}");
    }

    /// What presenting an image of two rows on kitty writes in frame after
    /// frame, with a bar over the middle of the second row in each frame
    /// that `bars` says, as a panel is drawn.
    fn image_frames(bars: &[bool]) -> Vec<String> {
        use crate::typeset::caps::Graphics;
        use crate::typeset::images::size::{Placement, Shows, Size};
        use crate::typeset::images::{ImageKey, Source};
        let size = Size {
            width: 40,
            height: 40,
            columns: 4,
            rows: 2,
        };
        let pixels = image::RgbaImage::from_pixel(40, 40, image::Rgba([9, 9, 9, 255]));
        let mut images = Images::new();
        images.insert("i.png", Ok(Source::Raster(pixels)));
        let placement = std::rc::Rc::new(Placement::block(
            ImageKey::from("i.png"),
            size,
            Shows::Image,
        ));
        let lines = [
            Line::image(std::rc::Rc::clone(&placement), 0),
            Line::image(placement, 1),
        ];
        let mut caps = with_cells(Capabilities::kitty(20));
        if let Graphics::Kitty(kitty) = &mut caps.graphics {
            kitty.terminal = Some("kitty(0.48.2)".into());
        }
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
        let mut presenter = Presenter::default();
        let spans = [Span::new("x", Style::default())];
        (bars.iter())
            .map(|&bar| {
                let mut surfaces = vec![Surface::Lines {
                    lines: &lines,
                    dim: false,
                }];
                if bar {
                    surfaces.push(Surface::Bar {
                        row: 2,
                        column: 2,
                        spans: &spans,
                    });
                }
                let scene = Scene {
                    window: (20, 2),
                    surfaces,
                };
                let mut out = Vec::new();
                presenter.present(&scene, &mut writer, &mut out).unwrap();
                String::from_utf8(out).unwrap()
            })
            .collect()
    }

    /// The cells of image row `row` in `out` that name their position.
    fn named(out: &str, row: usize) -> usize {
        use crate::present::kitty::graphics::{DIACRITICS, PLACEHOLDER};
        out.matches(&format!("{PLACEHOLDER}{}", DIACRITICS[row]))
            .count()
    }

    #[test]
    fn an_image_row_under_a_bar_has_its_placeholders_whole() {
        let frames = image_frames(&[true]);
        // The first row in short, the one under the bar whole.
        assert_eq!(named(&frames[0], 0), 1, "{frames:?}");
        assert_eq!(named(&frames[0], 1), 4, "{frames:?}");
    }

    #[test]
    fn a_row_a_bar_comes_over_and_leaves_is_written_again_each_time() {
        let frames = image_frames(&[false, true, false]);
        // Short, then whole under the bar, then short again; the first row
        // stays as it was and the bar's frame does not write it again.
        assert_eq!(named(&frames[0], 1), 1, "{frames:?}");
        assert_eq!(named(&frames[1], 1), 4, "{frames:?}");
        assert_eq!(named(&frames[1], 0), 0, "{frames:?}");
        assert_eq!(named(&frames[2], 1), 1, "{frames:?}");
    }

    #[test]
    fn a_row_that_is_the_same_is_left_alone_on_sixel() {
        let (before, after) = ([text("a"), text("b1")], [text("a"), text("b2")]);
        let frames = presented(&Capabilities::sixel(20), &[&before, &after]);
        assert!(!frames[1].contains('a'), "{:?}", frames[1]);
        assert!(frames[1].contains("b2"), "{:?}", frames[1]);
    }

    /// What presenting `frames` writes on a terminal with `caps`, in a
    /// window of 20 × 6, when each frame has these `shades` over three
    /// lines.
    fn shaded(caps: &Capabilities, window: (u16, u16), frames: &[&[Area]]) -> Vec<String> {
        let images = Images::new();
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, caps, Page::default(), &mut encoder);
        let mut presenter = Presenter::default();
        let lines = [text("one"), text("two"), text("three")];
        frames
            .iter()
            .map(|areas| {
                let mut surfaces = vec![Surface::Lines {
                    lines: &lines,
                    dim: false,
                }];
                if !areas.is_empty() {
                    surfaces.push(Surface::Shade { areas });
                }
                let scene = Scene { window, surfaces };
                let mut out = Vec::new();
                presenter.present(&scene, &mut writer, &mut out).unwrap();
                String::from_utf8(out).unwrap()
            })
            .collect()
    }

    const AREAS: [Area; 2] = [
        Area {
            column: 1,
            row: 1,
            columns: 20,
            rows: 1,
        },
        Area {
            column: 14,
            row: 2,
            columns: 7,
            rows: 3,
        },
    ];

    /// The id of the shade image and what places it as placement `number`.
    fn shade_id() -> u32 {
        crate::present::kitty::graphics::SHADE_ID
    }

    fn placed_at(row: usize, column: usize, number: u32, size: (usize, usize)) -> String {
        format!(
            "\x1b[{row};{column}H\x1b_Ga=p,q=2,i={},p={number},c={},r={},",
            shade_id(),
            size.0,
            size.1
        )
    }

    #[test]
    fn the_shade_is_sent_once_and_placed_over_each_area() {
        let kitty = with_cells(Capabilities::kitty(20));
        let frames = shaded(&kitty, (20, 6), &[&AREAS]);
        let out = &frames[0];
        let sent = format!("a=t,q=2,f=32,o=z,s=128,v=128,i={};", shade_id());
        assert_eq!(out.matches(&sent).count(), 1, "{out:?}");
        assert!(out.contains(&placed_at(1, 1, 1, (20, 1))), "{out:?}");
        assert!(out.contains(&placed_at(2, 14, 2, (7, 3))), "{out:?}");
        assert_eq!(out.matches("a=p,").count(), 2, "{out:?}");
        // The image and the placements come after the screen is cleared.
        assert!(at(out, "\x1b[2J") < at(out, &sent), "{out:?}");
    }

    #[test]
    fn the_same_shade_in_the_next_frame_is_not_sent_again() {
        let kitty = with_cells(Capabilities::kitty(20));
        let frames = shaded(&kitty, (20, 6), &[&AREAS, &AREAS]);
        assert!(!frames[1].contains("\x1b_G"), "{:?}", frames[1]);
    }

    #[test]
    fn a_shade_that_changes_is_taken_away_and_placed_again() {
        let kitty = with_cells(Capabilities::kitty(20));
        let frames = shaded(&kitty, (20, 6), &[&AREAS, &AREAS[..1]]);
        let out = &frames[1];
        let delete = format!("a=d,d=I,i={},q=2", shade_id());
        assert!(at(out, &delete) < at(out, "a=t,"), "{out:?}");
        assert_eq!(out.matches("a=p,").count(), 1, "{out:?}");
    }

    #[test]
    fn a_shade_that_goes_is_deleted_and_nothing_is_placed() {
        let kitty = with_cells(Capabilities::kitty(20));
        let frames = shaded(&kitty, (20, 6), &[&AREAS, &[], &[]]);
        let delete = format!("a=d,d=I,i={},q=2", shade_id());
        assert!(frames[1].contains(&delete), "{:?}", frames[1]);
        assert!(!frames[1].contains("a=p,") && !frames[1].contains("a=t,"));
        assert!(!frames[2].contains(&delete), "{:?}", frames[2]);
    }

    #[test]
    fn a_shade_that_goes_in_a_frame_that_clears_the_screen_is_deleted_too() {
        let kitty = with_cells(Capabilities::kitty(20));
        let images = Images::new();
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, &kitty, Page::default(), &mut encoder);
        let mut presenter = Presenter::default();
        let lines = [text("one")];
        let mut present = |window, areas: &[Area]| {
            let mut surfaces = vec![Surface::Lines {
                lines: &lines,
                dim: false,
            }];
            if !areas.is_empty() {
                surfaces.push(Surface::Shade { areas });
            }
            let mut out = Vec::new();
            let scene = Scene { window, surfaces };
            presenter.present(&scene, &mut writer, &mut out).unwrap();
            String::from_utf8(out).unwrap()
        };
        present((20, 6), &AREAS);
        let out = present((24, 6), &[]);
        let delete = format!("a=d,d=I,i={},q=2", shade_id());
        assert!(out.contains("\x1b[2J") && out.contains(&delete), "{out:?}");
    }

    #[test]
    fn closing_the_presenter_deletes_a_shade_that_was_sent_and_only_then() {
        let kitty = with_cells(Capabilities::kitty(20));
        let images = Images::new();
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, &kitty, Page::default(), &mut encoder);
        let mut presenter = Presenter::default();
        let delete = format!("a=d,d=I,i={},q=2", shade_id());
        let mut none = Vec::new();
        presenter.close(&mut none).unwrap();
        assert!(none.is_empty(), "nothing was sent");
        let lines = [text("one")];
        let scene = Scene {
            window: (20, 6),
            surfaces: vec![
                Surface::Lines {
                    lines: &lines,
                    dim: false,
                },
                Surface::Shade { areas: &AREAS },
            ],
        };
        presenter
            .present(&scene, &mut writer, &mut Vec::new())
            .unwrap();
        let mut out = Vec::new();
        presenter.close(&mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap().matches(&delete).count(), 1);
        let mut again = Vec::new();
        presenter.close(&mut again).unwrap();
        assert!(again.is_empty());
    }

    #[test]
    fn a_window_that_changes_has_its_shade_placed_again() {
        let kitty = with_cells(Capabilities::kitty(20));
        let images = Images::new();
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, &kitty, Page::default(), &mut encoder);
        let mut presenter = Presenter::default();
        let lines = [text("one")];
        let mut present = |window| {
            let surfaces = vec![
                Surface::Lines {
                    lines: &lines,
                    dim: false,
                },
                Surface::Shade { areas: &AREAS },
            ];
            let mut out = Vec::new();
            let scene = Scene { window, surfaces };
            presenter.present(&scene, &mut writer, &mut out).unwrap();
            String::from_utf8(out).unwrap()
        };
        present((20, 6));
        let out = present((24, 6));
        assert!(out.contains("\x1b[2J"), "{out:?}");
        assert_eq!(out.matches("a=p,").count(), 2, "{out:?}");
    }

    #[test]
    fn no_shade_under_tmux_with_sixel_or_without_graphics() {
        let tmux = Capabilities {
            tmux_passthrough: true,
            ..with_cells(Capabilities::kitty(20))
        };
        for caps in [tmux, Capabilities::sixel(20), Capabilities::ansi(20)] {
            let frames = shaded(&caps, (20, 6), &[&AREAS, &[]]);
            for out in &frames {
                assert!(!out.contains("a=p,") && !out.contains("a=t,"), "{out:?}");
                assert!(!out.contains("a=d,d=I"), "{out:?}");
            }
        }
    }

    /// What `presented` takes for a scrolling test: the document's lines, a
    /// status line when there is one (on the row below them), whether a
    /// shade is in the scene, and the shift the pager hints at.
    struct Shot<'a> {
        lines: &'a [Line],
        status: Option<&'a str>,
        shade: bool,
        hint: Option<isize>,
    }

    fn shot(lines: &[Line]) -> Shot<'_> {
        Shot {
            lines,
            status: None,
            shade: false,
            hint: None,
        }
    }

    /// What each of `shots` writes on a presenter that scrolls or not, on
    /// sixel, where a dirty row is erased and written on its own.
    fn scrolled(scroll: bool, shots: &[Shot<'_>]) -> Vec<String> {
        scrolled_on(Page::default(), scroll, shots)
    }

    /// As [`scrolled`], on a page of `page`'s colours.
    fn scrolled_on(page: Page, scroll: bool, shots: &[Shot<'_>]) -> Vec<String> {
        let caps = Capabilities::sixel(20);
        let images = Images::new();
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, &caps, page, &mut encoder);
        let mut presenter = Presenter::scrolling(scroll);
        let rows = shots.iter().map(|s| s.lines.len()).max().unwrap_or(0);
        let areas = [Area {
            column: 1,
            row: 1,
            columns: 2,
            rows: 2,
        }];
        shots
            .iter()
            .map(|shot| {
                let status = shot
                    .status
                    .map(|text| vec![Span::new(text, Style::default())]);
                let mut surfaces = vec![Surface::Lines {
                    lines: shot.lines,
                    dim: false,
                }];
                if let Some(spans) = &status {
                    surfaces.push(Surface::Bar {
                        row: rows + 1,
                        column: 1,
                        spans,
                    });
                }
                if shot.shade {
                    surfaces.push(Surface::Shade { areas: &areas });
                }
                let scene = Scene {
                    window: (20, (rows + usize::from(status.is_some())) as u16),
                    surfaces,
                };
                if let Some(rows) = shot.hint {
                    presenter.hint(rows);
                }
                let mut out = Vec::new();
                presenter.present(&scene, &mut writer, &mut out).unwrap();
                String::from_utf8(out).unwrap()
            })
            .collect()
    }

    fn numbered(range: std::ops::Range<usize>) -> Vec<Line> {
        range.map(|n| text(&format!("row {n}"))).collect()
    }

    /// The frame that writes row `r` (from 0) anew on sixel.
    fn row_written(r: usize, content: &str) -> String {
        let at = r + 1;
        format!("\x1b[{at};1H\x1b[2K\x1b[{at};1H{content}")
    }

    #[test]
    fn a_frame_shifted_by_the_pager_writes_the_scroll_and_the_new_rows() {
        // (rows moved up, the scroll sequence), the same frames the other way.
        for (first, then, scroll) in [(0, 1, "1S"), (0, 3, "3S"), (3, 0, "3T"), (1, 0, "1T")] {
            let (old, new) = (numbered(first..first + 8), numbered(then..then + 8));
            let frames = scrolled(true, &[shot(&old), shot(&new)]);
            let mut expected = format!("\x1b[?2026h\x1b[0m\x1b[{scroll}");
            // The rows that are new: the bottom ones going up, the top ones
            // going down.
            let n = first.abs_diff(then);
            let fresh: Vec<usize> = match then > first {
                true => (8 - n..8).collect(),
                false => (0..n).collect(),
            };
            for r in fresh {
                expected.push_str(&row_written(r, &format!("row {}", then + r)));
            }
            expected.push_str("\x1b[?2026l");
            assert_eq!(frames[1], expected, "from {first} to {then}");
        }
    }

    /// The background a terminal that fills erased cells with it has in
    /// effect after `bytes`, as the colour parameters of SGR sequences
    /// leave it: `None` for the default.
    fn background_after(bytes: &str) -> Option<String> {
        let mut background = None;
        let mut rest = bytes;
        while let Some(at) = rest.find("\x1b[") {
            rest = &rest[at + 2..];
            let end = rest.find(|c: char| !(c.is_ascii_digit() || c == ';'));
            let Some(end) = end.filter(|end| rest[*end..].starts_with('m')) else {
                continue;
            };
            let mut parameters = rest[..end].split(';').peekable();
            while let Some(code) = parameters.next() {
                match code {
                    "" | "0" | "49" => background = None,
                    "48" if parameters.peek() == Some(&"2") => {
                        let color: Vec<_> = parameters.by_ref().take(4).collect();
                        background = Some(color.join(";"));
                    }
                    _ => {}
                }
            }
        }
        background
    }

    #[test]
    fn the_rows_that_come_into_view_have_the_page_background() {
        // A terminal fills the rows a scroll brings in with the background
        // in effect when it scrolls, and a row that stays blank is not
        // written again to give it the colour.
        let page = Page {
            background: Some(Color::Rgb(1, 2, 3)),
            foreground: None,
        };
        let (old, new) = (numbered(0..6), numbered(1..7));
        let frames = scrolled_on(page, true, &[shot(&old), shot(&new)]);
        let out = &frames[1];
        let at = out
            .find("\x1b[1S")
            .unwrap_or_else(|| panic!("no scroll: {out:?}"));
        assert_eq!(
            background_after(&out[..at]).as_deref(),
            Some("2;1;2;3"),
            "{out:?}"
        );
        // The colours are put back after it, so that the rows written
        // next are not painted by accident.
        assert_eq!(background_after(&out[..at + 4]).as_deref(), Some("2;1;2;3"));
        assert_eq!(background_after(&out[at + 4..][..8]), None, "{out:?}");
    }

    #[test]
    fn without_a_page_background_the_scroll_starts_from_the_default_colours() {
        let (old, new) = (numbered(0..6), numbered(1..7));
        let frames = scrolled(true, &[shot(&old), shot(&new)]);
        assert!(
            frames[1].starts_with("\x1b[?2026h\x1b[0m\x1b[1S"),
            "{:?}",
            frames[1]
        );
    }

    #[test]
    fn a_scroll_leaves_the_status_line_to_be_written_again() {
        let (old, new) = (numbered(0..6), numbered(1..7));
        let frames = scrolled(
            true,
            &[
                Shot {
                    status: Some("at 0"),
                    ..shot(&old)
                },
                Shot {
                    status: Some("at 1"),
                    ..shot(&new)
                },
            ],
        );
        let out = &frames[1];
        assert!(out.starts_with("\x1b[?2026h\x1b[0m\x1b[1S"), "{out:?}");
        assert!(out.contains(&row_written(5, "row 6")), "{out:?}");
        assert!(out.contains("\x1b[7;1H"), "the status line: {out:?}");
        assert!(out.contains("at 1"), "{out:?}");
        // Neither a row that moved nor the status line's old place is
        // written but where the new frame differs.
        assert!(!out.contains("row 1"), "{out:?}");
    }

    #[test]
    fn a_panel_over_the_document_is_written_after_a_scroll() {
        // The same bar on the same rows in both frames, over lines that
        // moved: its rows do not match what moved there.
        let (old, new) = (numbered(0..8), numbered(1..9));
        let caps = Capabilities::sixel(20);
        let images = Images::new();
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
        let mut presenter = Presenter::scrolling(true);
        let panel = [Span::new("panel", Style::default())];
        let outs: Vec<String> = [&old, &new]
            .iter()
            .map(|lines| {
                let scene = Scene {
                    window: (20, 8),
                    surfaces: vec![
                        Surface::Lines { lines, dim: false },
                        Surface::Bar {
                            row: 3,
                            column: 5,
                            spans: &panel,
                        },
                    ],
                };
                let mut out = Vec::new();
                presenter.present(&scene, &mut writer, &mut out).unwrap();
                String::from_utf8(out).unwrap()
            })
            .collect();
        assert!(
            outs[1].starts_with("\x1b[?2026h\x1b[0m\x1b[1S"),
            "{:?}",
            outs[1]
        );
        assert!(outs[1].contains("\x1b[3;5H"), "{:?}", outs[1]);
        assert!(outs[1].contains("row 3\x1b[3;5Hpanel"), "{:?}", outs[1]);
    }

    #[test]
    fn a_frame_drawn_whole_does_not_scroll() {
        let kitty = with_cells(Capabilities::kitty(20));
        let images = Images::new();
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, &kitty, Page::default(), &mut encoder);
        let mut presenter = Presenter::scrolling(true);
        let (old, new) = (numbered(0..8), numbered(1..9));
        let with_band: Vec<Line> = [banded()].into_iter().chain(numbered(2..9)).collect();
        // A band, then the window's size changing: each frame is shifted
        // from the one before and is drawn whole.
        let frames = [(&old, 8), (&with_band, 8), (&new, 8), (&numbered(2..9), 7)];
        for (lines, rows) in frames {
            let scene = Scene {
                window: (20, rows),
                surfaces: vec![Surface::Lines { lines, dim: false }],
            };
            let mut out = Vec::new();
            presenter.present(&scene, &mut writer, &mut out).unwrap();
            let out = String::from_utf8(out).unwrap();
            assert!(!out.contains("\x1b[1S"), "{out:?}");
        }
    }

    #[test]
    fn a_frame_with_a_shade_does_not_scroll() {
        let (old, new) = (numbered(0..8), numbered(1..9));
        let shaded = Shot {
            shade: true,
            ..shot(&new)
        };
        let frames = scrolled(true, &[shot(&old), shaded]);
        assert!(!frames[1].contains("\x1b[1S"), "{:?}", frames[1]);
        // Nor the frame after it, while the shade is still placed.
        let after = numbered(2..10);
        let frames = scrolled(
            true,
            &[
                shot(&old),
                Shot {
                    shade: true,
                    ..shot(&new)
                },
                Shot {
                    shade: true,
                    ..shot(&after)
                },
            ],
        );
        assert!(!frames[2].contains("\x1b[1S"), "{:?}", frames[2]);
    }

    #[test]
    fn with_the_switch_off_a_shifted_frame_is_written_as_before() {
        let (old, new) = (numbered(0..8), numbered(1..9));
        let off = scrolled(false, &[shot(&old), shot(&new)]);
        let default = presented(&Capabilities::sixel(20), &[&old, &new]);
        assert_eq!(off, default);
        assert!(!off[1].contains("\x1b[1S"), "{:?}", off[1]);
        assert!(off[1].contains("row 8"), "{:?}", off[1]);
    }

    #[test]
    fn frames_that_are_not_shifted_are_not_scrolled() {
        // Nothing moved: the changed row is written alone.
        let old = numbered(0..8);
        let mut new = old.clone();
        new[4] = text("changed");
        let frames = scrolled(true, &[shot(&old), shot(&new)]);
        assert_eq!(
            frames[1],
            format!("\x1b[?2026h{}\x1b[?2026l", row_written(4, "changed"))
        );
        // Another document: too few rows match to pay.
        let other = numbered(20..28);
        let frames = scrolled(true, &[shot(&old), shot(&other)]);
        assert!(!frames[1].contains("\x1b[1S"), "{:?}", frames[1]);
        assert!(!frames[1].contains('T'), "{:?}", frames[1]);
        // Half match, not more than half.
        let half: Vec<Line> = numbered(2..6).into_iter().chain(numbered(40..44)).collect();
        let frames = scrolled(true, &[shot(&old), shot(&half)]);
        assert!(!frames[1].contains("\x1b[2S"), "{:?}", frames[1]);
    }

    #[test]
    fn a_row_that_scrolls_in_is_erased_in_the_page_background() {
        // 3cd38434 sets the background for the scroll, which a terminal
        // that fills erased cells with it follows; this does not depend
        // on that: the last row is blank in both frames and is still
        // written, so what it holds is what kiri says.
        let page = Page {
            background: Some(Color::Rgb(1, 2, 3)),
            foreground: None,
        };
        let (old, new) = (numbered(0..8), numbered(1..8));
        let frames = scrolled_on(page, true, &[shot(&old), shot(&new)]);
        let erase = "\x1b[8;1H\x1b[0;48;2;1;2;3m\x1b[2K\x1b[0m";
        assert!(frames[1].contains("\x1b[1S"), "{:?}", frames[1]);
        assert!(frames[1].contains(erase), "{:?}", frames[1]);
    }

    #[test]
    fn a_slot_that_the_scroll_cuts_at_the_top_is_written_again() {
        use crate::typeset::size::TextSize;
        let heading = Line::new(vec![Span::new(
            "H",
            Style {
                size: TextSize::new(2).unwrap(),
                ..Style::default()
            },
        )]);
        let mut old = vec![heading];
        old.extend(["a", "b", "c", "d", "e", "f"].map(text));
        let mut new = vec![Line::blank()];
        new.extend(["a", "b", "c", "d", "e", "f", "g"].map(text));
        let kitty = with_cells(Capabilities::kitty(20));
        let frames = presented_by(Presenter::scrolling(true), &kitty, &[&old, &new]);
        let out = &frames[1];
        assert!(out.contains("\x1b[0m\x1b[1S"), "{out:?}");
        // The lower row of the heading that is left on the first row is
        // not a blank row: it is erased.
        assert!(out.contains("\x1b[1;1H\x1b[2K"), "{out:?}");
    }

    #[test]
    fn blank_rows_do_not_count_as_matches() {
        // Shifted up by 3, two of the five rows of text match, which is
        // not more than half; the three blank rows at the bottom match the
        // blank rows of the old frame too. Counting those would make 5 of
        // 5, and scroll.
        let blank = Line::blank;
        let old = [
            text("x"),
            text("y"),
            blank(),
            blank(),
            blank(),
            text("u"),
            text("v"),
            text("w"),
        ];
        let new = [
            text("p"),
            text("q"),
            text("r"),
            text("x"),
            text("y"),
            blank(),
            blank(),
            blank(),
        ];
        let frames = scrolled(true, &[shot(&old), shot(&new)]);
        assert!(
            !frames[1].contains('S') && !frames[1].contains('T'),
            "{:?}",
            frames[1]
        );
    }

    #[test]
    fn a_shift_that_matches_no_more_rows_than_none_is_not_a_scroll() {
        // Rows that repeat: shifted by one, seven rows match, and so they do
        // where they are. The changed row is written alone.
        let old: Vec<Line> = (0..8).map(|_| text("a")).collect();
        let mut new = old.clone();
        new[7] = text("b");
        let frames = scrolled(true, &[shot(&old), shot(&new)]);
        assert_eq!(
            frames[1],
            format!("\x1b[?2026h{}\x1b[?2026l", row_written(7, "b"))
        );
    }

    #[test]
    fn a_hint_that_matches_every_row_it_reaches_is_taken_without_looking_further() {
        // Rows that repeat, a b a b...; the frame is the old one moved up by
        // 3 rows and then three rows more of the pattern: shift 1 matches
        // 7 rows, the pager's 3 matches the 5 it can reach, all of them.
        let row = |n: usize| text(if n.is_multiple_of(2) { "a" } else { "b" });
        let old: Vec<Line> = (0..8).map(row).collect();
        let new: Vec<Line> = (3..8)
            .map(row)
            .chain([text("a"), text("b"), text("x")])
            .collect();
        let hinted = scrolled(
            true,
            &[
                shot(&old),
                Shot {
                    hint: Some(3),
                    ..shot(&new)
                },
            ],
        );
        assert!(
            hinted[1].starts_with("\x1b[?2026h\x1b[0m\x1b[3S"),
            "{:?}",
            hinted[1]
        );
    }

    #[test]
    fn the_shift_the_pager_hints_at_is_tried_first() {
        // Rows that repeat: a shift of 1 and one of 3 match as many.
        let row = |n: usize| text(if n.is_multiple_of(2) { "a" } else { "b" });
        let old: Vec<Line> = (0..8).map(row).collect();
        let new: Vec<Line> = (3..8)
            .map(row)
            .chain([text("x"), text("y"), text("z")])
            .collect();
        let none = scrolled(true, &[shot(&old), shot(&new)]);
        assert!(
            none[1].starts_with("\x1b[?2026h\x1b[0m\x1b[1S"),
            "{:?}",
            none[1]
        );
        let hinted = scrolled(
            true,
            &[
                shot(&old),
                Shot {
                    hint: Some(3),
                    ..shot(&new)
                },
            ],
        );
        assert!(
            hinted[1].starts_with("\x1b[?2026h\x1b[0m\x1b[3S"),
            "{:?}",
            hinted[1]
        );
    }

    #[test]
    fn a_scroll_on_kitty_moves_image_rows_and_sends_only_the_new_ones() {
        let frames = sent_frames_scrolling(&[
            vec![Some(0), None, Some(1), None],
            vec![None, Some(1), None, Some(2)],
        ]);
        assert!(
            frames[1].starts_with("\x1b[?2026h") && frames[1].contains("\x1b[1S"),
            "{:?}",
            frames[1]
        );
        // The image that scrolled in is sent; the image 1 that moved is not.
        assert_eq!(frames[1].matches("a=T").count(), 1, "{:?}", frames[1]);
    }
}
