//! The HTML that wraps blocks: elements that centre, read whole, and the tags of `<p>`, `<div>`, `<center>` and `<details>` in an HTML block.
//!
//! An element that centres (`<p>` or `<div>` with `align="center"`,
//! `<center>`, a heading with it) is one HTML block when no blank line is
//! in it, and `centred` reads it. Else the joiner of wrappers joins its
//! blocks by the tags that `events` reads.

use std::ops::Range;

use super::{
    Img, Phrase, Tag, Token, block_img, heading_level, id_of, inlines, is_line_break, skip_blank,
    spanned_tokens,
};
use crate::typeset::model::Inline;

/// Whether the start tag of an element centres what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Centres {
    Always,
    /// When its `align` is `center`.
    ByAlign,
    Never,
}

/// An element whose tags the joiner of wrappers reads, and what its tags
/// do: a row of `ELEMENTS`.
#[derive(Debug, PartialEq, Eq)]
pub struct Element {
    pub name: &'static str,
    pub centres: Centres,
    /// A browser ends it without its end tag, at the end tag of an element
    /// round it and at the start of an element that ends it.
    pub implied_end: bool,
    /// Its end tag ends the elements still open inside it.
    pub ends_inside: bool,
    /// Its start tag opens it when other things share its HTML block too.
    pub opens_beside: bool,
}

/// The elements whose tags join blocks. The start tag of each ends a `<p>`
/// that is open, as in a browser.
pub static ELEMENTS: [Element; 4] = [
    Element {
        name: "p",
        centres: Centres::ByAlign,
        implied_end: true,
        ends_inside: false,
        opens_beside: true,
    },
    Element {
        name: "div",
        centres: Centres::ByAlign,
        implied_end: false,
        ends_inside: false,
        opens_beside: true,
    },
    Element {
        name: "center",
        centres: Centres::Always,
        implied_end: false,
        ends_inside: false,
        opens_beside: true,
    },
    Element {
        name: "details",
        centres: Centres::Never,
        implied_end: false,
        ends_inside: true,
        opens_beside: false,
    },
];

/// The element of `ELEMENTS` named `name`, in lower case.
fn element(name: &str) -> Option<&'static Element> {
    ELEMENTS.iter().find(|element| element.name == name)
}

impl Element {
    /// Whether its start tag `tag` centres what it holds.
    fn centres_with(&self, tag: &Tag) -> bool {
        match self.centres {
            Centres::Always => true,
            Centres::ByAlign => is_centred(tag),
            Centres::Never => false,
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

/// What a tag of an HTML block is to the joiner of wrappers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// The start tag of an element of `ELEMENTS`, and whether it centres.
    Start(&'static Element, bool),
    /// The end tag of an element of `ELEMENTS`.
    End(&'static Element),
    /// A `<summary>` start tag, with the content up to the `</summary>`
    /// after it and the byte after that; `None` without one.
    Summary(Option<(Vec<Inline>, usize)>),
    /// Text, or a tag of another element.
    Other,
}

/// A tag or a text of an HTML block, and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub mark: Mark,
    pub span: Range<usize>,
}

/// The tags and texts of an HTML block in order, without its comments and
/// its whitespace between tags. `None` for HTML that `tokens` cannot read.
pub fn events(html: &str) -> Option<Vec<Event>> {
    let tokens = spanned_tokens(html)?;
    let mut events = Vec::new();
    for (at, (token, span)) in tokens.iter().enumerate() {
        let mark = match token {
            Token::Comment => continue,
            Token::Text(text) if text.trim().is_empty() => continue,
            Token::Text(_) => Mark::Other,
            Token::Open(tag) if tag.name == "summary" => {
                let close = (tokens[at + 1..].iter())
                    .find(|(token, _)| matches!(token, Token::Close(name) if name == "summary"));
                Mark::Summary(close.and_then(|(_, close)| {
                    Some((inlines(&html[span.end..close.start])?, close.end))
                }))
            }
            Token::Open(tag) => match element(&tag.name) {
                Some(element) => Mark::Start(element, element.centres_with(tag)),
                None => Mark::Other,
            },
            Token::Close(name) => element(name).map_or(Mark::Other, Mark::End),
        };
        events.push(Event {
            mark,
            span: span.clone(),
        });
    }
    Some(events)
}

/// A part of an HTML block that holds `<details>` tags, as `details_parts`
/// cuts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailsPart<'a> {
    /// A `<details>`, with the content of the `<summary>` right after it;
    /// `None` without one.
    Start(&'static Element, Option<Vec<Inline>>),
    /// A `</details>`, as written.
    End(&'static Element, &'a str),
    /// The HTML between those tags, and the byte where it starts, when it
    /// is more than whitespace and comments.
    Html(usize, &'a str),
}

/// What is left of the HTML block `source` from the byte `from`, whose
/// events are `events`, cut at its `<details>` and `</details>` tags, each
/// `<details>` with the `<summary>` right after it. `None` for a block
/// without those tags, or with a `<summary>` there that does not end.
pub fn details_parts<'a>(
    source: &'a str,
    from: usize,
    events: &[Event],
) -> Option<Vec<DetailsPart<'a>>> {
    let mut parts = Vec::new();
    // Where the HTML since the last tag starts, and whether it holds more
    // than whitespace and comments.
    let (mut html_start, mut between) = (from, false);
    let mut events = events.iter().peekable();
    while let Some(event) = events.next() {
        let (part, end) = match event.mark {
            Mark::Start(element, _) if element.name == "details" => {
                match events.peek().map(|e| &e.mark) {
                    Some(Mark::Summary(summary)) => {
                        let (content, end) = summary.clone()?;
                        while events.next_if(|event| event.span.start < end).is_some() {}
                        (DetailsPart::Start(element, Some(content)), end)
                    }
                    _ => (DetailsPart::Start(element, None), event.span.end),
                }
            }
            Mark::End(element) if element.name == "details" => {
                let tag = &source[event.span.clone()];
                (DetailsPart::End(element, tag), event.span.end)
            }
            _ => {
                between = true;
                continue;
            }
        };
        if between {
            parts.push(DetailsPart::Html(
                html_start,
                &source[html_start..event.span.start],
            ));
        }
        parts.push(part);
        (html_start, between) = (end, false);
    }
    if parts.is_empty() {
        return None;
    }
    if between {
        parts.push(DetailsPart::Html(html_start, &source[html_start..]));
    }
    Some(parts)
}

/// A block inside a centring element, as `centred` reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// A heading, with the `id` its tag has.
    Heading(u8, Vec<Inline>, Option<String>),
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
/// or a tag kiri does not read, an element with nothing in it, and text
/// after its end tag.
pub fn centred(html: &str) -> Option<Vec<Piece>> {
    let tokens = spanned_tokens(html)?;
    let [(Token::Open(tag), open), rest @ ..] = skip_blank(&tokens) else {
        return None;
    };
    let heading = heading_level(&tag.name).filter(|_| is_centred(tag));
    let element = element(&tag.name).filter(|element| element.centres_with(tag));
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
                    part: Part::Heading(level, content, id_of(tag)),
                }]
            })
        }
        None => read(
            html,
            inside,
            element.is_none_or(|element| element.name != "p"),
        ),
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
                let content = inline_content(&html[span.end..tokens[close].1.start])?;
                let part = match heading_level(&tag.name) {
                    Some(level) if !content.is_empty() => Part::Heading(level, content, id_of(tag)),
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

/// The inlines of HTML that is only text, comments and the tags kiri reads
/// in text, as a paragraph of a centring element reads them. `None` for
/// HTML with other tags.
pub fn inline_html(html: &str) -> Option<Vec<Inline>> {
    let tokens = spanned_tokens(html)?;
    let inline = tokens.iter().all(|(token, _)| inline_only(token));
    inline.then(|| inline_content(html)).flatten()
}

/// The inlines of `html`, with no white space at the start of a line after
/// a `<br>` or at its end before one, where a browser shows none.
fn inline_content(html: &str) -> Option<Vec<Inline>> {
    let mut content = inlines(html)?;
    for at in 0..content.len() {
        let line_break = matches!(&content[at], Inline::Html(tag) if is_line_break(tag));
        if !line_break {
            continue;
        }
        if let Some(Inline::Text(text)) = content.get_mut(at + 1) {
            *text = text
                .trim_start_matches(|c: char| c.is_ascii_whitespace())
                .to_owned();
        }
        if let Some(Inline::Text(text)) =
            at.checked_sub(1).and_then(|before| content.get_mut(before))
        {
            *text = text
                .trim_end_matches(|c: char| c.is_ascii_whitespace())
                .to_owned();
        }
    }
    content.retain(|inline| !matches!(inline, Inline::Text(text) if text.is_empty()));
    Some(content)
}

/// Whether `token` may be in a paragraph of a centring element: text, a
/// comment, or one of the tags that kiri reads in text: those of a
/// `Phrase`, `<br>` and `<img>`.
fn inline_only(token: &Token<'_>) -> bool {
    let read = |name: &str| name == "br" || Phrase::from_name(name).is_some();
    match token {
        Token::Text(_) | Token::Comment => true,
        Token::Open(Tag { name, .. }) => name == "img" || read(name),
        Token::Close(name) => read(name),
    }
}

#[cfg(test)]
mod tests;
