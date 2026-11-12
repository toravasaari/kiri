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
type Outcome = Result<Source, ImageError>;

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
mod tests {
    use std::path::Path;

    use super::*;
    use crate::typeset::diagrams::error::DiagramError;
    use crate::typeset::diagrams::request::{Drawn, Request};

    /// Draws every language that kiri knows, in a block or in a text.
    struct Everything;

    impl Renderer for Everything {
        fn draws(&self, language: &str) -> bool {
            crate::typeset::diagrams::language::language(language).is_some()
        }

        fn render(&self, _: &Request) -> Result<Drawn, DiagramError> {
            unreachable!("nothing is drawn")
        }
    }

    #[test]
    fn the_embeds_of_the_model_are_those_the_plan_wants_in_the_same_order() {
        use crate::typeset::targets::{EmbedKind, targets};
        let mut seen = 0;
        for path in crate::testing::documents() {
            let document = crate::typeset::parse::parse(&std::fs::read_to_string(&path).unwrap());
            let mut wanted = Vec::new();
            collect_blocks(
                &document.blocks,
                (&document.places, &mut Vec::new()),
                &Everything,
                (None, true),
                &mut wanted,
            );
            let wanted: Vec<String> = (wanted.iter())
                .map(|image| match image {
                    Wanted::Url(url) => format!("image {url}"),
                    Wanted::Diagram { language, .. }
                        if ["math", INLINE_FORMULA].contains(language) =>
                    {
                        "formula".to_owned()
                    }
                    Wanted::Diagram { language, .. } => format!("diagram {language}"),
                    Wanted::Heading { .. } => unreachable!("no headings are asked for"),
                })
                .collect();
            // An image file has no part in the plan of a document: its
            // pixels come with the file.
            let embeds: Vec<String> = (targets([&document]).embeds.iter())
                .filter_map(|embed| match &embed.kind {
                    EmbedKind::Image { url } => Some(format!("image {url}")),
                    EmbedKind::Formula => Some("formula".to_owned()),
                    EmbedKind::Diagram { language } => Some(format!("diagram {language}")),
                    EmbedKind::ImageFile { .. } => None,
                })
                .collect();
            assert_eq!(embeds, wanted, "{}", path.display());
            seen += embeds.len();
        }
        assert!(seen > 50, "{seen}");
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
            let (wanted, _) = plan(&parts, &NoRenderer, &Looks::default(), None);
            let urls: Vec<String> = (wanted.iter())
                .map(|(_, image)| image.key(&Looks::default()).to_string())
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
            let (wanted, _) = plan(&parts, &NoRenderer, &Looks::default(), None);
            let urls: Vec<String> = (wanted.iter())
                .map(|(_, image)| image.key(&Looks::default()).to_string())
                .collect();
            urls
        };
        assert!(urls(false).is_empty());
        assert_eq!(urls(true), ["s.png", "i.png"]);
    }

    /// Draws math, and nothing is drawn by it here.
    struct Math;

    impl Renderer for Math {
        fn draws(&self, language: &str) -> bool {
            language == INLINE_FORMULA || language == "math"
        }

        fn render(&self, _: &Request) -> Result<Drawn, DiagramError> {
            unreachable!("nothing is drawn")
        }
    }

    /// Draws every diagram language the test sources use.
    struct Diagrams;

    impl Renderer for Diagrams {
        fn draws(&self, language: &str) -> bool {
            language == "mermaid"
        }

        fn render(&self, _: &Request) -> Result<Drawn, DiagramError> {
            unreachable!("nothing is drawn")
        }
    }

    /// The lines the diagrams of `source` are said to start on, in order.
    fn diagram_lines(source: &str) -> Vec<usize> {
        let document = crate::typeset::parse::parse(source);
        let parts = [Part {
            document: &document,
            number: 0,
            base: Path::new(""),
            by_line: true,
            inline_images: false,
        }];
        let (wanted, _) = plan(&parts, &Diagrams, &Looks::default(), None);
        (wanted.iter())
            .filter_map(|(_, image)| match image {
                Wanted::Diagram { line, .. } => Some(*line),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_diagram_is_named_by_the_line_it_starts_on_wherever_it_is() {
        let source = "\
---
title: x
---
# Heading

```mermaid
graph TD
  a --> a
```

- item

  > ```mermaid
  > graph TD
  >   b --> b
  > ```

<details>
<summary>More</summary>

```mermaid
graph TD
  c --> c
```

<pre lang=\"mermaid\">graph TD
  d --> d</pre>

</details>
";
        assert_eq!(diagram_lines(source), [6, 13, 21, 26]);
    }

    #[test]
    fn the_keys_wanted_are_in_the_order_of_the_documents_with_their_numbers() {
        let one = crate::typeset::parse::parse("![a](a.png)\n\n```math\nx\n```\n");
        let two = crate::typeset::parse::parse("![a](a.png)\n");
        let part = |document, number| Part {
            document,
            number,
            base: Path::new(""),
            by_line: true,
            inline_images: false,
        };
        let parts = [part(&one, 0), part(&two, 1)];
        assert_eq!(
            keys(&parts, &Math, &Looks::default()),
            [
                ImageKey::from("a.png").in_document(0),
                ImageKey::rendered("math", "x\n").in_document(0),
                ImageKey::from("a.png").in_document(1),
            ]
        );
        // Without a renderer for it, the block is no image.
        assert_eq!(keys(&parts, &NoRenderer, &Looks::default()).len(), 2);
    }

    #[test]
    fn formulas_in_the_middle_of_text_are_wanted_only_when_asked_for() {
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
            let (wanted, _) = plan(&parts, renderer, &Looks::default(), None);
            let keys: Vec<String> = (wanted.iter())
                .map(|(_, image)| image.key(&Looks::default()).to_string())
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
    fn an_inline_formula_is_keyed_as_reading_mode_asks_for_it() {
        let document = crate::typeset::parse::parse("Let $x$ be.\n");
        let looks = Looks::default();
        let parts = [Part {
            document: &document,
            number: 0,
            base: Path::new(""),
            by_line: true,
            inline_images: true,
        }];
        let (wanted, _) = plan(&parts, &Math, &looks, None);
        let keys: Vec<ImageKey> = wanted.iter().map(|(_, image)| image.key(&looks)).collect();
        // Reading mode adds the same "\n" before it asks.
        let asked = ImageKey::diagram(INLINE_FORMULA, "x\n", looks.diagram, 0);
        assert_eq!(keys, [asked]);
    }
}
