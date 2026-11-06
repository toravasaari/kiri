//! Decoding raster images.
//!
//! The decoders of BMP, TGA, QOI and other formats are generic over the
//! reader, so they are compiled where the reader's type is chosen: here.

use std::io::Cursor;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader, Limits, RgbaImage};

/// Why `decode_rgba8` gave no image.
#[derive(Debug)]
pub enum DecodeError {
    /// The bytes are in no format the `image` crate recognises.
    UnknownFormat,
    /// More pixels than allowed, or more memory than allowed to decode
    /// them.
    TooLarge { width: u32, height: u32 },
    /// The decoder failed or this build cannot decode the format.
    Image(ImageError),
}

/// Decodes `bytes` into 8-bit RGBA, turned upright according to its EXIF
/// orientation. Animations give their first frame. The bytes name their
/// format, or else it is `fallback`, if given. An image may have at most
/// `max_pixels` pixels, and its decoder may allocate at most `max_alloc`
/// bytes.
pub fn decode_rgba8(
    bytes: &[u8],
    fallback: Option<ImageFormat>,
    max_pixels: u64,
    max_alloc: u64,
) -> Result<RgbaImage, DecodeError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .expect("reading from memory does not fail");
    if let (None, Some(format)) = (reader.format(), fallback) {
        reader.set_format(format);
    }
    if reader.format().is_none() {
        return Err(DecodeError::UnknownFormat);
    }
    decode_reader(reader, max_pixels, max_alloc)
}

/// `decode_rgba8` in `format`, whatever format the bytes name.
pub fn decode_rgba8_as(
    bytes: &[u8],
    format: ImageFormat,
    max_pixels: u64,
    max_alloc: u64,
) -> Result<RgbaImage, DecodeError> {
    let mut reader = ImageReader::new(Cursor::new(bytes));
    reader.set_format(format);
    decode_reader(reader, max_pixels, max_alloc)
}

/// The width and height of the image in `bytes` as `decode_rgba8` would
/// give them, upright by its EXIF orientation, read from its header and
/// with no pixel decoded. `format` names the format when the bytes may
/// not, as in `decode_rgba8_as`; otherwise it is guessed from the bytes,
/// or is `fallback`. The same limit on `max_pixels` applies.
pub fn dimensions(
    bytes: &[u8],
    format: Option<ImageFormat>,
    fallback: Option<ImageFormat>,
    max_pixels: u64,
) -> Result<(u32, u32), DecodeError> {
    let mut reader = ImageReader::new(Cursor::new(bytes));
    match format {
        Some(format) => reader.set_format(format),
        None => {
            reader = reader
                .with_guessed_format()
                .expect("reading from memory does not fail");
            if let (None, Some(format)) = (reader.format(), fallback) {
                reader.set_format(format);
            }
            if reader.format().is_none() {
                return Err(DecodeError::UnknownFormat);
            }
        }
    }
    let mut decoder = reader.into_decoder().map_err(DecodeError::Image)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > max_pixels {
        return Err(DecodeError::TooLarge { width, height });
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    Ok(match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (height, width),
        _ => (width, height),
    })
}

fn decode_reader(
    mut reader: ImageReader<Cursor<&[u8]>>,
    max_pixels: u64,
    max_alloc: u64,
) -> Result<RgbaImage, DecodeError> {
    let mut limits = Limits::default();
    limits.max_alloc = Some(max_alloc);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(DecodeError::Image)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > max_pixels {
        return Err(DecodeError::TooLarge { width, height });
    }
    // Broken EXIF data does not make the picture unusable.
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(|error| match error {
        ImageError::Limits(_) => DecodeError::TooLarge { width, height },
        error => DecodeError::Image(error),
    })?;
    image.apply_orientation(orientation);
    Ok(image.into_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG of `width` × `height` pixels, red at the top left.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut image = RgbaImage::new(width, height);
        image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    const LIMIT: u64 = 1 << 30;

    #[test]
    fn decodes_into_rgba() {
        let image = decode_rgba8(&png(3, 2), None, LIMIT, LIMIT).unwrap();
        assert_eq!(image.dimensions(), (3, 2));
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(2, 1).0, [0, 0, 0, 0]);
    }

    #[test]
    fn reports_bytes_in_no_known_format() {
        let result = decode_rgba8(b"not an image", None, LIMIT, LIMIT);
        assert!(
            matches!(result, Err(DecodeError::UnknownFormat)),
            "{result:?}"
        );
    }

    #[test]
    fn falls_back_on_the_given_format_only_when_the_bytes_name_none() {
        let result = decode_rgba8(b"not an image", Some(ImageFormat::Png), LIMIT, LIMIT);
        assert!(matches!(result, Err(DecodeError::Image(_))), "{result:?}");
        // A PNG stays a PNG.
        let result = decode_rgba8(&png(3, 2), Some(ImageFormat::Bmp), LIMIT, LIMIT);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn decodes_in_the_given_format_whatever_the_bytes_name() {
        let result = decode_rgba8_as(&png(3, 2), ImageFormat::Png, LIMIT, LIMIT);
        assert!(result.is_ok(), "{result:?}");
        let result = decode_rgba8_as(&png(3, 2), ImageFormat::Bmp, LIMIT, LIMIT);
        assert!(matches!(result, Err(DecodeError::Image(_))), "{result:?}");
    }

    #[test]
    fn dimensions_are_those_of_the_decoded_image_without_decoding_it() {
        let bytes = png(5, 3);
        assert_eq!(dimensions(&bytes, None, None, LIMIT).unwrap(), (5, 3));
        let format = Some(ImageFormat::Png);
        assert_eq!(dimensions(&bytes, format, None, LIMIT).unwrap(), (5, 3));
        assert!(matches!(
            dimensions(b"not an image", None, None, LIMIT),
            Err(DecodeError::UnknownFormat)
        ));
        assert!(matches!(
            dimensions(&bytes, None, None, 14),
            Err(DecodeError::TooLarge { .. })
        ));
    }

    #[test]
    fn refuses_more_pixels_than_allowed() {
        assert!(decode_rgba8(&png(3, 2), None, 6, LIMIT).is_ok());
        let result = decode_rgba8(&png(3, 2), None, 5, LIMIT);
        assert!(
            matches!(
                result,
                Err(DecodeError::TooLarge {
                    width: 3,
                    height: 2
                })
            ),
            "{result:?}"
        );
    }
}
