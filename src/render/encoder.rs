//! What the terminal holds of the images kiri sent it, and under which id.
//!
//! The encoder scales an image to the size a layout shows it at, sends it
//! through `kitty::graphics` and remembers the id it went under, so that a
//! later row of the same image only names the id. The escape sequences
//! themselves are `kitty::graphics`'s; the encoder knows placements and
//! ids.

use std::collections::HashMap;
use std::io::{self, Write};

use image::RgbaImage;
use image::imageops;

use super::image::{Placement, Size, cell_size};
use crate::caps::Capabilities;
use crate::images::{Images, Source};
use crate::kitty::graphics::{self, ImageId};

/// The images sent to the terminal, by where and how large they are shown.
#[derive(Debug, Default)]
pub struct Encoder {
    sent: HashMap<Placement, ImageId>,
}

impl Encoder {
    /// An encoder that has sent nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The id of the image `placement` shows, from `images`, on a terminal
    /// with `caps`: sent to `out` first if it has not been.
    pub fn id(
        &mut self,
        placement: &Placement,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        if let Some(&id) = self.sent.get(placement) {
            return Ok(id);
        }
        let id = send(placement, images, caps, out)?;
        self.sent.insert(placement.clone(), id);
        Ok(id)
    }
}

/// Scales the image `placement` shows to its size and sends it.
fn send(
    placement: &Placement,
    images: &Images,
    caps: &Capabilities,
    out: &mut impl Write,
) -> io::Result<ImageId> {
    let Some(Ok(source)) = images.get(&placement.key) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("no pixels for image {}", placement.key),
        ));
    };
    let pixels = fit(source, placement.size, cell_size(caps));
    let cells = graphics::Cells {
        columns: placement.size.columns,
        rows: placement.size.rows,
    };
    let (width, height) = pixels.dimensions();
    let id = ImageId::new(pixels.as_raw(), width, height, cells);
    let pixels = graphics::Pixels {
        width,
        height,
        rgba: pixels.as_raw(),
    };
    graphics::transmit(out, id, &pixels, cells)?;
    Ok(id)
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
