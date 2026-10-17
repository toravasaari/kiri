//! Loading the images a document refers to.
//!
//! This is the stage between the document model and layout: it resolves
//! each image URL, reads the file and decodes it, so layout and output work
//! on pixels and never touch the file system. A failure is a value, not an
//! error for the whole document, because the renderer still shows the alt
//! text in place of an image it could not load.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use image::error::{ImageFormatHint, UnsupportedErrorKind};
use image::imageops::{self, FilterType};
use image::metadata::Orientation;
use image::{DynamicImage, ImageBuffer, ImageDecoder, ImageReader, Limits, Rgba, RgbaImage};
use resvg::tiny_skia;
use resvg::usvg::{self, fontdb};

use crate::model::{Block, Document, Inline};

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

/// `image` scaled to `width` × `height`. Transparent pixels still have a
/// colour, often white or black, which a plain resize would blend into the
/// edges around them as a halo. Scaling in premultiplied alpha, where a
/// pixel counts as much as it covers, leaves that colour out.
fn scale(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    const FILTER: FilterType = FilterType::CatmullRom;
    if image.pixels().all(|pixel| pixel[3] == u8::MAX) {
        return imageops::resize(image, width, height, FILTER);
    }
    // Sixteen bits per channel keep the colour of faint pixels.
    let premultiplied: ImageBuffer<Rgba<u16>, Vec<u16>> =
        ImageBuffer::from_fn(image.width(), image.height(), |x, y| {
            let [red, green, blue, alpha] = image.get_pixel(x, y).0;
            let times_alpha = |channel: u8| {
                let product = u32::from(channel) * u32::from(alpha);
                // From 0..=255², rounded to 0..=65535.
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
        // The filter can overshoot, so a channel may exceed its alpha.
        let divided = |channel: u16| {
            let value = (u32::from(channel) * 255 + u32::from(alpha) / 2) / u32::from(alpha);
            value.min(255) as u8
        };
        let alpha = ((u32::from(alpha) + 128) / 257) as u8;
        Rgba([divided(red), divided(green), divided(blue), alpha])
    })
}

/// A parsed SVG document.
#[derive(Clone)]
pub struct Svg {
    tree: Arc<usvg::Tree>,
    /// The file it was parsed from, which tells two SVGs apart.
    bytes: Arc<[u8]>,
    /// The natural size in whole CSS pixels.
    size: (u32, u32),
}

impl Svg {
    fn render(&self, width: u32, height: u32) -> RgbaImage {
        let mut image = RgbaImage::new(width, height);
        let Some(mut pixmap) = tiny_skia::Pixmap::new(width, height) else {
            // Too large for the renderer; left transparent.
            return image;
        };
        let size = self.tree.size();
        let transform = tiny_skia::Transform::from_scale(
            width as f32 / size.width(),
            height as f32 / size.height(),
        );
        resvg::render(&self.tree, transform, &mut pixmap.as_mut());
        // The renderer works in premultiplied alpha, the terminal does not.
        for (pixel, drawn) in image.pixels_mut().zip(pixmap.pixels()) {
            let colour = drawn.demultiply();
            pixel.0 = [colour.red(), colour.green(), colour.blue(), colour.alpha()];
        }
        image
    }
}

impl PartialEq for Svg {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl fmt::Debug for Svg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Svg").field("size", &self.size).finish()
    }
}

/// The fonts that text in SVG images is drawn with. They are loaded when
/// the first SVG needs them, so a document without SVGs never reads the
/// system's fonts.
pub struct Fonts {
    load: fn() -> fontdb::Database,
    database: OnceCell<Arc<fontdb::Database>>,
}

impl Fonts {
    /// The fonts installed on the system.
    pub fn system() -> Self {
        Self::new(|| {
            let mut database = fontdb::Database::new();
            database.load_system_fonts();
            choose_generic_families(&mut database);
            database
        })
    }

    /// No fonts at all: text in SVGs is left out.
    pub fn none() -> Self {
        Self::new(fontdb::Database::new)
    }

    fn new(load: fn() -> fontdb::Database) -> Self {
        Self {
            load,
            database: OnceCell::new(),
        }
    }

    /// Whether an SVG has needed the fonts yet.
    pub fn loaded(&self) -> bool {
        self.database.get().is_some()
    }

    fn database(&self) -> Arc<fontdb::Database> {
        Arc::clone(self.database.get_or_init(|| Arc::new((self.load)())))
    }
}

/// Installed fonts for the generic families, in fontconfig's order of
/// preference and then the usual ones on macOS and Windows.
const SERIF: &[&str] = &[
    "Noto Serif",
    "DejaVu Serif",
    "Liberation Serif",
    "Times New Roman",
    "Nimbus Roman",
    "Times",
];
const SANS_SERIF: &[&str] = &[
    "Noto Sans",
    "DejaVu Sans",
    "Liberation Sans",
    "Verdana",
    "Arial",
    "Nimbus Sans",
    "Helvetica",
];
const MONOSPACE: &[&str] = &[
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Courier New",
    "Nimbus Mono PS",
    "Menlo",
    "Courier",
];

/// Makes `serif`, `sans-serif` and `monospace` name installed fonts. The
/// database takes them from fontconfig's aliases, which may name a font
/// that is not installed, and then SVG text would vanish; a browser always
/// finds one.
fn choose_generic_families(database: &mut fontdb::Database) {
    let installed: HashSet<&str> = (database.faces())
        .flat_map(|face| face.families.iter().map(|(name, _)| name.as_str()))
        .collect();
    let choose = |family, candidates| {
        generic_family(database.family_name(&family), candidates, &installed).map(str::to_owned)
    };
    // Text in another generic family falls back to serif, so serif takes
    // any font at all when none of the usual ones is there.
    let any = installed.iter().min().map(|name| (*name).to_owned());
    let serif = choose(fontdb::Family::Serif, SERIF).or(any);
    let sans_serif = choose(fontdb::Family::SansSerif, SANS_SERIF);
    let monospace = choose(fontdb::Family::Monospace, MONOSPACE);
    if let Some(name) = serif {
        database.set_serif_family(name);
    }
    if let Some(name) = sans_serif {
        database.set_sans_serif_family(name);
    }
    if let Some(name) = monospace {
        database.set_monospace_family(name);
    }
}

/// `configured` when it is installed, otherwise the first installed of
/// `candidates`.
fn generic_family<'a>(
    configured: &'a str,
    candidates: &[&'a str],
    installed: &HashSet<&str>,
) -> Option<&'a str> {
    std::iter::once(configured)
        .chain(candidates.iter().copied())
        .find(|name| installed.contains(name))
}

impl fmt::Debug for Fonts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fonts")
            .field("loaded", &self.loaded())
            .finish()
    }
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
            ImageError::NotAFile(path) => write!(f, "{}: not a regular file", path.display()),
            ImageError::FileTooLarge { path, limit } => {
                write!(
                    f,
                    "{}: larger than the {} limit",
                    path.display(),
                    size(*limit)
                )
            }
            ImageError::DecompressedTooLarge { limit } => {
                write!(f, "decompresses to more than the {} limit", size(*limit))
            }
            ImageError::NoPath => f.write_str("the URL names no file"),
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

/// Loads the image at `url`: a `data:` URL, a local `file:` URL, or a path
/// that is absolute or relative to `base`. Paths may use URL percent
/// escapes and end in a `?query` or `#fragment`, as in a browser. Only
/// regular files of at most `max_file_size` bytes are read.
pub fn load_url(
    url: &str,
    base: &Path,
    max_file_size: u64,
    fonts: &Fonts,
) -> Result<Source, ImageError> {
    match scheme(url) {
        Some(scheme) if scheme.eq_ignore_ascii_case("data") => {
            decode(&data_url(url)?, max_file_size, fonts)
        }
        Some(scheme)
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") =>
        {
            Err(ImageError::Remote)
        }
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
            let path = file_url_path(&url[scheme.len() + 1..]).ok_or(ImageError::Remote)?;
            let bytes = read_file(&local_path(path, base)?, max_file_size)?;
            decode(&bytes, max_file_size, fonts)
        }
        Some(scheme) => Err(ImageError::UnsupportedScheme(scheme.to_ascii_lowercase())),
        None => {
            let bytes = read_file(&local_path(url, base)?, max_file_size)?;
            decode(&bytes, max_file_size, fonts)
        }
    }
}

/// The path of a `file:` URL, given what follows `file:`, or `None` when
/// it names another host. The host may be left out, empty or `localhost`.
/// Without `//` and a host, the path may be relative too: `file:a.png`
/// is resolved against the document, like a plain path.
fn file_url_path(rest: &str) -> Option<&str> {
    let Some(authority_and_path) = rest.strip_prefix("//") else {
        return Some(rest);
    };
    let (host, path) = match authority_and_path.find(['/', '?', '#']) {
        Some(end) => authority_and_path.split_at(end),
        None => (authority_and_path, ""),
    };
    (host.is_empty() || host.eq_ignore_ascii_case("localhost")).then_some(path)
}

/// The file a path in a URL names: without a query or fragment, with
/// percent escapes decoded, and relative to `base` unless it is absolute.
/// Escapes are decoded before the path is split, so `%2F` separates
/// directories like `/` does.
fn local_path(url_path: &str, base: &Path) -> Result<PathBuf, ImageError> {
    let url_path = url_path.split(['?', '#']).next().unwrap_or(url_path);
    if url_path.is_empty() {
        return Err(ImageError::NoPath);
    }
    let decoded = percent_decode(url_path.as_bytes());
    Ok(base.join(String::from_utf8(decoded).as_deref().unwrap_or(url_path)))
}

/// The contents of the regular file at `path`. Directories, devices and
/// pipes are refused before they are opened: reading `/dev/zero` never
/// ends, and opening a pipe waits for a writer. So is a file larger than
/// `limit` bytes, without reading more of it than that.
fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>, ImageError> {
    let unreadable = |error: io::Error| match error.kind() {
        io::ErrorKind::NotFound => ImageError::Missing(path.to_owned()),
        _ => ImageError::Unreadable {
            path: path.to_owned(),
            reason: error.to_string(),
        },
    };
    let metadata = std::fs::metadata(path).map_err(unreadable)?;
    if !metadata.is_file() {
        return Err(ImageError::NotAFile(path.to_owned()));
    }
    let too_large = || ImageError::FileTooLarge {
        path: path.to_owned(),
        limit,
    };
    if metadata.len() > limit {
        return Err(too_large());
    }
    // The file may have grown since.
    let file = std::fs::File::open(path).map_err(unreadable)?;
    read_at_most(file, limit)
        .map_err(unreadable)?
        .ok_or_else(too_large)
}

/// Everything `reader` gives, or `None` when that is more than `limit`
/// bytes; then it reads no further than one byte past the limit.
fn read_at_most(reader: impl Read, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= limit).then_some(bytes))
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
/// A compressed SVG may decompress to at most `max_size` bytes, the limit
/// on a file. Text in SVGs is drawn with `fonts`.
pub fn decode(bytes: &[u8], max_size: u64, fonts: &Fonts) -> Result<Source, ImageError> {
    if looks_like_svg(bytes) {
        decode_svg(bytes, max_size, fonts)
    } else {
        decode_raster(bytes)
    }
}

/// The magic number of gzip, which compresses `.svgz` files.
const GZIP: &[u8] = &[0x1f, 0x8b];

/// SVG has no magic number: it is XML with an `<svg` element, or that
/// compressed with gzip.
fn looks_like_svg(bytes: &[u8]) -> bool {
    let text = bytes.strip_prefix("\u{feff}".as_bytes()).unwrap_or(bytes);
    let text = text.trim_ascii_start();
    bytes.starts_with(GZIP)
        || (text.starts_with(b"<") && text.windows(4).any(|window| window == b"<svg"))
}

fn decode_svg(bytes: &[u8], max_size: u64, fonts: &Fonts) -> Result<Source, ImageError> {
    // usvg would decompress without a limit: a few kilobytes of gzip can
    // grow to gigabytes.
    let plain;
    let text = if bytes.starts_with(GZIP) {
        plain = gunzip(bytes, max_size)?;
        &plain[..]
    } else {
        bytes
    };
    let text = std::str::from_utf8(text)
        .map_err(|_| ImageError::Corrupt(usvg::Error::NotAnUtf8Str.to_string()))?;
    let embedded = usvg::ImageHrefResolver::default_data_resolver();
    let options = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            // Images embedded in data: URLs show, but not compressed SVGs,
            // which nothing would limit; a browser does not show them
            // either.
            resolve_data: Box::new(move |mime, data, options| {
                (!data.starts_with(GZIP))
                    .then(|| embedded(mime, data, options))
                    .flatten()
            }),
            // Like an image in a browser, an SVG loads no files of its own.
            resolve_string: Box::new(|_, _| None),
        },
        fontdb: fonts.database(),
        ..usvg::Options::default()
    };
    // From text, so usvg never decompresses anything itself.
    let tree = usvg::Tree::from_str(text, &options)
        .map_err(|error| ImageError::Corrupt(error.to_string()))?;
    let natural = |length: f32| (length.round() as u32).max(1);
    let size = (natural(tree.size().width()), natural(tree.size().height()));
    Ok(Source::Svg(Svg {
        tree: Arc::new(tree),
        bytes: bytes.into(),
        size,
    }))
}

/// `bytes` decompressed with gzip, if that gives at most `limit` bytes.
fn gunzip(bytes: &[u8], limit: u64) -> Result<Vec<u8>, ImageError> {
    match read_at_most(flate2::read::GzDecoder::new(bytes), limit) {
        Ok(Some(plain)) => Ok(plain),
        Ok(None) => Err(ImageError::DecompressedTooLarge { limit }),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Err(ImageError::Truncated),
        Err(error) => Err(ImageError::Corrupt(format!("gzip: {error}"))),
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

    /// The image at `url` with the default size limit and no fonts.
    fn open(url: &str, base: &Path) -> Result<Source, ImageError> {
        load_url(url, base, DEFAULT_MAX_FILE_SIZE, &Fonts::none())
    }

    fn fixture(name: &str) -> Result<Source, ImageError> {
        open(name, &fixtures().join("images"))
    }

    fn raster(name: &str) -> RgbaImage {
        match fixture(name) {
            Ok(Source::Raster(image)) => image,
            Ok(other) => panic!("{name} is not a raster image: {other:?}"),
            Err(error) => panic!("{name} does not load: {error:?}"),
        }
    }

    /// `bytes` decoded, which must give an SVG.
    fn svg(bytes: &[u8], fonts: &Fonts) -> Source {
        match decode(bytes, DEFAULT_MAX_FILE_SIZE, fonts) {
            Ok(source @ Source::Svg(_)) => source,
            other => panic!("not an SVG: {other:?}"),
        }
    }

    fn svg_fixture(name: &str, fonts: &Fonts) -> Source {
        svg(
            &std::fs::read(fixtures().join("images").join(name)).unwrap(),
            fonts,
        )
    }

    /// How many pixels differ between two images of the same size.
    fn differing_pixels(a: &RgbaImage, b: &RgbaImage) -> usize {
        assert_eq!(a.dimensions(), b.dimensions());
        a.pixels().zip(b.pixels()).filter(|(x, y)| x != y).count()
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
    fn decodes_svg_at_its_natural_size_in_css_pixels() {
        let none = Fonts::none();
        let size = |name| svg_fixture(name, &none).size();
        assert_eq!(size("pattern.svg"), (320, 240));
        assert_eq!(size("airfoil-forces.svg"), (791, 333));
        // Millimetres at 96 CSS pixels to the inch, rounded.
        assert_eq!(size("sunset-icon.svg"), (358, 210));
        assert_eq!(size("soccer-ball.svg"), (114, 110));
        assert_eq!(size("planetary-gearset.svg"), (4000, 3000));
        // Without a width and height, the viewBox gives the size.
        assert_eq!(size("speedway.svg"), (3000, 2013));
        assert_eq!(size("silhouette.svg"), (2689, 1560));
    }

    #[test]
    fn draws_an_svg_at_the_size_it_is_shown_with_sharp_edges() {
        let pattern = svg_fixture("pattern.svg", &Fonts::none());
        let large = pattern.render(640, 480);
        assert_eq!(large.dimensions(), (640, 480));
        assert_eq!(rgba(&large, 319, 239), [255, 0, 0, 255]);
        assert_eq!(rgba(&large, 320, 239), [0, 255, 0, 255]);
        assert_eq!(rgba(&large, 319, 240), [0, 0, 255, 255]);
        assert_eq!(rgba(&large, 320, 240), [255, 255, 0, 255]);
        let small = pattern.render(32, 24);
        assert_eq!(rgba(&small, 15, 11), [255, 0, 0, 255]);
        assert_eq!(rgba(&small, 16, 12), [255, 255, 0, 255]);
    }

    #[test]
    fn draws_svg_transparency_unpremultiplied() {
        let half = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
            <rect width="4" height="4" fill="#0000ff" fill-opacity="0.5"/></svg>"##;
        let pixel = rgba(&svg(half, &Fonts::none()).render(4, 4), 1, 1);
        assert_eq!(pixel[..3], [0, 0, 255]);
        assert!((126..=129).contains(&pixel[3]), "{pixel:?}");
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
            for (x, y, pixel) in scaled.enumerate_pixels() {
                let [red, green, blue, alpha] = pixel.0;
                let [red_0, green_0, blue_0, alpha_0] = rgba(&expected, x, y);
                // Colour weighed by alpha: a faint pixel may change hue
                // without anyone seeing it.
                let shown = |channel: u8, alpha: u8| u32::from(channel) * u32::from(alpha) / 255;
                let close = |a: u8, b: u8| shown(a, alpha).abs_diff(shown(b, alpha_0)) <= 2;
                assert!(
                    close(red, red_0)
                        && close(green, green_0)
                        && close(blue, blue_0)
                        && alpha.abs_diff(alpha_0) <= 2,
                    "{name} at {width} × {height}: ({x}, {y}) is {:?}, not {:?}",
                    pixel.0,
                    [red_0, green_0, blue_0, alpha_0],
                );
            }
        }
    }

    /// How kiri scaled images with the image crate's resize: in
    /// premultiplied alpha with 16 bits per channel, unless the image is
    /// opaque.
    fn scale_with_the_image_crate(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
        use image::imageops::{self, FilterType};
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
    fn decodes_svg_compressed_with_a_bom_or_with_an_xml_prolog() {
        let plain = std::fs::read(fixtures().join("images/pattern.svg")).unwrap();
        let text = String::from_utf8(plain.clone()).unwrap();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        io::Write::write_all(&mut gzip, &plain).unwrap();
        let variants = [
            ("gzip", gzip.finish().unwrap()),
            ("BOM", format!("\u{feff}{text}").into_bytes()),
            (
                "prolog",
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n\
                     <!-- A comment -->\n\
                     <!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \
                     \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n{text}"
                )
                .into_bytes(),
            ),
            (
                "BOM and prolog",
                format!("\u{feff}<?xml version=\"1.0\"?>\n{text}").into_bytes(),
            ),
        ];
        let expected = svg(&plain, &Fonts::none()).render(32, 24);
        for (name, bytes) in variants {
            let image = svg(&bytes, &Fonts::none());
            assert_eq!(image.size(), (320, 240), "{name}");
            assert_eq!(image.render(32, 24), expected, "{name}");
        }
        // A .svgz file on disk.
        let svgz = svg_fixture("pattern.svgz", &Fonts::none());
        assert_eq!(svgz.render(32, 24), expected);
    }

    /// `svg` compressed with gzip.
    fn gzip(svg: &[u8]) -> Vec<u8> {
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        io::Write::write_all(&mut gzip, svg).unwrap();
        gzip.finish().unwrap()
    }

    #[test]
    fn refuses_a_compressed_svg_that_decompresses_past_the_limit() {
        // Just over the default limit of spaces, which compress to a few
        // kilobytes.
        let mut large =
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1">"#.to_vec();
        large.resize(DEFAULT_MAX_FILE_SIZE as usize, b' ');
        large.extend_from_slice(b"</svg>");
        let compressed = gzip(&large);
        assert!(compressed.len() < 64 * 1024, "{}", compressed.len());
        let fonts = Fonts::none();
        assert_eq!(
            decode(&compressed, DEFAULT_MAX_FILE_SIZE, &fonts),
            Err(ImageError::DecompressedTooLarge {
                limit: DEFAULT_MAX_FILE_SIZE
            })
        );
        // At the limit, it still loads.
        let size = large.len() as u64;
        assert_eq!(
            decode(&compressed, size, &fonts).map(|svg| svg.size()),
            Ok((2, 1))
        );
        assert_eq!(
            decode(&compressed[..compressed.len() / 2], size, &fonts),
            Err(ImageError::Truncated)
        );
        // The limit on files applies to data: URLs too.
        let url = format!("data:image/svg+xml;base64,{}", STANDARD.encode(&compressed));
        assert_eq!(
            load_url(&url, Path::new("/"), size - 1, &fonts),
            Err(ImageError::DecompressedTooLarge { limit: size - 1 })
        );
    }

    #[test]
    fn does_not_draw_a_compressed_svg_inside_an_svg() {
        // Nothing would limit how large it grows; a browser does not show
        // it either.
        let red = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
            <rect width="4" height="4" fill="#ff0000"/></svg>"##;
        let nested = |bytes: &[u8]| {
            let outer = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
                <image width="4" height="4" href="data:image/svg+xml;base64,{}"/></svg>"#,
                STANDARD.encode(bytes)
            );
            svg(outer.as_bytes(), &Fonts::none()).render(4, 4)
        };
        assert_eq!(rgba(&nested(red), 2, 2), [255, 0, 0, 255]);
        assert_eq!(rgba(&nested(&gzip(red)), 2, 2)[3], 0);
    }

    #[test]
    fn reports_a_broken_svg() {
        assert!(
            matches!(fixture("truncated.svg"), Err(ImageError::Corrupt(_))),
            "{:?}",
            fixture("truncated.svg")
        );
    }

    #[test]
    fn loads_svg_data_urls() {
        let bytes = std::fs::read(fixtures().join("images/pattern.svg")).unwrap();
        let url = format!("data:image/svg+xml;base64,{}", STANDARD.encode(&bytes));
        let loaded = open(&url, Path::new("/"));
        assert_eq!(loaded, fixture("pattern.svg"));
        assert!(loaded.is_ok());
        let inline = "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' \
            width='2' height='1'/>";
        let loaded = open(inline, Path::new("/"));
        assert_eq!(loaded.map(|source| source.size()), Ok((2, 1)));
    }

    #[test]
    fn loads_fonts_only_for_a_document_with_an_svg() {
        let fonts = Fonts::none();
        let without = "![a](images/pattern.png)\n\n![b](images/missing.svg)\n\n\
            ![c](images/not-an-image.png)\n";
        load_with(
            &crate::parse::parse(without),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &fonts,
        );
        assert!(!fonts.loaded());
        let with = "![d](images/pattern.svg)\n";
        load_with(
            &crate::parse::parse(with),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &fonts,
        );
        assert!(fonts.loaded());
    }

    #[test]
    fn leaves_out_svg_text_without_fonts() {
        let text = br##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="30">
            <text x="5" y="25" font-size="24" font-family="sans-serif">Kiri</text></svg>"##;
        let image = svg(text, &Fonts::none()).render(80, 30);
        assert!(image.pixels().all(|pixel| pixel.0[3] == 0));
    }

    /// DejaVu Sans from the fixtures and nothing else, so text in SVGs
    /// looks the same on every machine.
    fn fixture_fonts() -> Fonts {
        Fonts::new(|| {
            let mut database = fontdb::Database::new();
            let font =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fonts/DejaVuSans.ttf");
            database
                .load_font_file(font)
                .expect("the fixture font loads");
            choose_generic_families(&mut database);
            database
        })
    }

    #[test]
    fn draws_svg_text_with_the_fonts_it_is_given() {
        let fonts = fixture_fonts();
        // Greek words in sans-serif, set by a style sheet, and English ones
        // in the default font.
        for (name, width, height) in [
            ("airfoil-forces.svg", 791, 333),
            ("planetary-gearset.svg", 800, 600),
        ] {
            let without = svg_fixture(name, &Fonts::none()).render(width, height);
            let with = svg_fixture(name, &fonts).render(width, height);
            let text = differing_pixels(&without, &with);
            assert!(text > 1000, "{name}: {text} pixels of text");
        }
    }

    #[test]
    fn generic_families_take_the_first_installed_font() {
        let installed = HashSet::from(["DejaVu Sans", "Arial", "Noto Serif"]);
        assert_eq!(
            generic_family("Noto Serif", SERIF, &installed),
            Some("Noto Serif")
        );
        assert_eq!(
            generic_family("FreeSans", SANS_SERIF, &installed),
            Some("DejaVu Sans")
        );
        assert_eq!(generic_family("FreeMono", MONOSPACE, &installed), None);
    }

    #[test]
    fn does_not_load_files_an_svg_refers_to() {
        // An image in a browser does not load anything further either.
        let path = fixtures().join("images/pattern.png");
        let linked = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24">
            <image width="32" height="24" href="{}"/></svg>"#,
            path.display()
        );
        let image = svg(linked.as_bytes(), &Fonts::none()).render(32, 24);
        assert!(image.pixels().all(|pixel| pixel.0[3] == 0));
    }

    #[test]
    fn draws_images_embedded_in_an_svg() {
        let png = std::fs::read(fixtures().join("images/pattern.png")).unwrap();
        let embedded = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24">
            <image width="32" height="24" href="data:image/png;base64,{}"/></svg>"#,
            STANDARD.encode(&png)
        );
        let image = svg(embedded.as_bytes(), &Fonts::none()).render(32, 24);
        assert_eq!(rgba(&image, 8, 6), [255, 0, 0, 255]);
    }

    #[test]
    fn loads_base64_data_urls() {
        let bytes = std::fs::read(fixtures().join("images/pattern.png")).unwrap();
        let url = format!("data:image/png;base64,{}", STANDARD.encode(&bytes));
        let loaded = open(&url, Path::new("/nonexistent"));
        assert_eq!(loaded, fixture("pattern.png"));
        assert!(loaded.is_ok());
    }

    #[test]
    fn loads_percent_encoded_data_urls() {
        let bytes = std::fs::read(fixtures().join("images/pattern.png")).unwrap();
        let escaped: String = bytes.iter().map(|byte| format!("%{byte:02X}")).collect();
        let loaded = open(&format!("data:image/png,{escaped}"), Path::new("/"));
        assert_eq!(loaded, fixture("pattern.png"));
        assert!(loaded.is_ok());
    }

    #[test]
    fn rejects_broken_data_urls() {
        let base = Path::new("/");
        assert_eq!(
            open("data:image/png;base64,no*base64", base),
            Err(ImageError::InvalidData)
        );
        assert_eq!(
            open("data:image/png;base64", base),
            Err(ImageError::InvalidData)
        );
    }

    #[test]
    fn resolves_paths_against_the_base_and_keeps_absolute_ones() {
        let base = fixtures();
        assert!(open("images/pattern.png", &base).is_ok());
        let absolute = base.join("images/pattern.png");
        assert!(open(absolute.to_str().unwrap(), Path::new("/nonexistent")).is_ok());
    }

    #[test]
    fn decodes_percent_escapes_in_paths() {
        assert!(fixture("pattern%2Epng").is_ok());
        // An escaped slash separates directories too.
        assert!(fixture("pngsuite%2Fbasn0g16.png").is_ok());
    }

    #[test]
    fn leaves_the_query_and_fragment_out_of_paths() {
        let pattern = fixture("pattern.png");
        assert!(pattern.is_ok());
        for url in ["pattern.png?v=2", "pattern.png#top", "pattern.png?a=1#b?c"] {
            assert_eq!(fixture(url), pattern, "{url}");
        }
        let base = fixtures().join("images");
        assert_eq!(
            fixture("no-such.png?v=2"),
            Err(ImageError::Missing(base.join("no-such.png")))
        );
        // Escaped, they are part of the name, as in a browser.
        assert_eq!(
            fixture("pattern%3Fv=2.png"),
            Err(ImageError::Missing(base.join("pattern?v=2.png")))
        );
        assert_eq!(
            fixture("pattern%23top.png"),
            Err(ImageError::Missing(base.join("pattern#top.png")))
        );
    }

    #[test]
    fn loads_file_urls_as_local_files() {
        let pattern = fixture("pattern.png");
        assert!(pattern.is_ok());
        let path = fixtures().join("images/pattern.png");
        let path = path.to_str().unwrap();
        let elsewhere = Path::new("/nonexistent");
        for url in [
            format!("file://{path}"),
            format!("file://localhost{path}"),
            format!("FILE://LocalHost{path}"),
            // RFC 8089 allows leaving out the empty host.
            format!("file:{path}"),
            format!("file://{path}?v=2#top"),
            format!("file://{}", path.replace(".png", "%2Epng")),
        ] {
            assert_eq!(open(&url, elsewhere), pattern, "{url}");
        }
        assert_eq!(
            open("file:///no/such.png", elsewhere),
            Err(ImageError::Missing("/no/such.png".into()))
        );
    }

    #[test]
    fn reports_a_url_without_a_path() {
        for base in [Path::new(""), &fixtures()] {
            for url in [
                "file://",
                "file://?x",
                "file://#top",
                "file:",
                "file://localhost",
                "",
                "?v=2",
                "#top",
            ] {
                assert_eq!(
                    open(url, base),
                    Err(ImageError::NoPath),
                    "{url:?} in {base:?}"
                );
            }
        }
    }

    #[test]
    fn does_not_load_file_urls_from_other_hosts() {
        assert_eq!(
            open("file://server/share/a.png", Path::new("/")),
            Err(ImageError::Remote)
        );
    }

    #[test]
    fn reports_a_missing_file_with_its_resolved_path() {
        let base = fixtures().join("images");
        assert_eq!(
            open("no-such.png", &base),
            Err(ImageError::Missing(base.join("no-such.png")))
        );
    }

    #[test]
    fn reads_only_regular_files() {
        let images = fixtures().join("images");
        assert_eq!(
            fixture("pngsuite"),
            Err(ImageError::NotAFile(images.join("pngsuite")))
        );
        // A device that never ends.
        assert_eq!(
            open("/dev/zero", Path::new("/")),
            Err(ImageError::NotAFile("/dev/zero".into()))
        );
    }

    #[cfg(unix)]
    #[test]
    fn does_not_wait_on_a_named_pipe() {
        let dir = std::env::temp_dir().join(format!("kiri-test-fifo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("image.png");
        let _ = std::fs::remove_file(&fifo);
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        // Opening a pipe that nobody writes to would block.
        let loaded = open("image.png", &dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(loaded, Err(ImageError::NotAFile(fifo)));
    }

    #[test]
    fn refuses_a_file_above_the_size_limit() {
        let base = fixtures().join("images");
        let size = std::fs::metadata(base.join("pattern.png")).unwrap().len();
        let fonts = Fonts::none();
        assert!(load_url("pattern.png", &base, size, &fonts).is_ok());
        assert_eq!(
            load_url("pattern.png", &base, size - 1, &fonts),
            Err(ImageError::FileTooLarge {
                path: base.join("pattern.png"),
                limit: size - 1
            })
        );
    }

    #[test]
    fn reads_no_more_than_one_byte_past_the_limit() {
        assert_eq!(
            read_at_most(&b"12345"[..], 5).unwrap(),
            Some(b"12345".to_vec())
        );
        assert_eq!(read_at_most(&b"123456"[..], 5).unwrap(), None);
        // An endless stream ends at the limit.
        let mut counted = Counted(io::repeat(0), 0);
        assert_eq!(read_at_most(&mut counted, 1000).unwrap(), None);
        assert_eq!(counted.1, 1001);
    }

    /// A reader that counts the bytes read through it.
    struct Counted<R>(R, u64);

    impl<R: Read> Read for Counted<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.0.read(buf)?;
            self.1 += n as u64;
            Ok(n)
        }
    }

    #[test]
    fn does_not_fetch_remote_images() {
        let base = fixtures();
        assert_eq!(
            open("https://example.com/a.png", &base),
            Err(ImageError::Remote)
        );
        assert_eq!(
            open("HTTP://example.com/a.png", &base),
            Err(ImageError::Remote)
        );
        assert_eq!(
            open("ftp://example.com/a.png", &base),
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
            decode(farbfeld, DEFAULT_MAX_FILE_SIZE, &Fonts::none()),
            Err(ImageError::UnsupportedFormat("Farbfeld".into()))
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
                "big.png: larger than the 10 MB limit",
                "big.png: larger than the 1500-byte limit",
                "decompresses to more than the 10 MB limit",
                "20000 × 20000 pixels is too large to load",
            ]
        );
    }
}
