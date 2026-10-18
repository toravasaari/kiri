//! Image scaling for kiri.
//!
//! The API is a single function on plain byte buffers, so the generic code
//! of fast_image_resize is compiled in this crate, which debug builds
//! optimise, and not in kiri.

use fast_image_resize::images::{TypedImage, TypedImageRef};
use fast_image_resize::pixels::{U8x4, U16x4};
use fast_image_resize::{self as fir, PixelTrait, ResizeAlg, ResizeOptions, Resizer};

/// Scales `pixels`, `width` × `height` pixels of 8-bit RGBA in rows from
/// the top, with straight (not premultiplied) alpha, to `to_width` ×
/// `to_height` with a Catmull-Rom filter. Returns the scaled pixels in the
/// same layout.
///
/// Transparent pixels still have a colour, often white or black, which a
/// plain resize would blend into the edges around them as a halo. Scaling
/// in premultiplied alpha, where a pixel counts as much as it covers, leaves
/// that colour out. Images with no transparent pixels skip that step.
///
/// # Panics
///
/// If `pixels` does not hold `width` × `height` pixels, or a size is zero.
pub fn scale_rgba8(
    pixels: &[u8],
    width: u32,
    height: u32,
    to_width: u32,
    to_height: u32,
) -> Vec<u8> {
    assert_eq!(
        pixels.len(),
        width as usize * height as usize * 4,
        "{width} × {height} RGBA pixels"
    );
    assert!(
        width > 0 && height > 0 && to_width > 0 && to_height > 0,
        "a size of zero"
    );
    let (rgba, _) = pixels.as_chunks::<4>();
    if rgba.iter().all(|pixel| pixel[3] == u8::MAX) {
        // Straight from one buffer to the other.
        let source = TypedImageRef::<U8x4>::from_buffer(width, height, pixels)
            .expect("width × height pixels");
        let mut bytes = vec![0; to_width as usize * to_height as usize * 4];
        let mut scaled =
            TypedImage::<U8x4>::from_buffer(to_width, to_height, &mut bytes).expect("enough bytes");
        resize(&source, &mut scaled);
        return bytes;
    }
    // Sixteen bits per channel keep the colour of faint pixels.
    let premultiplied: Vec<U16x4> = rgba
        .iter()
        .map(|&[red, green, blue, alpha]| {
            let times_alpha = |channel: u8| {
                let product = u32::from(channel) * u32::from(alpha);
                // From 0..=255², rounded to 0..=65535.
                ((u64::from(product) * 65535 + 65025 / 2) / 65025) as u16
            };
            U16x4::new([
                times_alpha(red),
                times_alpha(green),
                times_alpha(blue),
                u16::from(alpha) * 257,
            ])
        })
        .collect();
    let source = TypedImageRef::new(width, height, &premultiplied).expect("width × height pixels");
    let mut scaled = TypedImage::<U16x4>::new(to_width, to_height);
    resize(&source, &mut scaled);
    scaled
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let [red, green, blue, alpha] = pixel.0;
            if alpha == 0 {
                return [0, 0, 0, 0];
            }
            // The filter can overshoot, so a channel may exceed its alpha.
            let divided = |channel: u16| {
                let value = (u32::from(channel) * 255 + u32::from(alpha) / 2) / u32::from(alpha);
                value.min(255) as u8
            };
            let alpha = ((u32::from(alpha) + 128) / 257) as u8;
            [divided(red), divided(green), divided(blue), alpha]
        })
        .collect()
}

/// `source` resized with a Catmull-Rom filter to the size of `scaled`. Any
/// alpha is left alone: the caller has already multiplied it in.
fn resize<P: PixelTrait>(source: &TypedImageRef<P>, scaled: &mut TypedImage<P>) {
    let options = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(fir::FilterType::CatmullRom))
        .use_alpha(false);
    Resizer::new()
        .resize_typed(source, scaled, &options)
        .expect("source and destination of the same pixel type");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `width` × `height` pixels of one colour.
    fn filled(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        rgba.repeat(width as usize * height as usize)
    }

    #[test]
    fn one_colour_stays_that_colour() {
        for rgba in [[10, 200, 30, 255], [10, 200, 30, 128]] {
            let scaled = scale_rgba8(&filled(40, 30, rgba), 40, 30, 7, 5);
            assert_eq!(scaled, filled(7, 5, rgba), "{rgba:?}");
        }
    }

    #[test]
    fn the_colour_of_transparent_pixels_does_not_bleed_in() {
        // Red on the left, transparent white on the right.
        let pixels: Vec<u8> = (0..16 * 4)
            .flat_map(|i| match i % 16 < 8 {
                true => [255, 0, 0, 255],
                false => [255, 255, 255, 0],
            })
            .collect();
        let scaled = scale_rgba8(&pixels, 16, 4, 6, 2);
        for pixel in scaled.as_chunks::<4>().0 {
            if pixel[3] == 0 {
                assert_eq!(pixel, &[0, 0, 0, 0]);
            } else {
                assert_eq!(&pixel[..3], [255, 0, 0], "{pixel:?}");
            }
        }
    }

    #[test]
    #[should_panic(expected = "2 × 2 RGBA pixels")]
    fn a_buffer_of_the_wrong_size_panics() {
        scale_rgba8(&[0; 12], 2, 2, 1, 1);
    }
}
