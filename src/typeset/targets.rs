//! What a document has that can be followed or drawn: its links and its
//! embeds, as stretches of its text.
//!
//! What a document holds is asked of the model, and where it is shown is
//! asked of a layout. A link or an embed is a range of places in the text of
//! the document, so it is the same one in every layout, and a layout that
//! shows it somewhere else, or not at all, says so for itself.
//!
//! This lists each link as it is written. That a terminal shows a link
//! without an address as no link at all, or two links next to each other
//! with one address as one hyperlink, is how a terminal shows links, not what
//! the document has, and is for the part that handles a terminal.
//! Whether an embed is drawn is a layout's answer as well: a diagram here is
//! a code block in a language that kiri can draw, whether or not a
//! program for it is at hand.
//!
//! `targets` makes them: the links and embeds of the shown documents as ranges
//! of their text.

use std::ops::Range;
use std::path::Path;

use crate::typeset::anchors::{Ids, anchor_text, decode};
use crate::typeset::diagrams::language;
use crate::typeset::diagrams::request::INLINE_FORMULA;
use crate::typeset::input::is_markdown;
use crate::typeset::model::{Block, Document, Inline};
use crate::typeset::parse::{anchor_ids, html_headings};
use crate::typeset::position::Pos;
use crate::typeset::text::pieces_with;

/// A link and where its text is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub range: Range<Pos>,
    pub url: String,
    /// The title written after the address: `[a](u "title")`.
    pub title: Option<String>,
    pub goes: Goes,
}

/// Where a link goes, as its address says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Goes {
    /// No address: `[a]()`.
    Nowhere,
    /// `#id`: a place in the link's own file. An empty `id` is the start.
    Here { id: String },
    /// A path with no scheme, relative or absolute, with or without `#id`.
    File {
        path: String,
        id: Option<String>,
        /// Whether the path has a Markdown extension.
        markdown: bool,
    },
    /// An address with a scheme or a host: `https:`, `mailto:`, `//host`.
    Away,
}

/// Where a link to `url` goes: told by the address alone, without looking
/// at any file.
pub fn goes(url: &str) -> Goes {
    if url.is_empty() {
        return Goes::Nowhere;
    }
    if let Some(id) = url.strip_prefix('#') {
        return Goes::Here {
            id: decode(id).into_owned(),
        };
    }
    if url.starts_with("//") {
        return Goes::Away;
    }
    match scheme(url) {
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
            // `file:///a/b`, `file://host/a/b` and `file:/a/b`: the path
            // starts at the first `/` after the host.
            let rest = &url[scheme.len() + 1..];
            let path = match rest.strip_prefix("//") {
                Some(after) => after.find('/').map_or("", |at| &after[at..]),
                None => rest,
            };
            file(path)
        }
        Some(_) => Goes::Away,
        None => file(url),
    }
}

/// The scheme of `url`: letters, digits, `+`, `.` and `-` from a letter up
/// to a `:`. One letter is a drive of Windows, as in `C:\a.md`.
fn scheme(url: &str) -> Option<&str> {
    let end = url.find(':')?;
    let name = &url[..end];
    let mut chars = name.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c));
    (valid && end > 1).then_some(name)
}

/// A link to the file `address`, a path with a query and a fragment.
fn file(address: &str) -> Goes {
    let (path, id) = match address.split_once('#') {
        Some((path, id)) => (
            path,
            Some(decode(id).into_owned()).filter(|id| !id.is_empty()),
        ),
        None => (address, None),
    };
    let path = path.split('?').next().unwrap_or("");
    let path = decode(path).into_owned();
    let markdown = is_markdown(Path::new(&path));
    Goes::File { path, id, markdown }
}

/// A place a link can go to by its id: a heading, or an `<a id>` or
/// `<a name>` of HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    /// The heading or the block of HTML, or the empty place in a text where
    /// an inline tag is.
    pub range: Range<Pos>,
    pub id: String,
    pub kind: NamedKind,
    /// The level and text of a heading that is written in a block of
    /// HTML, such as `<h2>` in a `<div>`, where the block holds no
    /// heading to read them from; `None` for the rest.
    pub inner: Option<Inner>,
}

/// The level and the plain text of a heading in a block of HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inner {
    pub level: u8,
    /// As [`anchor_text`] gives it.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedKind {
    Heading,
    Html,
    /// An [`Inline::Anchor`], such as a footnote's.
    Anchor,
}

/// An image, a formula or a diagram, and where its text is: the alt text of
/// an image, the source of a formula or a diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Embed {
    pub range: Range<Pos>,
    pub kind: EmbedKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedKind {
    /// An image in a text, or alone in a paragraph.
    Image { url: String },
    /// A formula in a text, or a code block in `math`.
    Formula,
    /// A code block in a language that kiri draws diagrams in.
    Diagram { language: String },
    /// A file shown as an image on its own: the range is its whole block,
    /// as it has no text.
    ImageFile { name: String },
}

/// The links and the embeds of the shown documents, each in the order of
/// the documents by where it starts. An image in a link is both, and the
/// ranges are the same stretch of text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Targets {
    pub links: Vec<Link>,
    pub embeds: Vec<Embed>,
    /// The places that have an id, in the order of the documents:
    /// `range.start.file` is the file.
    pub named: Vec<Named>,
}

impl Targets {
    /// The place named `id` in file `file`: the one written exactly as
    /// `id`, and else the first in lower case, as GitHub's own script finds
    /// `#Installation` at `installation`. A heading's id is in lower case.
    pub fn named(&self, file: usize, id: &str) -> Option<&Named> {
        let of_file = || {
            self.named
                .iter()
                .filter(|named| named.range.start.file == file)
        };
        of_file().find(|named| named.id == id).or_else(|| {
            let lower = id.to_lowercase();
            of_file().find(|named| named.id == lower)
        })
    }
}

/// The headings written in the block of HTML `source`, as places in its
/// text. A heading is named by its `id`, and else as a Markdown heading
/// is, with the same numbering in `ids`.
fn html_headings_named(
    source: &str,
    ids: &mut Ids,
    at: &impl Fn(usize, Range<usize>) -> Range<Pos>,
) -> Vec<Named> {
    html_headings(source)
        .into_iter()
        .map(|inner| Named {
            range: at(0, inner.bytes.clone()),
            id: inner.id.unwrap_or_else(|| ids.of(&inner.content)),
            kind: NamedKind::Heading,
            inner: Some(Inner {
                level: inner.level,
                text: anchor_text(&inner.content),
            }),
        })
        .collect()
}

/// The links, the embeds and the named places of `files`, shown in this order: file 0 is the
/// first.
pub fn targets<'a>(files: impl IntoIterator<Item = &'a Document>) -> Targets {
    let mut found = Targets::default();
    for (file, document) in files.into_iter().enumerate() {
        // The ids of the headings of a file are numbered within the file.
        let mut ids = Ids::new();
        for (path, block) in document.blocks() {
            let at = |text: usize, bytes: Range<usize>| Range {
                start: Pos::text(file, &path, text, bytes.start),
                end: Pos::text(file, &path, text, bytes.end),
            };
            // The cells of a table of a CSV's source are only text, with no
            // link or embed in them: they are not made to be looked through.
            if matches!(block, Block::Table(table) if table.source().is_some()) {
                continue;
            }
            let pieces = pieces_with(block, &mut |text, inline, bytes| match inline {
                Inline::Link { url, title, .. } => found.links.push(Link {
                    range: at(text, bytes),
                    url: url.clone(),
                    title: title.clone(),
                    goes: goes(url),
                }),
                Inline::Html(tag) => {
                    let here = at(text, bytes.start..bytes.start);
                    found
                        .named
                        .extend(anchor_ids(tag).into_iter().map(|id| Named {
                            range: here.clone(),
                            id,
                            kind: NamedKind::Html,
                            inner: None,
                        }));
                }
                Inline::Anchor(id) => found.named.push(Named {
                    range: at(text, bytes.start..bytes.start),
                    id: id.clone(),
                    kind: NamedKind::Anchor,
                    inner: None,
                }),
                Inline::Image(image) => found.embeds.push(Embed {
                    range: at(text, bytes),
                    kind: EmbedKind::Image {
                        url: image.url.clone(),
                    },
                }),
                Inline::Formula { .. } => found.embeds.push(Embed {
                    range: at(text, bytes),
                    kind: EmbedKind::Formula,
                }),
                _ => {}
            });
            let whole = || Pos::block(file, &path)..Pos::end(file, &path);
            match block {
                Block::Heading { content, .. } => found.named.push(Named {
                    range: whole(),
                    id: ids.of(content),
                    kind: NamedKind::Heading,
                    inner: None,
                }),
                Block::Html(source) => {
                    found
                        .named
                        .extend(html_headings_named(source, &mut ids, &at));
                    found
                        .named
                        .extend(anchor_ids(source).into_iter().map(|id| Named {
                            range: whole(),
                            id,
                            kind: NamedKind::Html,
                            inner: None,
                        }));
                }
                Block::CodeBlock {
                    language: Some(name),
                    ..
                } if let Some(drawn) = language::language(name) => {
                    let end = pieces.first().map_or(0, |piece| piece.text.len());
                    found.embeds.push(Embed {
                        range: at(0, 0..end),
                        kind: match drawn.name {
                            "math" | INLINE_FORMULA => EmbedKind::Formula,
                            language => EmbedKind::Diagram {
                                language: language.to_owned(),
                            },
                        },
                    });
                }
                Block::ImageFile(name) => found.embeds.push(Embed {
                    range: Pos::block(file, &path)..Pos::end(file, &path),
                    kind: EmbedKind::ImageFile { name: name.clone() },
                }),
                _ => {}
            }
        }
    }
    // The blocks come in order, and the inlines of a text in the order they
    // end, an inline inside another first: by where they start, in order.
    found
        .links
        .sort_by(|a, b| a.range.start.cmp(&b.range.start));
    found
        .embeds
        .sort_by(|a, b| a.range.start.cmp(&b.range.start));
    found
        .named
        .sort_by(|a, b| a.range.start.cmp(&b.range.start));
    found
}

#[cfg(test)]
mod tests;
