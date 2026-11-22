//! Decoding image files, raster and SVG, each within a size limit.

use std::io;
use std::path::Path;
use std::sync::Arc;

use image::error::{ImageFormatHint, UnsupportedErrorKind};
use image::{ImageFormat, RgbaImage};
use kiri_tr_image_utils::DecodeError;

use crate::images::svg::{Fonts, Svg, decode_svg, looks_like_svg};
use crate::typeset::images::ImageError;
use crate::typeset::images::animation::{
    Animation, Decoded, FRAME_BUDGET, Frame, decode_animation,
};
use crate::typeset::images::size::Unit;
use crate::typeset::images::url;

/// The most pixels kiri decodes in one image: 64 megapixels, 256 MiB as
/// RGBA. A photo from a phone has about 12.
pub(crate) const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// The width and height in pixels that the header of the PNG file `bytes`
/// gives, read without decoding the image; `None` for what is no PNG.
pub(crate) fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let header = bytes.strip_prefix(b"\x89PNG\r\n\x1a\n")?;
    let ihdr = header.get(4..8)?;
    let number = |at: usize| Some(u32::from_be_bytes(header.get(at..at + 4)?.try_into().ok()?));
    (ihdr == b"IHDR").then_some(())?;
    Some((number(8)?, number(12)?))
}

/// The rows of `image` with any pixel of alpha above 8, from the first to
/// one past the last; `None` when there are none. The transparent rows
/// above and below them are not part of what the image shows.
pub fn opaque_rows(image: &RgbaImage) -> Option<std::ops::Range<u32>> {
    let mut rows =
        (0..image.height()).filter(|&y| (0..image.width()).any(|x| image.get_pixel(x, y)[3] > 8));
    let first = rows.next()?;
    let last = rows.next_back().unwrap_or(first);
    Some(first..last + 1)
}

/// The most memory a decoder may allocate, a guard for decoders that would
/// otherwise need more than the pixels themselves.
const MAX_ALLOC: u64 = 512 * 1024 * 1024;

/// A megabyte, the unit of the image file size limit.
pub const MB: u64 = 1_000_000;

/// The largest image file kiri reads unless told otherwise.
pub const DEFAULT_MAX_FILE_SIZE: u64 = 10 * MB;

/// A decoded image, at its natural size.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Pixels in 8-bit RGBA, already turned upright.
    Raster(RgbaImage),
    /// An animated GIF, WebP or APNG: every frame, in full. It is shown as
    /// its first frame unless the pager plays it.
    Animated(Arc<Animation>),
    /// A vector image, drawn at whatever size it is shown.
    Svg(Svg),
    /// Pixels in 8-bit RGBA drawn for a display with more than one pixel
    /// to the CSS pixel, as a browser draws a diagram, and shown at `css`,
    /// its width and height in CSS pixels.
    Dense { image: RgbaImage, css: (u32, u32) },
    /// Text drawn to whole cells, such as a heading, shown one pixel per
    /// terminal pixel as `Raster` is. `breaks[i]` is the column the word
    /// `i + 1` starts on, and `ends[i]` the column after the last that
    /// the ink of word `i` reaches: a line of the image can show the words
    /// from one to another.
    Text {
        image: RgbaImage,
        breaks: Vec<u16>,
        ends: Vec<u16>,
    },
}

impl Source {
    /// Width and height in pixels; for an SVG or a dense image, in CSS
    /// pixels.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Source::Raster(image) | Source::Text { image, .. } => image.dimensions(),
            Source::Animated(animation) => animation.first().dimensions(),
            Source::Svg(svg) => svg.size,
            Source::Dense { css, .. } => *css,
        }
    }

    /// What the pixels of [`Self::size`] are on the screen.
    pub fn unit(&self) -> Unit {
        match self {
            Source::Raster(_) | Source::Animated(_) | Source::Text { .. } => Unit::Pixel,
            Source::Svg(_) | Source::Dense { .. } => Unit::Css,
        }
    }

    /// A hash of what the image is, so that two are told apart without
    /// comparing their pixels.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        match self {
            Source::Raster(image) => (0u8, image.dimensions(), image.as_raw()).hash(&mut hasher),
            Source::Animated(animation) => {
                let frames = animation.frames.iter();
                let print = frames.map(|frame| (frame.image.as_raw(), frame.gap));
                (4u8, animation.first().dimensions(), animation.plays_print()).hash(&mut hasher);
                print.for_each(|frame| frame.hash(&mut hasher));
            }
            Source::Dense { image, css } => {
                (1u8, css, image.dimensions(), image.as_raw()).hash(&mut hasher)
            }
            Source::Text {
                image,
                breaks,
                ends,
            } => {
                (2u8, image.dimensions(), image.as_raw(), breaks, ends).hash(&mut hasher);
            }
            Source::Svg(svg) => {
                3u8.hash(&mut hasher);
                svg.fingerprint(&mut hasher);
            }
        }
        hasher.finish()
    }

    /// The image drawn `width` × `height` pixels large: a raster image
    /// scaled, an SVG drawn at that size.
    pub fn render(&self, width: u32, height: u32) -> RgbaImage {
        match self {
            Source::Raster(image) | Source::Dense { image, .. } | Source::Text { image, .. }
                if image.dimensions() == (width, height) =>
            {
                image.clone()
            }
            Source::Raster(image) | Source::Dense { image, .. } | Source::Text { image, .. } => {
                scale(image, width, height)
            }
            Source::Animated(animation) => match animation.first() {
                image if image.dimensions() == (width, height) => image.clone(),
                image => scale(image, width, height),
            },
            Source::Svg(svg) => svg.render(width, height),
        }
    }

    /// The frames of the animation this is, `None` for a still image.
    pub fn animation(&self) -> Option<&Arc<Animation>> {
        match self {
            Source::Animated(animation) => Some(animation),
            _ => None,
        }
    }
}

/// `image` scaled to `width` × `height`, in premultiplied alpha so that
/// the colour of transparent pixels does not show as a halo.
pub(super) fn scale(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let (from_width, from_height) = image.dimensions();
    let bytes =
        kiri_tr_image_utils::scale_rgba8(image.as_raw(), from_width, from_height, width, height);
    RgbaImage::from_raw(width, height, bytes).expect("width × height pixels")
}

/// The format of an image file, as far as it is known before decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Told by the content.
    Detect,
    /// An SVG, plain or compressed.
    Svg,
    /// A raster format, whatever the content tells.
    Raster(ImageFormat),
}

/// Reads and decodes the image file at `path` from `reader`, which gives
/// the whole file, in `format`. Files larger than `max_file_size` bytes are
/// refused, and text in SVGs is drawn with `fonts`.
pub fn read_image(
    reader: impl io::Read,
    path: &Path,
    format: Format,
    max_file_size: u64,
    fonts: &Fonts,
) -> Result<Source, ImageError> {
    read_image_as(reader, path, format, max_file_size, fonts, false)
}

/// `read_image`, giving an animation with every frame when `animated`, as
/// [`decode_named_as`] does.
pub fn read_image_as(
    reader: impl io::Read,
    path: &Path,
    format: Format,
    max_file_size: u64,
    fonts: &Fonts,
    animated: bool,
) -> Result<Source, ImageError> {
    let bytes = url::read_at_most(reader, max_file_size)
        .map_err(|error| ImageError::Unreadable {
            path: path.to_owned(),
            reason: error.to_string(),
        })?
        .ok_or_else(|| ImageError::FileTooLarge {
            path: path.to_owned(),
            limit: max_file_size,
        })?;
    match format {
        Format::Detect => decode_named_as(&bytes, None, max_file_size, fonts, animated),
        Format::Svg => decode_svg(&bytes, max_file_size, fonts).map(Source::Svg),
        Format::Raster(format) => decoded(kiri_tr_image_utils::decode_rgba8_as(
            &bytes, format, MAX_PIXELS, MAX_ALLOC,
        ))
        .map(Source::Raster),
    }
}

/// What [`read_image_as`] reads from the file, with no pixel decoded: the
/// natural size of the image, as [`Source::size`] gives it, and the unit
/// of that size. Raster images give it from their headers, turned upright
/// by their EXIF orientation as decoding does; an SVG is parsed, which is
/// all it takes to know its size. It fails as `read_image_as` does for a
/// file that is too large or cannot be read, and for a header that is not
/// one; a file that has one can still fail to decode.
pub fn read_size_as(
    reader: impl io::Read,
    path: &Path,
    format: Format,
    max_file_size: u64,
    fonts: &Fonts,
) -> Result<((u32, u32), Unit), ImageError> {
    let bytes = url::read_at_most(reader, max_file_size)
        .map_err(|error| ImageError::Unreadable {
            path: path.to_owned(),
            reason: error.to_string(),
        })?
        .ok_or_else(|| ImageError::FileTooLarge {
            path: path.to_owned(),
            limit: max_file_size,
        })?;
    let svg = |bytes: &[u8]| Ok((decode_svg(bytes, max_file_size, fonts)?.size, Unit::Css));
    let raster = |format| match kiri_tr_image_utils::dimensions(&bytes, format, MAX_PIXELS) {
        Ok(size) => Ok((size, Unit::Pixel)),
        Err(DecodeError::UnknownFormat) => Err(ImageError::UnknownFormat),
        Err(DecodeError::TooLarge { width, height }) => Err(ImageError::TooLarge { width, height }),
        Err(DecodeError::Image(error)) => Err(from_image_error(error)),
    };
    match format {
        Format::Svg => svg(&bytes),
        Format::Detect if looks_like_svg(&bytes) => svg(&bytes),
        Format::Detect => raster(None),
        Format::Raster(format) => raster(Some(format)),
    }
}

/// Decodes an image file's bytes, whatever the format, and turns it upright
/// according to its EXIF orientation. Animations give their first frame.
/// A compressed SVG may decompress to at most `max_size` bytes, the limit
/// on a file. Text in SVGs is drawn with `fonts`.
pub fn decode(bytes: &[u8], max_size: u64, fonts: &Fonts) -> Result<Source, ImageError> {
    decode_named(bytes, None, max_size, fonts)
}

/// `decode`, given the format that the file name or media type names, if
/// any. The bytes decide, as in a browser, except for TGA, which has no
/// signature to recognise it by.
pub(crate) fn decode_named(
    bytes: &[u8],
    named: Option<ImageFormat>,
    max_size: u64,
    fonts: &Fonts,
) -> Result<Source, ImageError> {
    decode_named_as(bytes, named, max_size, fonts, false)
}

/// `decode_named`, giving an animated GIF, WebP or APNG with every frame
/// when `animated`, as long as the frames take at most [`FRAME_BUDGET`]
/// bytes; an animation over it shows its first frame.
pub(crate) fn decode_named_as(
    bytes: &[u8],
    named: Option<ImageFormat>,
    max_size: u64,
    fonts: &Fonts,
    animated: bool,
) -> Result<Source, ImageError> {
    let budget = animated.then_some(FRAME_BUDGET);
    decode_named_with(bytes, named, max_size, fonts, budget)
}

/// `decode_named_as`, with the frames of an animation kept when they take
/// at most `budget` bytes, if there is one; over it the animation keeps its
/// first frame and says that its frames are over the budget.
pub(crate) fn decode_named_with(
    bytes: &[u8],
    named: Option<ImageFormat>,
    max_size: u64,
    fonts: &Fonts,
    budget: Option<u64>,
) -> Result<Source, ImageError> {
    if looks_like_svg(bytes) {
        return decode_svg(bytes, max_size, fonts).map(Source::Svg);
    }
    match budget.map(|budget| decode_animation(bytes, budget)) {
        Some(Ok(Decoded::Animated(animation))) => {
            return Ok(Source::Animated(Arc::new(animation)));
        }
        Some(Ok(Decoded::OverBudget { plays })) => {
            let image = decode_raster(bytes, named)?;
            let frame = Frame {
                image,
                gap: std::time::Duration::ZERO,
            };
            return Ok(Source::Animated(Arc::new(Animation {
                frames: vec![frame],
                plays,
                over_budget: true,
            })));
        }
        _ => {}
    }
    decode_raster(bytes, named).map(Source::Raster)
}

pub(super) fn decode_raster(
    bytes: &[u8],
    named: Option<ImageFormat>,
) -> Result<RgbaImage, ImageError> {
    // Only TGA has no signature; for other formats a wrong name must not
    // turn data that is no image into a broken image.
    let fallback = named.filter(|&format| format == ImageFormat::Tga);
    decoded(kiri_tr_image_utils::decode_rgba8(
        bytes, fallback, MAX_PIXELS, MAX_ALLOC,
    ))
}

/// The image `kiri_tr_image_utils` decoded, or why it gave none.
fn decoded(result: Result<RgbaImage, DecodeError>) -> Result<RgbaImage, ImageError> {
    match result {
        Ok(image) => Ok(image),
        Err(DecodeError::UnknownFormat) => Err(ImageError::UnknownFormat),
        Err(DecodeError::TooLarge { width, height }) => Err(ImageError::TooLarge { width, height }),
        Err(DecodeError::Image(error)) => Err(from_image_error(error)),
    }
}

pub(super) fn from_image_error(error: image::ImageError) -> ImageError {
    match error {
        image::ImageError::IoError(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            ImageError::Truncated
        }
        image::ImageError::Unsupported(error) => match error.kind() {
            UnsupportedErrorKind::Format(ImageFormatHint::Unknown) => ImageError::UnknownFormat,
            UnsupportedErrorKind::Format(ImageFormatHint::Exact(format)) => {
                ImageError::UnsupportedFormat(format_name(format))
            }
            _ => ImageError::Corrupt(error.to_string()),
        },
        error => ImageError::Corrupt(error.to_string()),
    }
}

/// The name people use for `format`, for messages.
fn format_name(format: ImageFormat) -> String {
    match format {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Gif => "GIF",
        ImageFormat::WebP => "WebP",
        ImageFormat::Pnm => "PNM",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Tga => "TGA",
        ImageFormat::Dds => "DDS",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Ico => "ICO",
        ImageFormat::Hdr => "Radiance HDR",
        ImageFormat::OpenExr => "OpenEXR",
        ImageFormat::Farbfeld => "farbfeld",
        ImageFormat::Avif => "AVIF",
        ImageFormat::Qoi => "QOI",
        format => return format!("{format:?}"),
    }
    .to_owned()
}

#[cfg(test)]
mod tests;
