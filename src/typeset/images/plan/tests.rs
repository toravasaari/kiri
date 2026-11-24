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

/// What the plan wants of `document`, as words, with the language of a
/// diagram as kiri's registry names it, as `targets` has it: a formula
/// is a formula however it is written.
fn wanted_by_the_plan(document: &Document) -> Vec<String> {
    let mut wanted = Vec::new();
    collect_blocks(
        &document.blocks,
        (&document.places, &mut Vec::new()),
        &Everything,
        (None, true),
        &mut wanted,
    );
    (wanted.iter())
        .map(|image| match image {
            Wanted::Url(url) => format!("image {url}"),
            Wanted::Diagram { language, .. } => {
                let name = crate::typeset::diagrams::language::language(language)
                    .map_or(*language, |known| known.name);
                match name {
                    "math" | INLINE_FORMULA => "formula".to_owned(),
                    name => format!("diagram {name}"),
                }
            }
            Wanted::Heading { .. } => unreachable!("no headings are asked for"),
        })
        .collect()
}

/// The embeds of the model of `document`, as words as
/// `wanted_by_the_plan` has them. An image file has no part in the plan
/// of a document: its pixels come with the file.
fn embeds_of_the_model(document: &Document) -> Vec<String> {
    use crate::typeset::targets::{EmbedKind, targets};
    (targets([document]).embeds.iter())
        .filter_map(|embed| match &embed.kind {
            EmbedKind::Image { url } => Some(format!("image {url}")),
            EmbedKind::Formula => Some("formula".to_owned()),
            EmbedKind::Diagram { language } => Some(format!("diagram {language}")),
            EmbedKind::ImageFile { .. } => None,
        })
        .collect()
}

#[test]
fn the_embeds_of_the_model_are_those_the_plan_wants_in_the_same_order() {
    let mut seen = 0;
    for path in crate::testing::documents() {
        let document = crate::typeset::parse::parse(&std::fs::read_to_string(&path).unwrap());
        let wanted = wanted_by_the_plan(&document);
        assert_eq!(embeds_of_the_model(&document), wanted, "{}", path.display());
        seen += wanted.len();
    }
    assert!(seen > 50, "{seen}");
}

#[test]
fn the_plan_and_the_model_read_the_language_of_a_code_block_alike() {
    let source = "```Mermaid\na\n```\n\n```MATH\nb\n```\n\n```math-inline\nc\n```\n";
    let document = crate::typeset::parse::parse(source);
    let wanted = wanted_by_the_plan(&document);
    assert_eq!(wanted, ["diagram mermaid", "formula", "formula"]);
    assert_eq!(embeds_of_the_model(&document), wanted);
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
    let asked = ImageKey::diagram(INLINE_FORMULA, "x\n", looks.diagram_look_loading(), 0);
    assert_eq!(keys, [asked]);
}

#[test]
fn the_images_and_headings_inside_a_centred_wrapper_are_wanted_as_at_the_top_level() {
    let image = |url: &str| {
        Block::Paragraph(vec![Inline::Image(crate::typeset::model::Image {
            url: url.into(),
            alt: vec![Inline::Text("alt".into())],
            width: None,
            height: None,
        })])
    };
    let heading = Block::Heading {
        level: 1,
        content: vec![Inline::Text("Name".into())],
    };
    let blocks = [Block::Centered(vec![image("logo.png"), heading])];
    let wanted_in = |headings, blocks: &[Block]| {
        let mut wanted = Vec::new();
        collect_blocks(
            blocks,
            (&SourceMap::default(), &mut Vec::new()),
            &NoRenderer,
            (headings, false),
            &mut wanted,
        );
        (wanted.iter())
            .map(|image| match image {
                Wanted::Url(url) => (*url).to_owned(),
                Wanted::Heading { level, cell, .. } => format!("heading {level} {cell:?}"),
                Wanted::Diagram { .. } => unreachable!("no diagram"),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(wanted_in(None, &blocks), ["logo.png"]);
    assert_eq!(
        wanted_in(Some((10, 20)), &blocks),
        ["logo.png", "heading 1 (10, 20)"]
    );
    // In a quote, as without the wrapper, a heading is not an image.
    let quoted = [Block::BlockQuote(blocks.to_vec())];
    assert_eq!(wanted_in(Some((10, 20)), &quoted), ["logo.png"]);
}
