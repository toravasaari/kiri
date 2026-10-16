//! Loading the images a document refers to.
//!
//! This is the stage between the document model and layout: it resolves
//! each image URL, reads the file and decodes it, so layout and output work
//! on pixels and never touch the file system. A failure is a value, not an
//! error for the whole document, because the renderer still shows the alt
//! text in place of an image it could not load.

use std::collections::HashMap;
use std::fmt;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use image::error::{ImageFormatHint, UnsupportedErrorKind};
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits, RgbaImage};

use crate::model::{Block, Document, Inline};

/// The most pixels kiri decodes in one image: 64 megapixels, 256 MiB as
/// RGBA. A photo from a phone has about 12.
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// The most memory a decoder may allocate, a guard for decoders that would
/// otherwise need more than the pixels themselves.
const MAX_ALLOC: u64 = 512 * 1024 * 1024;

/// Standard base64 that does not insist on the trailing `=`.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// A decoded image, at its natural size.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Pixels in 8-bit RGBA, already turned upright.
    Raster(RgbaImage),
}

impl Source {
    /// Width and height in pixels.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Source::Raster(image) => image.dimensions(),
        }
    }
}

/// Why an image could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// No file at this path.
    Missing(PathBuf),
    /// The file exists but reading it failed.
    Unreadable { path: PathBuf, reason: String },
    /// `http:` and `https:` images are not fetched.
    Remote,
    /// A URL scheme other than `http`, `https` or `data`.
    UnsupportedScheme(String),
    /// A `data:` URL without a comma or with broken base64.
    InvalidData,
    /// The bytes are not in any image format kiri knows.
    UnknownFormat,
    /// A known image format that this build cannot decode.
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
            ImageError::Remote => f.write_str("remote images are not loaded"),
            ImageError::UnsupportedScheme(scheme) => write!(f, "{scheme}: URLs are not loaded"),
            ImageError::InvalidData => f.write_str("invalid data: URL"),
            ImageError::UnknownFormat => f.write_str("not an image format kiri knows"),
            ImageError::UnsupportedFormat(format) => {
                write!(f, "{format} images are not supported")
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

/// Every image of a document by URL, loaded or not.
#[derive(Debug, Default)]
pub struct Images {
    by_url: HashMap<String, Result<Source, ImageError>>,
}

impl Images {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the outcome of loading `url`, replacing an earlier one.
    pub fn insert(&mut self, url: impl Into<String>, image: Result<Source, ImageError>) {
        self.by_url.insert(url.into(), image);
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
}

/// Loads every image in `document` once, resolving relative paths against
/// `base`, the directory of the Markdown file.
pub fn load(document: &Document, base: &Path) -> Images {
    let mut urls = Vec::new();
    collect_blocks(&document.blocks, &mut urls);
    let mut images = Images::new();
    for url in urls {
        if images.get(url).is_none() {
            images.insert(url, load_url(url, base));
        }
    }
    images
}

fn collect_blocks<'a>(blocks: &'a [Block], urls: &mut Vec<&'a str>) {
    for block in blocks {
        match block {
            Block::Heading { content, .. } | Block::Paragraph(content) => {
                collect_inlines(content, urls);
            }
            Block::BlockQuote(blocks) => collect_blocks(blocks, urls),
            Block::List(list) => {
                for item in &list.items {
                    collect_blocks(&item.blocks, urls);
                }
            }
            Block::Table(table) => {
                for cell in table.header.iter().chain(table.rows.iter().flatten()) {
                    collect_inlines(cell, urls);
                }
            }
            Block::CodeBlock { .. } | Block::ThematicBreak | Block::Html(_) => {}
        }
    }
}

fn collect_inlines<'a>(inlines: &'a [Inline], urls: &mut Vec<&'a str>) {
    for inline in inlines {
        match inline {
            Inline::Image { url, .. } => urls.push(url),
            Inline::Emphasis(content)
            | Inline::Strong(content)
            | Inline::Strikethrough(content)
            | Inline::Link { content, .. } => collect_inlines(content, urls),
            Inline::Text(_)
            | Inline::Code(_)
            | Inline::SoftBreak
            | Inline::LineBreak
            | Inline::Html(_) => {}
        }
    }
}

/// Loads the image at `url`: a `data:` URL, or a path that is absolute or
/// relative to `base`. Paths may use URL percent escapes, as in a browser.
pub fn load_url(url: &str, base: &Path) -> Result<Source, ImageError> {
    match scheme(url) {
        Some(scheme) if scheme.eq_ignore_ascii_case("data") => decode(&data_url(url)?),
        Some(scheme)
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") =>
        {
            Err(ImageError::Remote)
        }
        Some(scheme) => Err(ImageError::UnsupportedScheme(scheme.to_ascii_lowercase())),
        None => {
            let decoded = percent_decode(url.as_bytes());
            let path = base.join(String::from_utf8(decoded).as_deref().unwrap_or(url));
            match std::fs::read(&path) {
                Ok(bytes) => decode(&bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    Err(ImageError::Missing(path))
                }
                Err(error) => Err(ImageError::Unreadable {
                    path,
                    reason: error.to_string(),
                }),
            }
        }
    }
}

/// The URL's scheme, such as `https`, or `None` for a path. A single letter
/// is a Windows drive, not a scheme.
fn scheme(url: &str) -> Option<&str> {
    let (scheme, _) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    (valid && scheme.len() > 1).then_some(scheme)
}

/// The bytes in a `data:[<media type>][;base64],<data>` URL. The media type
/// is ignored: the format comes from the bytes, as in a browser.
fn data_url(url: &str) -> Result<Vec<u8>, ImageError> {
    let (header, data) = url.split_once(',').ok_or(ImageError::InvalidData)?;
    let data = percent_decode(data.as_bytes());
    let is_base64 = header
        .rsplit(';')
        .next()
        .is_some_and(|part| part.trim().eq_ignore_ascii_case("base64"));
    if !is_base64 {
        return Ok(data);
    }
    let data: Vec<u8> = data
        .into_iter()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    BASE64.decode(data).map_err(|_| ImageError::InvalidData)
}

/// Turns `%XX` escapes into bytes and leaves everything else as it is.
fn percent_decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let escaped = (input[i] == b'%')
            .then(|| input.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(input[i]);
                i += 1;
            }
        }
    }
    out
}

/// Decodes an image file's bytes, whatever the format, and turns it upright
/// according to its EXIF orientation. Animations give their first frame.
pub fn decode(bytes: &[u8]) -> Result<Source, ImageError> {
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
                ImageError::UnsupportedFormat(format!("{format:?}"))
            }
            _ => ImageError::Corrupt(error.to_string()),
        },
        error => ImageError::Corrupt(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn fixture(name: &str) -> Result<Source, ImageError> {
        load_url(name, &fixtures().join("images"))
    }

    fn raster(name: &str) -> RgbaImage {
        match fixture(name) {
            Ok(Source::Raster(image)) => image,
            Err(error) => panic!("{name} does not load: {error:?}"),
        }
    }

    fn rgba(image: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
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
    fn loads_base64_data_urls() {
        let bytes = std::fs::read(fixtures().join("images/pattern.png")).unwrap();
        let url = format!("data:image/png;base64,{}", STANDARD.encode(&bytes));
        let loaded = load_url(&url, Path::new("/nonexistent"));
        assert_eq!(loaded, fixture("pattern.png"));
        assert!(loaded.is_ok());
    }

    #[test]
    fn loads_percent_encoded_data_urls() {
        let bytes = std::fs::read(fixtures().join("images/pattern.png")).unwrap();
        let escaped: String = bytes.iter().map(|byte| format!("%{byte:02X}")).collect();
        let loaded = load_url(&format!("data:image/png,{escaped}"), Path::new("/"));
        assert_eq!(loaded, fixture("pattern.png"));
        assert!(loaded.is_ok());
    }

    #[test]
    fn rejects_broken_data_urls() {
        let base = Path::new("/");
        assert_eq!(
            load_url("data:image/png;base64,no*base64", base),
            Err(ImageError::InvalidData)
        );
        assert_eq!(
            load_url("data:image/png;base64", base),
            Err(ImageError::InvalidData)
        );
    }

    #[test]
    fn resolves_paths_against_the_base_and_keeps_absolute_ones() {
        let base = fixtures();
        assert!(load_url("images/pattern.png", &base).is_ok());
        let absolute = base.join("images/pattern.png");
        assert!(load_url(absolute.to_str().unwrap(), Path::new("/nonexistent")).is_ok());
    }

    #[test]
    fn decodes_percent_escapes_in_paths() {
        assert!(fixture("pattern%2Epng").is_ok());
    }

    #[test]
    fn reports_a_missing_file_with_its_resolved_path() {
        let base = fixtures().join("images");
        assert_eq!(
            load_url("no-such.png", &base),
            Err(ImageError::Missing(base.join("no-such.png")))
        );
    }

    #[test]
    fn reports_a_directory_as_unreadable() {
        assert!(matches!(
            fixture("pngsuite"),
            Err(ImageError::Unreadable { .. })
        ));
    }

    #[test]
    fn does_not_fetch_remote_images() {
        let base = fixtures();
        assert_eq!(
            load_url("https://example.com/a.png", &base),
            Err(ImageError::Remote)
        );
        assert_eq!(
            load_url("HTTP://example.com/a.png", &base),
            Err(ImageError::Remote)
        );
        assert_eq!(
            load_url("ftp://example.com/a.png", &base),
            Err(ImageError::UnsupportedScheme("ftp".into()))
        );
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
        assert_eq!(
            decode(farbfeld),
            Err(ImageError::UnsupportedFormat("Farbfeld".into()))
        );
    }

    #[test]
    fn loads_each_image_of_a_document_once() {
        let source = "\
# Title ![h](images/icon.png)

![a](images/pattern.png) and ![again](images/pattern.png)

- ![b](images/missing.png)

> [![c](images/transparent.png)](https://example.com)

| x |
|---|
| ![d](https://example.com/d.png) |
";
        let images = load(&crate::parse::parse(source), &fixtures());
        assert_eq!(images.len(), 5);
        assert!(images.get("images/icon.png").unwrap().is_ok());
        assert!(images.get("images/pattern.png").unwrap().is_ok());
        assert!(images.get("images/transparent.png").unwrap().is_ok());
        assert!(matches!(
            images.get("images/missing.png"),
            Some(Err(ImageError::Missing(_)))
        ));
        assert_eq!(
            images.get("https://example.com/d.png"),
            Some(&Err(ImageError::Remote))
        );
        assert!(images.get("https://example.com").is_none());
    }

    #[test]
    fn explains_errors_in_words() {
        let messages = [
            ImageError::Missing("a/b.png".into()).to_string(),
            ImageError::Remote.to_string(),
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
                "20000 × 20000 pixels is too large to load",
            ]
        );
    }
}
