//! The sixel side of the encoder: the slices of the rows of a placement.
//!
//! A sixel has no ids and nothing the terminal holds for kiri, so what the
//! terminal holds is not tracked: the ledger keeps its table of placements
//! to ids, which tells whether a placement's pixels are still the ones
//! scaled, and the bytes of each row are cached here, by image, row,
//! columns and the colour under them. The sixel encoder itself is
//! `crate::present::sixel`.

use std::io;
use std::ops::Range;
use std::rc::Rc;

use image::RgbaImage;

use super::{Encoder, Scaled, frame_of};
use crate::present::kitty::graphics::ImageId;
use crate::present::ledger::Held;
use crate::present::sixel::{Lru, Options, Prepared, Rgb, Tile, tiles};
use crate::typeset::caps::{Capabilities, Graphics};
use crate::typeset::images::Images;
use crate::typeset::images::size::{Placement, cell_size};

/// What the encoded slices may take, in bytes.
const SLICES: usize = 64 << 20;
/// What the pictures the slices are cut from may take: about four bytes a
/// pixel.
const PREPARED: usize = 128 << 20;

/// A row of an image as the terminal is sent it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SliceKey {
    image: ImageId,
    row: u16,
    columns: (u16, u16),
    background: Option<Rgb>,
}

/// The slices encoded, and what they are made from.
#[derive(Debug)]
pub(super) struct Cache {
    slices: Lru<SliceKey, Vec<Tile>>,
    /// Pictures ready for slicing, by image and the colour under them.
    prepared: Lru<(ImageId, Option<Rgb>), Prepared>,
    /// The pixels of the image scaled last, which the rows of its placement
    /// that are asked for next are cut from.
    scaled: Option<(ImageId, Rc<RgbaImage>)>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            slices: Lru::new(SLICES),
            prepared: Lru::new(PREPARED),
            scaled: None,
        }
    }
}

impl Encoder {
    /// The sixels of row `row` of the image `placement` shows, from `images`
    /// on a terminal with `caps`, over `columns` of its cells: one, or
    /// tiles side by side when the row is wider than the terminal takes.
    /// The picture is flattened onto `background`, the colour the cells show
    /// under it; without one the pixels that are mostly transparent are
    /// left undrawn.
    pub fn sixel_row(
        &mut self,
        placement: &Placement,
        row: u16,
        columns: &Range<u16>,
        background: Option<Rgb>,
        images: &Images,
        caps: &Capabilities,
    ) -> io::Result<Rc<Vec<Tile>>> {
        let Graphics::Sixel(limits) = &caps.graphics else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the terminal does not draw sixel",
            ));
        };
        let image = self.sixel_id(placement, images, caps)?;
        let key = SliceKey {
            image,
            row,
            columns: (columns.start, columns.end),
            background,
        };
        if let Some(tiles) = self.sixel.slices.get(&key) {
            return Ok(tiles);
        }
        let options = Options {
            registers: usize::from(limits.registers).max(2),
            background,
        };
        let prepared = self.prepared(image, placement, images, caps, options)?;
        let (width, height) = cell_size(caps);
        let (width, height) = (u32::from(width), u32::from(height));
        let left = u32::from(columns.start) * width;
        let across = u32::from(columns.end.saturating_sub(columns.start)) * width;
        let top = u32::from(row) * height;
        let tiles: Vec<Tile> = tiles(across, width, u32::from(limits.max_width))
            .into_iter()
            .map(|(x, w)| Tile {
                offset: (x / width) as u16,
                bytes: prepared.slice(left + x, top, w, height),
            })
            .collect();
        let weight = tiles.iter().map(|tile| tile.bytes.len() + 16).sum();
        Ok(self.sixel.slices.put(key, tiles, weight))
    }

    /// The id of the pixels `placement` shows: the ledger's, while they are
    /// the ones scaled, and otherwise they are scaled again. Nothing is sent
    /// anywhere, so a terminal that lost what it drew changes nothing here.
    fn sixel_id(
        &mut self,
        placement: &Placement,
        images: &Images,
        caps: &Capabilities,
    ) -> io::Result<ImageId> {
        let cell = cell_size(caps);
        let states = self.state_hash(placement, images);
        let current = (self.ledger.still(placement))
            .filter(|held| held.cell == cell && held.checked && held.states == states);
        if let Some(held) = current {
            return Ok(held.id);
        }
        let scaled = self.scaled(placement, images, caps)?;
        let held = Held {
            id: scaled.id,
            cell,
            checked: true,
            sent: true,
            states,
        };
        self.ledger.put_still(placement.clone(), held);
        self.sixel.scaled = Some((scaled.id, Rc::new(scaled.pixels)));
        Ok(scaled.id)
    }

    fn scaled(
        &self,
        placement: &Placement,
        images: &Images,
        caps: &Capabilities,
    ) -> io::Result<Scaled> {
        Scaled::new(
            placement,
            images,
            (&self.pages, &self.overlays),
            cell_size(caps),
            caps.image_run,
            frame_of(placement, caps),
            &self.marks,
        )
    }

    /// The picture of `image` ready for slicing with `options`.
    fn prepared(
        &mut self,
        image: ImageId,
        placement: &Placement,
        images: &Images,
        caps: &Capabilities,
        options: Options,
    ) -> io::Result<Rc<Prepared>> {
        let key = (image, options.background);
        if let Some(prepared) = self.sixel.prepared.get(&key) {
            return Ok(prepared);
        }
        let pixels = match &self.sixel.scaled {
            Some((id, pixels)) if *id == image => Rc::clone(pixels),
            _ => Rc::new(self.scaled(placement, images, caps)?.pixels),
        };
        let prepared = Prepared::new(&pixels, options);
        let weight = prepared.weight();
        Ok(self.sixel.prepared.put(key, prepared, weight))
    }

    /// Lets go of the ids the lines no longer place, which the terminal
    /// holds nothing of.
    pub fn discard_unused(&mut self) {
        self.end_frame();
        self.ledger.take_unused();
    }

    /// Lets go of everything, as when the pager quits, with nothing to
    /// delete from the terminal.
    pub fn discard_all(&mut self) {
        self.end_frame();
        self.ledger.drain();
        self.sixel = Cache::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::images::size::{Shows, Size};
    use crate::typeset::images::{ImageKey, Source};

    /// A red image of 2 × 3 cells of 10 × 20 pixels, named `name`.
    fn red(name: &str) -> (Images, Placement) {
        let size = Size {
            width: 20,
            height: 60,
            columns: 2,
            rows: 3,
        };
        let mut images = Images::new();
        let pixels = RgbaImage::from_pixel(20, 60, image::Rgba([200, 0, 0, 255]));
        images.insert(name, Ok(crate::typeset::images::Source::Raster(pixels)));
        (
            images,
            Placement::block(ImageKey::from(name), size, Shows::Image),
        )
    }

    #[test]
    fn a_row_asked_for_again_is_the_one_encoded_and_its_picture_is_prepared_once() {
        let (images, placement) = red("r.png");
        let caps = Capabilities::sixel(40);
        let mut encoder = Encoder::new();
        let mut ask = |row: u16, background: Option<Rgb>| {
            (encoder.sixel_row(&placement, row, &(0..2), background, &images, &caps)).unwrap()
        };
        let first = ask(0, None);
        assert_eq!(first.len(), 1);
        assert!(!first[0].bytes.is_empty());
        assert!(Rc::ptr_eq(&first, &ask(0, None)));
        // Another row, and the same row on another colour, are others.
        let second = ask(1, None);
        assert!(!Rc::ptr_eq(&first, &second));
        let tinted = ask(0, Some([1, 2, 3]));
        assert!(!Rc::ptr_eq(&first, &tinted));
        assert_eq!(encoder.sixel.slices.len(), 3);
        // Two pictures: with the colour under them and without.
        assert_eq!(encoder.sixel.prepared.len(), 2);
    }

    #[test]
    fn pixels_that_change_are_sliced_again_and_those_that_do_not_are_not() {
        let (images, placement) = red("r.png");
        let caps = Capabilities::sixel(40);
        let mut encoder = Encoder::new();
        let ask = |encoder: &mut Encoder, images: &Images| {
            (encoder.sixel_row(&placement, 0, &(0..2), None, images, &caps)).unwrap()
        };
        let before = ask(&mut encoder, &images);
        // A reload that leaves the pixels as they were scales them again
        // and finds the same id, so the slice is the cached one.
        encoder.changed();
        assert!(Rc::ptr_eq(&before, &ask(&mut encoder, &images)));
        let mut blue = Images::new();
        let pixels = RgbaImage::from_pixel(20, 60, image::Rgba([0, 0, 200, 255]));
        blue.insert("r.png", Ok(Source::Raster(pixels)));
        encoder.changed();
        let after = ask(&mut encoder, &blue);
        assert_ne!(before[0].bytes, after[0].bytes);
    }

    #[test]
    fn a_terminal_that_draws_no_sixel_is_refused() {
        let (images, placement) = red("r.png");
        let caps = Capabilities::kitty(40);
        let mut encoder = Encoder::new();
        let error = encoder.sixel_row(&placement, 0, &(0..2), None, &images, &caps);
        assert_eq!(error.unwrap_err().kind(), io::ErrorKind::Unsupported);
    }
}
