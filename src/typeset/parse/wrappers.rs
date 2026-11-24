//! The HTML elements that wrap blocks of Markdown, joined into one block that holds the blocks between their tags.
//!
//! comrak gives an element with blank lines in it as an HTML block for its
//! start tag, the blocks inside it and an HTML block for its end tag.
//!
//! Today the one element is `<details>`.

use comrak::nodes::Sourcepos;

use super::html::{self, DetailsPart, Part};
use super::{Block, Extent, Found, Inline, Position, html_element, html_inlines, linked_image};

/// The block that an HTML block of one element that centres is, and the
/// places of the blocks in it, which are read from `whole`, the place of
/// the HTML block. `None` for any other HTML.
pub(super) fn centred(html: &str, whole: Sourcepos) -> Option<(Block, Vec<Position>)> {
    let (blocks, positions) = html::centred(html)?
        .into_iter()
        .map(|piece| {
            let block = match piece.part {
                Part::Heading(level, content) => Block::Heading {
                    level,
                    content: html_inlines(content),
                },
                Part::Paragraph(content) => Block::Paragraph(html_inlines(content)),
                Part::Image(img) => Block::Paragraph(vec![linked_image(img)]),
            };
            let line = whole.start.line + html[..piece.at].matches('\n').count();
            (block, Position::inside(whole, line))
        })
        .unzip();
    Some((Block::Centered(blocks), positions))
}

/// `blocks` with each `<details>` in them as one block. comrak gives a
/// `<details>` with blank lines in it as an HTML block that starts it,
/// the blocks inside and an HTML block that ends it. One that never ends
/// runs to the end of `blocks`, as in a browser.
///
/// comrak ends an HTML block only at a blank line, so a block can hold
/// more than a `<details>` with its `<summary>`, or more than a
/// `</details>`. A block that starts with a `<details>`, or that holds a
/// `</details>` while one is open, is read tag by tag; the HTML between
/// its tags is read as the body of a `<details>` in one block is.
///
/// A `<details>` starts where its first tag does, and the blocks read from
/// the HTML inside it start where the HTML block they are in does.
pub(super) fn join_details(
    blocks: impl Iterator<Item = (Block, Position)>,
) -> Vec<(Block, Position)> {
    let mut joined = Vec::new();
    // Each `<details>` still open: its summary, its blocks so far and
    // the HTML block its first tag is in.
    type Open = (Option<Vec<Inline>>, Vec<(Block, Position)>, Sourcepos);
    let mut open: Vec<Open> = Vec::new();
    fn push(joined: &mut Vec<(Block, Position)>, open: &mut [Open], block: (Block, Position)) {
        match open.last_mut() {
            Some((_, blocks, _)) => blocks.push(block),
            None => joined.push(block),
        }
    }
    for (block, position) in blocks {
        let at = position.sourcepos();
        if let Block::Html(source) = &block {
            if let Some(parts) = html::details_parts(source)
                && reads_details(&parts, !open.is_empty())
            {
                for part in parts {
                    match part {
                        DetailsPart::Start(summary) => {
                            open.push((summary.map(html_inlines), Vec::new(), at));
                        }
                        DetailsPart::End(tag) => match open.pop() {
                            Some((summary, blocks, first)) => {
                                let end = Some(at.end);
                                let block = details(summary, blocks, first, end);
                                push(&mut joined, &mut open, block);
                            }
                            // A `</details>` with none open stays source.
                            None => push(
                                &mut joined,
                                &mut open,
                                (
                                    Block::Html(tag.to_owned()),
                                    Position::inside(at, at.start.line),
                                ),
                            ),
                        },
                        DetailsPart::Html(body) => {
                            for block in details_body(body) {
                                push(
                                    &mut joined,
                                    &mut open,
                                    (block, Position::inside(at, at.start.line)),
                                );
                            }
                        }
                    }
                }
                continue;
            }
            // A `<summary>` after a blank line is still the summary when
            // nothing came before it.
            if let Some((summary @ None, blocks, _)) = open.last_mut()
                && blocks.is_empty()
                && let Some(content) = html::summary(source)
            {
                *summary = Some(html_inlines(content));
                continue;
            }
        }
        push(&mut joined, &mut open, (block, position));
    }
    while let Some((summary, blocks, first)) = open.pop() {
        push(
            &mut joined,
            &mut open,
            details(summary, blocks, first, None),
        );
    }
    joined
}

/// Whether `join_details` reads an HTML block of `parts` tag by tag: when
/// it starts with a `<details>`, or holds a `</details>` and one is `open`.
fn reads_details(parts: &[DetailsPart], open: bool) -> bool {
    match parts {
        [DetailsPart::Start(_), ..] => true,
        _ => open && (parts.iter()).any(|part| matches!(part, DetailsPart::End(_))),
    }
}

/// A `<details>` with `summary`, or with the one a browser gives it
/// without, that starts where the HTML block `first` does and ends with
/// the HTML block that closes it, which ends at `end`, or else with the
/// last block inside it.
fn details(
    summary: Option<Vec<Inline>>,
    blocks: Vec<(Block, Position)>,
    first: Sourcepos,
    end: Option<comrak::nodes::LineColumn>,
) -> (Block, Position) {
    let (blocks, children): (Vec<_>, Vec<_>) = blocks.into_iter().unzip();
    let block = Block::Details {
        summary: summary.unwrap_or_else(|| vec![Inline::Text("Details".into())]),
        blocks,
    };
    let end = end
        .or_else(|| children.last().map(|last| last.sourcepos().end))
        .unwrap_or(first.end);
    let position = Position {
        at: Some(Found {
            sourcepos: Sourcepos {
                start: first.start,
                end,
            },
            line: first.start.line,
            extent: Extent::Exact,
        }),
        children,
    };
    (block, position)
}

/// The blocks of HTML inside a `<details>` that is not Markdown: what
/// `html_element` reads, or its text as one paragraph.
fn details_body(body: &str) -> Vec<Block> {
    if let Some(block) = html_element(body) {
        return vec![block];
    }
    match html::plain_text(body) {
        Some(text) if !text.is_empty() => vec![Block::Paragraph(vec![Inline::Text(text)])],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
