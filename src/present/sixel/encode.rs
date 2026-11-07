//! The sixel encoder: slices of a flattened picture as `DCS ... ST`.

use image::RgbaImage;

use super::Options;
use super::palette::{Indexed, Palette, Rgb, flatten};

/// A picture made ready for slicing: flattened onto its background, with the
/// palette its reduced slices share. Build it once per picture; every slice
/// is then cheap.
pub struct Prepared {
    width: u32,
    height: u32,
    flat: Vec<Option<Rgb>>,
    palette: Palette,
    registers: usize,
}

impl Prepared {
    pub fn new(img: &RgbaImage, options: Options) -> Self {
        let flat: Vec<Option<Rgb>> = img
            .pixels()
            .map(|p| flatten(*p, options.background))
            .collect();
        let registers = options.registers.max(2);
        Self {
            width: img.width(),
            height: img.height(),
            palette: Palette::of(&flat, registers),
            flat,
            registers,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The sixel of the region `x, y, w, h` of the picture, as one
    /// `DCS ... ST` sequence. The region is cut to the picture; an empty one
    /// gives nothing.
    ///
    /// Every slice defines the registers it uses, because a palette belongs
    /// to its image. `P2` is 1: what is undrawn shows the cell's own
    /// background. The raster attributes give the exact size, and no `-`
    /// comes before the terminator, so the cursor stays on the last row of
    /// the slice.
    pub fn slice(&self, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
        let x1 = x.saturating_add(w).min(self.width);
        let y1 = y.saturating_add(h).min(self.height);
        if x >= x1 || y >= y1 {
            return Vec::new();
        }
        let (w, h) = (x1 - x, y1 - y);
        let mut region = Vec::with_capacity((w * h) as usize);
        for row in y..y1 {
            let start = (row * self.width + x) as usize;
            region.extend_from_slice(&self.flat[start..start + w as usize]);
        }
        let (colours, indexed) = self.palette.index(&region, w, h, self.registers);
        encode(&colours, &indexed)
    }
}

/// Cuts a width in pixels into tiles at cell boundaries, none wider than
/// `max_px` (at least one cell). Returns the pixel offset and width of each.
pub fn tiles(width_px: u32, cell_w: u32, max_px: u32) -> Vec<(u32, u32)> {
    let cell_w = cell_w.max(1);
    let per_tile = (max_px / cell_w).max(1) * cell_w;
    let mut out = Vec::new();
    let mut x = 0;
    while x < width_px {
        let w = per_tile.min(width_px - x);
        out.push((x, w));
        x += w;
    }
    out
}

fn percent(c: u8) -> u32 {
    (u32::from(c) * 100 + 127) / 255
}

/// Run-length codes a run of one character: `!n c` from four on.
fn run(out: &mut Vec<u8>, ch: u8, n: u32) {
    match n {
        0 => {}
        1..=3 => out.extend(std::iter::repeat_n(ch, n as usize)),
        _ => {
            out.push(b'!');
            out.extend_from_slice(n.to_string().as_bytes());
            out.push(ch);
        }
    }
}

/// For each band of six pixel rows, the colours in it with their bits for
/// each column.
fn bands(img: &Indexed, colours: usize) -> Vec<Vec<(u16, Vec<u8>)>> {
    let (w, h) = (img.width as usize, img.height as usize);
    (0..h)
        .step_by(6)
        .map(|band| {
            let mut by_colour: Vec<(u16, Vec<u8>)> = Vec::new();
            let mut slot: Vec<Option<usize>> = vec![None; colours];
            for (k, y) in (band..(band + 6).min(h)).enumerate() {
                for x in 0..w {
                    if let Some(c) = img.pixels[y * w + x] {
                        let s = *slot[c as usize].get_or_insert_with(|| {
                            by_colour.push((c, vec![0; w]));
                            by_colour.len() - 1
                        });
                        by_colour[s].1[x] |= 1 << k;
                    }
                }
            }
            by_colour
        })
        .collect()
}

fn encode(colours: &[Rgb], img: &Indexed) -> Vec<u8> {
    let bands = bands(img, colours.len());
    let mut out = Vec::new();
    out.extend_from_slice(b"\x1bP0;1;0q");
    out.extend_from_slice(format!("\"1;1;{};{}", img.width, img.height).as_bytes());
    // Only the registers the slice uses, numbered from 0 in order of first
    // use in the bands.
    let mut register: Vec<Option<usize>> = vec![None; colours.len()];
    let mut used = 0;
    for (c, _) in bands.iter().flatten() {
        if register[*c as usize].is_none() {
            register[*c as usize] = Some(used);
            let [r, g, b] = colours[*c as usize];
            let def = format!("#{used};2;{};{};{}", percent(r), percent(g), percent(b));
            out.extend_from_slice(def.as_bytes());
            used += 1;
        }
    }
    for (n, band) in bands.iter().enumerate() {
        if n > 0 {
            out.push(b'-');
        }
        for (i, (c, bits)) in band.iter().enumerate() {
            if i > 0 {
                out.push(b'$');
            }
            let reg = register[*c as usize].unwrap_or(0);
            out.extend_from_slice(format!("#{reg}").as_bytes());
            // From the first to the last column the colour occurs in.
            let first = bits.iter().position(|b| *b != 0).unwrap_or(0);
            let last = bits.iter().rposition(|b| *b != 0).unwrap_or(0);
            run(&mut out, b'?', first as u32);
            let mut cur = bits[first];
            let mut n = 0;
            for b in &bits[first..=last] {
                if *b == cur {
                    n += 1;
                } else {
                    run(&mut out, 63 + cur, n);
                    cur = *b;
                    n = 1;
                }
            }
            run(&mut out, 63 + cur, n);
        }
    }
    out.extend_from_slice(b"\x1b\\");
    out
}
