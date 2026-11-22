//! The few HTML tags in Markdown that kiri reads instead of showing them
//! as source: `<img>`, alone or in tags that only wrap it, `<a>` around
//! an `<img>`, `<br>`, `<kbd>`, `<mark>`, `<b>`, `<strong>`, `<i>`,
//! `<em>`, `<s>`, `<del>` and `<code>`, `<h1>` to `<h6>` and `<pre>` alone
//! in a block, and `<details>` with its `<summary>`.

use std::ops::Range;

use crate::typeset::model::{Inline, Length};

/// An `<img>` tag with a `src`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Img {
    pub src: String,
    pub alt: String,
    pub width: Option<Length>,
    pub height: Option<Length>,
    /// The `href` of an `<a>` around the image.
    pub link: Option<String>,
}

/// Tags a block may wrap its one `<img>` in and still be that image.
/// A browser shows `<picture>` as its `<img>` unless a `<source>` fits.
/// An `<a>` with an `href` makes the image a link.
const WRAPPERS: [&str; 6] = ["p", "div", "center", "picture", "source", "a"];

/// The image that inline HTML, one tag, is.
pub fn inline_img(html: &str) -> Option<Img> {
    match tokens(html)?.as_slice() {
        [Token::Open(tag)] => img(tag),
        _ => None,
    }
}

/// The `href` of inline HTML that is one `<a>` tag.
pub fn link_start(html: &str) -> Option<String> {
    match tokens(html)?.as_slice() {
        [Token::Open(tag)] if tag.name == "a" => tag.get("href").map(str::to_owned),
        _ => None,
    }
}

/// The ids that `<a id>` and `<a name>` tags in `html` give: the `id` and
/// the `name` of each, in the order written, the one once when they are the
/// same. Other tags and an `<a>` without either give none.
pub fn anchor_ids(html: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for token in tokens(html).unwrap_or_default() {
        let Token::Open(tag) = token else { continue };
        if tag.name != "a" {
            continue;
        }
        for attribute in ["id", "name"] {
            if let Some(id) = tag.get(attribute).filter(|id| !id.is_empty())
                && !ids.iter().any(|known| known == id)
            {
                ids.push(id.to_owned());
            }
        }
    }
    ids
}

/// A part of an HTML block that holds `<details>` tags, as
/// `details_parts` cuts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailsPart<'a> {
    /// A `<details>`, with the content of the `<summary>` right after it
    /// as text and inline HTML; `None` without one.
    Start(Option<Vec<Inline>>),
    /// A `</details>`, as written.
    End(&'a str),
    /// The HTML between those tags, when it is more than whitespace and
    /// comments.
    Html(&'a str),
}

/// An HTML block cut at its `<details>` and `</details>` tags, each
/// `<details>` with the `<summary>` right after it. `None` for a block
/// without those tags, with a `<summary>` there that does not end, or
/// that `tokens` cannot read.
pub fn details_parts(html: &str) -> Option<Vec<DetailsPart<'_>>> {
    let tokens = spanned_tokens(html)?;
    let mut parts = Vec::new();
    // The token and the byte where the HTML since the last tag starts.
    let (mut html_token, mut html_start) = (0, 0);
    let mut index = 0;
    while let Some((token, span)) = tokens.get(index) {
        // The part a tag makes, and the token and byte after it.
        let (part, next, end) = match token {
            Token::Open(tag) if tag.name == "details" => {
                let rest = skip_blank(&tokens[index + 1..]);
                match rest {
                    [(Token::Open(tag), open), ..] if tag.name == "summary" => {
                        let close = rest.iter().position(
                            |(token, _)| matches!(token, Token::Close(name) if name == "summary"),
                        )?;
                        let close_span = &rest[close].1;
                        let summary = inlines(&html[open.end..close_span.start])?;
                        let next = tokens.len() - rest.len() + close + 1;
                        (DetailsPart::Start(Some(summary)), next, close_span.end)
                    }
                    _ => (DetailsPart::Start(None), index + 1, span.end),
                }
            }
            Token::Close(name) if name == "details" => {
                (DetailsPart::End(&html[span.clone()]), index + 1, span.end)
            }
            _ => {
                index += 1;
                continue;
            }
        };
        if !skip_blank(&tokens[html_token..index]).is_empty() {
            parts.push(DetailsPart::Html(&html[html_start..span.start]));
        }
        parts.push(part);
        (html_token, html_start, index) = (next, end, next);
    }
    if parts.is_empty() {
        return None;
    }
    if !skip_blank(&tokens[html_token..]).is_empty() {
        parts.push(DetailsPart::Html(&html[html_start..]));
    }
    Some(parts)
}

/// The content of an HTML block that is one `<summary>` element, with
/// nothing else but whitespace and comments.
pub fn summary(html: &str) -> Option<Vec<Inline>> {
    let tokens = spanned_tokens(html)?;
    let [(Token::Open(tag), open), rest @ ..] = skip_blank(&tokens) else {
        return None;
    };
    if tag.name != "summary" {
        return None;
    }
    let end = rest
        .iter()
        .position(|(token, _)| matches!(token, Token::Close(name) if name == "summary"))?;
    if !skip_blank(&rest[end + 1..]).is_empty() {
        return None;
    }
    inlines(&html[open.end..rest[end].1.start])
}

/// The level and content of an HTML block that is one `<h1>` to `<h6>`
/// element, with nothing else but whitespace and comments. Its attributes
/// are ignored. `None` for an empty one.
pub fn heading(html: &str) -> Option<(u8, Vec<Inline>)> {
    let tokens = spanned_tokens(html)?;
    let [(Token::Open(tag), open), rest @ ..] = skip_blank(&tokens) else {
        return None;
    };
    let level = heading_level(&tag.name)?;
    let end = rest
        .iter()
        .position(|(token, _)| matches!(token, Token::Close(name) if *name == tag.name))?;
    // Another heading inside this one is no heading of its own: the block
    // is two headings, one of them not closed, or worse.
    if rest[..end].iter().any(
        |(token, _)| matches!(token, Token::Open(inner) if heading_level(&inner.name).is_some()),
    ) {
        return None;
    }
    if !skip_blank(&rest[end + 1..]).is_empty() {
        return None;
    }
    let content = inlines(&html[open.end..rest[end].1.start])?;
    (!content.is_empty()).then_some((level, content))
}

/// A `<h1>` to `<h6>` element somewhere in an HTML block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnerHeading {
    pub level: u8,
    pub content: Vec<Inline>,
    /// The `id` attribute, when it is there and not empty.
    pub id: Option<String>,
    /// The bytes of the element in the block, from its start tag to the end
    /// of its end tag.
    pub bytes: Range<usize>,
}

/// The `<h1>` to `<h6>` elements in `html`, in the order written, each
/// with its content. A heading with no end tag, one with another heading
/// in it and one that `inlines` cannot read are left out, as `heading`
/// leaves out a block of them.
pub fn headings(html: &str) -> Vec<InnerHeading> {
    let Some(tokens) = spanned_tokens(html) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (at, (token, open)) in tokens.iter().enumerate() {
        let Token::Open(tag) = token else { continue };
        let Some(level) = heading_level(&tag.name) else {
            continue;
        };
        let rest = &tokens[at + 1..];
        let Some(end) = (rest.iter())
            .position(|(token, _)| matches!(token, Token::Close(name) if *name == tag.name))
        else {
            continue;
        };
        if rest[..end].iter().any(
            |(token, _)| matches!(token, Token::Open(inner) if heading_level(&inner.name).is_some()),
        ) {
            continue;
        }
        let Some(content) = inlines(&html[open.end..rest[end].1.start]) else {
            continue;
        };
        found.push(InnerHeading {
            level,
            content,
            id: tag.get("id").filter(|id| !id.is_empty()).map(str::to_owned),
            bytes: open.start..rest[end].1.end,
        });
    }
    found
}

/// The language and the code of an HTML block that is one `<pre>`, with
/// one `<code>` in it or not, and only text between the tags: what a
/// fence with that text would give. The language is the `lang` of the
/// `<pre>`, else a `language-` or `lang-` class of the `<code>`. `None`
/// for anything else, and for no code.
pub fn pre(html: &str) -> Option<(Option<String>, String)> {
    let tokens = spanned_tokens(html)?;
    let [(Token::Open(pre), pre_open), inside @ ..] = skip_blank(&tokens) else {
        return None;
    };
    if pre.name != "pre" {
        return None;
    }
    // The `<code>` right after the `<pre>`, past whitespace only. That
    // whitespace is part of the code, as in a browser, but for its first
    // newline.
    let (lead, from_code) = match inside {
        [(Token::Text(blank), _), after @ ..] if blank.trim().is_empty() => (*blank, after),
        _ => ("", inside),
    };
    let lead = drop_first_newline(lead);
    let (code, text_start, inner, body) = match from_code {
        [(Token::Open(code), open), after @ ..] if code.name == "code" => {
            (Some(code), open.end, "code", after)
        }
        _ => (None, pre_open.end, "pre", inside),
    };
    // Without a `<code>` the whitespace is in the text itself.
    let lead = if code.is_some() { lead } else { "" };
    // The text, and the tag that ends it.
    let (text_end, after) = match body {
        [(Token::Close(name), close), after @ ..] if name == inner => (close.start, after),
        [(Token::Text(_), _), (Token::Close(name), close), after @ ..] if name == inner => {
            (close.start, after)
        }
        _ => return None,
    };
    let after = match (code, after) {
        (None, after) => after,
        (Some(_), [(Token::Text(blank), _), (Token::Close(name), _), after @ ..])
            if blank.trim().is_empty() && name == "pre" =>
        {
            after
        }
        (Some(_), [(Token::Close(name), _), after @ ..]) if name == "pre" => after,
        _ => return None,
    };
    if !skip_blank(after).is_empty() {
        return None;
    }
    // A browser drops the one newline right after the opening tag; here
    // that is the last one, whether the blanks before `<code>` stay or not.
    let raw = &html[text_start..text_end];
    let raw = drop_first_newline(raw);
    if lead.trim().is_empty() && raw.trim().is_empty() {
        return None;
    }
    let mut text = lead.to_owned();
    text.push_str(&decode_entities(raw));
    // A fence keeps the newline that ends its last line.
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let language = (pre
        .get("lang")
        .and_then(|lang| lang.split_ascii_whitespace().next()))
    .or_else(|| {
        code?
            .get("class")?
            .split_ascii_whitespace()
            .find_map(|class| {
                class
                    .strip_prefix("language-")
                    .or_else(|| class.strip_prefix("lang-"))
                    .filter(|lang| !lang.is_empty())
            })
    });
    Some((language.map(str::to_owned), text))
}

/// The level of a tag named `h1` to `h6`.
fn heading_level(name: &str) -> Option<u8> {
    match name.as_bytes() {
        [b'h', digit @ b'1'..=b'6'] => Some(digit - b'0'),
        _ => None,
    }
}

/// `text` without the one newline that may start it.
fn drop_first_newline(text: &str) -> &str {
    text.strip_prefix("\r\n")
        .or_else(|| text.strip_prefix('\n'))
        .unwrap_or(text)
}

/// The text of HTML without its tags and comments, entities decoded and
/// whitespace collapsed as a browser does. `None` for HTML that `tokens`
/// cannot read.
pub fn plain_text(html: &str) -> Option<String> {
    let mut text = String::new();
    for token in tokens(html)? {
        if let Token::Text(raw) = token {
            text.push_str(&decode_entities(raw));
        }
    }
    Some(collapse_whitespace(&text).trim_matches(' ').to_owned())
}

/// HTML as inlines: its text, entities decoded and whitespace collapsed,
/// and each tag as inline HTML, as comrak gives inline HTML in Markdown.
fn inlines(html: &str) -> Option<Vec<Inline>> {
    let mut inlines = Vec::new();
    for (token, span) in spanned_tokens(html)? {
        match token {
            Token::Text(raw) => {
                let text = collapse_whitespace(&decode_entities(raw));
                match inlines.last_mut() {
                    // A comment between two texts shows nothing, so
                    // their spaces at the join are one.
                    Some(Inline::Text(last)) => {
                        let text = if last.ends_with(' ') {
                            text.trim_start_matches(' ')
                        } else {
                            &text
                        };
                        last.push_str(text);
                    }
                    _ => inlines.push(Inline::Text(text)),
                }
            }
            Token::Open(_) | Token::Close(_) => inlines.push(Inline::Html(html[span].to_owned())),
            Token::Comment => {}
        }
    }
    // Whitespace at the start and the end shows nothing.
    if let Some(Inline::Text(first)) = inlines.first_mut() {
        *first = first.trim_start_matches(' ').to_owned();
    }
    if let Some(Inline::Text(last)) = inlines.last_mut() {
        *last = last.trim_end_matches(' ').to_owned();
    }
    inlines.retain(|inline| !matches!(inline, Inline::Text(text) if text.is_empty()));
    Some(inlines)
}

/// Runs of ASCII whitespace as one space each.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in text.chars() {
        if c.is_ascii_whitespace() {
            space = true;
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

/// `tokens` from the first one that is not whitespace or a comment.
fn skip_blank<'t, 'a>(tokens: &'t [(Token<'a>, Range<usize>)]) -> &'t [(Token<'a>, Range<usize>)] {
    let start = tokens
        .iter()
        .position(|(token, _)| match token {
            Token::Comment => false,
            Token::Text(text) => !text.trim().is_empty(),
            _ => true,
        })
        .unwrap_or(tokens.len());
    &tokens[start..]
}

/// An inline element that kiri reads from its opening and closing tags,
/// with the inlines between them as its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phrase {
    Kbd,
    Mark,
    Sub,
    Sup,
    B,
    Strong,
    I,
    Em,
    S,
    Del,
    Code,
}

impl Phrase {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "kbd" => Some(Self::Kbd),
            "mark" => Some(Self::Mark),
            "sub" => Some(Self::Sub),
            "sup" => Some(Self::Sup),
            "b" => Some(Self::B),
            "strong" => Some(Self::Strong),
            "i" => Some(Self::I),
            "em" => Some(Self::Em),
            "s" => Some(Self::S),
            "del" => Some(Self::Del),
            "code" => Some(Self::Code),
            _ => None,
        }
    }
}

/// The element that inline HTML, one opening tag, starts.
pub fn phrase_start(html: &str) -> Option<Phrase> {
    match tokens(html)?.as_slice() {
        [Token::Open(tag)] => Phrase::from_name(&tag.name),
        _ => None,
    }
}

/// The element that inline HTML, one closing tag, ends.
pub fn phrase_end(html: &str) -> Option<Phrase> {
    match tokens(html)?.as_slice() {
        [Token::Close(name)] => Phrase::from_name(name),
        _ => None,
    }
}

/// Whether inline HTML is one `<br>`, in any of its spellings.
pub fn is_line_break(html: &str) -> bool {
    matches!(tokens(html).as_deref(), Some([Token::Open(tag)]) if tag.name == "br")
}

/// Whether inline HTML is one `</a>`.
pub fn is_link_end(html: &str) -> bool {
    matches!(tokens(html).as_deref(), Some([Token::Close(name)]) if name == "a")
}

/// The image an HTML block shows: one `<img>` among only tags that wrap
/// it, comments and whitespace.
pub fn block_img(html: &str) -> Option<Img> {
    let mut found = None;
    let mut link = None;
    for token in tokens(html)? {
        match token {
            Token::Open(tag) if tag.name == "img" => {
                if found.is_some() {
                    return None;
                }
                found = Some(Img {
                    link: link.clone(),
                    ..img(&tag)?
                });
            }
            Token::Open(tag) if tag.name == "a" => link = tag.get("href").map(str::to_owned),
            Token::Close(name) if name == "a" => link = None,
            Token::Open(Tag { name, .. }) | Token::Close(name)
                if WRAPPERS.contains(&name.as_str()) => {}
            Token::Comment => {}
            Token::Text(text) if text.trim().is_empty() => {}
            _ => return None,
        }
    }
    found
}

fn img(tag: &Tag) -> Option<Img> {
    if tag.name != "img" {
        return None;
    }
    // A browser strips the whitespace around a URL.
    let src = tag
        .get("src")
        .map(|src| src.trim_matches(|c: char| c.is_ascii_whitespace()))
        .filter(|src| !src.is_empty())?;
    Some(Img {
        src: src.to_owned(),
        alt: tag.get("alt").unwrap_or_default().to_owned(),
        width: tag.get("width").and_then(length),
        height: tag.get("height").and_then(length),
        link: None,
    })
}

/// A `width` or `height` value, read as a browser does: a number of CSS
/// pixels, or a percentage when `%` follows it. Anything after that is
/// ignored. A value that is no number or rounds to 0 gives `None`.
fn length(value: &str) -> Option<Length> {
    let value = value.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let digits = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    if digits == 0 {
        return None;
    }
    let mut end = digits;
    if let Some(fraction) = value[digits..].strip_prefix('.') {
        end += 1 + fraction
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(fraction.len());
    }
    let number: f64 = value[..end].parse().ok()?;
    // `as` saturates at u32::MAX.
    let rounded = number.round() as u32;
    if rounded == 0 {
        return None;
    }
    Some(if value[end..].starts_with('%') {
        Length::Percent(rounded)
    } else {
        Length::Pixels(rounded)
    })
}

#[derive(Debug)]
enum Token<'a> {
    Open(Tag),
    /// A closing tag's name, in lower case.
    Close(String),
    Comment,
    Text(&'a str),
}

#[derive(Debug)]
struct Tag {
    /// In lower case, as are the attribute names.
    name: String,
    /// Values with their entities decoded.
    attributes: Vec<(String, String)>,
}

impl Tag {
    /// An attribute's value. The first of repeated attributes counts, as
    /// in a browser.
    fn get(&self, name: &str) -> Option<&str> {
        (self.attributes.iter())
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// `html` as tags, comments and the text between them. `None` for
/// anything else that starts with `<`, such as a declaration, and for a
/// tag or comment cut short.
fn tokens(html: &str) -> Option<Vec<Token<'_>>> {
    Some(
        spanned_tokens(html)?
            .into_iter()
            .map(|(token, _)| token)
            .collect(),
    )
}

/// `tokens` with where each one is in `html`.
fn spanned_tokens(html: &str) -> Option<Vec<(Token<'_>, Range<usize>)>> {
    let mut tokens = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let start = html.len() - rest.len();
        let token;
        if let Some(open) = rest.strip_prefix('<') {
            if let Some(comment) = open.strip_prefix("!--") {
                rest = &comment[comment.find("-->")? + 3..];
                token = Token::Comment;
            } else if let Some(closing) = open.strip_prefix('/') {
                let (name, after) = tag_name(closing)?;
                rest = after.trim_start().strip_prefix('>')?;
                token = Token::Close(name);
            } else {
                let (name, after) = tag_name(open)?;
                let (attributes, after) = attributes(after)?;
                rest = after;
                token = Token::Open(Tag { name, attributes });
            }
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            token = Token::Text(&rest[..end]);
            rest = &rest[end..];
        }
        tokens.push((token, start..html.len() - rest.len()));
    }
    Some(tokens)
}

/// A tag name at the start of `text`, in lower case, and the text after it.
fn tag_name(text: &str) -> Option<(String, &str)> {
    if !text.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let end = text
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .unwrap_or(text.len());
    Some((text[..end].to_ascii_lowercase(), &text[end..]))
}

/// The attributes of an opening tag up to its `>` or `/>`, and the text
/// after the tag.
fn attributes(mut text: &str) -> Option<(Vec<(String, String)>, &str)> {
    let mut attributes = Vec::new();
    loop {
        // A stray `/` is ignored, as a browser does.
        text = text.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
        if let Some(after) = text.strip_prefix('>') {
            return Some((attributes, after));
        }
        let end = text.find(|c: char| c.is_ascii_whitespace() || matches!(c, '/' | '>' | '='));
        let end = end.filter(|&end| end > 0)?;
        let name = text[..end].to_ascii_lowercase();
        text = &text[end..];
        let mut value = String::new();
        if let Some(after) = text.trim_start().strip_prefix('=') {
            let after = after.trim_start();
            let (raw, rest) = match after.chars().next()? {
                quote @ ('"' | '\'') => {
                    let close = after[1..].find(quote)? + 1;
                    (&after[1..close], &after[close + 1..])
                }
                _ => {
                    let end = after
                        .find(|c: char| c.is_ascii_whitespace() || c == '>')
                        .unwrap_or(after.len());
                    after.split_at(end)
                }
            };
            value = decode_entities(raw);
            text = rest;
        }
        attributes.push((name, value));
    }
}

/// `text` with its character references decoded: numeric ones and the
/// named ones a README is likely to hold, not the whole HTML5 table.
/// Others stay as written.
fn decode_entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        // The `;` is looked for only as far as a name can reach, so that a
        // long run of `&` takes linear time.
        let decoded = (rest[1..].bytes().take(LONGEST_ENTITY + 1))
            .position(|byte| byte == b';')
            .and_then(|end| Some((entity(&rest[1..end + 1])?, end + 2)));
        match decoded {
            Some((c, length)) => {
                out.push(c);
                rest = &rest[length..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The longest entity name between `&` and `;` that is decoded. The names
/// `entity` knows are far shorter; only a number padded with zeros could
/// be longer, and it stays as written.
const LONGEST_ENTITY: usize = 32;

/// The character an entity's name, between `&` and `;`, stands for.
fn entity(name: &str) -> Option<char> {
    let code = if let Some(number) = name.strip_prefix('#') {
        let (digits, radix) = match number.strip_prefix(['x', 'X']) {
            Some(hex) => (hex, 16),
            None => (number, 10),
        };
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return None;
        }
        // Too large a number is no character either.
        u32::from_str_radix(digits, radix).unwrap_or(u32::MAX)
    } else {
        return Some(match name {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "nbsp" => '\u{a0}',
            "copy" => '\u{a9}',
            "reg" => '\u{ae}',
            "trade" => '\u{2122}',
            "hellip" => '\u{2026}',
            "mdash" => '\u{2014}',
            "ndash" => '\u{2013}',
            "lsquo" => '\u{2018}',
            "rsquo" => '\u{2019}',
            "ldquo" => '\u{201c}',
            "rdquo" => '\u{201d}',
            "sbquo" => '\u{201a}',
            "bdquo" => '\u{201e}',
            "laquo" => '\u{ab}',
            "raquo" => '\u{bb}',
            "lsaquo" => '\u{2039}',
            "rsaquo" => '\u{203a}',
            "bull" => '\u{2022}',
            "middot" => '\u{b7}',
            "times" => '\u{d7}',
            "divide" => '\u{f7}',
            "minus" => '\u{2212}',
            "plusmn" => '\u{b1}',
            "deg" => '\u{b0}',
            "micro" => '\u{b5}',
            "para" => '\u{b6}',
            "sect" => '\u{a7}',
            "dagger" => '\u{2020}',
            "Dagger" => '\u{2021}',
            "euro" => '\u{20ac}',
            "pound" => '\u{a3}',
            "yen" => '\u{a5}',
            "cent" => '\u{a2}',
            "larr" => '\u{2190}',
            "rarr" => '\u{2192}',
            "uarr" => '\u{2191}',
            "darr" => '\u{2193}',
            "harr" => '\u{2194}',
            "ensp" => '\u{2002}',
            "emsp" => '\u{2003}',
            "thinsp" => '\u{2009}',
            _ => return None,
        });
    };
    // A browser shows U+0000 and what is no character as the replacement
    // character.
    Some(
        char::from_u32(code)
            .filter(|&c| c != '\0')
            .unwrap_or('\u{fffd}'),
    )
}

#[cfg(test)]
mod tests;
