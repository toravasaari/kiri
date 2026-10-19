//! The few HTML tags in Markdown that kiri reads instead of showing them
//! as source: `<img>`, alone or in tags that only wrap it, `<a>` around
//! an `<img>`, `<br>`, `<kbd>`, `<mark>`, and `<details>` with its
//! `<summary>`.

use std::ops::Range;

use crate::model::{Inline, Length};

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
                    Some(Inline::Text(last)) => last.push_str(&text),
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
}

impl Phrase {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "kbd" => Some(Self::Kbd),
            "mark" => Some(Self::Mark),
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
/// named ones an attribute is likely to hold. Others stay as written.
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
mod tests {
    use super::*;

    fn img(src: &str, alt: &str) -> Option<Img> {
        Some(Img {
            src: src.into(),
            alt: alt.into(),
            width: None,
            height: None,
            link: None,
        })
    }

    fn sized(width: Option<Length>, height: Option<Length>) -> Option<Img> {
        Some(Img {
            width,
            height,
            ..img("a.png", "")?
        })
    }

    fn linked(src: &str, href: &str) -> Option<Img> {
        Some(Img {
            link: Some(href.into()),
            ..img(src, "")?
        })
    }

    #[test]
    fn reads_src_and_alt() {
        assert_eq!(
            inline_img(r#"<img src="a.png" alt="An image">"#),
            img("a.png", "An image")
        );
    }

    #[test]
    fn reads_any_quoting_case_and_self_closing() {
        assert_eq!(inline_img("<IMG SRC='a.png' Alt=x />"), img("a.png", "x"));
        assert_eq!(inline_img("<img src=a.png alt=\"x\"/>"), img("a.png", "x"));
        assert_eq!(
            inline_img("<img\n  src=\"a.png\"\n  alt=\"two\nlines\">"),
            img("a.png", "two\nlines")
        );
        assert_eq!(
            inline_img(r#"<img alt="a > b" src="a.png">"#),
            img("a.png", "a > b")
        );
    }

    #[test]
    fn an_attribute_without_a_value_is_empty() {
        assert_eq!(inline_img("<img hidden src=a.png alt>"), img("a.png", ""));
    }

    #[test]
    fn the_first_of_repeated_attributes_counts() {
        assert_eq!(
            inline_img("<img src=a.png src=b.png alt=x>"),
            img("a.png", "x")
        );
    }

    #[test]
    fn decodes_entities_in_values() {
        assert_eq!(
            inline_img(r#"<img src="a.png?x=1&amp;y=2" alt="&lt;Fish &amp; chips&gt;">"#),
            img("a.png?x=1&y=2", "<Fish & chips>")
        );
        assert_eq!(
            inline_img(r#"<img src=a.png alt="&quot;&apos;&nbsp;&#65;&#x42;&#X43;">"#),
            img("a.png", "\"'\u{a0}ABC")
        );
    }

    #[test]
    fn keeps_entities_it_does_not_know() {
        assert_eq!(
            inline_img(r#"<img src=a.png alt="&copy; &amp &#; &#xZZ; &#0; a&b">"#),
            img("a.png", "&copy; &amp &#; &#xZZ; \u{fffd} a&b")
        );
    }

    #[test]
    fn keeps_an_ampersand_far_from_a_semicolon() {
        let long = format!("&{};", "a".repeat(40));
        assert_eq!(
            inline_img(&format!("<img src=a.png alt=\"{long} &amp;\">")),
            img("a.png", &format!("{long} &"))
        );
    }

    #[test]
    fn an_img_needs_a_src() {
        assert_eq!(inline_img("<img alt=x>"), None);
        assert_eq!(inline_img("<img src alt=x>"), None);
        assert_eq!(inline_img("<img src=\" \n\" alt=x>"), None);
    }

    #[test]
    fn trims_whitespace_around_the_src() {
        assert_eq!(
            inline_img("<img src=\" a.png\n\" alt=\" x \">"),
            img("a.png", " x ")
        );
    }

    #[test]
    fn inline_html_is_an_image_only_when_it_is_one_img_tag() {
        assert_eq!(inline_img("<span>"), None);
        assert_eq!(inline_img("</img>"), None);
        assert_eq!(inline_img("<imgx src=a.png>"), None);
        assert_eq!(inline_img("<img src=a.png"), None);
        assert_eq!(inline_img("<img src=a.png><img src=b.png>"), None);
    }

    #[test]
    fn reads_width_and_height_in_css_pixels() {
        use Length::Pixels;
        assert_eq!(
            inline_img("<img src=a.png width=320 height=\"240\">"),
            sized(Some(Pixels(320)), Some(Pixels(240)))
        );
        assert_eq!(
            inline_img("<img src=a.png width=\" 100px\">"),
            sized(Some(Pixels(100)), None)
        );
        assert_eq!(
            inline_img("<img src=a.png height=\"80.6\">"),
            sized(None, Some(Pixels(81)))
        );
    }

    #[test]
    fn reads_a_percentage() {
        assert_eq!(
            inline_img("<img src=a.png width=\"50%\" height=\"25.5 %\">"),
            sized(Some(Length::Percent(50)), Some(Length::Pixels(26)))
        );
        assert_eq!(
            inline_img("<img src=a.png width=\"33.4%\">"),
            sized(Some(Length::Percent(33)), None)
        );
    }

    #[test]
    fn ignores_lengths_that_are_no_size() {
        for value in ["", "px", "-10", "0", "0.4", "0%", ".5", "auto"] {
            assert_eq!(
                inline_img(&format!(
                    "<img src=a.png width=\"{value}\" height=\"{value}\">"
                )),
                sized(None, None),
                "{value:?}"
            );
        }
        assert_eq!(
            inline_img("<img src=a.png width=99999999999>"),
            sized(Some(Length::Pixels(u32::MAX)), None)
        );
    }

    #[test]
    fn a_block_of_one_img_is_an_image() {
        assert_eq!(block_img("<img src=a.png alt=x>\n"), img("a.png", "x"));
    }

    #[test]
    fn a_block_may_wrap_its_img() {
        let wrapped = "<p align=\"center\">\n  <img src=\"a.png\" alt=\"x\">\n</p>\n";
        assert_eq!(block_img(wrapped), img("a.png", "x"));
        let centred = "<CENTER><div><img src=a.png alt=x></div></CENTER>\n";
        assert_eq!(block_img(centred), img("a.png", "x"));
        let commented = "<!-- logo -->\n<p><img src=a.png alt=x></p>\n";
        assert_eq!(block_img(commented), img("a.png", "x"));
    }

    #[test]
    fn a_picture_shows_its_img() {
        let picture = "<picture>\n\
            \x20 <source media=\"(prefers-color-scheme: dark)\" srcset=\"dark.png\">\n\
            \x20 <img src=\"light.png\" alt=\"Logo\">\n\
            </picture>\n";
        assert_eq!(block_img(picture), img("light.png", "Logo"));
    }

    #[test]
    fn reads_a_link_from_its_tags() {
        assert_eq!(
            link_start(r#"<a href="https://x/?a=1&amp;b=2">"#),
            Some("https://x/?a=1&b=2".into())
        );
        assert_eq!(link_start("<A HREF=x title=y>"), Some("x".into()));
        assert_eq!(link_start("<a>"), None);
        assert_eq!(link_start("<a href=x><b>"), None);
        assert_eq!(link_start("<abbr href=x>"), None);
        assert!(is_link_end("</a>"));
        assert!(is_link_end("</A >"));
        assert!(!is_link_end("<a>"));
        assert!(!is_link_end("</abbr>"));
    }

    #[test]
    fn reads_a_line_break_in_any_spelling() {
        for br in ["<br>", "<br/>", "<br />", "<BR>", "<Br\n>", "<br class=x>"] {
            assert!(is_line_break(br), "{br:?}");
        }
        for other in ["<brx>", "<br", "<br><br>", "<b>", "</br>", "<!-- <br> -->"] {
            assert!(!is_line_break(other), "{other:?}");
        }
    }

    #[test]
    fn reads_the_tags_of_a_kbd() {
        assert_eq!(phrase_start("<kbd>"), Some(Phrase::Kbd));
        assert_eq!(phrase_start("<KBD title=\"key\">"), Some(Phrase::Kbd));
        assert_eq!(phrase_end("</kbd>"), Some(Phrase::Kbd));
        assert_eq!(phrase_end("</Kbd >"), Some(Phrase::Kbd));
        for other in ["</kbd>", "<kbdx>", "<b>", "<kbd><kbd>", "<kbd"] {
            assert_eq!(phrase_start(other), None, "{other:?}");
        }
        for other in ["<kbd>", "</kbdx>", "</b>", "</kbd></kbd>"] {
            assert_eq!(phrase_end(other), None, "{other:?}");
        }
    }

    #[test]
    fn reads_the_tags_of_a_mark() {
        assert_eq!(phrase_start("<mark class=x>"), Some(Phrase::Mark));
        assert_eq!(phrase_end("</MARK>"), Some(Phrase::Mark));
        assert_eq!(phrase_start("<marker>"), None);
    }

    fn text(text: &str) -> Inline {
        Inline::Text(text.into())
    }

    fn start(summary: &str) -> DetailsPart<'static> {
        DetailsPart::Start(Some(vec![text(summary)]))
    }

    #[test]
    fn cuts_a_block_at_its_details_start() {
        assert_eq!(
            details_parts("<details>\n<summary>A  <b>bold</b>\n &amp; more</summary>\n"),
            Some(vec![DetailsPart::Start(Some(vec![
                text("A "),
                Inline::Html("<b>".into()),
                text("bold"),
                Inline::Html("</b>".into()),
                text(" & more"),
            ]))])
        );
        assert_eq!(
            details_parts("<!-- note -->\n<DETAILS open>\n"),
            Some(vec![DetailsPart::Start(None)])
        );
        assert_eq!(
            details_parts("<details><summary></summary>Text\n"),
            Some(vec![
                DetailsPart::Start(Some(Vec::new())),
                DetailsPart::Html("Text\n"),
            ])
        );
        assert_eq!(
            details_parts("<p>\n<details>\n<!-- c -->\n<summary>S</summary>\n"),
            Some(vec![DetailsPart::Html("<p>\n"), start("S")])
        );
    }

    #[test]
    fn cuts_a_block_at_each_details_tag() {
        assert_eq!(
            details_parts("<details><summary>S</summary>Some <i>text</i></details> <!-- end -->\n"),
            Some(vec![
                start("S"),
                DetailsPart::Html("Some <i>text</i>"),
                DetailsPart::End("</details>"),
            ])
        );
        assert_eq!(
            details_parts("<details>\n<summary>A</summary>\n<details>\n<summary>B</summary>\n"),
            Some(vec![start("A"), start("B")])
        );
        assert_eq!(
            details_parts("<details><summary>S</summary><details>In</details>Out</details>\n"),
            Some(vec![
                start("S"),
                DetailsPart::Start(None),
                DetailsPart::Html("In"),
                DetailsPart::End("</details>"),
                DetailsPart::Html("Out"),
                DetailsPart::End("</details>"),
            ])
        );
    }

    #[test]
    fn cuts_a_block_at_its_details_end() {
        assert_eq!(
            details_parts("</details>\n"),
            Some(vec![DetailsPart::End("</details>")])
        );
        assert_eq!(
            details_parts("<!-- end -->\n</DETAILS >\n"),
            Some(vec![DetailsPart::End("</DETAILS >")])
        );
        assert_eq!(
            details_parts("</p>\n</details>\nAfter.\n"),
            Some(vec![
                DetailsPart::Html("</p>\n"),
                DetailsPart::End("</details>"),
                DetailsPart::Html("\nAfter.\n"),
            ])
        );
        assert_eq!(
            details_parts("</details></details>\n"),
            Some(vec![
                DetailsPart::End("</details>"),
                DetailsPart::End("</details>"),
            ])
        );
    }

    #[test]
    fn a_block_without_details_tags_has_no_parts() {
        assert_eq!(details_parts("<p>\nText\n</p>\n"), None);
        assert_eq!(details_parts("<detailsx>\n</detailsx>\n"), None);
        assert_eq!(details_parts("<summary>S</summary>\n"), None);
        assert_eq!(details_parts("<details><summary>Cut short\n"), None);
        assert_eq!(details_parts("<details\n"), None);
        assert_eq!(details_parts("<!-- <details> -->\n"), None);
    }

    #[test]
    fn reads_a_summary_block() {
        assert_eq!(
            summary("<summary>The <kbd>S</kbd></summary>\n"),
            Some(vec![
                text("The "),
                Inline::Html("<kbd>".into()),
                text("S"),
                Inline::Html("</kbd>".into()),
            ])
        );
        assert_eq!(summary("<summary>S</summary>more\n"), None);
        assert_eq!(summary("<summary>S\n"), None);
        assert_eq!(summary("<p>S</p>\n"), None);
    }

    #[test]
    fn strips_tags_from_plain_text() {
        assert_eq!(
            plain_text("\n Some <b>bold</b>\n &amp;  <!-- not -->more \n"),
            Some("Some bold & more".into())
        );
        assert_eq!(plain_text("<p></p>\n"), Some(String::new()));
        assert_eq!(plain_text("a < b"), None);
    }

    #[test]
    fn a_block_may_link_its_img() {
        let linked_block = "<p align=center>\n  <a href=\"https://x\"><img src=a.png></a>\n</p>\n";
        assert_eq!(block_img(linked_block), linked("a.png", "https://x"));
        assert_eq!(block_img("<a><img src=a.png></a>\n"), img("a.png", ""));
        assert_eq!(
            block_img("<a href=x></a><img src=a.png>\n"),
            img("a.png", "")
        );
    }

    #[test]
    fn a_block_with_more_than_an_img_is_not_an_image() {
        assert_eq!(block_img("<p>Text <img src=a.png></p>\n"), None);
        assert_eq!(block_img("<p><img src=a.png><img src=b.png></p>\n"), None);
        assert_eq!(block_img("<p><b><img src=a.png></b></p>\n"), None);
        assert_eq!(block_img("<p><br><img src=a.png></p>\n"), None);
        assert_eq!(block_img("<p>&nbsp;<img src=a.png></p>\n"), None);
        assert_eq!(block_img("<div>\n</div>\n"), None);
        assert_eq!(block_img("<!DOCTYPE html><img src=a.png>\n"), None);
        assert_eq!(block_img("<!-- <img src=a.png> -->\n"), None);
        assert_eq!(block_img("<img alt=x>\n"), None);
    }

    #[test]
    fn a_block_cut_short_is_not_an_image() {
        assert_eq!(block_img("<img src=a.png>\n<p\n"), None);
        assert_eq!(block_img("<img src=a.png>\n<!-- open\n"), None);
        assert_eq!(block_img("<img src=\"a.png>\n"), None);
    }
}
