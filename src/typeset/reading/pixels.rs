//! The pixels of a picture on a reading-mode page, premultiplied once when
//! the picture is made, so that drawing it is a blit and not a copy.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use image::RgbaImage;
use resvg::tiny_skia::{IntSize, Pixmap, PixmapRef};

use super::animated;
use super::paint::premultiplied;

/// A picture's pixels as tiny-skia holds them, premultiplied, with the id
/// of the animation it is the hole of, if it is one: premultiplying loses
/// the colours a hole carries its id in.
pub struct Pixels {
    /// None for a picture with no area.
    map: Option<Pixmap>,
    size: (u32, u32),
    hole: Option<u64>,
}

impl Pixels {
    /// `image` premultiplied. The image itself is let go of.
    pub fn new(image: RgbaImage) -> Self {
        let size = image.dimensions();
        let hole = animated::id_of(&image);
        let mut bytes = image.into_raw();
        for pixel in bytes.as_chunks_mut::<4>().0 {
            *pixel = premultiplied(*pixel);
        }
        let map = IntSize::from_wh(size.0, size.1).and_then(|size| Pixmap::from_vec(bytes, size));
        Self { map, size, hole }
    }

    /// `image` premultiplied, with an id from what it is.
    pub fn with_id(image: RgbaImage) -> (Self, u64) {
        let mut hasher = DefaultHasher::new();
        image.dimensions().hash(&mut hasher);
        image.as_raw().hash(&mut hasher);
        let id = hasher.finish();
        (Self::new(image), id)
    }

    pub fn width(&self) -> u32 {
        self.size.0
    }

    pub fn height(&self) -> u32 {
        self.size.1
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.size
    }

    /// The id the picture carries if it is the hole of an animation.
    pub fn hole(&self) -> Option<u64> {
        self.hole
    }

    /// The bytes the pixels take.
    pub fn bytes(&self) -> usize {
        self.map.as_ref().map_or(0, |map| map.data().len())
    }

    /// The premultiplied pixels, unless the picture has no area.
    pub(super) fn map(&self) -> Option<PixmapRef<'_>> {
        self.map.as_ref().map(Pixmap::as_ref)
    }
}

impl std::fmt::Debug for Pixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pixels({} x {})", self.size.0, self.size.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::reading::glyph_cache::GlyphCache;
    use crate::typeset::reading::paint::Canvas;
    use image::Rgba;
    use resvg::tiny_skia::{PixmapPaint, Transform};

    #[test]
    fn pixels_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Pixels>();
    }

    #[test]
    fn an_id_is_the_hash_of_the_straight_pixels_and_not_the_premultiplied_ones() {
        // The id names the image to the terminal: a change in it sends every
        // picture again.
        let image = RgbaImage::from_fn(5, 3, |x, y| Rgba([(x * 40) as u8, (y * 90) as u8, 7, 100]));
        let mut hasher = DefaultHasher::new();
        image.dimensions().hash(&mut hasher);
        image.as_raw().hash(&mut hasher);
        let expected = hasher.finish();
        assert_eq!(Pixels::with_id(image).1, expected);
    }

    /// A picture of every kind of pixel: clear, translucent, opaque.
    fn mixed(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            let alpha = [0, 1, 77, 128, 200, 255][(x + y) as usize % 6];
            Rgba([(x * 29 % 256) as u8, (y * 53 % 256) as u8, 201, alpha])
        })
    }

    /// What `Canvas::picture` did before pictures were premultiplied once:
    /// premultiply the rows on the canvas into a pixmap of their own and
    /// draw that. Kept as the reference.
    fn drawn_the_old_way(canvas: &mut Canvas, image: &RgbaImage, x: f32, y: f32) {
        let (left, top) = (x.round() as i64, y.round() as i64);
        let (width, height) = (i64::from(image.width()), i64::from(image.height()));
        let from = (-top).clamp(0, height);
        let to = (i64::from(canvas.pixmap.height()) - top).clamp(0, height);
        if from >= to || left >= i64::from(canvas.pixmap.width()) || left + width <= 0 {
            return;
        }
        let data: Vec<u8> = (image.rows().skip(from as usize).take((to - from) as usize))
            .flat_map(|row| row.flat_map(|pixel| premultiplied(pixel.0)))
            .collect();
        let size = IntSize::from_wh(width as u32, (to - from) as u32).unwrap();
        let pixmap = Pixmap::from_vec(data, size).unwrap();
        canvas.pixmap.draw_pixmap(
            left as i32,
            (top + from) as i32,
            pixmap.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    #[test]
    fn a_picture_premultiplied_once_draws_the_pixels_the_old_path_drew() {
        let image = mixed(13, 9);
        let opaque = RgbaImage::from_fn(13, 9, |x, y| Rgba([x as u8 * 17, y as u8 * 9, 40, 255]));
        // Whole and fractional positions, inside the canvas, past each edge
        // and wholly off it.
        let places = [
            (7.0, 3.0),
            (2.4, 1.6),
            (2.5, 1.5),
            (-3.3, -2.7),
            (14.0, 11.5),
            (-20.0, 0.0),
            (0.0, 30.0),
        ];
        for (name, image) in [("translucent", &image), ("opaque", &opaque)] {
            for (x, y) in places {
                for underlay in [None, Some([30, 90, 160])] {
                    let (mut old_glyphs, mut new_glyphs) =
                        (GlyphCache::default(), GlyphCache::default());
                    let mut old = Canvas::new(20, 16, &mut old_glyphs);
                    let mut new = Canvas::new(20, 16, &mut new_glyphs);
                    if let Some(colour) = underlay {
                        old.rect(0.0, 0.0, 20.0, 16.0, colour);
                        new.rect(0.0, 0.0, 20.0, 16.0, colour);
                    }
                    drawn_the_old_way(&mut old, image, x, y);
                    new.picture(&Pixels::new(image.clone()), x, y);
                    assert_eq!(
                        old.into_image().into_raw(),
                        new.into_image().into_raw(),
                        "{name} picture at ({x}, {y}) over {underlay:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_hole_keeps_its_id_through_premultiplying() {
        let mut image = RgbaImage::new(4, 4);
        for (at, bytes) in [[1, 2, 3], [4, 5, 6], [7, 8, 0]].iter().enumerate() {
            image.put_pixel(at as u32, 0, Rgba([bytes[0], bytes[1], bytes[2], 1]));
        }
        let expected = animated::id_of(&image);
        assert!(expected.is_some());
        assert_eq!(Pixels::new(image).hole(), expected);
        assert_eq!(Pixels::new(mixed(4, 4)).hole(), None);
    }

    #[test]
    fn a_picture_without_area_is_nothing_to_draw() {
        let pixels = Pixels::new(RgbaImage::new(0, 5));
        assert_eq!(pixels.dimensions(), (0, 5));
        assert!(pixels.map().is_none());
        let mut glyphs = GlyphCache::default();
        let mut canvas = Canvas::new(4, 4, &mut glyphs);
        canvas.picture(&pixels, 0.0, 0.0);
        assert!(canvas.into_image().pixels().all(|pixel| pixel.0[3] == 0));
    }
}
