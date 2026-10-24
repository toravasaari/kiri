//! What the terminal holds of the images kiri sent it, and under which id.
//!
//! The encoder scales an image to the size a layout shows it at, sends it
//! through `kitty::graphics` and remembers the id it went under, so that a
//! later row of the same image only names the id. The escape sequences
//! themselves are `kitty::graphics`'s; the encoder knows placements and
//! ids.

use std::collections::{HashMap, HashSet};
use std::io::{self, Write};

use image::RgbaImage;
use image::imageops;

use super::image::{Placement, Size, cell_size};
use super::style::Line;
use crate::caps::Capabilities;
use crate::images::{Images, Source};
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
        let Some(Ok(source)) = images.get(&placement.key) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("no pixels for image {}", placement.key),
            ));
        };
        let pixels = fit(source, placement.size, cell);
        let cells = graphics::Cells {
            columns: placement.size.columns,
            rows: placement.size.rows,
        };
        let (width, height) = pixels.dimensions();
        let id = ImageId::new(pixels.as_raw(), width, height, cells);
        Ok(Self { id, pixels, cells })
    }

    fn send(&self, out: &mut impl Write) -> io::Result<()> {
        let (width, height) = self.pixels.dimensions();
        let pixels = graphics::Pixels {
            width,
            height,
            rgba: self.pixels.as_raw(),
        };
        graphics::transmit(out, self.id, &pixels, self.cells)
    }
}

/// `source` drawn at `size` and placed at the top left of the cells it
/// covers, the rest transparent. The terminal then shows it pixel for pixel
/// when the cells are `cell` pixels, and scales it as a whole when they are
/// not.
fn fit(source: &Source, size: Size, cell: (u16, u16)) -> RgbaImage {
    let mut canvas = RgbaImage::new(
        u32::from(size.columns) * u32::from(cell.0),
        u32::from(size.rows) * u32::from(cell.1),
    );
    imageops::replace(&mut canvas, &source.render(size.width, size.height), 0, 0);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::ImageKey;

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
