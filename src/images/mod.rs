//! The images a document refers to, and what is loaded of them.
//!
//! This is the stage between the document model and layout: each image URL
//! is resolved, and each diagram in a code block is drawn as an image, SVG
//! or raster, so layout and output work on pixels and never touch the file
//! system. A failure is a value, not an error for the whole document,
//! because the renderer still shows the alt text in place of an image it
//! could not load.
//!
//! This file owns the key of an image, the store of loaded images that
//! layout reads, and the plan of which images a document wants. The
//! functions that read files and run the renderers, `load`, `load_with` and
//! `load_parts`, are in `src/load/`, the platform's loading.
//!
//! What does not belong here is any new way to read, fetch or draw an
//! image, which is work for threads and programs, and nothing that decodes
//! a format, which has a file of its own in this directory.
//!
//! Reading and fetching go to the platform's work runner, as the loads in
//! `src/images/background.rs` and `src/images/pipeline.rs` do, and
//! decoding and drawing from given data to the typesetting engine
//! (`src/typeset/images/`).

mod background;
mod pipeline;
mod stamps;
pub(crate) mod svg;
pub(crate) mod url;
mod warnings;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::diagrams::DiagramError;
use crate::diagrams::{FormulaColors, INLINE_FORMULA, NoRenderer, Renderer, Theme};
use crate::typeset::diagnostic::Diagnostic;
use crate::typeset::model::{Block, Document, Inline};

pub use crate::typeset::images::decode::{
    DEFAULT_MAX_FILE_SIZE, Format, MB, Source, decode, read_image,
};
pub use crate::typeset::images::heading::{
    CodeLook, HeadingLook, HeadingWord, LevelLook, draw_heading, heading_words, plain_words,
};
#[cfg(test)]
pub(crate) use background::{Arrival, Gated};
pub use background::{Loads, STOPPING};
pub use pipeline::{
    Interrupted, Loaded, Loading, load_documents, load_headings_in_background, load_in_background,
    load_inline_in_background, measure_in_background, reload_in_background,
};
pub use stamps::Stamps;
pub use svg::{EMOJI, Fonts, SCRIPTS, Svg, installed};

pub(crate) use crate::typeset::images::decode::{MAX_PIXELS, png_size};
#[cfg(all(test, feature = "mermaid"))]
pub(crate) use crate::typeset::images::drawn::decode_diagram;
pub(crate) use url::read_at_most;

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
    Diagram { name: String, reason: DiagramError },
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
    /// A heading drawn as an image: its level, its words with the inline
    /// code they are in, and the size in pixels of the cells it is drawn
    /// for, as its words start on whole cells.
    Heading {
        document: usize,
        level: u8,
        words: Vec<HeadingWord>,
        cell: (u16, u16),
    },
    /// A part of a page of reading mode's text, drawn when it is shown:
    /// the page, by a hash of what it shows and how, and the part's number
    /// from its top.
    Page {
        document: usize,
        page: u64,
        slice: u32,
        /// A hash of what is drawn over the part, such as a search's
        /// marks; 0 for nothing.
        overlay: u64,
    },
    /// A band of a page of reading mode, a code block or table that
    /// scrolls sideways, drawn when it is shown: the page, the band's
    /// number on it, the part of its rows, at most 30, which the key
    /// stands for, how many pixels it is scrolled, and whether it is the
    /// one selected.
    Band {
        document: usize,
        page: u64,
        band: u32,
        slice: u32,
        offset: u32,
        selected: bool,
        /// As for a part of a page.
        overlay: u64,
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

    /// The key of the heading of `level` with the text `text`, without
    /// inline code, drawn as an image for cells of `cell` pixels, in the
    /// first document.
    pub fn heading(level: u8, text: &str, cell: (u16, u16)) -> Self {
        ImageKey::heading_of(level, plain_words(text), cell)
    }

    /// The key of the heading of `level` with `words`, drawn as an image
    /// for cells of `cell` pixels, in the first document.
    pub fn heading_of(level: u8, words: Vec<HeadingWord>, cell: (u16, u16)) -> Self {
        ImageKey::Heading {
            document: 0,
            level,
            words,
            cell,
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
            ImageKey::Heading {
                level, words, cell, ..
            } => ImageKey::Heading {
                document,
                level,
                words,
                cell,
            },
            ImageKey::Page {
                page,
                slice,
                overlay,
                ..
            } => ImageKey::Page {
                document,
                page,
                slice,
                overlay,
            },
            ImageKey::Band {
                page,
                band,
                slice,
                offset,
                selected,
                overlay,
                ..
            } => ImageKey::Band {
                document,
                page,
                band,
                slice,
                offset,
                selected,
                overlay,
            },
        }
    }

    /// The number of the document the image belongs to.
    pub fn document(&self) -> usize {
        match *self {
            ImageKey::Url { document, .. }
            | ImageKey::Block { document, .. }
            | ImageKey::Heading { document, .. }
            | ImageKey::Page { document, .. }
            | ImageKey::Band { document, .. } => document,
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
            ImageKey::Heading { words, .. } => {
                let text: Vec<&str> = words.iter().map(|word| word.text.as_str()).collect();
                write!(f, "heading {:?}", text.join(" "))
            }
            ImageKey::Page { slice, .. } => write!(f, "part {slice} of a page of text"),
            ImageKey::Band { band, .. } => write!(f, "band {band} of a page of text"),
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
    /// The colours formulas are drawn in; by default none, which leaves
    /// them to the page.
    pub formula: FormulaColors,
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
    /// How headings are drawn as images; `None`, the default, draws none.
    pub headings: Option<HeadingLook>,
}

impl Default for LoadOptions<'_> {
    fn default() -> Self {
        LoadOptions {
            renderer: &NoRenderer,
            theme: Theme::Light,
            formula: FormulaColors::default(),
            by_line: true,
            progress: None,
            panel: true,
            largest: None,
            headings: None,
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
    /// What each loaded image is, by a hash of it.
    prints: std::cell::RefCell<HashMap<ImageKey, u64>>,
}

/// What a block shows in place of its image while the image loads.
#[derive(Debug, Clone)]
pub enum Placeholder {
    /// The image the block showed before, dimmed.
    Dimmed(Source),
    /// Nothing that tells a size: the block shows as it does without its
    /// image.
    Unsized,
    /// A box of this many rows, only to measure how tall the documents are
    /// before their images come; never drawn.
    Rows(u16),
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
        // A hash of the new image is made when it is asked for.
        self.prints.get_mut().remove(&key);
        self.by_key.insert(key, image);
    }

    /// A hash of the image loaded under `key`, which another image under
    /// the same key does not have; `None` when none loaded.
    pub fn print(&self, key: &ImageKey) -> Option<u64> {
        if let Some(print) = self.prints.borrow().get(key) {
            return Some(*print);
        }
        let print = self.by_key.get(key)?.as_ref().ok()?.fingerprint();
        self.prints.borrow_mut().insert(key.clone(), print);
        Some(print)
    }

    /// Takes the outcome for `key` out, as for an image shown on its own;
    /// `None` when it was never loaded.
    pub fn remove(&mut self, key: &ImageKey) -> Option<Result<Source, ImageError>> {
        self.order.retain(|loaded| loaded != key);
        self.late.remove(key);
        self.prints.get_mut().remove(key);
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
    pub fn warnings(&self) -> Vec<Diagnostic> {
        self.warnings_with(true)
    }

    /// `warnings`, with one for each diagram that could not be drawn only
    /// when `render_errors`.
    pub fn warnings_with(&self, render_errors: bool) -> Vec<Diagnostic> {
        warnings::warnings(self.iter(), render_errors)
    }

    /// `warnings_with` for the images that came from loading in the
    /// background and are still shown, in the documents' order, each with
    /// the number of its document.
    pub fn late_warnings(&self, render_errors: bool) -> Vec<(usize, Diagnostic)> {
        let mut told = HashSet::new();
        (self.shown.iter())
            .filter(|key| self.late.contains(*key) && told.insert(*key))
            .filter_map(|key| Some((key, self.by_key.get(key)?)))
            .flat_map(|(key, image)| {
                let warnings = warnings::warnings(std::iter::once((key, image)), render_errors);
                warnings
                    .into_iter()
                    .map(|warning| (key.document(), warning))
            })
            .collect()
    }

    /// `warnings_with` for the images of the document numbered
    /// `document` alone.
    pub fn warnings_in(&self, document: usize, render_errors: bool) -> Vec<Diagnostic> {
        let images = self.iter().filter(|(key, _)| key.document() == document);
        warnings::warnings(images, render_errors)
    }
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
    /// Whether the images in the middle of text are wanted too, as reading
    /// mode draws them; otherwise only an image that is a block of its
    /// own is, as the others show as their alt text.
    pub inline_images: bool,
}

/// The outcome of loading an image.
type Outcome = Result<Source, ImageError>;

/// The images `parts` want, each once with the number of its part, in
/// the order of the documents, and beside each what `kept` holds of it,
/// taken out of `kept`; `None` for one to load. A diagram that failed is
/// to be drawn again, as what made it fail, such as a time limit or a
/// missing program, may have passed. The headings are wanted as images,
/// drawn for cells of `headings` pixels, only when it is given.
pub(crate) fn plan<'p>(
    parts: &[Part<'p>],
    renderer: &dyn Renderer,
    headings: Option<(u16, u16)>,
    mut kept: Option<&mut Images>,
) -> (Vec<(usize, Wanted<'p>)>, Vec<Option<Outcome>>) {
    let mut wanted = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let mut blocks = Vec::new();
        let inline = part.inline_images;
        collect_blocks(
            &part.document.blocks,
            renderer,
            (headings, inline),
            &mut blocks,
        );
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
    collect_blocks(&document.blocks, renderer, (None, false), &mut wanted);
    (wanted.into_iter())
        .filter_map(|image| match image {
            Wanted::Diagram { language, .. } => Some(language),
            Wanted::Url(_) | Wanted::Heading { .. } => None,
        })
        .collect()
}

/// The URLs of the images `document` shows as image blocks and those in
/// the middle of text, whichever of them the layout in use draws.
fn shown_urls(document: &Document) -> Vec<&str> {
    let mut wanted = Vec::new();
    collect_blocks(&document.blocks, &NoRenderer, (None, true), &mut wanted);
    (wanted.into_iter())
        .filter_map(|image| match image {
            Wanted::Url(url) => Some(url),
            Wanted::Diagram { .. } | Wanted::Heading { .. } => None,
        })
        .collect()
}

/// An image a document shows as a block.
pub(crate) enum Wanted<'a> {
    Url(&'a str),
    /// A code block drawn as a diagram, and the line it starts on.
    Diagram {
        language: &'a str,
        source: Cow<'a, str>,
        /// Where it starts; 0 for a formula in the middle of text, which a
        /// line does not name.
        line: usize,
    },
    /// A heading drawn as an image: its level, its words and the size of
    /// the cells it is drawn for.
    Heading {
        level: u8,
        words: Vec<HeadingWord>,
        cell: (u16, u16),
    },
}

impl Wanted<'_> {
    pub(crate) fn key(&self) -> ImageKey {
        match *self {
            Wanted::Url(url) => ImageKey::from(url),
            Wanted::Heading {
                level,
                ref words,
                cell,
            } => ImageKey::heading_of(level, words.clone(), cell),
            Wanted::Diagram {
                language,
                ref source,
                ..
            } => ImageKey::block(language, source),
        }
    }
}

/// The images `blocks` show as image blocks, and the code blocks among
/// them that `renderer` draws. Other images are only ever shown as alt
/// text, so they are not loaded at all, unless `inline`, as reading mode
/// draws them where they stand in the text. With `headings`, the headings
/// among `blocks` and in the details blocks among them, which layout
/// takes as top level, are wanted as images too, drawn for cells of that
/// many pixels.
fn collect_blocks<'a>(
    blocks: &'a [Block],
    renderer: &dyn Renderer,
    (headings, inline): (Option<(u16, u16)>, bool),
    wanted: &mut Vec<Wanted<'a>>,
) {
    for block in blocks {
        match block {
            Block::Paragraph(content) => match Inline::lone_image(content) {
                Some(image) => wanted.push(Wanted::Url(&image.url)),
                None if inline => inline_images(content, renderer, wanted),
                None => {}
            },
            Block::Details { summary, blocks } => {
                if inline {
                    inline_images(summary, renderer, wanted);
                }
                collect_blocks(blocks, renderer, (headings, inline), wanted)
            }
            Block::BlockQuote(blocks) | Block::Alert { blocks, .. } => {
                collect_blocks(blocks, renderer, (None, inline), wanted)
            }
            Block::List(list) => {
                for item in &list.items {
                    collect_blocks(&item.blocks, renderer, (None, inline), wanted);
                }
            }
            Block::CodeBlock {
                language: Some(language),
                code,
                line,
            } if renderer.draws(language) => wanted.push(Wanted::Diagram {
                language,
                source: Cow::Borrowed(code),
                line: *line,
            }),
            Block::Heading { level, content } if let Some(cell) = headings => {
                wanted.push(Wanted::Heading {
                    level: *level,
                    words: heading_words(content),
                    cell,
                });
                if inline {
                    inline_images(content, renderer, wanted);
                }
            }
            Block::Heading { content, .. } if inline => inline_images(content, renderer, wanted),
            Block::Table(table) if inline => {
                for cell in table.header.iter().chain(table.rows.iter().flatten()) {
                    inline_images(cell, renderer, wanted);
                }
            }
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

/// The images among `content`, in the text, wherever they stand in it.
fn inline_images<'a>(content: &'a [Inline], renderer: &dyn Renderer, wanted: &mut Vec<Wanted<'a>>) {
    for inline in content {
        match inline {
            Inline::Image(image) => wanted.push(Wanted::Url(&image.url)),
            Inline::Formula { source, .. } if renderer.draws(INLINE_FORMULA) => {
                wanted.push(Wanted::Diagram {
                    language: INLINE_FORMULA,
                    source: Cow::Owned(format!("{source}\n")),
                    line: 0,
                });
            }
            Inline::Emphasis(inner)
            | Inline::Strong(inner)
            | Inline::Strikethrough(inner)
            | Inline::Kbd(inner)
            | Inline::Mark(inner)
            | Inline::Subscript(inner)
            | Inline::Superscript(inner)
            | Inline::Formula { written: inner, .. }
            | Inline::Link { content: inner, .. } => inline_images(inner, renderer, wanted),
            Inline::Text(_)
            | Inline::Code(_)
            | Inline::SoftBreak
            | Inline::LineBreak
            | Inline::Html(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::typeset::diagnostic::Code;

    /// The text of each of `diagnostics`, as standard error tells them.
    fn texts(diagnostics: Vec<Diagnostic>) -> Vec<String> {
        diagnostics.iter().map(ToString::to_string).collect()
    }

    use image::RgbaImage;

    use super::*;
    use crate::diagrams::{Drawn, Renderer, Request};

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
            .map(|(_, w)| w.to_string())
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
    fn images_in_the_middle_of_text_are_wanted_only_when_asked_for() {
        let source = "\
Text ![a](a.png) and *![b](b.png)* in a paragraph.

# A heading with ![c](c.png)

| table |
|---|
| a cell ![d](d.png) |

- an item with ![e](e.png)

![alone](f.png)
";
        let document = crate::typeset::parse::parse(source);
        let urls = |inline_images| {
            let parts = [Part {
                document: &document,
                number: 0,
                base: Path::new(""),
                by_line: true,
                inline_images,
            }];
            let (wanted, _) = plan(&parts, &NoRenderer, None, None);
            let urls: Vec<String> = (wanted.iter())
                .map(|(_, image)| image.key().to_string())
                .collect();
            urls
        };
        assert_eq!(urls(false), ["f.png"]);
        assert_eq!(
            urls(true),
            ["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"]
        );
    }

    #[test]
    fn an_image_is_hashed_when_asked_and_once_and_not_when_it_is_inserted() {
        let mut images = Images::new();
        let pixels = |red| image::RgbaImage::from_pixel(8, 8, image::Rgba([red, 0, 0, 255]));
        images.insert("a.png", Ok(Source::Raster(pixels(1))));
        // Inserting costs no hash: it is made when the cache asks.
        assert!(images.prints.borrow().is_empty());
        let key = ImageKey::from("a.png");
        let first = images.print(&key).expect("a loaded image has one");
        assert_eq!(images.prints.borrow().len(), 1);
        assert_eq!(images.print(&key), Some(first));
        // The same pixels loaded again are the same, other pixels not, and
        // a failure has none.
        images.insert("a.png", Ok(Source::Raster(pixels(1))));
        assert_eq!(images.print(&key), Some(first));
        images.insert("a.png", Ok(Source::Raster(pixels(2))));
        assert_ne!(images.print(&key), Some(first));
        images.insert("a.png", Err(ImageError::Missing("a.png".into())));
        assert_eq!(images.print(&key), None);
    }

    #[test]
    fn an_image_in_a_details_summary_is_wanted_only_when_asked_for() {
        // The parser gives a summary the images that HTML puts in it; a
        // Markdown image or a formula there stays text.
        let source = "<details>\n<summary>Open <img src=\"s.png\" alt=\"s\"></summary>\n\n\
                      Inside <img src=\"i.png\" alt=\"i\">.\n\n</details>\n";
        let document = crate::typeset::parse::parse(source);
        let urls = |inline_images| {
            let parts = [Part {
                document: &document,
                number: 0,
                base: Path::new(""),
                by_line: true,
                inline_images,
            }];
            let (wanted, _) = plan(&parts, &NoRenderer, None, None);
            let urls: Vec<String> = (wanted.iter())
                .map(|(_, image)| image.key().to_string())
                .collect();
            urls
        };
        assert!(urls(false).is_empty());
        assert_eq!(urls(true), ["s.png", "i.png"]);
    }

    #[test]
    fn formulas_in_the_middle_of_text_are_wanted_only_when_asked_for() {
        struct Math;
        impl Renderer for Math {
            fn draws(&self, language: &str) -> bool {
                language == INLINE_FORMULA || language == "math"
            }

            fn render(&self, _: &Request) -> Result<Drawn, DiagramError> {
                unreachable!("nothing is drawn")
            }
        }
        let source = "\
Let $x^2$ be, and $x^2$ again, and cost $5 and $6.

# A heading with $y$

| table |
|---|
| a cell $z$ |

```math
x^2
```
";
        let document = crate::typeset::parse::parse(source);
        let keys = |inline_images, renderer: &dyn Renderer| {
            let parts = [Part {
                document: &document,
                number: 0,
                base: Path::new(""),
                by_line: true,
                inline_images,
            }];
            let (wanted, _) = plan(&parts, renderer, None, None);
            let keys: Vec<String> = (wanted.iter())
                .map(|(_, image)| image.key().to_string())
                .collect();
            keys
        };
        // A formula in the text is drawn in its own style, so it is not
        // the picture of the block with the same source.
        assert_eq!(keys(false, &Math).len(), 1);
        assert_eq!(keys(true, &Math).len(), 4);
        assert!(keys(true, &NoRenderer).is_empty());
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
            texts(images.warnings()),
            [
                "a.png: remote images are not loaded",
                "mermaid block: not an image format kiri knows",
            ]
        );
    }

    #[test]
    fn a_warning_has_the_kind_and_the_subject_it_is_about() {
        let mut images = Images::new();
        images.insert("gone.png", Err(ImageError::Missing("/doc/gone.png".into())));
        images.insert(
            ImageKey::block("mermaid", "x"),
            Err(ImageError::Diagram {
                name: "mermaid diagram on line 3".into(),
                reason: "parse error".into(),
            }),
        );
        images.insert("https://example.com/a.png", Err(ImageError::Remote));
        images.insert("", Err(ImageError::NoPath));
        let told = images.warnings();
        let seen: Vec<_> = told
            .iter()
            .map(|d| (d.code, d.subject.as_deref(), d.message.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                (Code::Image, Some("/doc/gone.png"), "file not found"),
                (
                    Code::Diagram,
                    Some("mermaid diagram on line 3"),
                    "parse error"
                ),
                (
                    Code::Image,
                    Some("https://example.com/a.png"),
                    "remote images are not loaded"
                ),
                (Code::Image, None, "an image URL is empty"),
            ]
        );
        assert!(told.iter().all(|d| d.file.is_none()));
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
            texts(images.warnings()),
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
