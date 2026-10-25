//! What the terminal holds of the images kiri sent it, and under which id.
//!
//! The encoder scales an image to the size a layout shows it at, sends it
//! through `kitty::graphics` and remembers the id it went under, so that a
//! later row of the same image only names the id. The escape sequences
//! themselves are `kitty::graphics`'s; the encoder knows placements and
//! ids.

use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::time::Duration;

use image::RgbaImage;
use image::imageops;

use super::image::{Placement, Shows, Size, cell_size};
use super::loading::{self, Ink};
use super::style::Line;
use crate::caps::Capabilities;
use crate::images::{Images, Placeholder, Source};
use crate::kitty::graphics::{self, ImageId};

/// The images the terminal holds, by where and how large they are shown.
/// One encoder can serve one layout after another, as the pager's does,
/// so that an image the terminal holds is not sent again.
#[derive(Debug, Default)]
pub struct Encoder {
    held: HashMap<Placement, Held>,
    /// Images the terminal holds that no placement has any more, to be
    /// deleted before the next frame.
    unused: Vec<ImageId>,
    /// The bands that tell an image is loading, by their columns and the
    /// pixels of their cells: one image for all the bands of a width,
    /// which [`Encoder::animate`] sends anew for each step.
    bands: HashMap<(u16, (u16, u16)), ImageId>,
    /// The bands drawn since the screen was last cleared, which are the
    /// ones on it.
    shown: HashSet<(u16, (u16, u16))>,
}

/// An image the terminal holds.
#[derive(Debug, Clone, Copy)]
struct Held {
    id: ImageId,
    /// The pixels of the cells it was scaled to fit.
    cell: (u16, u16),
    /// Whether `id` is known to be that of the pixels as they are now:
    /// not after they may have changed, until the image is scaled again.
    checked: bool,
}

impl Encoder {
    /// An encoder that has sent nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The id of the image `placement` shows, from `images`, on a terminal
    /// with `caps`: sent to `out` first unless the terminal holds it. An
    /// image sent for `placement` before, which the pixels no longer
    /// give, is deleted.
    pub fn id(
        &mut self,
        placement: &Placement,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        let cell = cell_size(caps);
        let old = self.held.get(placement).filter(|held| held.cell == cell);
        if let Some(held) = old.filter(|held| held.checked) {
            return Ok(held.id);
        }
        let scaled = Scaled::new(placement, images, cell)?;
        let held = Held {
            id: scaled.id,
            cell,
            checked: true,
        };
        if let Some(old) = self.held.insert(placement.clone(), held) {
            if old.id == scaled.id {
                return Ok(scaled.id);
            }
            // At once, not queued as `keep` does: the old id is known to
            // be stale only now that the image has been scaled again.
            if !self.holds(old.id) {
                graphics::delete(out, old.id)?;
            }
        }
        scaled.send(out)?;
        Ok(scaled.id)
    }

    /// The id of the band `columns` cells of `cell` pixels wide, in
    /// `ink`, which is on the screen until it is next cleared: sent to
    /// `out` first, as it is `elapsed` after its start, unless the
    /// terminal holds it since the screen was last cleared. A band moves
    /// on as each frame sends it anew: its pixels cannot be changed where
    /// it is placed, as the terminal blends new pixels over the old ones.
    pub fn band(
        &mut self,
        columns: u16,
        cell: (u16, u16),
        ink: Ink,
        elapsed: Duration,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        self.shown.insert((columns, cell));
        if let Some(&id) = self.bands.get(&(columns, cell)) {
            return Ok(id);
        }
        let band = loading::band(columns, cell, ink, elapsed);
        let cells = graphics::Cells { columns, rows: 1 };
        let id = ImageId::new(BAND, band.width(), band.height(), cells);
        graphics::transmit_only(out, id, &pixels(&band))?;
        self.bands.insert((columns, cell), id);
        Ok(id)
    }

    /// Tells the encoder that the screen was cleared: no band is on it
    /// until one is drawn again, and the terminal no longer holds the
    /// bands, which nothing places any more.
    pub fn cleared(&mut self) {
        self.shown.clear();
        self.bands.clear();
    }

    /// Whether a band is on the screen.
    pub fn has_bands(&self) -> bool {
        !self.shown.is_empty()
    }

    /// Keeps only the images `lines` place on a terminal with `caps`, as
    /// the lines of a new layout do: the others are deleted by the next
    /// [`Encoder::delete_unused`]. An image is placed by a line whether
    /// or not the line is in view.
    pub fn keep(&mut self, lines: &[Line], caps: &Capabilities) {
        let cell = cell_size(caps);
        let placed: HashSet<&Placement> = (lines.iter())
            .filter_map(|line| line.image.as_ref())
            .map(|image| image.placement.as_ref())
            .collect();
        let bands: HashSet<u16> = (lines.iter())
            .filter_map(|line| line.band)
            .map(|band| band.columns)
            .collect();
        self.bands.retain(|&(columns, band_cell), &mut id| {
            let kept = band_cell == cell && bands.contains(&columns);
            if !kept {
                self.unused.push(id);
            }
            kept
        });
        let mut gone = Vec::new();
        self.held.retain(|placement, held| {
            let kept = held.cell == cell && placed.contains(placement);
            if !kept {
                gone.push(held.id);
            }
            kept
        });
        for id in gone {
            if !self.holds(id) && !self.unused.contains(&id) {
                self.unused.push(id);
            }
        }
    }

    /// Deletes from the terminal the images [`Encoder::keep`] let go.
    pub fn delete_unused(&mut self, out: &mut impl Write) -> io::Result<()> {
        for id in self.unused.drain(..) {
            graphics::delete(out, id)?;
        }
        Ok(())
    }

    /// Tells the encoder that the pixels in the images it is given may
    /// have changed, as after a reload: each image is scaled again when it
    /// is next shown, and sent again only when that gives another id.
    pub fn changed(&mut self) {
        for held in self.held.values_mut() {
            held.checked = false;
        }
    }

    /// Deletes from the terminal every image it holds, as when the pager
    /// quits.
    pub fn delete_all(&mut self, out: &mut impl Write) -> io::Result<()> {
        self.unused.extend(self.bands.drain().map(|(_, id)| id));
        for (_, held) in self.held.drain() {
            if !self.unused.contains(&held.id) {
                self.unused.push(held.id);
            }
        }
        self.delete_unused(out)
    }

    /// Whether some placement still has image `id`.
    fn holds(&self, id: ImageId) -> bool {
        self.held.values().any(|held| held.id == id)
    }
}

/// An image scaled to the cells it covers, and its id.
struct Scaled {
    id: ImageId,
    pixels: RgbaImage,
    cells: graphics::Cells,
}

impl Scaled {
    /// The image `placement` shows, from `images`, scaled to its size in
    /// cells of `cell` pixels.
    fn new(placement: &Placement, images: &Images, cell: (u16, u16)) -> io::Result<Self> {
        let key = &placement.key;
        let size = placement.size;
        let pixels = match (placement.shows, images.get(key), images.placeholder(key)) {
            (Shows::Image, Some(Ok(source)), _) => fit(source, size, cell),
            (Shows::Placeholder, _, Some(Placeholder::Dimmed(old))) => {
                loading::dimmed(fit(old, size, cell))
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("no pixels for image {key}"),
                ));
            }
        };
        let cells = graphics::Cells {
            columns: placement.size.columns,
            rows: placement.size.rows,
        };
        let (width, height) = pixels.dimensions();
        let id = ImageId::new(pixels.as_raw(), width, height, cells);
        Ok(Self { id, pixels, cells })
    }

    fn send(&self, out: &mut impl Write) -> io::Result<()> {
        send(out, self.id, &self.pixels, self.cells)
    }
}

/// What a band's id is made from besides its size: it is the same image
/// whatever step of its light part it shows.
const BAND: &[u8] = b"kiri: the band of an image that is loading";

/// Sends `pixels` as image `id` over `cells`.
fn send(
    out: &mut impl Write,
    id: ImageId,
    pixels: &RgbaImage,
    cells: graphics::Cells,
) -> io::Result<()> {
    graphics::transmit(out, id, &self::pixels(pixels), cells)
}

/// `image` as the pixels `graphics` sends.
fn pixels(image: &RgbaImage) -> graphics::Pixels<'_> {
    let (width, height) = image.dimensions();
    graphics::Pixels {
        width,
        height,
        rgba: image.as_raw(),
    }
}

/// A transparent canvas of the cells of `size`, each of `cell` pixels.
fn canvas(size: Size, cell: (u16, u16)) -> RgbaImage {
    RgbaImage::new(
        u32::from(size.columns) * u32::from(cell.0),
        u32::from(size.rows) * u32::from(cell.1),
    )
}

/// `source` drawn at `size` and placed at the top left of the cells it
/// covers, the rest transparent. The terminal then shows it pixel for pixel
/// when the cells are `cell` pixels, and scales it as a whole when they are
/// not.
fn fit(source: &Source, size: Size, cell: (u16, u16)) -> RgbaImage {
    let mut canvas = canvas(size, cell);
    imageops::replace(&mut canvas, &source.render(size.width, size.height), 0, 0);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::ImageKey;

    const INK: Ink = Ink {
        rgb: [200, 200, 200],
        dark: true,
    };

    /// `key` placed over `columns` × `rows` cells of 10 × 20 pixels,
    /// showing `shows`.
    fn placed(key: &str, columns: u16, rows: u16, shows: Shows) -> Placement {
        Placement {
            key: ImageKey::from(key),
            size: Size {
                width: u32::from(columns) * 10,
                height: u32::from(rows) * 20,
                columns,
                rows,
            },
            shows,
        }
    }

    fn caps() -> Capabilities {
        Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(80)
        }
    }

    #[test]
    fn a_placeholder_is_the_old_image_dimmed() {
        let mut images = Images::new();
        let red = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 200]));
        images.wait("a.png".into(), Placeholder::Dimmed(Source::Raster(red)));
        let dimmed = Scaled::new(
            &placed("a.png", 2, 1, Shows::Placeholder),
            &images,
            (10, 20),
        );
        assert_eq!(dimmed.unwrap().pixels.get_pixel(5, 5).0, [255, 0, 0, 60]);
        // Nothing to show as the image itself.
        assert!(Scaled::new(&placed("a.png", 2, 1, Shows::Image), &images, (10, 20)).is_err());
    }

    #[test]
    fn a_band_is_sent_once_a_frame_as_it_is_at_the_frame_s_time() {
        let mut encoder = Encoder::new();
        let mut out = Vec::new();
        let cell = (10, 20);
        let start = Duration::ZERO;
        let first = encoder.band(4, cell, INK, start, &mut out).unwrap();
        let second = encoder.band(4, cell, INK, start, &mut out).unwrap();
        assert_eq!(first, second);
        let sent = String::from_utf8(std::mem::take(&mut out)).unwrap();
        // Sent without a placement, which the writer makes.
        assert_eq!(sent.matches("\x1b_Ga=t,").count(), 1, "{sent:?}");
        assert!(encoder.has_bands());
        // Clearing the screen frees it, as nothing places it then: sent
        // again for the next frame, as it is then, under the same id.
        encoder.cleared();
        assert!(!encoder.has_bands());
        let later = Duration::from_millis(600);
        assert_eq!(encoder.band(4, cell, INK, later, &mut out).unwrap(), first);
        let again = String::from_utf8(std::mem::take(&mut out)).unwrap();
        assert_eq!(again.matches("\x1b_Ga=t,").count(), 1, "{again:?}");
        assert_ne!(again, sent);
        // Gone with the last line that has it.
        encoder.keep(&[], &caps());
        encoder.delete_unused(&mut out).unwrap();
        let mut deleted = Vec::new();
        graphics::delete(&mut deleted, first).unwrap();
        assert_eq!(out, deleted);
    }

    #[test]
    fn an_image_held_for_other_cells_is_sent_again() {
        let mut images = Images::new();
        let red = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]));
        images.insert("square.png", Ok(Source::Raster(red)));
        let placement = Placement {
            key: ImageKey::from("square.png"),
            size: Size {
                width: 20,
                height: 20,
                columns: 2,
                rows: 1,
            },
            shows: Shows::Image,
        };
        let caps = Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(80)
        };
        let taller = Capabilities {
            cell_pixels: Some((10, 25)),
            ..caps.clone()
        };
        let mut encoder = Encoder::new();
        let mut out = Vec::new();
        let old = encoder.id(&placement, &images, &caps, &mut out).unwrap();
        out.clear();
        // The same placement in taller cells, without a layout between:
        // scaled for them, under another id.
        let new = encoder.id(&placement, &images, &taller, &mut out).unwrap();
        assert_ne!(old, new);
        let mut expected = Vec::new();
        graphics::delete(&mut expected, old).unwrap();
        let out = String::from_utf8(out).unwrap();
        let expected = String::from_utf8(expected).unwrap();
        assert!(out.starts_with(&expected), "{out:?}");
        assert!(out.contains("\x1b_Ga=T,"), "{out:?}");
    }
}
