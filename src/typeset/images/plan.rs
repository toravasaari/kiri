//! The plan of which images a document wants: the lone images among its
//! blocks, the code blocks a renderer draws, and, when asked for, headings
//! and the images in the middle of text.

use std::borrow::Cow;
use std::collections::HashSet;

use crate::typeset::diagrams::renderer::{NoRenderer, Renderer};
use crate::typeset::diagrams::request::INLINE_FORMULA;
use crate::typeset::images::decode::Source;
use crate::typeset::images::error::ImageError;
use crate::typeset::images::heading::{HeadingWord, heading_words};
use crate::typeset::images::key::ImageKey;
use crate::typeset::images::options::{Looks, Part};
use crate::typeset::images::store::Images;
use crate::typeset::model::{Block, Document, Inline, SourceMap};

/// The outcome of loading an image.
pub(crate) type Outcome = Result<Source, ImageError>;

/// The images `parts` want, each once with the number of its part, in
/// the order of the documents, and beside each what `kept` holds of it,
/// taken out of `kept`; `None` for one to load. A diagram that failed is
/// to be drawn again, as what made it fail, such as a time limit or a
/// missing program, may have passed. The headings are wanted as images,
/// drawn as `looks` says, only when it has a look for them; the keys carry
/// what `looks` fingerprints.
pub(crate) fn plan<'p>(
    parts: &[Part<'p>],
    renderer: &dyn Renderer,
    looks: &Looks,
    mut kept: Option<&mut Images>,
) -> (Vec<(usize, Wanted<'p>)>, Vec<Option<Outcome>>) {
    let headings = looks.headings.map(|look| look.cell);
    let mut wanted = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let mut blocks = Vec::new();
        let inline = part.inline_images;
        collect_blocks(
            &part.document.blocks,
            (&part.document.places, &mut Vec::new()),
            renderer,
            (headings, inline),
            &mut blocks,
        );
        wanted.extend(blocks.into_iter().map(|image| (index, image)));
    }
    let key =
        |&(index, ref image): &(usize, Wanted)| image.key(looks).in_document(parts[index].number);
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

/// The keys of the images `parts` want, as `plan` finds them, each once, in
/// the order of the documents.
pub(crate) fn keys(parts: &[Part<'_>], renderer: &dyn Renderer, looks: &Looks) -> Vec<ImageKey> {
    let (wanted, _) = plan(parts, renderer, looks, None);
    (wanted.iter())
        .map(|(index, image)| image.key(looks).in_document(parts[*index].number))
        .collect()
}

/// The languages of the code blocks in `document` that `renderer` draws
/// as diagrams, in order, once for each block.
pub fn diagram_languages<'a>(document: &'a Document, renderer: &dyn Renderer) -> Vec<&'a str> {
    let mut wanted = Vec::new();
    collect_blocks(
        &document.blocks,
        (&document.places, &mut Vec::new()),
        renderer,
        (None, false),
        &mut wanted,
    );
    (wanted.into_iter())
        .filter_map(|image| match image {
            Wanted::Diagram { language, .. } => Some(language),
            Wanted::Url(_) | Wanted::Heading { .. } => None,
        })
        .collect()
}

/// The URLs of the images `document` shows as image blocks and those in
/// the middle of text, whichever of them the layout in use draws.
pub(crate) fn shown_urls(document: &Document) -> Vec<&str> {
    let mut wanted = Vec::new();
    collect_blocks(
        &document.blocks,
        (&document.places, &mut Vec::new()),
        &NoRenderer,
        (None, true),
        &mut wanted,
    );
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
    pub(crate) fn key(&self, looks: &Looks) -> ImageKey {
        match *self {
            Wanted::Url(url) => ImageKey::from(url),
            Wanted::Heading {
                level,
                ref words,
                cell,
            } => ImageKey::heading_of(level, words.clone(), cell).with_look(looks.heading(level)),
            Wanted::Diagram {
                language,
                ref source,
                ..
            } => ImageKey::rendered(language, source).with_look(looks.diagram),
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
    (places, path): (&SourceMap, &mut Vec<usize>),
    renderer: &dyn Renderer,
    (headings, inline): (Option<(u16, u16)>, bool),
    wanted: &mut Vec<Wanted<'a>>,
) {
    for (index, block) in blocks.iter().enumerate() {
        path.push(index);
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
                collect_blocks(blocks, (places, path), renderer, (headings, inline), wanted)
            }
            Block::BlockQuote(blocks) | Block::Alert { blocks, .. } => {
                collect_blocks(blocks, (places, path), renderer, (None, inline), wanted)
            }
            Block::List(list) => {
                for (number, item) in list.items.iter().enumerate() {
                    path.push(number);
                    let inside = (places, &mut *path);
                    collect_blocks(&item.blocks, inside, renderer, (None, inline), wanted);
                    path.pop();
                }
            }
            Block::CodeBlock {
                language: Some(language),
                code,
                ..
            } if renderer.draws(language) => wanted.push(Wanted::Diagram {
                language,
                source: Cow::Borrowed(code),
                line: places.line(path).unwrap_or(0),
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
            // A table without parsed cells has only text in them.
            Block::Table(table) if inline => {
                for cell in table.parsed_cells().into_iter().flatten() {
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
        path.pop();
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
mod tests;
