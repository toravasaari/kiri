//! The cell layout's side of the parity test: what the layout decided for
//! a block, as facts the pixel layout's summary gives too.

use crate::typeset::layout::{self, table, with_layout};
use crate::typeset::model::{Block, Document, Inline};
use crate::typeset::parity::{self, Apart, Fact, Field, Setup};
use crate::typeset::style::{Line, Span, Style, text};

/// The facts of `block`, laid out alone in the cell layout of `setup`.
pub(crate) fn summary(block: &Block, setup: &Setup) -> Vec<Fact> {
    let inputs = setup.inputs();
    let lines = |block: &Block| layout::layout(&Document::new(vec![block.clone()]), &inputs);
    let mut facts = Vec::new();
    match block {
        Block::Heading { level, content } => {
            // Laid out as plain text, so that a script or a link at the
            // start does not set the size the text is read by.
            let flat = Block::Heading {
                level: *level,
                content: plain(content),
            };
            let lines = lines(&flat);
            let colour = with_layout(&inputs, 0, |layout, _, _| layout.theme.rule);
            let rule = lines.iter().any(|line| {
                let spans = || line.spans.iter().filter(|span| !span.text.is_empty());
                spans().count() > 0
                    && spans()
                        .all(|span| span.style.fg == colour && span.text.chars().all(|c| c == '─'))
            });
            let at = format!("level {level}");
            // An empty heading has no text to tell the size by.
            if let Some(span) = first_text(&lines).filter(|_| !content.is_empty()) {
                let size = format!("{:.2}", span.style.size.value());
                facts.push(Fact::new(Field::HeadingSize, &at, size));
            }
            facts.push(Fact::new(Field::HeadingRule, at, rule.to_string()));
        }
        Block::BlockQuote(blocks) if matches!(blocks.first(), Some(Block::Paragraph(_))) => {
            // The first paragraph as plain text, so that a link or code at
            // its start does not stand for the quote's colour.
            let mut flat = blocks.clone();
            if let Block::Paragraph(content) = &mut flat[0] {
                *content = plain(content);
            }
            let own =
                |lines: &[Line]| first_text(lines).map(|span| (span.style.fg, span.style.dim));
            let flat = Block::BlockQuote(flat);
            let alone = Block::Paragraph(vec![Inline::Text("x".into())]);
            let value = match own(&lines(&flat)) == own(&lines(&alone)) {
                true => "own",
                false => "dimmed",
            };
            facts.push(Fact::new(Field::QuoteText, "first paragraph", value));
        }
        Block::Table(_) => {
            let rows = with_layout(&inputs, 0, |layout, _, _| {
                layout.table_styles(Style::default()).rows
            });
            let rows = match rows {
                table::Rows::Plain => "plain",
                table::Rows::Lines => "lines",
                table::Rows::Indent => "indent",
                table::Rows::Underline { .. } => "underline",
                table::Rows::Stripes { .. } => "stripes",
            };
            // A table too narrow for a box is stacked, with no frame.
            let framed =
                (lines(block).first()).is_some_and(|line| text(&line.spans).starts_with('╭'));
            let frame = if framed { "frame" } else { "stacked" };
            let (rows_at, frame_at) = (
                format!("{:?}", setup.table_rows),
                format!("{:?}", setup.table_style),
            );
            facts.push(Fact::new(Field::TableRows, rows_at, rows));
            facts.push(Fact::new(Field::TableFrame, frame_at, frame));
        }
        _ => {}
    }
    facts.extend(markers(block, &lines(block), &inputs));
    for (kind, content) in parity::apart(block) {
        // Set apart if it makes the text look other than it does without.
        let alone = |content: Vec<Inline>| lines(&Block::Paragraph(content));
        let looks = |lines: &[Line]| -> Vec<(String, Style)> {
            (lines.iter().flat_map(|line| &line.spans))
                .map(|span| (span.text.clone(), span.style.clone()))
                .collect()
        };
        let apart = looks(&alone(vec![kind.wrap(content.clone())])) != looks(&alone(content));
        facts.push(inline_fact(kind, apart));
    }
    facts
}

/// `content` as one run of plain text.
fn plain(content: &[Inline]) -> Vec<Inline> {
    vec![Inline::Text(Inline::plain_text(content))]
}

fn inline_fact(kind: Apart, apart: bool) -> Fact {
    let value = if apart { "styled" } else { "unstyled" };
    Fact::new(Field::InlineStyle, kind.name(), value)
}

/// Whether `span` only lines text up or draws the bar of a quote or alert.
fn is_frame(span: &Span) -> bool {
    matches!(span.text.trim(), "" | "│" | ">" | "▎")
}

/// The first span of `lines` that has text of its own: not a quote's bar
/// or the blank that lines things up.
fn first_text(lines: &[Line]) -> Option<&Span> {
    (lines.iter().flat_map(|line| &line.spans)).find(|span| !is_frame(span))
}

/// The markers and task boxes of the list items in `block`, read off its
/// `lines`: the first span of an item's first line that has a marker's
/// colour and reads as one.
fn markers(block: &Block, lines: &[Line], inputs: &crate::render::Inputs) -> Vec<Fact> {
    let slots = parity::slots(block);
    if slots.is_empty() {
        return Vec::new();
    }
    let colour = with_layout(inputs, 0, |layout, _, _| layout.theme.list_marker);
    let mut read: Vec<(String, Option<String>)> = Vec::new();
    for line in lines {
        let mut spans = (line.spans.iter()).filter(|span| !is_frame(span));
        let Some(first) = spans.next() else { continue };
        let word = first.text.trim();
        let is_box = matches!(word, "☑" | "☐");
        if first.style.fg != colour || !(is_box || parity::is_marker(word)) {
            continue;
        }
        // A numbered task has its box after the number.
        let after = spans
            .next()
            .map(|span| span.text.trim())
            .filter(|word| matches!(*word, "☑" | "☐"));
        read.push(match is_box {
            true => ("none".to_owned(), Some(word.to_owned())),
            false => (parity::marker_value(word), after.map(str::to_owned)),
        });
    }
    parity::marker_facts(&slots, &read)
}
