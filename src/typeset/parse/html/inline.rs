//! Tags of HTML in running text joined into the inlines they make.
//!
//! comrak gives each tag in a paragraph as inline HTML of its own, and the
//! tags of a `<summary>` or of a centred paragraph come the same way. Each
//! tag is read once, as it is added, and a start tag that may be ended is
//! remembered by its element, so that its end tag finds it, or finds that
//! there is none, without a look at the inlines before it.

use super::elements::Phrase;
use super::{Img, Token, img, tag_ids, tokens};
use crate::typeset::model::{Image, Inline};

/// How many start tags may be open at once. A start tag past that stays a
/// tag, so that the elements made of them are nested to that depth at most.
const NESTING: usize = 100;

/// How many kinds of phrase there are: `Phrase::A` is the last of them.
const PHRASES: usize = Phrase::A as usize + 1;

/// What one tag of inline HTML is, as it is read once.
pub(super) enum Shape {
    /// A `<br>`, in any of its spellings.
    Break,
    /// An `<img>` with a `src`.
    Image(Img),
    /// The start tag of a phrase. An `<a>` has the `href` it has, and the
    /// places its `id` and `name` give: without an `href` it is no link, but
    /// the start that a `</a>` after it ends.
    Start {
        phrase: Phrase,
        href: Option<String>,
        ids: Vec<String>,
    },
    /// The end tag of a phrase.
    End(Phrase),
    /// Any other tag, a comment, or what cannot be read.
    Other,
}

/// The shape of the inline HTML `html`: one tag, or else `Other`.
pub(super) fn read_tag(html: &str) -> Shape {
    let Some(tokens) = tokens(html) else {
        return Shape::Other;
    };
    match tokens.as_slice() {
        [Token::Open(tag)] => match tag.name.as_str() {
            "br" => Shape::Break,
            "img" => img(tag).map_or(Shape::Other, Shape::Image),
            "a" => Shape::Start {
                phrase: Phrase::A,
                href: tag.get("href").map(str::to_owned),
                ids: tag_ids(tag),
            },
            name => Phrase::from_name(name).map_or(Shape::Other, |phrase| Shape::Start {
                phrase,
                href: None,
                ids: Vec::new(),
            }),
        },
        [Token::Close(name)] => Phrase::from_name(name).map_or(Shape::Other, Shape::End),
        _ => Shape::Other,
    }
}

/// A start tag that no end tag has ended yet.
struct Start {
    /// Where its tag is among the inlines.
    at: usize,
    href: Option<String>,
    ids: Vec<String>,
}

/// Inlines as they are read, with the start tags among them that are not
/// ended. An end tag ends the start tag of its phrase that is open last, and
/// the tags and the text between them become the element; one that ends
/// nothing stays a tag, as does a start tag that nothing ends. A `</a>` is
/// ended by an `<a>` without an `href` as well, so that it does not end the
/// link before it: that one makes no link, only the places it names.
#[derive(Default)]
pub struct Joined {
    inlines: Vec<Inline>,
    /// The start tags still open, a stack for each phrase.
    open: [Vec<Start>; PHRASES],
    /// How many start tags are open, of every phrase.
    depth: usize,
}

impl Joined {
    /// Adds `inline`: a tag of inline HTML may start or end an element.
    pub fn push(&mut self, inline: Inline) {
        match inline {
            Inline::Html(tag) => {
                let shape = read_tag(&tag);
                self.push_tag(tag, shape);
            }
            inline => self.inlines.push(inline),
        }
    }

    /// Adds `inline`, joined to the text before it when both are text.
    pub fn push_joined(&mut self, inline: Inline) {
        match (self.inlines.last_mut(), inline) {
            (Some(Inline::Text(before)), Inline::Text(text)) => before.push_str(&text),
            (_, inline) => self.push(inline),
        }
    }

    /// Adds one tag of inline HTML: a line break, an image, or else the
    /// tag, which may start or end an element.
    pub fn push_html(&mut self, tag: String) {
        match read_tag(&tag) {
            Shape::Break => self.inlines.push(Inline::LineBreak),
            Shape::Image(img) => self.inlines.push(image(img)),
            shape => self.push_tag(tag, shape),
        }
    }

    /// The inlines, with the start tags that nothing ended as they are.
    pub fn into_inlines(self) -> Vec<Inline> {
        self.inlines
    }

    fn push_tag(&mut self, tag: String, shape: Shape) {
        let at = self.inlines.len();
        self.inlines.push(Inline::Html(tag));
        match shape {
            Shape::Start { phrase, href, ids } if self.depth < NESTING => {
                self.open[phrase as usize].push(Start { at, href, ids });
                self.depth += 1;
            }
            Shape::End(phrase) => self.end(phrase),
            _ => {}
        }
    }

    /// The end tag of `phrase`, the last of the inlines, ends the start tag
    /// of it that is open last, if there is one.
    fn end(&mut self, phrase: Phrase) {
        let Some(start) = self.open[phrase as usize].pop() else {
            return;
        };
        self.depth -= 1;
        // From here on the pair is closed, whether it makes an element or
        // stays as it is.
        let end = self.inlines.len() - 1;
        // `<code>` is code only around plain text; else both tags stay.
        let code = match phrase {
            Phrase::Code => match only_text(&self.inlines[start.at + 1..end]) {
                Some(code) => code,
                None => return,
            },
            _ => String::new(),
        };
        // A link has to hold something.
        if phrase == Phrase::A && start.href.is_some() && start.at + 2 == self.inlines.len() {
            return;
        }
        self.inlines.pop();
        let content: Vec<Inline> = self.inlines.drain(start.at..).skip(1).collect();
        // An `<a>` names the places of its `id` and `name`; with an `href`
        // it is a link to it. Without one, what it holds is left as it was,
        // after the places, and the start tags in it are open still.
        if phrase == Phrase::A {
            match start.href {
                Some(_) => self.forget_from(start.at),
                None => self.move_after(start.at, start.ids.len()),
            }
            return named_link(&mut self.inlines, start.ids, start.href, content);
        }
        self.forget_from(start.at);
        self.inlines.push(match phrase {
            Phrase::A => unreachable!("a link is made above"),
            Phrase::U => Inline::Underline(content),
            Phrase::Kbd => Inline::Kbd(content),
            Phrase::Mark => Inline::Mark(content),
            Phrase::Sub => Inline::Subscript(content),
            Phrase::Sup => Inline::Superscript(content),
            Phrase::B | Phrase::Strong => Inline::Strong(content),
            Phrase::I | Phrase::Em => Inline::Emphasis(content),
            Phrase::S | Phrase::Del => Inline::Strikethrough(content),
            Phrase::Code => Inline::Code(code),
        });
    }

    /// Moves the open start tags after the inline `at`, which an element
    /// of `places` anchors and its content take the place of: its start tag
    /// and its end tag are gone.
    fn move_after(&mut self, at: usize, places: usize) {
        for stack in &mut self.open {
            for start in stack.iter_mut().rev().take_while(|start| start.at > at) {
                start.at = start.at + places - 1;
            }
        }
    }

    /// Forgets the open start tags from the inline `at` on: they are inside
    /// an element that is made, so no end tag after it ends them.
    fn forget_from(&mut self, at: usize) {
        for stack in &mut self.open {
            while stack.last().is_some_and(|start| start.at >= at) {
                stack.pop();
                self.depth -= 1;
            }
        }
    }
}

/// The text of `inlines` when they are only non-empty text.
fn only_text(inlines: &[Inline]) -> Option<String> {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(part) => text.push_str(part),
            _ => return None,
        }
    }
    (!text.is_empty()).then_some(text)
}

/// Adds to `inlines` the places `ids` that an `<a>` names, as anchors, and
/// `content`, in a link to `url` when the `<a>` has an `href`.
fn named_link(
    inlines: &mut Vec<Inline>,
    ids: Vec<String>,
    url: Option<String>,
    content: Vec<Inline>,
) {
    inlines.extend(ids.into_iter().map(Inline::Anchor));
    match url {
        Some(url) => inlines.push(Inline::Link {
            url,
            title: None,
            content,
        }),
        None => inlines.extend(content),
    }
}

/// Text and inline HTML, as `html` gives the content of a `<summary>`,
/// with the tags kiri reads read.
pub fn html_inlines(content: Vec<Inline>) -> Vec<Inline> {
    let mut joined = Joined::default();
    for inline in content {
        match inline {
            Inline::Html(tag) => joined.push_html(tag),
            inline => joined.push(inline),
        }
    }
    joined.into_inlines()
}

/// An HTML `<img>` as Markdown would give it: the image, or a link around
/// it when the HTML had one, after the places that the `<a>` names.
pub fn linked_image(mut img: Img) -> Vec<Inline> {
    let (url, ids) = (img.link.take(), std::mem::take(&mut img.ids));
    let mut inlines = Vec::new();
    named_link(&mut inlines, ids, url, vec![image(img)]);
    inlines
}

/// An HTML `<img>` as the image Markdown would give.
fn image(img: Img) -> Inline {
    let alt = if img.alt.is_empty() {
        Vec::new()
    } else {
        vec![Inline::Text(img.alt)]
    };
    Inline::Image(Image {
        url: img.src,
        alt,
        width: img.width,
        height: img.height,
    })
}
