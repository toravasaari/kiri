//! Proof that comrak's inline positions are enough for a map from the text
//! of a block to the bytes it was written as. The map itself is not built:
//! the aligning here is a test helper that shows it can be.
//!
//! Every inline node that has text (`Text`, `Code`, `Math`, `ShortCode`,
//! `HtmlInline`) gets its bytes in the source from its `sourcepos`, through
//! the same table of line starts the parser uses for blocks. The node's text
//! is then found in those bytes character by character, where the two may
//! differ in four ways: a backslash escape is one character, an entity
//! stands for one or two characters, and after a line break the aligning
//! may skip the start a container puts on a line (`>`, indentation), which
//! is also the indentation of the lines of display math. The aligning is
//! looser than that: it may skip any run of spaces, tabs and `>` after a
//! line break, and any `&...;` may stand for any one or two characters. That
//! is enough to show that a map can be made, not a check of a map. A short
//! code and inline HTML map as a whole. A code span and a formula are inside
//! their delimiters, which are skipped.
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

/// How the model reads the text under a container, for the two texts the
/// model gives: `Inline::plain_text` and the document's text (`flatten`).
#[derive(Clone, Copy, PartialEq)]
enum Reads {
    /// A formula as written between dollars, a hard break as a space.
    PlainText,
    /// A formula as its LaTeX, a hard break as a line break.
    DocumentText,
}

/// Math as the model has it when kiri does not draw it: what
/// `math_as_written` reads, as plain text. The `Math` node gives the text
/// either way, so this does not weaken the proof.
fn written_math(math: &comrak::nodes::NodeMath) -> String {
    Inline::plain_text(&math_as_written(math))
}

/// What one tag of inline HTML is in the text, when kiri reads it as it
/// reads an element: a tag that starts or ends one is no text, a line break
/// is one, an image is its alt text. Other tags stay as they are.
fn tag_text(tag: &str, reads: Reads) -> String {
    if html::is_line_break(tag) {
        return if reads == Reads::PlainText { " " } else { "\n" }.to_owned();
    }
    if let Some(img) = html::inline_img(tag) {
        return img.alt;
    }
    let element = html::phrase_start(tag).is_some()
        || html::phrase_end(tag).is_some()
        || html::link_start(tag).is_some()
        || html::is_link_end(tag);
    if element {
        String::new()
    } else {
        tag.to_owned()
    }
}

/// The text of the inline nodes under `container` as the model reads it,
/// by the rules that `reads` says, and what the container has in it that
/// kiri reads in a way of its own.
fn nodes_text(container: Node<'_>, reads: Reads) -> (String, Feature) {
    let mut text = String::new();
    let mut feature = Feature::Plain;
    for node in container.descendants().skip(1) {
        match &node.data().value {
            NodeValue::Math(math) if math.display_math => {
                text.push_str(&written_math(math));
                feature = Feature::DisplayMath;
            }
            NodeValue::Math(math) => text.push_str(&match reads {
                Reads::PlainText => written_math(math),
                Reads::DocumentText => formula_source(&math.literal),
            }),
            NodeValue::HtmlInline(tag) => {
                text.push_str(&tag_text(tag, reads));
                if feature == Feature::Plain {
                    feature = Feature::InlineHtml;
                }
            }
            NodeValue::SoftBreak => text.push(' '),
            NodeValue::LineBreak => text.push(if reads == Reads::PlainText { ' ' } else { '\n' }),
            _ => {
                if let Some((held, _)) = text_of(node) {
                    text.push_str(&held);
                }
            }
        }
    }
    (text, feature)
}

/// What a container has that kiri reads in a way of its own.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Feature {
    Plain,
    InlineHtml,
    DisplayMath,
}

/// The containers left out of the proof, as the name of the file and the
/// line, counted from 1 in the whole file, where the container starts. They
/// have tags that kiri leaves as source: one that never closes, a mismatched
/// pair, the tags of a `<code>` with other tags inside. The helper here drops
/// every tag that starts or ends an element, without pairing them as
/// `push_inline` does; pairing them would repeat that production code in
/// a test. If one of these starts to match, or another stops, the test says
/// so.
const EXCLUDED: &[(&str, usize)] = &[
    ("S01-text.md", 148),
    ("html-tags.md", 14),
    ("html-tags.md", 96),
];

/// How many containers the fixtures and examples have that are compared, at
/// the least, so that the proof cannot shrink to nothing.
const COMPARED_AT_LEAST: usize = 900;

/// Compares the text of the nodes under every paragraph, heading and table
/// cell of the fixtures and examples with what the model gives. Returns how
/// many match, and the containers that do not, each as the name of its file
/// and its line. Only a container with display math or inline HTML may
/// differ, and it is for [`EXCLUDED`] to name it; a difference in any other
/// container panics.
fn count_texts(reads: Reads) -> (usize, Vec<(String, usize)>) {
    let mut compared = 0;
    let mut differ = Vec::new();
    for path in crate::testing::documents() {
        let source = std::fs::read_to_string(&path).unwrap();
        let markdown = front_matter::split(&source).map_or(source.as_str(), |(_, rest)| rest);
        let skipped = source[..source.len() - markdown.len()]
            .matches('\n')
            .count();
        let arena = Arena::new();
        let root = parse_document(&arena, markdown, &options());
        for node in root.descendants() {
            if !matches!(
                node.data().value,
                NodeValue::Paragraph | NodeValue::Heading(_) | NodeValue::TableCell
            ) {
                continue;
            }
            let (held, feature) = nodes_text(node, reads);
            let inlines = inlines(node, &mut Unknown::default());
            let model = match reads {
                Reads::PlainText => Inline::plain_text(&inlines),
                Reads::DocumentText => {
                    let mut flat = String::new();
                    crate::typeset::text::flatten(&inlines, &mut flat, &mut |_, _| {});
                    flat
                }
            };
            if held == model {
                compared += 1;
                continue;
            }
            let at = node.data().sourcepos;
            assert!(
                feature != Feature::Plain,
                "{} at {at:?}: {held:?} against {model:?}",
                path.display(),
            );
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            differ.push((name, at.start.line + skipped));
        }
    }
    (compared, differ)
}

fn check_texts(reads: Reads) {
    let (compared, differ) = count_texts(reads);
    let differ: Vec<(&str, usize)> = differ
        .iter()
        .map(|(name, line)| (name.as_str(), *line))
        .collect();
    assert_eq!(differ, EXCLUDED, "the containers left out of the proof");
    assert!(compared > COMPARED_AT_LEAST, "{compared}");
}

#[test]
fn the_texts_of_the_nodes_in_order_are_the_text_of_the_inlines_of_the_model() {
    check_texts(Reads::PlainText);
}

#[test]
fn the_texts_of_the_nodes_in_order_are_the_text_the_document_gives_the_block() {
    check_texts(Reads::DocumentText);
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

/// The first math node of `source`.
fn math_in(source: &str) -> comrak::nodes::NodeMath {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &options());
    root.descendants()
        .find_map(|node| match &node.data().value {
            NodeValue::Math(math) => Some(math.clone()),
            _ => None,
        })
        .expect("a math node")
}

#[test]
fn display_math_is_read_as_the_markdown_it_was_written_in() {
    // LaTeX that Markdown reads as escapes, emphasis, entities and a hard
    // break, which a copy of the reading by hand got wrong.
    let cases = [
        (r"a \\[2pt] b", r"$$a \[2pt] b$$"),
        (r"a \\, b", r"$$a \, b$$"),
        ("a*b*c", "$$abc$$"),
        ("x &amp; y", "$$x & y$$"),
        ("a\\\nb", "$$a b$$"),
    ];
    for (literal, written) in cases {
        let math = math_in(&format!("$${literal}$$\n"));
        assert_eq!(written_math(&math), written, "{literal:?}");
        // And it is what the model has for it.
        assert_eq!(
            written_math(&math),
            Inline::plain_text(&math_as_written(&math))
        );
    }
}
