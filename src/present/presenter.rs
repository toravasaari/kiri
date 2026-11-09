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
                let dirty = dirty(&shown.rows, &rendered.written, &rendered.sent);
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
        output::end_update(out)
    }
}

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
            Surface::Lines { .. } => None,
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
    use crate::typeset::caps::Capabilities;
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
        let images = Images::new();
        let mut encoder = Encoder::new();
        let mut writer = Writer::new(&images, caps, Page::default(), &mut encoder);
        let mut presenter = Presenter::default();
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
        let mut presenter = Presenter::default();
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
}
