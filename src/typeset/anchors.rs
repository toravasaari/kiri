//! The ids of headings, as GitHub makes them, so that `[a](#installation)`
//! goes where it does there.
//!
//! The ids are those of comrak's `Anchorizer`, which follows GitHub's
//! algorithm: the text of the heading in lower case, spaces as `-`, what is
//! no letter, number, mark or `-` or `_` dropped, and `-1`, `-2` after the
//! first of the same id in a file.
//!
//! `Ids` holds the ids of the headings of one file. `anchor_text` makes the
//! text of a heading into an id, and `decode` turns `%XX` in an address into
//! characters.

use std::borrow::Cow;

use comrak::Anchorizer;

use crate::typeset::model::{Image, Inline};

/// The ids of the headings of one file, in the order the headings come.
#[derive(Debug, Default)]
pub struct Ids {
    anchorizer: Anchorizer,
    empty_seen: bool,
}

impl Ids {
    pub fn new() -> Self {
        Self::default()
    }

    /// The id of a heading whose content is `content`: its text, as
    /// [`anchor_text`] gives it, made an id, with `-1`, `-2`… after the
    /// first of the same one.
    pub fn of(&mut self, content: &[Inline]) -> String {
        let text = anchor_text(content);
        // Since comrak 1.2.0 the anchorizer reserves the empty id, so the
        // first empty heading would get `-1`. GitHub gives it `""` and
        // the next `-1`: the first empty one skips the anchorizer.
        if !self.empty_seen && makes_empty_id(&text) {
            self.empty_seen = true;
            return String::new();
        }
        self.anchorizer.anchorize(&text)
    }
}

/// Whether `text` leaves no id. A fresh anchorizer has the empty id taken,
/// so such a text comes out as `-1` and then `-2`, which no other does.
fn makes_empty_id(text: &str) -> bool {
    let mut probe = Anchorizer::new();
    probe.anchorize(text) == "-1" && probe.anchorize(text) == "-2"
}

/// The text of a heading that its id is made of: what is shown, without
/// the tags of inline HTML, with a formula as its source and an image as
/// its alt text.
pub fn anchor_text(content: &[Inline]) -> String {
    let mut out = String::new();
    push(content, &mut out);
    out
}

fn push(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(text) | Inline::Code(text) => out.push_str(text),
            Inline::Html(_) => {}
            Inline::Emphasis(content)
            | Inline::Strong(content)
            | Inline::Strikethrough(content)
            | Inline::Kbd(content)
            | Inline::Mark(content)
            | Inline::Subscript(content)
            | Inline::Superscript(content)
            | Inline::Link { content, .. }
            | Inline::Image(Image { alt: content, .. }) => push(content, out),
            Inline::Formula { source, .. } => out.push_str(source),
            Inline::SoftBreak | Inline::LineBreak => out.push(' '),
        }
    }
}

/// `%XX` decoded, for a fragment or a path of an address. A run of escapes
/// that is no UTF-8, and an escape that is broken (`%zz`, a lone `%`), stay
/// as written.
pub fn decode(text: &str) -> Cow<'_, str> {
    if !text.contains('%') {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let hex = |at: usize| -> Option<u8> {
        let pair = bytes.get(at + 1..at + 3)?;
        let digits = std::str::from_utf8(pair).ok()?;
        u8::from_str_radix(digits, 16).ok()
    };
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'%' || hex(at).is_none() {
            let end = text[at..]
                .find('%')
                .map_or(text.len(), |n| (at + n).max(at + 1));
            out.push_str(&text[at..end]);
            at = end;
            continue;
        }
        // A run of valid escapes, decoded as one: a character can be several.
        let start = at;
        let mut run = Vec::new();
        while at < bytes.len()
            && bytes[at] == b'%'
            && let Some(byte) = hex(at)
        {
            run.push(byte);
            at += 3;
        }
        match String::from_utf8(run) {
            Ok(decoded) => out.push_str(&decoded),
            Err(_) => out.push_str(&text[start..at]),
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::model::Block;
    use crate::typeset::parse::parse;

    /// The ids of the headings of `source`, in order.
    fn ids(source: &str) -> Vec<String> {
        let document = parse(source);
        let mut ids = Ids::new();
        document
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Heading { content, .. } => Some(ids.of(content)),
                _ => None,
            })
            .collect()
    }

    fn id(heading: &str) -> String {
        ids(&format!("# {heading}\n")).remove(0)
    }

    #[test]
    fn heading_ids_are_github_s() {
        assert_eq!(id("Installation"), "installation");
        assert_eq!(id("Ticks aren't in"), "ticks-arent-in");
        assert_eq!(id("C++ & Rust"), "c--rust");
        assert_eq!(id("Ääkköset ovat sallittuja"), "ääkköset-ovat-sallittuja");
        assert_eq!(id("`code` in it"), "code-in-it");
        assert_eq!(id("[a link](u) here"), "a-link-here");
        assert_eq!(id("**Bold** and *em*"), "bold-and-em");
        assert_eq!(id("Hello, world! (v1.2)"), "hello-world-v12");
        assert_eq!(id("snake_case and kebab-case"), "snake_case-and-kebab-case");
        assert_eq!(id("Party \u{1F389} time"), "party--time");
        assert_eq!(id("Formula $x^2$"), "formula-x2");
    }

    #[test]
    fn a_second_heading_of_the_same_text_gets_a_number() {
        assert_eq!(
            ids("# stuff\n\n## Stuff\n\n### STUFF\n"),
            ["stuff", "stuff-1", "stuff-2"]
        );
    }

    #[test]
    fn inline_html_is_not_in_the_id() {
        assert_eq!(id("Foo <img src=x>"), "foo-");
        assert_eq!(id("Foo <b>bar</b>"), "foo-bar");
    }

    #[test]
    fn decode_turns_percent_escapes_into_characters_and_leaves_broken_ones() {
        assert_eq!(decode("plain"), "plain");
        assert!(matches!(decode("plain"), Cow::Borrowed(_)));
        assert_eq!(decode("a%20b"), "a b");
        assert_eq!(decode("%C3%A4%C3%B6"), "äö");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz%41"), "%zzA");
        assert_eq!(decode("%C3x"), "%C3x");
        assert_eq!(decode("50%%41"), "50%A");
        assert_eq!(decode("%e4%b8%ad"), "中");
    }
}
