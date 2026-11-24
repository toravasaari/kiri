//! The HTML that centres what it holds, `<p>` and `<div>` with `align="center"`, `<center>` and a heading with it, read whole or as a tag alone.
//!
//! comrak gives one HTML block for such an element when no blank line is in
//! it, and else one block for its start tag, the blocks inside and one for
//! its end tag. `centred` reads the first, `edge` tells the others.

use std::ops::Range;

use super::{
    Img, Tag, Token, block_img, heading_level, inlines, is_line_break, skip_blank, spanned_tokens,
};
use crate::typeset::model::Inline;

/// An element that centres what is inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    P,
    Div,
    Center,
}

impl Element {
    /// The element that the start tag `tag` opens, when it centres.
    fn of_start(tag: &Tag) -> Option<Self> {
        match tag.name.as_str() {
            "center" => Some(Self::Center),
            "p" if is_centred(tag) => Some(Self::P),
            "div" if is_centred(tag) => Some(Self::Div),
            _ => None,
        }
    }

    /// The element that the end tag `</name>` closes.
    fn of_end(name: &str) -> Option<Self> {
        match name {
            "center" => Some(Self::Center),
            "p" => Some(Self::P),
            "div" => Some(Self::Div),
            _ => None,
        }
    }
}

/// Whether the `align` of `tag` is `center`, in any case, as a browser
/// reads it.
fn is_centred(tag: &Tag) -> bool {
    (tag.get("align")).is_some_and(|align| {
        align
            .trim_matches(|c: char| c.is_ascii_whitespace())
            .eq_ignore_ascii_case("center")
    })
}

/// What an HTML block that is only a tag is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// The start tag of an element that centres.
    Start(Element),
    /// The end tag of `<p>`, `<div>` or `<center>`.
    End(Element),
}

/// The tag that an HTML block is, with nothing else but whitespace and
/// comments: the start of an element that centres, or the end of one that
/// may. `None` for any other HTML.
pub fn edge(html: &str) -> Option<Edge> {
    let tokens = spanned_tokens(html)?;
    let [(token, _), rest @ ..] = skip_blank(&tokens) else {
        return None;
    };
    if !skip_blank(rest).is_empty() {
        return None;
    }
    match token {
        Token::Open(tag) => Element::of_start(tag).map(Edge::Start),
        Token::Close(name) => Element::of_end(name).map(Edge::End),
        _ => None,
    }
}

/// A block inside a centring element, as `centred` reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Heading(u8, Vec<Inline>),
    /// Text and inline HTML.
    Paragraph(Vec<Inline>),
    /// An image, and the link round it, when it is all that the element
    /// holds and `<picture>` or the like is round it.
    Image(Img),
}

/// A block inside a centring element, and the byte of the HTML where it
/// starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub at: usize,
    pub part: Part,
}

/// The blocks of an HTML block that is one element that centres, with
/// nothing else but whitespace and comments around it: `<p>` or `<div>`
/// with `align="center"`, `<center>`, or `<h1>` to `<h6>` with
/// `align="center"`.
///
/// A heading, a `<p>` (its attributes ignored) and runs of inline HTML,
/// the tags that kiri reads in a paragraph, as paragraphs. Inside a `<p>`
/// or a heading only inline HTML is read, as in a browser, where those
/// tags end it. `None` for anything else inside, such as a table, a list
/// or a tag kiri does not read, a heading inside it with an `id`, an element
/// with nothing in it, and text after its end tag.
pub fn centred(html: &str) -> Option<Vec<Piece>> {
    let tokens = spanned_tokens(html)?;
    let [(Token::Open(tag), open), rest @ ..] = skip_blank(&tokens) else {
        return None;
    };
    let heading = heading_level(&tag.name).filter(|_| is_centred(tag));
    let element = Element::of_start(tag);
    if heading.is_none() && element.is_none() {
        return None;
    }
    let end = rest
        .iter()
        .position(|(token, _)| matches!(token, Token::Close(name) if *name == tag.name))?;
    if !skip_blank(&rest[end + 1..]).is_empty() {
        return None;
    }
    let inside = &rest[..end];
    let pieces = match heading {
        Some(level) => {
            let content = inline_content(&html[open.end..rest[end].1.start])?;
            let only_inline = inside.iter().all(|(token, _)| inline_only(token));
            (only_inline && !content.is_empty()).then(|| {
                vec![Piece {
                    at: open.start,
                    part: Part::Heading(level, content),
                }]
            })
        }
        None => read(html, inside, element != Some(Element::P)),
    };
    if let Some(pieces) = pieces.filter(|pieces| !pieces.is_empty()) {
        return Some(pieces);
    }
    // An image in tags that only wrap it, such as a `<picture>`.
    let img = block_img(html)?;
    Some(vec![Piece {
        at: open.start,
        part: Part::Image(img),
    }])
}

/// The pieces of `tokens`, the inside of an element. `blocks` is whether
/// `<p>` and headings may be inside it. `None` when something else is.
fn read(html: &str, tokens: &[(Token<'_>, Range<usize>)], blocks: bool) -> Option<Vec<Piece>> {
    let mut found = Vec::new();
    // The token where the run of inline HTML so far starts.
    let mut run = 0;
    let mut at = 0;
    while let Some((token, span)) = tokens.get(at) {
        match token {
            Token::Open(tag)
                if blocks && (tag.name == "p" || heading_level(&tag.name).is_some()) =>
            {
                paragraph(html, &tokens[run..at], &mut found)?;
                let close = tokens[at + 1..].iter().position(
                    |(token, _)| matches!(token, Token::Close(name) if *name == tag.name),
                )? + at
                    + 1;
                if !tokens[at + 1..close]
                    .iter()
                    .all(|(token, _)| inline_only(token))
                {
                    return None;
                }
                // A heading with an `id` is a place that links go to, and the
                // model's heading has no `id`: the block stays HTML, where
                // `html_headings` finds it.
                if tag.get("id").is_some_and(|id| !id.is_empty()) {
                    return None;
                }
                let content = inline_content(&html[span.end..tokens[close].1.start])?;
                let part = match heading_level(&tag.name) {
                    Some(level) if !content.is_empty() => Part::Heading(level, content),
                    Some(_) => return None,
                    None => Part::Paragraph(content),
                };
                if !matches!(&part, Part::Paragraph(content) if content.is_empty()) {
                    found.push(Piece {
                        at: span.start,
                        part,
                    });
                }
                at = close + 1;
                run = at;
            }
            token if inline_only(token) => at += 1,
            _ => return None,
        }
    }
    paragraph(html, &tokens[run..], &mut found)?;
    Some(found)
}

/// A paragraph of the inline HTML in `tokens`, added to `found` unless it
/// is empty. `None` when `inlines` cannot read it.
fn paragraph(
    html: &str,
    tokens: &[(Token<'_>, Range<usize>)],
    found: &mut Vec<Piece>,
) -> Option<()> {
    let tokens = skip_blank(tokens);
    let (Some((_, first)), Some((_, last))) = (tokens.first(), tokens.last()) else {
        return Some(());
    };
    let text = &html[first.start..last.end];
    let content = inline_content(text)?;
    if !content.is_empty() {
        // The text starts at its first word, not at the blank before it.
        let blank = text.len()
            - text
                .trim_start_matches(|c: char| c.is_ascii_whitespace())
                .len();
        found.push(Piece {
            at: first.start + blank,
            part: Part::Paragraph(content),
        });
    }
    Some(())
}

/// The inlines of `html`, with no space at the start of a line after a
/// `<br>`, where a browser shows none.
fn inline_content(html: &str) -> Option<Vec<Inline>> {
    let mut content = inlines(html)?;
    for at in 1..content.len() {
        if matches!(&content[at - 1], Inline::Html(tag) if is_line_break(tag))
            && let Inline::Text(text) = &mut content[at]
        {
            *text = text.trim_start_matches(' ').to_owned();
        }
    }
    content.retain(|inline| !matches!(inline, Inline::Text(text) if text.is_empty()));
    Some(content)
}

/// Whether `token` may be in a paragraph of a centring element: text, a
/// comment, or one of the tags that kiri reads in text.
fn inline_only(token: &Token<'_>) -> bool {
    const TAGS: [&str; 14] = [
        "kbd", "mark", "sub", "sup", "b", "strong", "i", "em", "s", "del", "code", "u", "a", "br",
    ];
    match token {
        Token::Text(_) | Token::Comment => true,
        Token::Open(Tag { name, .. }) => name == "img" || TAGS.contains(&name.as_str()),
        Token::Close(name) => TAGS.contains(&name.as_str()),
    }
}

#[cfg(test)]
mod tests;
