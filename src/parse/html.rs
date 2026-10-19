//! The few HTML tags in Markdown that kiri reads instead of showing them
//! as source: `<img>`, alone or in tags that only wrap it, `<a>` around
//! an `<img>`, `<br>`, `<kbd>` and `<mark>`.

use crate::model::Length;

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
    let mut tokens = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(start) = rest.strip_prefix('<') else {
            let end = rest.find('<').unwrap_or(rest.len());
            tokens.push(Token::Text(&rest[..end]));
            rest = &rest[end..];
            continue;
        };
        if let Some(comment) = start.strip_prefix("!--") {
            rest = &comment[comment.find("-->")? + 3..];
            tokens.push(Token::Comment);
        } else if let Some(closing) = start.strip_prefix('/') {
            let (name, after) = tag_name(closing)?;
            rest = after.trim_start().strip_prefix('>')?;
            tokens.push(Token::Close(name));
        } else {
            let (name, after) = tag_name(start)?;
            let (attributes, after) = attributes(after)?;
            rest = after;
            tokens.push(Token::Open(Tag { name, attributes }));
        }
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
