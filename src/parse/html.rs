//! The few HTML tags in Markdown that kiri reads instead of showing them
//! as source: `<img>`, alone or in tags that only wrap it, and `<a>`
//! around an `<img>`.

/// An `<img>` tag with a `src`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Img {
    pub src: String,
    pub alt: String,
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
    let src = tag.get("src").filter(|src| !src.is_empty())?;
    Some(Img {
        src: src.to_owned(),
        alt: tag.get("alt").unwrap_or_default().to_owned(),
        link: None,
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
        let decoded = rest[1..]
            .find(';')
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
            link: None,
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
    fn an_img_needs_a_src() {
        assert_eq!(inline_img("<img alt=x>"), None);
        assert_eq!(inline_img("<img src alt=x>"), None);
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
