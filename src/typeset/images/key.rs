//! What an image is made from, the key into `Images`.

use std::fmt;

use crate::typeset::images::heading::{HeadingWord, plain_words};

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
