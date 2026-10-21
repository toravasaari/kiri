//! Loading the images a document refers to.
//!
//! This is the stage between the document model and layout: it resolves
//! each image URL, reads the file and decodes it, and has each diagram in
//! a code block drawn as an image, SVG or raster, so layout and output
//! work on pixels and never touch the file system. A failure is a value,
//! not an error for the whole document, because the renderer still shows
//! the alt text in place of an image it could not load.

mod decode;
mod drawn;
mod svg;
mod url;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::diagrams::{Diagrams, Renderer, Request};
use crate::model::{Block, Document, Inline};

pub use decode::{DEFAULT_MAX_FILE_SIZE, Format, MB, Source, decode, read_image};
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

/// What an image is made from, the key into `Images`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageKey {
    /// An image file or `data:` URL, as the document gives it.
    Url(String),
    /// A code block drawn as an image, such as a diagram: its language, in
    /// lower case as code blocks match it in any case, and its source.
    Block { language: String, source: String },
}

impl ImageKey {
    /// The key of the code block `source` in `language`, drawn as an
    /// image: the same for a language in any case.
    pub fn block(language: &str, source: &str) -> Self {
        ImageKey::Block {
            language: language.to_ascii_lowercase(),
            source: source.to_owned(),
        }
    }
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
    collect_blocks(&document.blocks, diagrams.renderer, &mut wanted);
    let mut keys = HashSet::new();
    wanted.retain(|image| keys.insert(image.key()));
    // The diagrams first, all together, so a renderer can draw several
    // at once.
    let requests: Vec<Request> = (wanted.iter())
        .filter_map(|image| match *image {
            Wanted::Diagram {
                language, source, ..
            } => Some(Request::new(language, source, diagrams.theme)),
            Wanted::Url(_) => None,
        })
        .collect();
    let total = requests.len();
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
        .render_all(&requests, &one_more)
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
            | Block::Note(_) => {}
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
            let height = 20 * source.lines().count();
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="{height}"/>"#
            );
            Ok(Drawn::svg(svg, theme, None))
        }
    }

    pub(super) fn with_stub(stub: &Stub, theme: Theme, by_line: bool) -> Diagrams<'_> {
        Diagrams {
            renderer: stub,
            theme,
            by_line,
            progress: None,
            panel: true,
            largest: None,
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
