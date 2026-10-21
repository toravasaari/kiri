//! Loading the images a document refers to.
//!
//! This is the stage between the document model and layout: it resolves
//! each image URL, reads the file and decodes it, and has each diagram in
//! a code block drawn as an SVG, so layout and output work on pixels and
//! never touch the file system. A failure is a value, not an
//! error for the whole document, because the renderer still shows the alt
//! text in place of an image it could not load.

mod svg;
mod url;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use image::error::{ImageFormatHint, UnsupportedErrorKind};
use image::{ImageFormat, RgbaImage};
use kiri_image_utils::DecodeError;

use crate::diagrams::{Diagrams, Drawn, Theme};
use crate::model::{Block, Document, Inline};

pub use svg::{Fonts, Svg};
pub use url::load_url;

use svg::{decode_svg, looks_like_svg};
use url::scheme;

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
    /// Pixels in 8-bit RGBA drawn for a display with more than one pixel
    /// to the CSS pixel, as a browser draws a diagram, and shown at `css`,
    /// its width and height in CSS pixels.
    Dense { image: RgbaImage, css: (u32, u32) },
}

impl Source {
    /// Width and height in pixels; for an SVG or a dense image, in CSS
    /// pixels.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Source::Raster(image) => image.dimensions(),
            Source::Svg(svg) => svg.size,
            Source::Dense { css, .. } => *css,
        }
    }

    /// The image drawn `width` × `height` pixels large: a raster image
    /// scaled, an SVG drawn at that size.
    pub fn render(&self, width: u32, height: u32) -> RgbaImage {
        match self {
            Source::Raster(image) | Source::Dense { image, .. }
                if image.dimensions() == (width, height) =>
            {
                image.clone()
            }
            Source::Raster(image) | Source::Dense { image, .. } => scale(image, width, height),
            Source::Svg(svg) => svg.render(width, height),
        }
    }
}

/// `image` scaled to `width` × `height`, in premultiplied alpha so that
/// the colour of transparent pixels does not show as a halo.
fn scale(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let (from_width, from_height) = image.dimensions();
    let bytes =
        kiri_image_utils::scale_rgba8(image.as_raw(), from_width, from_height, width, height);
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
    /// A diagram that could not be drawn: how warnings name it, and why.
    Diagram { name: String, reason: String },
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
            ImageError::Diagram { name, reason } => write!(f, "{name}: {reason}"),
        }
    }
}

impl ImageError {
    /// Whether the message names the file or the diagram, so that it
    /// needs no other name in front.
    pub fn names_file(&self) -> bool {
        matches!(
            self,
            ImageError::Missing(_)
                | ImageError::Unreadable { .. }
                | ImageError::NotAFile(_)
                | ImageError::FileTooLarge { .. }
                | ImageError::Diagram { .. }
        )
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

/// What an image is made from, the key into `Images`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageKey {
    /// An image file or `data:` URL, as the document gives it.
    Url(String),
    /// A code block drawn as an image, such as a diagram: its language and
    /// its source.
    Block { language: String, source: String },
}

impl From<&str> for ImageKey {
    fn from(url: &str) -> Self {
        ImageKey::Url(url.to_owned())
    }
}

impl From<String> for ImageKey {
    fn from(url: String) -> Self {
        ImageKey::Url(url)
    }
}

impl fmt::Display for ImageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageKey::Url(url) => f.write_str(url),
            ImageKey::Block { language, .. } => write!(f, "{language} block"),
        }
    }
}

/// Every image of a document by its key, loaded or not.
#[derive(Debug, Default)]
pub struct Images {
    by_key: HashMap<ImageKey, Result<Source, ImageError>>,
    /// The keys in the order they were first inserted, the document's order.
    order: Vec<ImageKey>,
}

impl Images {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the outcome of loading `key`, replacing an earlier one.
    pub fn insert(&mut self, key: impl Into<ImageKey>, image: Result<Source, ImageError>) {
        let key = key.into();
        if !self.by_key.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.by_key.insert(key, image);
    }

    /// Takes the outcome for `key` out, as for an image shown on its own;
    /// `None` when it was never loaded.
    pub fn remove(&mut self, key: &ImageKey) -> Option<Result<Source, ImageError>> {
        self.order.retain(|loaded| loaded != key);
        self.by_key.remove(key)
    }

    /// The outcome for `key`, or `None` when it was never loaded.
    pub fn get(&self, key: &ImageKey) -> Option<&Result<Source, ImageError>> {
        self.by_key.get(key)
    }

    /// How many distinct keys were loaded.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Every key and its outcome, in the order they were first inserted.
    pub fn iter(&self) -> impl Iterator<Item = (&ImageKey, &Result<Source, ImageError>)> {
        (self.order.iter()).map(|key| (key, &self.by_key[key]))
    }

    /// One message for each image that did not load, in document order.
    pub fn warnings(&self) -> Vec<String> {
        self.warnings_with(true)
    }

    /// `warnings`, with one for each diagram that could not be drawn only
    /// when `render_errors`.
    pub fn warnings_with(&self, render_errors: bool) -> Vec<String> {
        self.iter()
            .filter_map(|(key, image)| match (key, image) {
                (_, Ok(_)) => None,
                (_, Err(ImageError::Diagram { .. })) if !render_errors => None,
                // These name the file, resolved against the document.
                (_, Err(error)) if error.names_file() => Some(error.to_string()),
                (ImageKey::Url(url), Err(ImageError::NoPath)) if url.is_empty() => {
                    Some("an image URL is empty".to_owned())
                }
                // A data: URL is the image itself, too long for a message.
                (ImageKey::Url(url), Err(error))
                    if scheme(url).is_some_and(|s| s.eq_ignore_ascii_case("data")) =>
                {
                    Some(format!("data: URL: {error}"))
                }
                (key, Err(error)) => Some(format!("{key}: {error}")),
            })
            .collect()
    }
}

/// Loads every image that `document` shows as an image block once,
/// resolving relative paths against `base`, the directory of the Markdown
/// file, and draws each diagram `diagrams` can draw once. Files larger
/// than `max_file_size` bytes are refused, and text in SVGs is drawn with
/// the system's fonts.
pub fn load(
    document: &Document,
    base: &Path,
    max_file_size: u64,
    diagrams: &Diagrams<'_>,
) -> Images {
    load_with(document, base, max_file_size, &Fonts::system(), diagrams)
}

/// `load`, drawing text in SVGs with `fonts`.
pub fn load_with(
    document: &Document,
    base: &Path,
    max_file_size: u64,
    fonts: &Fonts,
    diagrams: &Diagrams<'_>,
) -> Images {
    let mut wanted = Vec::new();
    collect_blocks(&document.blocks, diagrams, &mut wanted);
    let mut keys = HashSet::new();
    wanted.retain(|image| keys.insert(image.key()));
    // The diagrams first, all together, so a renderer can draw several
    // at once.
    let sources: Vec<(&str, &str)> = (wanted.iter())
        .filter_map(|image| match *image {
            Wanted::Diagram {
                language, source, ..
            } => Some((language, source)),
            Wanted::Url(_) => None,
        })
        .collect();
    let total = sources.len();
    let done = AtomicUsize::new(0);
    let progress = diagrams.progress;
    let tell = |done| {
        if let Some(progress) = progress {
            progress(done, total);
        }
    };
    if total > 0 {
        tell(0);
    }
    let one_more = || tell(done.fetch_add(1, Ordering::Relaxed) + 1);
    let mut drawn = (diagrams.renderer)
        .render_all(&sources, diagrams.theme, &one_more)
        .into_iter();
    let mut images = Images::new();
    for image in wanted {
        let key = image.key();
        let loaded = match image {
            Wanted::Url(url) => load_url(url, base, max_file_size, fonts),
            Wanted::Diagram {
                language,
                source,
                line,
            } => {
                let drawn = drawn.next().expect("a drawing for each diagram");
                finish_diagram(
                    language,
                    source,
                    line,
                    drawn,
                    max_file_size,
                    fonts,
                    diagrams,
                )
            }
        };
        images.insert(key, loaded);
    }
    images
}

/// An image a document shows as a block.
enum Wanted<'a> {
    Url(&'a str),
    /// A code block drawn as a diagram, and the line it starts on.
    Diagram {
        language: &'a str,
        source: &'a str,
        line: usize,
    },
}

impl Wanted<'_> {
    fn key(&self) -> ImageKey {
        match *self {
            Wanted::Url(url) => ImageKey::from(url),
            Wanted::Diagram {
                language, source, ..
            } => ImageKey::Block {
                language: language.to_owned(),
                source: source.to_owned(),
            },
        }
    }
}

/// The images `blocks` show as image blocks, and the code blocks among
/// them that `diagrams` draws. Other images are only ever shown as alt
/// text, so they are not loaded at all.
fn collect_blocks<'a>(blocks: &'a [Block], diagrams: &Diagrams<'_>, wanted: &mut Vec<Wanted<'a>>) {
    for block in blocks {
        match block {
            Block::Paragraph(content) => {
                wanted.extend(Inline::lone_image(content).map(|image| Wanted::Url(&image.url)));
            }
            Block::BlockQuote(blocks)
            | Block::Alert { blocks, .. }
            | Block::Details { blocks, .. } => collect_blocks(blocks, diagrams, wanted),
            Block::List(list) => {
                for item in &list.items {
                    collect_blocks(&item.blocks, diagrams, wanted);
                }
            }
            Block::CodeBlock {
                language: Some(language),
                code,
                line,
            } if diagrams.renderer.draws(language) => wanted.push(Wanted::Diagram {
                language,
                source: code,
                line: *line,
            }),
            Block::FrontMatter(_)
            | Block::Heading { .. }
            | Block::Table(_)
            | Block::CodeBlock { .. }
            | Block::ThematicBreak
            | Block::Html(_)
            | Block::Note(_) => {}
        }
    }
}

/// The diagram `source` in `language`, which starts on `line`, as
/// `diagrams` drew it, decoded as an SVG.
fn finish_diagram(
    language: &str,
    source: &str,
    line: usize,
    drawn: Result<Drawn, String>,
    max_size: u64,
    fonts: &Fonts,
    diagrams: &Diagrams<'_>,
) -> Result<Source, ImageError> {
    let failed = |reason: String| ImageError::Diagram {
        name: crate::diagrams::name(language, source, line, diagrams.by_line),
        reason,
    };
    let drawn = drawn.map_err(failed)?;
    let image = match looks_like_svg(&drawn.image) {
        true => decode_diagram(&String::from_utf8_lossy(&drawn.image), max_size, fonts),
        false => match decode_raster(&drawn.image, None) {
            Ok(Source::Raster(image)) => Ok(dense(image, drawn.scale, diagrams.largest)),
            other => other,
        },
    }
    .map_err(|error| failed(error.to_string()))?;
    // A diagram that chose colours of its own for the other page would
    // be dark lines and text on a dark page, or light ones on a light page.
    let panel = match (diagrams.theme, drawn.theme) {
        _ if !diagrams.panel => None,
        (Theme::Dark, Theme::Light) => Some(LIGHT_PANEL),
        (Theme::Light, Theme::Dark) => Some(drawn.background.unwrap_or(DARK_PANEL)),
        (Theme::Light, Theme::Light) | (Theme::Dark, Theme::Dark) => None,
    };
    Ok(match (image, panel) {
        (Source::Svg(svg), Some(colour)) => Source::Svg(svg.on_panel(colour, PANEL_PADDING)),
        (Source::Dense { image, css }, Some(colour)) => {
            let per_css = f64::from(image.width()) / f64::from(css.0.max(1));
            let padding = (f64::from(PANEL_PADDING) * per_css).round() as u32;
            let grown = |length: u32| length.saturating_add(2 * PANEL_PADDING as u32);
            Source::Dense {
                image: on_panel(&image, colour, padding),
                css: (grown(css.0), grown(css.1)),
            }
        }
        (image, _) => image,
    })
}

/// `image`, drawn at `per_css` pixels to the CSS pixel, as a dense image
/// of as many CSS pixels; reduced, keeping its shape, when it has more
/// pixels than fit in `largest`, as large as it can be shown, so that a
/// large diagram holds no more memory than showing it needs.
fn dense(image: RgbaImage, per_css: u32, largest: Option<(u32, u32)>) -> Source {
    let (width, height) = image.dimensions();
    let per_css = per_css.max(1);
    let css = (width.div_ceil(per_css), height.div_ceil(per_css));
    let factor = largest.map_or(1.0, |(most_wide, most_high)| {
        let wide = f64::from(most_wide.max(1)) / f64::from(width.max(1));
        let high = f64::from(most_high.max(1)) / f64::from(height.max(1));
        wide.min(high)
    });
    if factor >= 1.0 {
        return Source::Dense { image, css };
    }
    let reduced = |length: u32| ((f64::from(length) * factor).round() as u32).max(1);
    Source::Dense {
        image: scale(&image, reduced(width), reduced(height)),
        css,
    }
}

/// `image` over a panel of `colour`, `padding` pixels wider than it on
/// each side.
fn on_panel(image: &RgbaImage, colour: [u8; 3], padding: u32) -> RgbaImage {
    let [red, green, blue] = colour;
    let (width, height) = image.dimensions();
    let grown = |length: u32| length.saturating_add(padding.saturating_mul(2));
    let mut panel = RgbaImage::from_pixel(
        grown(width),
        grown(height),
        image::Rgba([red, green, blue, 255]),
    );
    for (x, y, pixel) in image.enumerate_pixels() {
        let [r, g, b, alpha] = pixel.0;
        let over = |top: u8, under: u8| {
            let mixed =
                u32::from(top) * u32::from(alpha) + u32::from(under) * (255 - u32::from(alpha));
            ((mixed + 127) / 255) as u8
        };
        let below = panel.get_pixel_mut(x + padding, y + padding);
        below.0 = [over(r, red), over(g, green), over(b, blue), 255];
    }
    panel
}

/// The colour of the panel under a light diagram on a dark page.
const LIGHT_PANEL: [u8; 3] = [255, 255, 255];

/// The colour of the panel under a dark diagram on a light page, when its
/// renderer does not tell its background: that of Mermaid's dark theme.
const DARK_PANEL: [u8; 3] = [0x33, 0x33, 0x33];

/// How far, in CSS pixels, the panel reaches past the diagram on each side,
/// beyond the margin of its own that the diagram has.
const PANEL_PADDING: f32 = 8.0;

/// A diagram's SVG decoded, and grown to show all of its text: its
/// renderer may have measured the text narrower than `fonts` draw it.
pub(crate) fn decode_diagram(
    svg: &str,
    max_size: u64,
    fonts: &Fonts,
) -> Result<Source, ImageError> {
    match decode_svg(svg.as_bytes(), max_size, fonts)? {
        Source::Svg(svg) => Ok(Source::Svg(svg.grown_to_text())),
        other => Ok(other),
    }
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
        Format::Detect => decode(&bytes, max_file_size, fonts),
        Format::Svg => decode_svg(&bytes, max_file_size, fonts),
        Format::Raster(format) => decoded(kiri_image_utils::decode_rgba8_as(
            &bytes, format, MAX_PIXELS, MAX_ALLOC,
        )),
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
fn decode_named(
    bytes: &[u8],
    named: Option<ImageFormat>,
    max_size: u64,
    fonts: &Fonts,
) -> Result<Source, ImageError> {
    if looks_like_svg(bytes) {
        decode_svg(bytes, max_size, fonts)
    } else {
        decode_raster(bytes, named)
    }
}

fn decode_raster(bytes: &[u8], named: Option<ImageFormat>) -> Result<Source, ImageError> {
    // Only TGA has no signature; for other formats a wrong name must not
    // turn data that is no image into a broken image.
    let fallback = named.filter(|&format| format == ImageFormat::Tga);
    decoded(kiri_image_utils::decode_rgba8(
        bytes, fallback, MAX_PIXELS, MAX_ALLOC,
    ))
}

/// The image `kiri_image_utils` decoded, or why it gave none.
fn decoded(result: Result<RgbaImage, DecodeError>) -> Result<Source, ImageError> {
    match result {
        Ok(image) => Ok(Source::Raster(image)),
        Err(DecodeError::UnknownFormat) => Err(ImageError::UnknownFormat),
        Err(DecodeError::TooLarge { width, height }) => Err(ImageError::TooLarge { width, height }),
        Err(DecodeError::Image(error)) => Err(from_image_error(error)),
    }
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
    use crate::diagrams::{Drawn, Renderer};

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
    fn loads_the_images_inside_details() {
        let document = crate::parse::parse(
            "<details>\n<summary>S</summary>\n\n![x](pattern.png)\n\n</details>\n",
        );
        let images = load(
            &document,
            &fixtures().join("images"),
            DEFAULT_MAX_FILE_SIZE,
            &Diagrams::none(),
        );
        assert!(
            matches!(images.get(&"pattern.png".into()), Some(Ok(_))),
            "{document:?}"
        );
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

    /// The fixtures in formats only the full build decodes, and the PNG
    /// each holds without loss.
    const FULL_BUILD_FORMATS: [(&str, &str, &str); 5] = [
        ("pattern.bmp", "BMP", "pattern.png"),
        ("pattern.tif", "TIFF", "pattern.png"),
        ("pattern.qoi", "QOI", "pattern.png"),
        ("pattern.ppm", "PNM", "pattern.png"),
        ("transparent.ico", "ICO", "transparent.png"),
    ];

    #[cfg(feature = "full")]
    #[test]
    fn decodes_the_full_builds_formats_without_loss() {
        for (name, _, png) in FULL_BUILD_FORMATS {
            assert_eq!(raster(name), raster(png), "{name}");
        }
    }

    #[cfg(not(feature = "full"))]
    #[test]
    fn names_the_formats_left_out_of_a_minimal_build() {
        for (name, format, _) in FULL_BUILD_FORMATS {
            let error = ImageError::UnsupportedFormat(format.into());
            assert_eq!(fixture(name), Err(error), "{name}");
        }
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
    fn reads_an_image_file_in_the_format_it_is_given() {
        let read = |name: &str, format| {
            let path = fixtures().join("images").join(name);
            let file = std::fs::File::open(&path).unwrap();
            read_image(file, &path, format, DEFAULT_MAX_FILE_SIZE, &Fonts::none())
        };
        let png = Format::Raster(ImageFormat::Png);
        assert!(matches!(read("icon.png", png), Ok(Source::Raster(_))));
        assert!(matches!(
            read("pattern.svg", Format::Svg),
            Ok(Source::Svg(_))
        ));
        // A JPEG is not a PNG, whatever its bytes tell.
        let result = read("photo-landscape.jpg", png).map(|_| ());
        assert!(matches!(result, Err(ImageError::Corrupt(_))), "{result:?}");
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
            &Diagrams::none(),
        );
        assert_eq!(images.len(), 3);
        assert!(images.get(&"images/pattern.png".into()).unwrap().is_ok());
        assert!(
            images
                .get(&"images/transparent.png".into())
                .unwrap()
                .is_ok()
        );
        assert!(matches!(
            images.get(&"images/missing.png".into()),
            Some(Err(ImageError::Missing(_)))
        ));
        assert!(images.get(&"https://example.com".into()).is_none());
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
            &Diagrams::none(),
        );
        assert!(images.is_empty(), "{images:?}");
    }

    /// Draws Mermaid as an empty SVG one line high for each line of
    /// source, and fails on a source that says `broken`. Remembers what
    /// it was asked to draw.
    #[derive(Default)]
    struct Stub {
        calls: std::cell::RefCell<Vec<(String, Theme)>>,
    }

    impl Renderer for Stub {
        fn draws(&self, language: &str) -> bool {
            language.eq_ignore_ascii_case("mermaid")
        }

        fn render(&self, _language: &str, source: &str, theme: Theme) -> Result<Drawn, String> {
            self.calls.borrow_mut().push((source.to_owned(), theme));
            if source.contains("broken") {
                return Err("parse error".into());
            }
            let height = 20 * source.lines().count();
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="{height}"/>"#
            );
            Ok(Drawn::svg(svg, theme, None))
        }
    }

    fn with_stub(stub: &Stub, theme: Theme, by_line: bool) -> Diagrams<'_> {
        Diagrams {
            renderer: stub,
            theme,
            by_line,
            progress: None,
            panel: true,
            largest: None,
        }
    }

    fn block(language: &str, source: &str) -> ImageKey {
        ImageKey::Block {
            language: language.into(),
            source: source.into(),
        }
    }

    #[test]
    fn draws_each_diagram_once_wherever_a_lone_image_would_show() {
        let source = "\
```mermaid
a
```

- ```mermaid
  b
  ```

> ```Mermaid
> c
> ```

> [!NOTE]
> ```mermaid
> d
> ```

<details>

```mermaid
e
```

</details>

```mermaid
a
```

```rust
fn main() {}
```

| `x` |
|---|
";
        let stub = Stub::default();
        let images = load_with(
            &crate::parse::parse(source),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
            &with_stub(&stub, Theme::Dark, true),
        );
        let calls = stub.calls.into_inner();
        let sources: Vec<&str> = calls.iter().map(|(source, _)| source.as_str()).collect();
        assert_eq!(sources, ["a\n", "b\n", "c\n", "d\n", "e\n"]);
        assert!(calls.iter().all(|(_, theme)| *theme == Theme::Dark));
        assert_eq!(images.len(), 5);
        for key in [block("mermaid", "a\n"), block("Mermaid", "c\n")] {
            match images.get(&key) {
                Some(Ok(Source::Svg(svg))) => assert_eq!(svg.size, (200, 20)),
                other => panic!("{key:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn tells_how_many_diagrams_are_drawn_of_how_many() {
        let source = "```mermaid\na\n```\n\n```mermaid\nb\n```\n\n```mermaid\na\n```\n";
        let told = std::sync::Mutex::new(Vec::new());
        let tell = |done, total| told.lock().unwrap().push((done, total));
        let stub = Stub::default();
        let diagrams = Diagrams {
            progress: Some(&tell),
            ..with_stub(&stub, Theme::Light, true)
        };
        let load = |source: &str| {
            load_with(
                &crate::parse::parse(source),
                &fixtures(),
                DEFAULT_MAX_FILE_SIZE,
                &Fonts::none(),
                &diagrams,
            )
        };
        load(source);
        // The same diagram twice is drawn once.
        assert_eq!(*told.lock().unwrap(), [(0, 2), (1, 2), (2, 2)]);
        told.lock().unwrap().clear();
        load("No diagrams.\n");
        assert_eq!(*told.lock().unwrap(), []);
    }

    #[test]
    fn draws_no_diagrams_without_a_renderer() {
        let images = load_with(
            &crate::parse::parse("```mermaid\na\n```\n"),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
            &Diagrams::none(),
        );
        assert!(images.is_empty(), "{images:?}");
    }

    #[test]
    fn a_diagram_that_fails_is_a_warning_naming_it() {
        let source = "# Plan\n\n```mermaid\nflowchart TD\n  broken\n```\n";
        let warnings = |by_line| {
            let stub = Stub::default();
            let images = load_with(
                &crate::parse::parse(source),
                &fixtures(),
                DEFAULT_MAX_FILE_SIZE,
                &Fonts::none(),
                &with_stub(&stub, Theme::Light, by_line),
            );
            images.warnings()
        };
        assert_eq!(warnings(true), ["mermaid diagram on line 3: parse error"]);
        assert_eq!(
            warnings(false),
            ["mermaid diagram \"flowchart TD\": parse error"]
        );
    }

    #[test]
    fn a_diagram_that_is_no_svg_is_a_warning_naming_it() {
        struct Text;
        impl Renderer for Text {
            fn draws(&self, _language: &str) -> bool {
                true
            }
            fn render(&self, _: &str, _: &str, theme: Theme) -> Result<Drawn, String> {
                Ok(Drawn::svg("not svg", theme, None))
            }
        }
        let images = load_with(
            &crate::parse::parse("```dot\ndigraph {}\n```\n"),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
            &Diagrams {
                renderer: &Text,
                theme: Theme::Light,
                by_line: true,
                progress: None,
                panel: true,
                largest: None,
            },
        );
        let warnings = images.warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("dot diagram on line 1: "),
            "{warnings:?}"
        );
    }

    #[test]
    fn a_diagram_grows_to_show_text_that_reaches_past_its_edges() {
        // A word that starts 20 pixels left of a 100 × 40 drawing.
        const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40">
            <text x="-20" y="30" font-size="20" font-family="sans-serif">Kiri</text></svg>"#;
        struct Overflowing;
        impl Renderer for Overflowing {
            fn draws(&self, _language: &str) -> bool {
                true
            }
            fn render(&self, _: &str, _: &str, theme: Theme) -> Result<Drawn, String> {
                Ok(Drawn::svg(SVG, theme, None))
            }
        }
        let fonts = Fonts::fixture();
        let images = load_with(
            &crate::parse::parse("```dot\ndigraph {}\n```\n"),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &fonts,
            &Diagrams {
                renderer: &Overflowing,
                theme: Theme::Light,
                by_line: true,
                progress: None,
                panel: true,
                largest: None,
            },
        );
        let Some(Ok(Source::Svg(svg))) = images.get(&block("dot", "digraph {}\n")) else {
            panic!("{images:?}");
        };
        let (width, height) = svg.size;
        assert!((118..=120).contains(&width), "{width} pixels wide");
        assert_eq!(height, 40);
        // The K's stem is at the left edge.
        let drawn = svg.render(width, height);
        let mut left_edge = (0..3).flat_map(|x| (0..height).map(move |y| (x, y)));
        assert!(left_edge.any(|(x, y)| rgba(&drawn, x, y)[3] > 0));
        // An SVG file shows only what lies inside its edges, as in a browser.
        match decode(SVG.as_bytes(), DEFAULT_MAX_FILE_SIZE, &fonts) {
            Ok(Source::Svg(file)) => assert_eq!(file.size, (100, 40)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_diagram_for_the_other_page_lies_on_a_panel_of_its_background() {
        /// An empty 100 × 40 drawing, for a page of the theme it holds and
        /// with the background it holds.
        struct Themed(Theme, Option<[u8; 3]>);
        impl Renderer for Themed {
            fn draws(&self, _language: &str) -> bool {
                true
            }
            fn render(&self, _: &str, _: &str, _: Theme) -> Result<Drawn, String> {
                let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40"/>"#;
                Ok(Drawn::svg(svg, self.0, self.1))
            }
        }
        let drawn = |page, own, background, panel| {
            let images = load_with(
                &crate::parse::parse("```dot\ndigraph {}\n```\n"),
                &fixtures(),
                DEFAULT_MAX_FILE_SIZE,
                &Fonts::none(),
                &Diagrams {
                    renderer: &Themed(own, background),
                    theme: page,
                    by_line: true,
                    progress: None,
                    panel,
                    largest: None,
                },
            );
            match images.get(&block("dot", "digraph {}\n")) {
                Some(Ok(Source::Svg(svg))) => svg.clone(),
                other => panic!("{other:?}"),
            }
        };
        let navy = Some([0x12, 0x34, 0x56]);
        // Eight pixels of panel around the drawing on each side: white
        // under a light diagram, whatever its background, and its own
        // background, or Mermaid's dark one, under a dark diagram.
        for (page, own, background, colour) in [
            (Theme::Dark, Theme::Light, None, [255, 255, 255]),
            (Theme::Dark, Theme::Light, navy, [255, 255, 255]),
            (Theme::Light, Theme::Dark, navy, [0x12, 0x34, 0x56]),
            (Theme::Light, Theme::Dark, None, [0x33, 0x33, 0x33]),
        ] {
            let case = format!("{own:?} on {page:?} with {background:?}");
            let panel = drawn(page, own, background, true);
            assert_eq!(panel.size, (116, 56), "{case}");
            let pixels = panel.render(116, 56);
            for (x, y) in [(0, 0), (58, 28), (115, 55)] {
                let [red, green, blue] = colour;
                assert_eq!(rgba(&pixels, x, y), [red, green, blue, 255], "{case}");
            }
        }
        // kiri's own choice of theme, and no panels when they are off.
        for (page, own, panel) in [
            (Theme::Dark, Theme::Dark, true),
            (Theme::Light, Theme::Light, true),
            (Theme::Dark, Theme::Light, false),
            (Theme::Light, Theme::Dark, false),
        ] {
            let case = format!("{own:?} on {page:?}, panel {panel}");
            let svg = drawn(page, own, navy, panel);
            assert_eq!(svg.size, (100, 40), "{case}");
            assert_eq!(rgba(&svg.render(100, 40), 0, 0)[3], 0, "{case}");
        }
    }

    /// `image` as a PNG file.
    fn png(image: &RgbaImage) -> Vec<u8> {
        let mut bytes = io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    /// Draws every diagram as the PNG of a 60 × 30 pixel image, half
    /// transparent red, at its scale, for a page of its theme.
    struct Png(u32, Theme);

    impl Renderer for Png {
        fn draws(&self, _language: &str) -> bool {
            true
        }

        fn render(&self, _: &str, _: &str, _: Theme) -> Result<Drawn, String> {
            let image = RgbaImage::from_pixel(60, 30, image::Rgba([255, 0, 0, 128]));
            Ok(Drawn {
                image: png(&image),
                scale: self.0,
                theme: self.1,
                background: None,
            })
        }
    }

    /// The diagram that `renderer` draws, on a page of `page`.
    fn drawn_by(renderer: &dyn Renderer, page: Theme) -> Result<Source, ImageError> {
        drawn_within(renderer, page, None)
    }

    /// The diagram that `renderer` draws, on a page of `page`, where it is
    /// shown at most `largest` pixels large.
    fn drawn_within(
        renderer: &dyn Renderer,
        page: Theme,
        largest: Option<(u32, u32)>,
    ) -> Result<Source, ImageError> {
        let images = load_with(
            &crate::parse::parse("```dot\ndigraph {}\n```\n"),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
            &Diagrams {
                renderer,
                theme: page,
                by_line: true,
                progress: None,
                panel: true,
                largest,
            },
        );
        (images.get(&block("dot", "digraph {}\n")).cloned()).expect("the diagram")
    }

    #[test]
    fn a_raster_diagram_larger_than_it_can_be_shown_is_reduced_as_it_loads() {
        // 60 × 30 pixels at three to the CSS pixel, 20 × 10 CSS pixels.
        let reduced = |largest, page| {
            let drawn = drawn_within(&Png(3, Theme::Light), page, Some(largest)).unwrap();
            let Source::Dense { image, .. } = &drawn else {
                panic!("{drawn:?}");
            };
            (image.dimensions(), drawn.size(), rgba(image, 0, 0))
        };
        let red = [255, 0, 0, 128];
        // At most 15 pixels wide: a quarter of the pixels each way, and
        // the same CSS pixels.
        assert_eq!(reduced((15, 100), Theme::Light), ((15, 8), (20, 10), red));
        // At most 15 pixels high: half of them.
        assert_eq!(reduced((100, 15), Theme::Light), ((30, 15), (20, 10), red));
        // One that fits stays as drawn.
        assert_eq!(reduced((60, 30), Theme::Light), ((60, 30), (20, 10), red));
        // Reduced before the panel goes under it: its eight CSS pixels are
        // six pixels at three quarters of a pixel to the CSS pixel.
        let white = [255, 255, 255, 255];
        assert_eq!(reduced((15, 100), Theme::Dark), ((27, 20), (36, 26), white));
    }

    #[test]
    fn a_raster_diagram_is_shown_at_the_size_of_its_css_pixels() {
        for scale in [1, 3] {
            let drawn = drawn_by(&Png(scale, Theme::Light), Theme::Light);
            let Ok(Source::Dense { image, css }) = &drawn else {
                panic!("{drawn:?}");
            };
            assert_eq!(
                (image.dimensions(), *css),
                ((60, 30), (60 / scale, 30 / scale))
            );
            let source = drawn.unwrap();
            assert_eq!(source.size(), (60 / scale, 30 / scale));
            assert_eq!(rgba(&source.render(60, 30), 5, 5), [255, 0, 0, 128]);
        }
    }

    #[test]
    fn a_raster_diagram_for_the_other_page_lies_on_a_panel() {
        // Eight CSS pixels of panel on each side, and the drawing over it.
        for scale in [1, 3] {
            let drawn = drawn_by(&Png(scale, Theme::Light), Theme::Dark).unwrap();
            let pad = 8 * scale;
            assert_eq!(drawn.size(), (60 / scale + 16, 30 / scale + 16));
            let Source::Dense { image, .. } = &drawn else {
                panic!("{drawn:?}");
            };
            assert_eq!(image.dimensions(), (60 + 2 * pad, 30 + 2 * pad));
            assert_eq!(rgba(image, 0, 0), [255, 255, 255, 255]);
            assert_eq!(rgba(image, pad + 5, pad + 5), [255, 127, 127, 255]);
            let drawn = drawn_by(&Png(scale, Theme::Dark), Theme::Light).unwrap();
            let Source::Dense { image, .. } = &drawn else {
                panic!("{drawn:?}");
            };
            assert_eq!(rgba(image, 0, 0), [0x33, 0x33, 0x33, 255]);
        }
    }

    #[test]
    fn reads_the_size_of_a_png_from_its_header() {
        let mut png = Vec::new();
        RgbaImage::new(7, 3)
            .write_to(&mut io::Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        assert_eq!(png_size(&png), Some((7, 3)));
        assert_eq!(png_size(&png[..24]), Some((7, 3)));
        assert_eq!(png_size(&png[..23]), None);
        assert_eq!(png_size(b"GIF89a"), None);
        let mut not_ihdr = png.clone();
        not_ihdr[12..16].copy_from_slice(b"IDAT");
        assert_eq!(png_size(&not_ihdr), None);
    }

    #[test]
    fn keeps_the_document_order() {
        let mut images = Images::new();
        for url in ["b.png", "a.png", "c.png"] {
            images.insert(url, Err(ImageError::Remote));
        }
        images.insert("a.png", Err(ImageError::InvalidData));
        let urls: Vec<String> = images.iter().map(|(key, _)| key.to_string()).collect();
        assert_eq!(urls, ["b.png", "a.png", "c.png"]);
        assert_eq!(
            images.get(&"a.png".into()),
            Some(&Err(ImageError::InvalidData))
        );
    }

    #[test]
    fn keeps_a_block_apart_from_a_url_with_the_same_text() {
        let block = ImageKey::Block {
            language: "mermaid".into(),
            source: "a.png".into(),
        };
        let mut images = Images::new();
        images.insert("a.png", Err(ImageError::Remote));
        images.insert(block.clone(), Err(ImageError::UnknownFormat));
        assert_eq!(images.len(), 2);
        assert_eq!(images.get(&block), Some(&Err(ImageError::UnknownFormat)));
        assert_eq!(
            images.warnings(),
            [
                "a.png: remote images are not loaded",
                "mermaid block: not an image format kiri knows",
            ]
        );
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
