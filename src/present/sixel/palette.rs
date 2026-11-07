//! Colours: flattening, counting and reducing to a palette.

use std::collections::HashMap;

use color_quant::NeuQuant;
use image::{Rgba, RgbaImage, imageops};

/// A colour as red, green and blue.
pub type Rgb = [u8; 3];

/// The picture as indices into a palette; `None` is a pixel left undrawn.
pub(super) struct Indexed {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Option<u16>>,
}

/// Mixes a pixel with what lies under it. Without a background a pixel is
/// either opaque or undrawn.
pub(super) fn flatten(pixel: Rgba<u8>, background: Option<Rgb>) -> Option<Rgb> {
    let [r, g, b, a] = pixel.0;
    match background {
        Some(bg) => {
            let mix = |c: u8, u: u8| {
                ((u16::from(c) * u16::from(a) + u16::from(u) * (255 - u16::from(a)) + 127) / 255)
                    as u8
            };
            Some([mix(r, bg[0]), mix(g, bg[1]), mix(b, bg[2])])
        }
        None if a >= 128 => Some([r, g, b]),
        None => None,
    }
}

/// The palette of a whole picture: its exact colours when they fit the
/// registers, otherwise a trained one that slices are dithered to.
pub(super) enum Palette {
    Exact,
    Reduced(Box<NeuQuant>),
}

impl Palette {
    /// Looks at the flattened picture.
    pub fn of(flat: &[Option<Rgb>], registers: usize) -> Self {
        if distinct(flat.iter().flatten().copied(), registers) {
            return Self::Exact;
        }
        let mut samples = Vec::with_capacity(flat.len() * 4);
        for c in flat.iter().flatten() {
            samples.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        // A sampling factor of 10 looks at every tenth pixel: fast, and
        // good enough for a palette that error diffusion then refines.
        Self::Reduced(Box::new(NeuQuant::new(10, registers.min(256), &samples)))
    }

    /// Turns a region of the flattened picture into indices into the
    /// returned colour list. The region's own colours are exact when they
    /// fit the registers; otherwise the picture's palette is used with
    /// dithering.
    pub fn index(
        &self,
        region: &[Option<Rgb>],
        width: u32,
        height: u32,
        registers: usize,
    ) -> (Vec<Rgb>, Indexed) {
        let exact =
            matches!(self, Self::Exact) || distinct(region.iter().flatten().copied(), registers);
        let mut colours: Vec<Rgb> = Vec::new();
        let mut seen: HashMap<Rgb, u16> = HashMap::new();
        let mut pixels = vec![None; region.len()];
        if exact {
            for (slot, c) in pixels.iter_mut().zip(region) {
                if let Some(c) = c {
                    let n = *seen.entry(*c).or_insert_with(|| {
                        colours.push(*c);
                        (colours.len() - 1) as u16
                    });
                    *slot = Some(n);
                }
            }
        } else if let Self::Reduced(nq) = self {
            let mut img = RgbaImage::new(width, height);
            for (px, c) in img.pixels_mut().zip(region) {
                *px = c.map_or(Rgba([0, 0, 0, 0]), |c| Rgba([c[0], c[1], c[2], 255]));
            }
            if width >= 2 && height >= 2 {
                imageops::dither(&mut img, nq.as_ref());
            } else {
                // The diffusion needs a neighbour on each axis.
                for px in img.pixels_mut() {
                    imageops::ColorMap::map_color(nq.as_ref(), px);
                }
            }
            for (slot, (px, c)) in pixels.iter_mut().zip(img.pixels().zip(region)) {
                if c.is_some() {
                    let key = [px[0], px[1], px[2]];
                    let n = *seen.entry(key).or_insert_with(|| {
                        colours.push(key);
                        (colours.len() - 1) as u16
                    });
                    *slot = Some(n);
                }
            }
        }
        (
            colours,
            Indexed {
                width,
                height,
                pixels,
            },
        )
    }
}

/// Whether the colours are no more than `limit`.
fn distinct(colours: impl Iterator<Item = Rgb>, limit: usize) -> bool {
    let mut set = std::collections::HashSet::new();
    for c in colours {
        set.insert(c);
        if set.len() > limit {
            return false;
        }
    }
    true
}
