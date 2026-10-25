//! Loading the images a document refers to.
//!
//! This is the stage between the document model and layout: it resolves
//! each image URL, reads the file and decodes it, and has each diagram in
//! a code block drawn as an image, SVG or raster, so layout and output
//! work on pixels and never touch the file system. A failure is a value,
//! not an error for the whole document, because the renderer still shows
//! the alt text in place of an image it could not load.

mod background;
mod decode;
mod drawn;
mod pipeline;
mod stamps;
mod svg;
mod url;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::diagrams::{NoRenderer, Renderer, Request, Theme};
use crate::model::{Block, Document, Inline};

#[cfg(test)]
pub(crate) use background::Arrival;
pub use background::{Loads, STOPPING};
pub use decode::{DEFAULT_MAX_FILE_SIZE, Format, MB, Source, decode, read_image};
pub use pipeline::{
    Interrupted, Loaded, Loading, load_documents, load_in_background, reload_in_background,
};
pub use stamps::Stamps;
pub use svg::{Fonts, Svg};

pub(crate) use decode::{MAX_PIXELS, png_size};
#[cfg(all(test, feature = "mermaid"))]
pub(crate) use drawn::decode_diagram;
pub(crate) use url::read_at_most;

use decode::decode_named;
use drawn::finish_diagram;
use url::{load_url, scheme};

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
pub(crate) fn size(bytes: u64) -> String {
    if bytes >= MB && bytes.is_multiple_of(MB) {
        format!("{} MB", bytes / MB)
    } else {
        format!("{bytes}-byte")
    }
}

/// What an image is made from, the key into `Images`. Each key also
/// names the document it belongs to, by its number among the documents
/// shown together, counting from 0: the same URL in two documents can
/// name two files, each relative to its own document's directory.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageKey {
    /// An image file or `data:` URL, as the document gives it.
    Url { document: usize, url: String },
    /// A code block drawn as an image, such as a diagram: its language, in
    /// lower case as code blocks match it in any case, and its source.
    Block {
        document: usize,
        language: String,
        source: String,
    },
}

impl ImageKey {
    /// The key of the code block `source` in `language`, drawn as an
    /// image, in the first document: the same for a language in any case.
    pub fn block(language: &str, source: &str) -> Self {
        ImageKey::Block {
            document: 0,
            language: language.to_ascii_lowercase(),
            source: source.to_owned(),
        }
    }

    /// The same image in the document numbered `document`.
    pub fn in_document(self, document: usize) -> Self {
        match self {
            ImageKey::Url { url, .. } => ImageKey::Url { document, url },
            ImageKey::Block {
                language, source, ..
            } => ImageKey::Block {
                document,
                language,
                source,
            },
        }
    }

    /// The number of the document the image belongs to.
    pub fn document(&self) -> usize {
        match *self {
            ImageKey::Url { document, .. } | ImageKey::Block { document, .. } => document,
        }
    }
}

impl From<&str> for ImageKey {
    /// The key of the image at `url` in the first document.
    fn from(url: &str) -> Self {
        ImageKey::from(url.to_owned())
    }
}

impl From<String> for ImageKey {
    /// The key of the image at `url` in the first document.
    fn from(url: String) -> Self {
        ImageKey::Url { document: 0, url }
    }
}

impl fmt::Display for ImageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageKey::Url { url, .. } => f.write_str(url),
            ImageKey::Block { language, .. } => write!(f, "{language} block"),
        }
    }
}

/// How the images of a document are loaded, and its diagrams drawn.
#[derive(Clone, Copy)]
pub struct LoadOptions<'a> {
    /// What draws the diagrams; by default nothing, so that every code
    /// block stays code.
    pub renderer: &'a dyn Renderer,
    /// The page the diagrams are drawn for, unless they choose their own
    /// colours; by default a light one.
    pub theme: Theme,
    /// Whether a warning names a diagram by its line in the document, or
    /// by its own first line, for a document that has no lines the reader
    /// can look up, such as one piped in; by default by its line.
    pub by_line: bool,
    /// Told how many of a document's diagrams are drawn, and of how many:
    /// first none, and then each time one more is.
    pub progress: Option<&'a (dyn Fn(usize, usize) + Sync)>,
    /// Whether a diagram whose colours are for the other page, light or
    /// dark, lies on a panel of its own background; by default it does.
    pub panel: bool,
    /// The most pixels, wide and high, an image can be shown at: a
    /// diagram drawn in pixels is reduced to fit them as it loads. `None`,
    /// the default, keeps it as drawn.
    pub largest: Option<(u32, u32)>,
}

impl Default for LoadOptions<'_> {
    fn default() -> Self {
        LoadOptions {
            renderer: &NoRenderer,
            theme: Theme::Light,
            by_line: true,
            progress: None,
            panel: true,
            largest: None,
        }
    }
}

/// Every image of a document by its key, loaded or not.
#[derive(Debug, Default)]
pub struct Images {
    by_key: HashMap<ImageKey, Result<Source, ImageError>>,
    /// The keys in the order they were first inserted, the document's order.
    order: Vec<ImageKey>,
    /// The images of the document before a reload in the background that
    /// the new one no longer shows, or that are loading anew, until they
    /// are taken: what was there while the new ones load.
    replaced: HashMap<ImageKey, Source>,
    /// The images loading in the background, each with what stands in
    /// for it meanwhile.
    loading: HashMap<ImageKey, Placeholder>,
    /// The images whose outcome came from loading in the background, which
    /// nothing has told about yet.
    late: HashSet<ImageKey>,
    /// The keys of the images the documents show, in their order, as a
    /// load in the background last found them.
    shown: Vec<ImageKey>,
}

/// What a block shows in place of its image while the image loads.
#[derive(Debug, Clone)]
pub enum Placeholder {
    /// The image the block showed before, dimmed.
    Dimmed(Source),
    /// Nothing that tells a size: the block shows as it does without its
    /// image.
    Unsized,
}

impl Images {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the outcome of loading `key`, replacing an earlier one,
    /// and what stood in for it while it loaded.
    pub fn insert(&mut self, key: impl Into<ImageKey>, image: Result<Source, ImageError>) {
        let key = key.into();
        self.loading.remove(&key);
        self.late.remove(&key);
        if !self.by_key.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.by_key.insert(key, image);
    }

    /// Takes the outcome for `key` out, as for an image shown on its own;
    /// `None` when it was never loaded.
    pub fn remove(&mut self, key: &ImageKey) -> Option<Result<Source, ImageError>> {
        self.order.retain(|loaded| loaded != key);
        self.late.remove(key);
        self.by_key.remove(key)
    }

    /// Takes in every outcome of `other`, after those it holds.
    pub fn extend(&mut self, mut other: Images) {
        for key in std::mem::take(&mut other.order) {
            if let Some(image) = other.by_key.remove(&key) {
                self.insert(key, image);
            }
        }
    }

    /// Records the outcome of loading `key` in the background, as
    /// [`Images::insert`] does; [`Images::late_warnings`] tells it if it
    /// failed.
    pub fn came(&mut self, key: ImageKey, image: Result<Source, ImageError>) {
        self.insert(key.clone(), image);
        self.late.insert(key);
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

    /// Takes `key` as loading, with `placeholder` in its place until it
    /// comes.
    pub fn wait(&mut self, key: ImageKey, placeholder: Placeholder) {
        self.loading.insert(key, placeholder);
    }

    /// What stands in for `key` while it loads; `None` when it is not
    /// loading.
    pub fn placeholder(&self, key: &ImageKey) -> Option<&Placeholder> {
        self.loading.get(key)
    }

    /// Whether any image is loading.
    pub fn loading(&self) -> bool {
        !self.loading.is_empty()
    }

    /// Takes the placeholders of the images loading, which are then no
    /// longer loading as far as `self` tells.
    pub fn take_placeholders(&mut self) -> HashMap<ImageKey, Placeholder> {
        std::mem::take(&mut self.loading)
    }

    /// Sets the image `key` aside, as [`reload_in_background`] does with
    /// one that is no longer shown or that loads anew.
    #[cfg(test)]
    pub(crate) fn set_aside(&mut self, key: &ImageKey) {
        if let Some(Ok(image)) = self.remove(key) {
            self.replaced.insert(key.clone(), image);
        }
    }

    /// Takes the images [`reload_in_background`] replaced.
    pub fn take_replaced(&mut self) -> HashMap<ImageKey, Source> {
        std::mem::take(&mut self.replaced)
    }

    /// One message for each image that did not load, in document order.
    pub fn warnings(&self) -> Vec<String> {
        self.warnings_with(true)
    }

    /// `warnings`, with one for each diagram that could not be drawn only
    /// when `render_errors`.
    pub fn warnings_with(&self, render_errors: bool) -> Vec<String> {
        warnings(self.iter(), render_errors)
    }

    /// `warnings_with` for the images that came from loading in the
    /// background and are still shown, in the documents' order, each with
    /// the number of its document.
    pub fn late_warnings(&self, render_errors: bool) -> Vec<(usize, String)> {
        let mut told = HashSet::new();
        (self.shown.iter())
            .filter(|key| self.late.contains(*key) && told.insert(*key))
            .filter_map(|key| Some((key, self.by_key.get(key)?)))
            .flat_map(|(key, image)| {
                let warnings = warnings(std::iter::once((key, image)), render_errors);
                warnings
                    .into_iter()
                    .map(|warning| (key.document(), warning))
            })
            .collect()
    }

    /// `warnings_with` for the images of the document numbered
    /// `document` alone.
    pub fn warnings_in(&self, document: usize, render_errors: bool) -> Vec<String> {
        let images = self.iter().filter(|(key, _)| key.document() == document);
        warnings(images, render_errors)
    }
}

/// The warnings about `images`, as `Images::warnings_with` makes them.
fn warnings<'a>(
    images: impl Iterator<Item = (&'a ImageKey, &'a Result<Source, ImageError>)>,
    render_errors: bool,
) -> Vec<String> {
    images
        .filter_map(|(key, image)| match (key, image) {
            (_, Ok(_)) => None,
            (_, Err(ImageError::Diagram { .. })) if !render_errors => None,
            // These name the file, resolved against the document.
            (_, Err(error)) if error.names_file() => Some(error.to_string()),
            (ImageKey::Url { url, .. }, Err(ImageError::NoPath)) if url.is_empty() => {
                Some("an image URL is empty".to_owned())
            }
            // A data: URL is the image itself, too long for a message.
            (ImageKey::Url { url, .. }, Err(error))
                if scheme(url).is_some_and(|s| s.eq_ignore_ascii_case("data")) =>
            {
                Some(format!("data: URL: {error}"))
            }
            (key, Err(error)) => Some(format!("{key}: {error}")),
        })
        .collect()
}

/// Loads every image that `document` shows as an image block once,
/// resolving relative paths against `base`, the directory of the Markdown
/// file, and draws each diagram that `options` can draw once. Files larger
/// than `max_file_size` bytes are refused, and text in SVGs is drawn with
/// the system's fonts.
pub fn load(
    document: &Document,
    base: &Path,
    max_file_size: u64,
    options: &LoadOptions<'_>,
) -> Images {
    load_with(document, base, max_file_size, &Fonts::system(), options)
}

/// `load`, drawing text in SVGs with `fonts`.
pub fn load_with(
    document: &Document,
    base: &Path,
    max_file_size: u64,
    fonts: &Fonts,
    options: &LoadOptions<'_>,
) -> Images {
    let part = Part {
        document,
        number: 0,
        base,
        by_line: options.by_line,
    };
    load_parts(&[part], max_file_size, fonts, options)
}

/// One of several documents shown together, whose images are loaded with
/// the others'.
#[derive(Clone, Copy)]
pub struct Part<'a> {
    pub document: &'a Document,
    /// Its number among the documents shown together, which the keys of
    /// its images carry.
    pub number: usize,
    /// The directory that its relative image paths start from.
    pub base: &'a Path,
    /// Whether a warning names a diagram by its line in the document, or
    /// by its own first line, as `LoadOptions::by_line` says.
    pub by_line: bool,
}

/// `load_with` for each of `parts`, into one map, with the diagrams of
/// all of them drawn together. `options.by_line` gives way to each part's
/// own.
pub fn load_parts(
    parts: &[Part<'_>],
    max_file_size: u64,
    fonts: &Fonts,
    options: &LoadOptions<'_>,
) -> Images {
    let (wanted, mut have) = plan(parts, options.renderer, None);
    let key = |&(index, ref image): &(usize, Wanted)| image.key().in_document(parts[index].number);
    // The diagrams first, all together, so a renderer can draw several
    // at once.
    let requests: Vec<Request> = (wanted.iter().zip(&have))
        .filter_map(|((_, image), have)| match *image {
            Wanted::Diagram {
                language, source, ..
            } if have.is_none() => Some(Request::new(language, source, options.theme)),
            Wanted::Diagram { .. } | Wanted::Url(_) => None,
        })
        .collect();
    let total = requests.len();
    let done = AtomicUsize::new(0);
    let progress = options.progress;
    let tell = |done| {
        if let Some(progress) = progress {
            progress(done, total);
        }
    };
    if total > 0 {
        tell(0);
    }
    let one_more = || tell(done.fetch_add(1, Ordering::Relaxed) + 1);
    let mut drawn = (options.renderer)
        .render_all(&requests, &one_more)
        .into_iter();
    let mut images = Images::new();
    for (image, have) in wanted.into_iter().zip(have.iter_mut()) {
        let key = key(&image);
        let (index, image) = image;
        let part = &parts[index];
        let loaded = match image {
            _ if have.is_some() => have.take().expect("an image kept"),
            Wanted::Url(url) => load_url(url, part.base, max_file_size, fonts),
            Wanted::Diagram {
                language,
                source,
                line,
            } => {
                let drawn = drawn.next().expect("a drawing for each diagram");
                let options = LoadOptions {
                    by_line: part.by_line,
                    ..*options
                };
                finish_diagram(
                    language,
                    source,
                    line,
                    drawn,
                    max_file_size,
                    fonts,
                    &options,
                )
            }
        };
        images.insert(key, loaded);
    }
    images
}

/// The outcome of loading an image.
type Outcome = Result<Source, ImageError>;

/// The images `parts` want, each once with the number of its part, in
/// the order of the documents, and beside each what `kept` holds of it,
/// taken out of `kept`; `None` for one to load. A diagram that failed is
/// to be drawn again, as what made it fail, such as a time limit or a
/// missing program, may have passed.
fn plan<'p>(
    parts: &[Part<'p>],
    renderer: &dyn Renderer,
    mut kept: Option<&mut Images>,
) -> (Vec<(usize, Wanted<'p>)>, Vec<Option<Outcome>>) {
    let mut wanted = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let mut blocks = Vec::new();
        collect_blocks(&part.document.blocks, renderer, &mut blocks);
        wanted.extend(blocks.into_iter().map(|image| (index, image)));
    }
    let key = |&(index, ref image): &(usize, Wanted)| image.key().in_document(parts[index].number);
    let mut keys = HashSet::new();
    wanted.retain(|image| keys.insert(key(image)));
    let have = (wanted.iter())
        .map(|image| {
            let kept = kept.as_deref_mut()?.remove(&key(image))?;
            let failed = kept.is_err() && matches!(image.1, Wanted::Diagram { .. });
            (!failed).then_some(kept)
        })
        .collect();
    (wanted, have)
}

/// The languages of the code blocks in `document` that `renderer` draws
/// as diagrams, in order, once for each block.
pub fn diagram_languages<'a>(document: &'a Document, renderer: &dyn Renderer) -> Vec<&'a str> {
    let mut wanted = Vec::new();
    collect_blocks(&document.blocks, renderer, &mut wanted);
    (wanted.into_iter())
        .filter_map(|image| match image {
            Wanted::Diagram { language, .. } => Some(language),
            Wanted::Url(_) => None,
        })
        .collect()
}

/// The URLs of the images `document` shows as image blocks.
fn lone_urls(document: &Document) -> Vec<&str> {
    let mut wanted = Vec::new();
    collect_blocks(&document.blocks, &NoRenderer, &mut wanted);
    (wanted.into_iter())
        .filter_map(|image| match image {
            Wanted::Url(url) => Some(url),
            Wanted::Diagram { .. } => None,
        })
        .collect()
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
            } => ImageKey::block(language, source),
        }
    }
}

/// The images `blocks` show as image blocks, and the code blocks among
/// them that `renderer` draws. Other images are only ever shown as alt
/// text, so they are not loaded at all.
fn collect_blocks<'a>(blocks: &'a [Block], renderer: &dyn Renderer, wanted: &mut Vec<Wanted<'a>>) {
    for block in blocks {
        match block {
            Block::Paragraph(content) => {
                wanted.extend(Inline::lone_image(content).map(|image| Wanted::Url(&image.url)));
            }
            Block::BlockQuote(blocks)
            | Block::Alert { blocks, .. }
            | Block::Details { blocks, .. } => collect_blocks(blocks, renderer, wanted),
            Block::List(list) => {
                for item in &list.items {
                    collect_blocks(&item.blocks, renderer, wanted);
                }
            }
            Block::CodeBlock {
                language: Some(language),
                code,
                line,
            } if renderer.draws(language) => wanted.push(Wanted::Diagram {
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
            | Block::Note(_)
            | Block::Error(_)
            // Its pixels are not loaded from a URL.
            | Block::ImageFile(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use super::*;
    use crate::diagrams::{Drawn, Renderer, Theme};

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

    pub(super) fn rgba(image: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
        image.get_pixel(x, y).0
    }

    #[test]
    fn late_warnings_tell_what_came_failed_once_in_the_documents_order() {
        let missing = |url: &str| Err(ImageError::Missing(PathBuf::from(url)));
        let mut images = Images::new();
        let (a, b, c) = (
            ImageKey::from("a.png"),
            ImageKey::from("b.png"),
            ImageKey::from("c.png"),
        );
        images.shown = vec![a.clone(), b.clone(), c.clone(), b.clone()];
        images.came(b.clone(), missing("b.png"));
        images.came(a.clone(), missing("a.png"));
        // Told before the pager opened, as when it loaded first.
        images.insert(c.clone(), missing("c.png"));
        let told: Vec<_> = images
            .late_warnings(true)
            .into_iter()
            .map(|(_, w)| w)
            .collect();
        assert_eq!(told, [missing_text("a.png"), missing_text("b.png")]);
        // Loaded at last.
        images.came(a, Ok(Source::Raster(RgbaImage::new(1, 1))));
        assert_eq!(images.late_warnings(true).len(), 1);
    }

    /// The warning about the missing file `url`.
    fn missing_text(url: &str) -> String {
        ImageError::Missing(PathBuf::from(url)).to_string()
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
            &LoadOptions::default(),
        );
        assert!(
            matches!(images.get(&"pattern.png".into()), Some(Ok(_))),
            "{document:?}"
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
            &LoadOptions::default(),
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
            &LoadOptions::default(),
        );
        assert!(images.is_empty(), "{images:?}");
    }

    /// Draws Mermaid as an empty SVG one line high for each line of
    /// source, and fails on a source that says `broken`, and on one that
    /// says `flaky` the first time it is asked to draw it. Remembers what
    /// it was asked to draw.
    #[derive(Default)]
    pub(super) struct Stub {
        pub(super) calls: std::cell::RefCell<Vec<(String, Theme)>>,
    }

    impl Renderer for Stub {
        fn draws(&self, language: &str) -> bool {
            language.eq_ignore_ascii_case("mermaid")
        }

        fn render(&self, request: &Request) -> Result<Drawn, String> {
            let &Request { source, theme, .. } = request;
            self.calls.borrow_mut().push((source.to_owned(), theme));
            if source.contains("broken") {
                return Err("parse error".into());
            }
            let asked = self
                .calls
                .borrow()
                .iter()
                .filter(|(drawn, _)| drawn == source)
                .count();
            if source.contains("flaky") && asked == 1 {
                return Err("timed out".into());
            }
            let height = 20 * source.lines().count();
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="{height}"/>"#
            );
            Ok(Drawn::svg(svg, theme, None))
        }
    }

    pub(super) fn with_stub(stub: &Stub, theme: Theme, by_line: bool) -> LoadOptions<'_> {
        LoadOptions {
            renderer: stub,
            theme,
            by_line,
            ..LoadOptions::default()
        }
    }

    #[test]
    fn a_reload_draws_only_the_diagrams_it_does_not_have() {
        let stub = Stub::default();
        let options = with_stub(&stub, Theme::Light, true);
        let part = |document| Part {
            document,
            number: 0,
            base: Path::new(""),
            by_line: true,
        };
        let before = crate::parse::parse("```mermaid\na\n```\n\n```mermaid\nb\n```\n");
        let after = crate::parse::parse("```mermaid\na\n```\n\n```mermaid\nc\n```\n");
        let mut images = load_parts(
            &[part(&before)],
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
            &options,
        );
        let calls = |stub: &Stub| -> Vec<String> {
            stub.calls
                .borrow()
                .iter()
                .map(|(source, _)| source.clone())
                .collect()
        };
        assert_eq!(calls(&stub), ["a\n", "b\n"]);
        let parts = [part(&after)];
        let (wanted, have) = plan(&parts, &stub, Some(&mut images));
        let keys: Vec<_> = (wanted.iter()).map(|(_, image)| image.key()).collect();
        assert_eq!(
            keys,
            [
                ImageKey::block("mermaid", "a\n"),
                ImageKey::block("mermaid", "c\n")
            ]
        );
        // `a` is taken as it was, `c` is to be drawn, and `b` is left.
        assert!(matches!(have[..], [Some(Ok(_)), None]), "{have:?}");
        let left: Vec<_> = images.iter().map(|(key, _)| key.clone()).collect();
        assert_eq!(left, [ImageKey::block("mermaid", "b\n")]);
    }

    #[test]
    fn a_reload_draws_again_a_diagram_that_failed() {
        let stub = Stub::default();
        let options = with_stub(&stub, Theme::Light, true);
        let document = crate::parse::parse("```mermaid\nflaky\n```\n");
        let parts = [Part {
            document: &document,
            number: 0,
            base: Path::new(""),
            by_line: true,
        }];
        let key = ImageKey::block("mermaid", "flaky\n");
        let mut images = load_parts(&parts, DEFAULT_MAX_FILE_SIZE, &Fonts::none(), &options);
        assert!(matches!(images.get(&key), Some(Err(_))), "{images:?}");
        let (wanted, have) = plan(&parts, &stub, Some(&mut images));
        assert_eq!(wanted.len(), 1);
        assert!(matches!(have[..], [None]), "{have:?}");
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
        for key in [
            ImageKey::block("mermaid", "a\n"),
            ImageKey::block("Mermaid", "c\n"),
        ] {
            match images.get(&key) {
                Some(Ok(Source::Svg(svg))) => assert_eq!(svg.size, (200, 20)),
                other => panic!("{key:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_diagram_is_drawn_once_whatever_the_case_of_its_language() {
        let source = "```Mermaid\na\n```\n\n```mermaid\na\n```\n\n```MERMAID\na\n```\n";
        let stub = Stub::default();
        let images = load_with(
            &crate::parse::parse(source),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
            &with_stub(&stub, Theme::Light, true),
        );
        assert_eq!(stub.calls.into_inner().len(), 1);
        assert_eq!(images.len(), 1);
        for language in ["Mermaid", "mermaid", "MERMAID"] {
            let key = ImageKey::block(language, "a\n");
            assert!(matches!(images.get(&key), Some(Ok(_))), "{language}");
        }
    }

    #[test]
    fn tells_how_many_diagrams_are_drawn_of_how_many() {
        let source = "```mermaid\na\n```\n\n```mermaid\nb\n```\n\n```mermaid\na\n```\n";
        let told = std::sync::Mutex::new(Vec::new());
        let tell = |done, total| told.lock().unwrap().push((done, total));
        let stub = Stub::default();
        let options = LoadOptions {
            progress: Some(&tell),
            ..with_stub(&stub, Theme::Light, true)
        };
        let load = |source: &str| {
            load_with(
                &crate::parse::parse(source),
                &fixtures(),
                DEFAULT_MAX_FILE_SIZE,
                &Fonts::none(),
                &options,
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
            &LoadOptions::default(),
        );
        assert!(images.is_empty(), "{images:?}");
    }

    #[test]
    fn documents_loaded_together_keep_their_own_images() {
        // Both documents show `fig.png`, each from its own directory, and
        // the same diagram, which fails in both.
        let scratch = crate::testing::Scratch::new("images-two-documents");
        for (dir, width) in [("a", 3), ("b", 5)] {
            std::fs::create_dir(scratch.join(dir)).unwrap();
            let image = RgbaImage::new(width, 1);
            image.save(scratch.join(dir).join("fig.png")).unwrap();
        }
        let first = crate::parse::parse("![a](fig.png)\n\n```mermaid\nbroken\n```\n");
        let second = crate::parse::parse("Text.\n\n![b](fig.png)\n\n```mermaid\nbroken\n```\n");
        let (a, b) = (scratch.join("a"), scratch.join("b"));
        let parts = [
            Part {
                document: &first,
                number: 0,
                base: &a,
                by_line: true,
            },
            Part {
                document: &second,
                number: 1,
                base: &b,
                by_line: true,
            },
        ];
        let stub = Stub::default();
        let images = load_parts(
            &parts,
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
            &with_stub(&stub, Theme::Light, true),
        );
        let width = |document| match images.get(&ImageKey::from("fig.png").in_document(document)) {
            Some(Ok(source)) => source.size().0,
            other => panic!("{document}: {other:?}"),
        };
        assert_eq!((width(0), width(1)), (3, 5));
        // Each document is told about its own diagram, by its own line.
        assert_eq!(
            images.warnings_in(0, true),
            ["mermaid diagram on line 3: parse error"]
        );
        assert_eq!(
            images.warnings_in(1, true),
            ["mermaid diagram on line 5: parse error"]
        );
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
        let block = ImageKey::block("mermaid", "a.png");
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
