//! Loading the images a document refers to.
//!
//! This is the stage between the document model and layout: it resolves
//! each image URL, reads the file and decodes it, so layout and output work
//! on pixels and never touch the file system. A failure is a value, not an
//! error for the whole document, because the renderer still shows the alt
//! text in place of an image it could not load.

mod svg;
mod url;

use std::collections::HashMap;
use std::fmt;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};

use image::error::{ImageFormatHint, UnsupportedErrorKind};
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, RgbaImage};

use crate::model::{Block, Document, Inline};

pub use svg::{Fonts, Svg};
pub use url::load_url;

use svg::{decode_svg, looks_like_svg};
use url::scheme;

/// The most pixels kiri decodes in one image: 64 megapixels, 256 MiB as
/// RGBA. A photo from a phone has about 12.
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

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
    /// A vector image, drawn at whatever size it is shown.
    Svg(Svg),
}

impl Source {
    /// Width and height in pixels; for an SVG, in CSS pixels.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Source::Raster(image) => image.dimensions(),
            Source::Svg(svg) => svg.size,
        }
    }

    /// The image drawn `width` × `height` pixels large: a raster image
    /// scaled, an SVG drawn at that size.
    pub fn render(&self, width: u32, height: u32) -> RgbaImage {
        match self {
            Source::Raster(image) if image.dimensions() == (width, height) => image.clone(),
            Source::Raster(image) => scale(image, width, height),
            Source::Svg(svg) => svg.render(width, height),
        }
    }
}

/// `image` scaled to `width` × `height`, in premultiplied alpha so that
/// the colour of transparent pixels does not show as a halo.
fn scale(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let (from_width, from_height) = image.dimensions();
    let bytes =
        kiri_image_scale::scale_rgba8(image.as_raw(), from_width, from_height, width, height);
    RgbaImage::from_raw(width, height, bytes).expect("width × height pixels")
}

/// Why an image could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// No file at this path.
    Missing(PathBuf),
    /// The file exists but reading it failed.
    Unreadable { path: PathBuf, reason: String },
    /// A directory, device, pipe or anything else that is not a regular
    /// file.
    NotAFile(PathBuf),
    /// The file is larger than the limit, in bytes.
    FileTooLarge { path: PathBuf, limit: u64 },
    /// A compressed SVG that grows past the limit, in bytes, when it is
    /// decompressed.
    DecompressedTooLarge { limit: u64 },
    /// The URL is empty or only a query or fragment, or a `file:` URL
    /// without a path.
    NoPath,
    /// `http:` and `https:` images are not fetched.
    Remote,
    /// A URL scheme other than `http`, `https`, `data` or `file`.
    UnsupportedScheme(String),
    /// A `data:` URL without a comma or with broken base64.
    InvalidData,
    /// The bytes are not in any image format kiri knows.
    UnknownFormat,
    /// A known image format that this build cannot decode, by name.
    UnsupportedFormat(String),
    /// The data ends before the image does.
    Truncated,
    /// Too many pixels to decode safely.
    TooLarge { width: u32, height: u32 },
    /// The decoder rejected the data.
    Corrupt(String),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageError::Missing(path) => write!(f, "{}: file not found", path.display()),
            ImageError::Unreadable { path, reason } => write!(f, "{}: {reason}", path.display()),
            ImageError::NotAFile(path) => write!(f, "{}: not a regular file", path.display()),
            ImageError::FileTooLarge { path, limit } => {
                write!(
                    f,
                    "{}: larger than the {} limit; see --max-image-size",
                    path.display(),
                    size(*limit)
                )
            }
            ImageError::DecompressedTooLarge { limit } => {
                write!(
                    f,
                    "decompresses to more than the {} limit; see --max-image-size",
                    size(*limit)
                )
            }
            ImageError::NoPath => f.write_str("the URL names no file"),
            ImageError::Remote => f.write_str("remote images are not loaded"),
            ImageError::UnsupportedScheme(scheme) => write!(f, "{scheme}: URLs are not loaded"),
            ImageError::InvalidData => f.write_str("invalid data: URL"),
            ImageError::UnknownFormat => f.write_str("not an image format kiri knows"),
            ImageError::UnsupportedFormat(format) => {
                write!(f, "{format} images are not supported in this build")
            }
            ImageError::Truncated => f.write_str("the image data is cut short"),
            ImageError::TooLarge { width, height } => {
                write!(f, "{width} × {height} pixels is too large to load")
            }
            ImageError::Corrupt(reason) => write!(f, "broken image: {reason}"),
        }
    }
}

impl std::error::Error for ImageError {}

/// A size limit in bytes for people, to go before a noun: whole megabytes
/// when it is that, as in "the 10 MB limit", and otherwise "the 1500-byte
/// limit".
fn size(bytes: u64) -> String {
    if bytes >= MB && bytes.is_multiple_of(MB) {
        format!("{} MB", bytes / MB)
    } else {
        format!("{bytes}-byte")
    }
}

/// Every image of a document by URL, loaded or not.
#[derive(Debug, Default)]
pub struct Images {
    by_url: HashMap<String, Result<Source, ImageError>>,
    /// The URLs in the order they were first inserted, the document's order.
    order: Vec<String>,
}

impl Images {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the outcome of loading `url`, replacing an earlier one.
    pub fn insert(&mut self, url: impl Into<String>, image: Result<Source, ImageError>) {
        let url = url.into();
        if !self.by_url.contains_key(&url) {
            self.order.push(url.clone());
        }
        self.by_url.insert(url, image);
    }

    /// The outcome for `url`, or `None` when it was never loaded.
    pub fn get(&self, url: &str) -> Option<&Result<Source, ImageError>> {
        self.by_url.get(url)
    }

    /// How many distinct URLs were loaded.
    pub fn len(&self) -> usize {
        self.by_url.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_url.is_empty()
    }

    /// Every URL and its outcome, in the order they were first inserted.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Result<Source, ImageError>)> {
        (self.order.iter()).map(|url| (url.as_str(), &self.by_url[url]))
    }

    /// One message for each image that did not load, in document order.
    pub fn warnings(&self) -> Vec<String> {
        self.iter()
            .filter_map(|(url, image)| match image {
                Ok(_) => None,
                // These name the file, resolved against the document.
                Err(
                    error @ (ImageError::Missing(_)
                    | ImageError::Unreadable { .. }
                    | ImageError::NotAFile(_)
                    | ImageError::FileTooLarge { .. }),
                ) => Some(error.to_string()),
                Err(ImageError::NoPath) if url.is_empty() => {
                    Some("an image URL is empty".to_owned())
                }
                // A data: URL is the image itself, too long for a message.
                Err(error) if scheme(url).is_some_and(|s| s.eq_ignore_ascii_case("data")) => {
                    Some(format!("data: URL: {error}"))
                }
                Err(error) => Some(format!("{url}: {error}")),
            })
            .collect()
    }
}

/// Loads every image that `document` shows as an image block once,
/// resolving relative paths against `base`, the directory of the Markdown
/// file. Files larger than `max_file_size` bytes are refused, and text in
/// SVGs is drawn with the system's fonts.
pub fn load(document: &Document, base: &Path, max_file_size: u64) -> Images {
    load_with(document, base, max_file_size, &Fonts::system())
}

/// `load`, drawing text in SVGs with `fonts`.
pub fn load_with(document: &Document, base: &Path, max_file_size: u64, fonts: &Fonts) -> Images {
    let mut urls = Vec::new();
    collect_blocks(&document.blocks, &mut urls);
    let mut images = Images::new();
    for url in urls {
        if images.get(url).is_none() {
            images.insert(url, load_url(url, base, max_file_size, fonts));
        }
    }
    images
}

/// The URLs of the images `blocks` show as image blocks. The others are
/// only ever shown as alt text, so they are not loaded at all.
fn collect_blocks<'a>(blocks: &'a [Block], urls: &mut Vec<&'a str>) {
    for block in blocks {
        match block {
            Block::Paragraph(content) => urls.extend(Inline::lone_image(content)),
            Block::BlockQuote(blocks) => collect_blocks(blocks, urls),
            Block::List(list) => {
                for item in &list.items {
                    collect_blocks(&item.blocks, urls);
                }
            }
            Block::Heading { .. }
            | Block::Table(_)
            | Block::CodeBlock { .. }
            | Block::ThematicBreak
            | Block::Html(_) => {}
        }
    }
}

/// Decodes an image file's bytes, whatever the format, and turns it upright
/// according to its EXIF orientation. Animations give their first frame.
/// A compressed SVG may decompress to at most `max_size` bytes, the limit
/// on a file. Text in SVGs is drawn with `fonts`.
pub fn decode(bytes: &[u8], max_size: u64, fonts: &Fonts) -> Result<Source, ImageError> {
    if looks_like_svg(bytes) {
        decode_svg(bytes, max_size, fonts)
    } else {
        decode_raster(bytes)
    }
}

fn decode_raster(bytes: &[u8]) -> Result<Source, ImageError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .expect("reading from memory does not fail");
    if reader.format().is_none() {
        return Err(ImageError::UnknownFormat);
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(from_image_error)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(ImageError::TooLarge { width, height });
    }
    // Broken EXIF data does not make the picture unusable.
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(|error| match error {
        image::ImageError::Limits(_) => ImageError::TooLarge { width, height },
        error => from_image_error(error),
    })?;
    image.apply_orientation(orientation);
    Ok(Source::Raster(image.into_rgba8()))
}

fn from_image_error(error: image::ImageError) -> ImageError {
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
mod tests {
    use super::*;

    pub(super) fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    /// The image at `url` with the default size limit and no fonts.
    pub(super) fn open(url: &str, base: &Path) -> Result<Source, ImageError> {
        load_url(url, base, DEFAULT_MAX_FILE_SIZE, &Fonts::none())
    }

    pub(super) fn fixture(name: &str) -> Result<Source, ImageError> {
        open(name, &fixtures().join("images"))
    }

    fn raster(name: &str) -> RgbaImage {
        match fixture(name) {
            Ok(Source::Raster(image)) => image,
            Ok(other) => panic!("{name} is not a raster image: {other:?}"),
            Err(error) => panic!("{name} does not load: {error:?}"),
        }
    }

    pub(super) fn rgba(image: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
        image.get_pixel(x, y).0
    }

    /// Mean absolute difference per channel, for comparing lossy images.
    fn difference(a: &RgbaImage, b: &RgbaImage) -> f64 {
        assert_eq!(a.dimensions(), b.dimensions());
        let total: u64 = a
            .as_raw()
            .iter()
            .zip(b.as_raw())
            .map(|(x, y)| u64::from(x.abs_diff(*y)))
            .sum();
        total as f64 / a.as_raw().len() as f64
    }

    #[test]
    fn decodes_png_with_its_colours_in_place() {
        let image = raster("pattern.png");
        assert_eq!(image.dimensions(), (320, 240));
        assert_eq!(rgba(&image, 20, 20), [255, 0, 0, 255]);
        assert_eq!(rgba(&image, 300, 20), [0, 255, 0, 255]);
        assert_eq!(rgba(&image, 20, 220), [0, 0, 255, 255]);
        assert_eq!(rgba(&image, 300, 220), [255, 255, 0, 255]);
    }

    #[test]
    fn keeps_transparency() {
        let image = raster("transparent.png");
        assert_eq!(rgba(&image, 0, 0)[3], 0);
        assert_eq!(rgba(&image, 100, 100), [255, 0, 0, 255]);
        let half = rgba(&image, 170, 170);
        assert_eq!(half[2], 255);
        assert!((120..=136).contains(&half[3]), "{half:?}");
    }

    #[test]
    fn decodes_png_bit_depths_palettes_and_interlacing() {
        for name in [
            "basn0g16", "basn3p08", "basn6a16", "basi0g08", "basi6a16", "tbbn3p08",
        ] {
            let image = raster(&format!("pngsuite/{name}.png"));
            assert_eq!(image.dimensions(), (32, 32), "{name}");
        }
    }

    #[test]
    fn decodes_the_default_image_of_an_animated_png() {
        assert_eq!(raster("beachball-apng.png").dimensions(), (100, 100));
    }

    #[test]
    fn decodes_jpeg() {
        assert_eq!(raster("photo-landscape.jpg").dimensions(), (1920, 1277));
        assert_eq!(raster("photo-portrait.jpg").dimensions(), (1280, 1918));
    }

    #[test]
    fn decodes_progressive_and_cmyk_jpeg_like_the_baseline() {
        let baseline = raster("photo-landscape.jpg");
        for name in ["photo-progressive.jpg", "photo-cmyk.jpg"] {
            let other = raster(name);
            let difference = difference(&baseline, &other);
            assert!(difference < 4.0, "{name}: {difference}");
        }
    }

    #[test]
    fn turns_jpeg_upright_by_its_exif_orientation() {
        let upright = raster("exif-1.jpg");
        assert_eq!(upright.dimensions(), (600, 399));
        for orientation in 2..=8 {
            let name = format!("exif-{orientation}.jpg");
            let image = raster(&name);
            assert_eq!(image.dimensions(), (600, 399), "{name}");
            let difference = difference(&upright, &image);
            assert!(difference < 4.0, "{name}: {difference}");
        }
    }

    #[test]
    fn turns_png_and_webp_upright_by_their_exif_orientation() {
        let upright = raster("pattern.png");
        for name in ["pattern-exif-6.png", "pattern-exif-6.webp"] {
            // Stored on its side, 240 × 320, with orientation 6.
            assert_eq!(raster(name), upright, "{name}");
        }
    }

    #[test]
    fn decodes_the_first_frame_of_an_animated_gif() {
        let image = raster("animated.gif");
        assert_eq!(image.dimensions(), (64, 64));
        assert_eq!(rgba(&image, 32, 32), [255, 0, 0, 255]);
        assert_eq!(raster("jiggy.gif").dimensions(), (512, 512));
    }

    #[test]
    fn decodes_webp() {
        assert_eq!(raster("lossless.webp").dimensions(), (200, 200));
        assert_eq!(raster("lossy.webp").dimensions(), (300, 300));
        let animated = raster("animated.webp");
        assert_eq!(animated.dimensions(), (64, 64));
        assert_eq!(rgba(&animated, 32, 32), [255, 0, 0, 255]);
    }

    #[test]
    fn scales_transparency_without_a_halo() {
        // Dark blue on the left; on the right, fully transparent pixels
        // whose colour is white, as many image editors save them.
        let image = RgbaImage::from_fn(16, 16, |x, _| {
            if x < 8 {
                image::Rgba([0, 0, 128, 255])
            } else {
                image::Rgba([255, 255, 255, 0])
            }
        });
        let small = Source::Raster(image).render(4, 4);
        for (x, y, pixel) in small.enumerate_pixels() {
            // Allowing for rounding and the filter's slight ringing.
            let [red, green, blue, alpha] = pixel.0;
            if alpha > 0 {
                let close = red <= 2 && green <= 2 && blue.abs_diff(128) <= 2;
                assert!(close, "({x}, {y}) is {pixel:?}");
            }
        }
        // The edge is still there: the transparent side stays transparent.
        assert_eq!(rgba(&small, 0, 0)[3], 255);
        assert_eq!(rgba(&small, 3, 0)[3], 0);
    }

    #[test]
    fn scales_raster_images_like_the_image_crate() {
        let cases = [
            ("pattern.png", 32, 24),
            ("pattern.png", 213, 160),
            ("photo-landscape.jpg", 300, 200),
            ("transparent.png", 64, 64),
            ("transparent.png", 150, 150),
            ("compass-rose.png", 225, 200),
            ("icon.png", 23, 23),
        ];
        for (name, width, height) in cases {
            let image = raster(name);
            let scaled = Source::Raster(image.clone()).render(width, height);
            let expected = scale_with_the_image_crate(&image, width, height);
            assert_eq!(scaled.dimensions(), (width, height), "{name}");
            // Colour weighed by alpha: a faint pixel may change hue without
            // anyone seeing it.
            let shown = |pixel: [u8; 4]| {
                let alpha = u32::from(pixel[3]);
                pixel
                    .map(|channel| u32::from(channel) * alpha / 255)
                    .map(|value| value as u8)
            };
            let mut differences = Vec::new();
            for (x, y, pixel) in scaled.enumerate_pixels() {
                let (a, b) = (shown(pixel.0), shown(rgba(&expected, x, y)));
                let alpha = pixel[3].abs_diff(rgba(&expected, x, y)[3]);
                differences.extend((0..3).map(|channel| a[channel].abs_diff(b[channel])));
                differences.push(alpha);
            }
            // Ringing where hard edges cross comes out a little different,
            // because fast_image_resize rounds and clamps between its two
            // passes; elsewhere the two agree to rounding.
            let largest = differences.iter().max().unwrap();
            let rounding = differences.iter().filter(|&&d| d <= 2).count();
            assert!(
                *largest <= 16,
                "{name} at {width} × {height}: off by {largest}"
            );
            assert!(
                rounding * 100 >= differences.len() * 99,
                "{name} at {width} × {height}: {} of {} values off by more than 2",
                differences.len() - rounding,
                differences.len(),
            );
        }
    }

    /// How kiri scaled images with the image crate's resize: in
    /// premultiplied alpha with 16 bits per channel, unless the image is
    /// opaque.
    fn scale_with_the_image_crate(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
        use image::imageops::{self, FilterType};
        use image::{ImageBuffer, Rgba};
        const FILTER: FilterType = FilterType::CatmullRom;
        if image.pixels().all(|pixel| pixel[3] == u8::MAX) {
            return imageops::resize(image, width, height, FILTER);
        }
        let premultiplied: ImageBuffer<Rgba<u16>, Vec<u16>> =
            ImageBuffer::from_fn(image.width(), image.height(), |x, y| {
                let [red, green, blue, alpha] = image.get_pixel(x, y).0;
                let times_alpha = |channel: u8| {
                    let product = u32::from(channel) * u32::from(alpha);
                    ((u64::from(product) * 65535 + 65025 / 2) / 65025) as u16
                };
                Rgba([
                    times_alpha(red),
                    times_alpha(green),
                    times_alpha(blue),
                    u16::from(alpha) * 257,
                ])
            });
        let scaled = imageops::resize(&premultiplied, width, height, FILTER);
        RgbaImage::from_fn(width, height, |x, y| {
            let [red, green, blue, alpha] = scaled.get_pixel(x, y).0;
            if alpha == 0 {
                return Rgba([0, 0, 0, 0]);
            }
            let divided = |channel: u16| {
                let value = (u32::from(channel) * 255 + u32::from(alpha) / 2) / u32::from(alpha);
                value.min(255) as u8
            };
            let alpha = ((u32::from(alpha) + 128) / 257) as u8;
            Rgba([divided(red), divided(green), divided(blue), alpha])
        })
    }

    #[test]
    fn reports_a_truncated_png() {
        assert_eq!(fixture("truncated.png"), Err(ImageError::Truncated));
    }

    #[test]
    fn shows_what_there_is_of_a_truncated_jpeg() {
        // Like browsers, the JPEG decoder fills in what is missing.
        assert_eq!(raster("truncated.jpg").dimensions(), (1920, 1277));
    }

    #[test]
    fn refuses_images_with_too_many_pixels() {
        assert_eq!(
            fixture("huge.png"),
            Err(ImageError::TooLarge {
                width: 20000,
                height: 20000
            })
        );
    }

    #[test]
    fn reports_data_that_is_no_image() {
        assert_eq!(fixture("not-an-image.png"), Err(ImageError::UnknownFormat));
    }

    #[test]
    fn reports_a_format_this_build_cannot_decode() {
        let farbfeld = b"farbfeld\0\0\0\x01\0\0\0\x01\0\0\0\0\0\0\0\0";
        let error = decode(farbfeld, DEFAULT_MAX_FILE_SIZE, &Fonts::none()).unwrap_err();
        assert_eq!(error, ImageError::UnsupportedFormat("farbfeld".into()));
        assert_eq!(
            error.to_string(),
            "farbfeld images are not supported in this build"
        );
    }

    #[test]
    fn names_formats_as_people_write_them() {
        let names = [
            ImageFormat::Bmp,
            ImageFormat::Ico,
            ImageFormat::Tiff,
            ImageFormat::Qoi,
            ImageFormat::Pnm,
            ImageFormat::Tga,
            ImageFormat::OpenExr,
            ImageFormat::Farbfeld,
        ]
        .map(format_name);
        assert_eq!(
            names,
            [
                "BMP", "ICO", "TIFF", "QOI", "PNM", "TGA", "OpenEXR", "farbfeld"
            ]
        );
    }

    #[test]
    fn loads_each_image_shown_as_a_block_once() {
        let source = "\
![a](images/pattern.png)

![again](images/pattern.png)

- ![b](images/missing.png)

> [![c](images/transparent.png)](https://example.com)
";
        let images = load_with(
            &crate::parse::parse(source),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
        );
        assert_eq!(images.len(), 3);
        assert!(images.get("images/pattern.png").unwrap().is_ok());
        assert!(images.get("images/transparent.png").unwrap().is_ok());
        assert!(matches!(
            images.get("images/missing.png"),
            Some(Err(ImageError::Missing(_)))
        ));
        assert!(images.get("https://example.com").is_none());
    }

    #[test]
    fn does_not_load_images_shown_as_alt_text() {
        let source = "\
# Title ![h](images/icon.png)

Text ![inline](images/pattern.png) and words.

[![with words](images/transparent.png) and words](https://example.com)

![one](images/icon.png) ![two](images/icon.png)

| x |
|---|
| ![d](images/missing.png) |
";
        let images = load_with(
            &crate::parse::parse(source),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
        );
        assert!(images.is_empty(), "{images:?}");
    }

    #[test]
    fn keeps_the_document_order() {
        let mut images = Images::new();
        for url in ["b.png", "a.png", "c.png"] {
            images.insert(url, Err(ImageError::Remote));
        }
        images.insert("a.png", Err(ImageError::InvalidData));
        let urls: Vec<&str> = images.iter().map(|(url, _)| url).collect();
        assert_eq!(urls, ["b.png", "a.png", "c.png"]);
        assert_eq!(images.get("a.png"), Some(&Err(ImageError::InvalidData)));
    }

    #[test]
    fn warns_about_each_image_that_did_not_load_in_document_order() {
        let mut images = Images::new();
        images.insert("https://example.com/a.png", Err(ImageError::Remote));
        images.insert("ok.png", Ok(Source::Raster(RgbaImage::new(1, 1))));
        images.insert("gone.png", Err(ImageError::Missing("/doc/gone.png".into())));
        images.insert("data:image/png;base64,AAAA", Err(ImageError::UnknownFormat));
        images.insert("file://?x", Err(ImageError::NoPath));
        images.insert("", Err(ImageError::NoPath));
        assert_eq!(
            images.warnings(),
            [
                "https://example.com/a.png: remote images are not loaded",
                "/doc/gone.png: file not found",
                "data: URL: not an image format kiri knows",
                "file://?x: the URL names no file",
                "an image URL is empty",
            ]
        );
    }

    #[test]
    fn explains_errors_in_words() {
        let messages = [
            ImageError::Missing("a/b.png".into()).to_string(),
            ImageError::Remote.to_string(),
            ImageError::NotAFile("/dev/zero".into()).to_string(),
            ImageError::FileTooLarge {
                path: "big.png".into(),
                limit: 10 * MB,
            }
            .to_string(),
            ImageError::FileTooLarge {
                path: "big.png".into(),
                limit: 1500,
            }
            .to_string(),
            ImageError::DecompressedTooLarge { limit: 10 * MB }.to_string(),
            ImageError::TooLarge {
                width: 20000,
                height: 20000,
            }
            .to_string(),
        ];
        assert_eq!(
            messages,
            [
                "a/b.png: file not found",
                "remote images are not loaded",
                "/dev/zero: not a regular file",
                "big.png: larger than the 10 MB limit; see --max-image-size",
                "big.png: larger than the 1500-byte limit; see --max-image-size",
                "decompresses to more than the 10 MB limit; see --max-image-size",
                "20000 × 20000 pixels is too large to load",
            ]
        );
    }
}
