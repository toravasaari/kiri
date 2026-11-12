//! Proof that comrak's inline positions are enough for a map from the text
//! of a block to the bytes it was written as. The map itself is not built:
//! the aligning here is a test helper that shows it can be.
//!
//! Every inline node that has text (`Text`, `Code`, `Math`, `ShortCode`,
//! `HtmlInline`) gets its bytes in the source from its `sourcepos`, through
//! the same table of line starts the parser uses for blocks. The node's text
//! is then found in those bytes character by character, with four rules for
//! where the two differ: a backslash escape is one character, an entity is
//! one or two characters, the start a container puts on a line after a
//! line break (`>`, indentation) is skipped, and so is the indentation of
//! the lines of display math. A short code and inline HTML map as a whole.
//! A code span and a formula are inside their delimiters, which are skipped.
//!
//! What this covers is the Markdown that comrak reads. The texts of front
//! matter (kiri's YAML, TOML and JSON readers), of the summary of a
//! `<details>` that kiri's own HTML reader gives, and of a CSV file do not
//! come from comrak. A map for them would come from those readers, which
//! kiri holds, so they do not stand in the way of a map.

use comrak::Node;
use comrak::nodes::{NodeValue, Sourcepos};

use super::*;
use crate::typeset::model::Inline;

/// The text a node holds, and whether it is mapped as a whole.
enum Kind {
    /// Aligned character by character.
    Text,
    /// A code span or a formula: between delimiters, aligned inside them.
    Delimited(char),
    /// A short code or inline HTML: the text is not in the bytes.
    Whole,
}

/// The text of `node`, if it is a node that holds text.
fn text_of(node: Node<'_>) -> Option<(String, Kind)> {
    Some(match &node.data().value {
        NodeValue::Text(text) => (text.to_string(), Kind::Text),
        NodeValue::Code(code) => (code.literal.clone(), Kind::Delimited('`')),
        NodeValue::Math(math) => (math.literal.clone(), Kind::Delimited('$')),
        NodeValue::ShortCode(code) => (code.emoji.clone(), Kind::Whole),
        NodeValue::HtmlInline(tag) => (tag.clone(), Kind::Whole),
        _ => return None,
    })
}

/// Whether `text` is what the characters of `source` say, under the rules.
fn aligns(source: &[char], text: &[char], start: bool) -> bool {
    let (Some(&s), t) = (source.first(), text.first()) else {
        return text.is_empty();
    };
    // The same character.
    if Some(&s) == t && aligns(&source[1..], &text[1..], false) {
        return true;
    }
    // A backslash escape is one character: `\*` is `*`.
    if s == '\\' && source.get(1).is_some_and(char::is_ascii_punctuation) {
        return aligns(&source[1..], text, false);
    }
    // An entity is one character or two: `&amp;`, `&#x1F600;`, `&ngE;`.
    if s == '&'
        && let Some(end) = source.iter().take(34).position(|&c| c == ';')
        && source[1..end]
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == '#')
    {
        for taken in 1..=2 {
            if text.len() >= taken && aligns(&source[end + 1..], &text[taken..], false) {
                return true;
            }
        }
    }
    // A line break, which the text keeps or turns into a space, and after
    // it what a container puts at the start of a line: `>` and indentation.
    if s == '\n' || s == '\r' {
        let breaks = if s == '\r' && source.get(1) == Some(&'\n') {
            2
        } else {
            1
        };
        let after = &source[breaks..];
        let kept = match t {
            Some('\n' | ' ') => &text[1..],
            _ => text,
        };
        let skippable = after
            .iter()
            .take_while(|c| matches!(c, ' ' | '\t' | '>'))
            .count();
        for skipped in 0..=skippable {
            if aligns(&after[skipped..], kept, false) {
                return true;
            }
        }
    }
    // A code span drops one space at each end when both have one.
    if s == ' ' && start && aligns(&source[1..], text, false) {
        return true;
    }
    source == [' '] && text.is_empty()
}

/// The part of `bytes` between the delimiters of a code span or a formula:
/// the run of `delimiter` at each end.
fn inside_delimiters(bytes: &str, delimiter: char) -> &str {
    let rest = bytes.trim_start_matches(delimiter);
    let run = bytes.len() - rest.len();
    bytes
        .get(run..bytes.len().saturating_sub(run))
        .unwrap_or_default()
}

/// What a node's bytes in the source are, by the same table of line starts
/// the parser uses for blocks.
fn bytes_of<'a>(node: Node<'_>, text: &Text<'a>) -> Option<&'a str> {
    let at: Sourcepos = node.data().sourcepos;
    let found = Found {
        sourcepos: at,
        line: at.start.line,
        extent: Extent::Exact,
    };
    text.source.get(text.place(found).bytes)
}

/// Every text-bearing inline node of `source` with the problems that its
/// bytes do not give it, for the places that fail.
fn problems(source: &str) -> (usize, Vec<String>) {
    let markdown = match front_matter::split(source) {
        Some((_, markdown)) => markdown,
        None => source,
    };
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &options());
    let text = Text::new(source, source.len() - markdown.len());
    let (mut seen, mut failed) = (0, Vec::new());
    for node in root.descendants() {
        let Some((held, kind)) = text_of(node) else {
            continue;
        };
        seen += 1;
        let at = node.data().sourcepos;
        let Some(bytes) = bytes_of(node, &text) else {
            failed.push(format!(
                "{at:?}: bytes outside the source or inside a character"
            ));
            continue;
        };
        let ok = match kind {
            Kind::Whole => true,
            Kind::Text => aligns(
                &bytes.chars().collect::<Vec<_>>(),
                &held.chars().collect::<Vec<_>>(),
                false,
            ),
            Kind::Delimited(delimiter) => aligns(
                &inside_delimiters(bytes, delimiter)
                    .chars()
                    .collect::<Vec<_>>(),
                &held.chars().collect::<Vec<_>>(),
                true,
            ),
        };
        if !ok {
            failed.push(format!("{at:?}: {held:?} is not in {bytes:?}"));
        }
    }
    (seen, failed)
}

#[test]
fn the_inline_nodes_of_the_fixtures_and_examples_find_their_bytes() {
    let mut seen = 0;
    for path in crate::testing::documents() {
        let source = std::fs::read_to_string(&path).unwrap();
        let (count, failed) = problems(&source);
        assert!(failed.is_empty(), "{}: {failed:#?}", path.display());
        seen += count;
    }
    assert!(seen > 1000, "{seen}");
}

#[test]
fn the_hard_cases_find_their_bytes() {
    let cases = [
        // A list in a quote, and a lazy continuation line.
        "> - one\n>   two\n> - three\nlazy\n",
        "> a quote\nlazy line of it\n",
        // A table cell.
        "| a | b |\n|---|---|\n| ä好 | `c` |\n",
        // Characters of more than one byte.
        "ä好 and 好ä\n\n> 好 *ä*\n",
        // CRLF.
        "# A\r\n\r\nb\r\nc\r\n\r\n> d\r\n> e\r\n",
        // An entity and an escape.
        "fish &amp; chips \\* not \\\\ &#x1F600; &copy; &ngE; x\n",
        // A short code.
        "a :rocket: b\n",
        // An inline formula, and a formula in backticks.
        "the $x^2$ and $`y`$ here\n",
        // A display formula in a paragraph, indented.
        "before\n$$\n  a + b\n  c\n$$\nafter\n",
        // A code span that goes on over the line break of a quote.
        "> a `code\n> span` here\n",
        "`` ` `` and ` x `\n",
        // Inline HTML.
        "a <kbd>b</kbd> c\n",
        // Front matter in front.
        "---\ntitle: ä\n---\n\n> *a* &amp; b\n",
    ];
    for source in cases {
        let (seen, failed) = problems(source);
        assert!(seen > 0, "{source:?}");
        assert!(failed.is_empty(), "{source:?}: {failed:#?}");
    }
}

/// The text of the inline nodes under `container`, as kiri's model flattens
/// them without formulas, hard breaks and tags.
fn nodes_text(container: Node<'_>) -> Option<String> {
    let mut text = String::new();
    for node in container.descendants().skip(1) {
        match &node.data().value {
            NodeValue::Math(_) | NodeValue::LineBreak | NodeValue::HtmlInline(_) => return None,
            NodeValue::SoftBreak => text.push(' '),
            _ => {
                if let Some((held, _)) = text_of(node) {
                    text.push_str(&held);
                }
            }
        }
    }
    Some(text)
}

#[test]
fn the_texts_of_the_nodes_in_order_are_the_text_of_the_inlines_of_the_model() {
    let mut compared = 0;
    for path in crate::testing::documents() {
        let source = std::fs::read_to_string(&path).unwrap();
        let markdown = front_matter::split(&source).map_or(source.as_str(), |(_, rest)| rest);
        let arena = Arena::new();
        let root = parse_document(&arena, markdown, &options());
        for node in root.descendants() {
            let container = matches!(
                node.data().value,
                NodeValue::Paragraph | NodeValue::Heading(_) | NodeValue::TableCell
            );
            let Some(held) = container.then(|| nodes_text(node)).flatten() else {
                continue;
            };
            let model = Inline::plain_text(&inlines(node, &mut Unknown::default()));
            assert_eq!(
                held,
                model,
                "{} at {:?}",
                path.display(),
                node.data().sourcepos
            );
            compared += 1;
        }
    }
    assert!(compared > 500, "{compared}");
}

#[test]
fn the_aligning_does_not_accept_text_that_is_not_in_the_bytes() {
    let chars = |text: &str| text.chars().collect::<Vec<_>>();
    assert!(!aligns(&chars("abc"), &chars("abd"), false));
    assert!(!aligns(&chars("abc"), &chars("ab"), false));
    assert!(!aligns(&chars("a &amp; b"), &chars("a b"), false));
    assert!(aligns(&chars("a &amp; b"), &chars("a & b"), false));
    assert!(aligns(&chars("a\n> b"), &chars("a b"), false));
}

/// The text of the inline nodes under `container` as the document's text
/// has it: an inline formula as its LaTeX, a hard break as a line break.
/// `None` for a container with inline HTML, which kiri reads into elements,
/// or display math, which kiri reads as the Markdown it was written in.
fn document_text_of_nodes(container: Node<'_>) -> Option<String> {
    let mut text = String::new();
    for node in container.descendants().skip(1) {
        match &node.data().value {
            NodeValue::Math(math) if math.display_math => return None,
            NodeValue::Math(math) => text.push_str(&formula_source(&math.literal)),
            NodeValue::HtmlInline(_) => return None,
            NodeValue::SoftBreak => text.push(' '),
            NodeValue::LineBreak => text.push('\n'),
            _ => {
                if let Some((held, _)) = text_of(node) {
                    text.push_str(&held);
                }
            }
        }
    }
    Some(text)
}

#[test]
fn the_texts_of_the_nodes_in_order_are_the_text_the_document_gives_the_block() {
    let mut compared = 0;
    for path in crate::testing::documents() {
        let source = std::fs::read_to_string(&path).unwrap();
        let markdown = front_matter::split(&source).map_or(source.as_str(), |(_, rest)| rest);
        let arena = Arena::new();
        let root = parse_document(&arena, markdown, &options());
        for node in root.descendants() {
            let container = matches!(
                node.data().value,
                NodeValue::Paragraph | NodeValue::Heading(_) | NodeValue::TableCell
            );
            let Some(held) = container.then(|| document_text_of_nodes(node)).flatten() else {
                continue;
            };
            let mut flat = String::new();
            let inlines = inlines(node, &mut Unknown::default());
            crate::typeset::text::flatten(&inlines, &mut flat, &mut |_, _| {});
            assert_eq!(
                held,
                flat,
                "{} at {:?}",
                path.display(),
                node.data().sourcepos
            );
            compared += 1;
        }
    }
    assert!(compared > 500, "{compared}");
}
