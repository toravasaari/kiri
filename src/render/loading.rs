//! The pixels of what stands in for an image while it loads: the image
//! the block showed before, dimmed, and the band below it, or below the
//! block's code or alt text, that tells that something is loading, a thin
//! line whose light part runs from left to right, again and again.

use std::f32::consts::PI;
use std::time::Duration;

use image::{Rgba, RgbaImage};

use super::output::Page;
use crate::caps::Capabilities;
use crate::color::Color;

/// How much of an old image shows through while its block's new one
/// loads.
pub const DIMMED: f32 = 0.3;

/// How long the light part of the band takes to run across it once.
pub const PERIOD: Duration = Duration::from_millis(2400);

/// How many pixels high the band is.
const BAND_HEIGHT: u32 = 3;

/// How much of the band's width its light part takes.
const LIGHT_SHARE: f32 = 0.5;

/// The colour placeholders are drawn in, the text colour, and whether
/// the page is dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ink {
    pub rgb: [u8; 3],
    pub dark: bool,
}

impl Ink {
    /// The ink for text on `page` on a terminal with `caps`: the page's
    /// text colour or else the terminal's, where it is known as RGB, and
    /// otherwise a light grey on a dark page and a dark one on a light
    /// page. A page whose background is not known counts as dark.
    pub fn new(page: Page, caps: &Capabilities) -> Self {
        let dark = caps.dark_background.unwrap_or(true);
        let known =
            (page.foreground.or(caps.foreground)).and_then(|color| color.rgb(&caps.palette));
        let rgb = match known {
            Some(Color::Rgb(r, g, b)) => [r, g, b],
            _ if dark => [230, 230, 230],
            _ => [40, 40, 40],
        };
        Self { rgb, dark }
    }
}

/// `image` with `DIMMED` of its opacity.
pub fn dimmed(mut image: RgbaImage) -> RgbaImage {
    for pixel in image.pixels_mut() {
        pixel.0[3] = (f32::from(pixel.0[3]) * DIMMED).round() as u8;
    }
    image
}

/// The band at `elapsed` since it started, `columns` cells of `cell`
/// pixels wide and one cell high: a line `BAND_HEIGHT` pixels high a
/// sixth of the cell down, in `ink` at a low opacity, with a light part
/// half its width that runs in from beyond its left end and out at its
/// right end once every [`PERIOD`].
pub fn band(columns: u16, cell: (u16, u16), ink: Ink, elapsed: Duration) -> RgbaImage {
    let width = u32::from(columns) * u32::from(cell.0);
    let height = u32::from(cell.1);
    let top = height / 6;
    let bottom = (top + BAND_HEIGHT).min(height);
    let (base, peak) = if ink.dark { (0.14, 0.75) } else { (0.12, 0.6) };
    let light = LIGHT_SHARE * width as f32;
    let share = (elapsed.as_secs_f32() % PERIOD.as_secs_f32()) / PERIOD.as_secs_f32();
    let start = share * (width as f32 + light) - light;
    let [r, g, b] = ink.rgb;
    let alpha: Vec<u8> = (0..width)
        .map(|x| {
            let u = (x as f32 - start) / light;
            let lit = match (0.0..=1.0).contains(&u) {
                true => (PI * u).sin().powi(2),
                false => 0.0,
            };
            ((base + (peak - base) * lit) * 255.0) as u8
        })
        .collect();
    RgbaImage::from_fn(width, height, |x, y| match (top..bottom).contains(&y) {
        true => Rgba([r, g, b, alpha[x as usize]]),
        false => Rgba([0, 0, 0, 0]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INK: Ink = Ink {
        rgb: [200, 200, 200],
        dark: true,
    };

    #[test]
    fn the_ink_is_the_text_colour_where_it_is_known() {
        let caps = Capabilities {
            dark_background: Some(false),
            ..Capabilities::kitty(80)
        };
        let page = Page {
            foreground: Some(Color::Rgb(1, 2, 3)),
            ..Page::default()
        };
        let ink = Ink::new(page, &caps);
        assert_eq!(
            ink,
            Ink {
                rgb: [1, 2, 3],
                dark: false
            }
        );
        // A palette colour the terminal did not tell.
        let page = Page {
            foreground: Some(Color::Palette(7)),
            ..Page::default()
        };
        assert_eq!(Ink::new(page, &caps).rgb, [40, 40, 40]);
        let unknown = Capabilities::kitty(80);
        assert_eq!(Ink::new(Page::default(), &unknown).rgb, [230, 230, 230]);
    }

    #[test]
    fn a_dimmed_image_keeps_its_colours_at_less_opacity() {
        let image = RgbaImage::from_pixel(2, 1, Rgba([10, 20, 30, 200]));
        let dimmed = dimmed(image);
        assert_eq!(dimmed.get_pixel(1, 0).0, [10, 20, 30, 60]);
    }

    /// The opacity of each pixel along the band's line.
    fn line(band: &RgbaImage) -> Vec<u8> {
        let y = band.height() / 6;
        (0..band.width())
            .map(|x| band.get_pixel(x, y).0[3])
            .collect()
    }

    #[test]
    fn the_band_is_three_pixels_high_a_sixth_of_the_cell_down() {
        let band = band(2, (10, 24), INK, Duration::ZERO);
        assert_eq!(band.dimensions(), (20, 24));
        let lit: Vec<u32> = (0..24).filter(|&y| band.get_pixel(0, y).0[3] > 0).collect();
        assert_eq!(lit, [4, 5, 6]);
        assert_eq!(band.get_pixel(0, 4).0[..3], [200, 200, 200]);
    }

    #[test]
    fn the_light_part_runs_across_the_band_and_starts_again() {
        let at = |millis| line(&band(10, (10, 20), INK, Duration::from_millis(millis)));
        // Wholly beyond the left end at the start.
        let start = at(0);
        assert!(start.iter().all(|&alpha| alpha == start[0]), "{start:?}");
        // Brightest at the middle halfway through: the light part, half
        // as wide as the band, has run once its own width and the band's.
        let half = at(1200);
        let brightest = (0..100).max_by_key(|&x| half[x]).unwrap();
        assert!((48..=52).contains(&brightest), "{brightest}");
        assert!(half[brightest] > 180 && half[0] < 40, "{half:?}");
        // A period on, as at the start.
        assert_eq!(at(2400), start);
    }
}
