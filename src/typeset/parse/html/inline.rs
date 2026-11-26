//! Tags of HTML in running text joined into the inlines they make.
//!
//! comrak gives each tag in a paragraph as inline HTML of its own, and the
//! tags of a `<summary>` or of a centred paragraph come the same way. Each
//! tag is read once, as it is added, and a start tag that may be ended is
//! remembered by its element, so that its end tag finds it, or finds that
//! there is none, without a look at the inlines before it.

use super::elements::{HtmlLevel, HtmlTags, Phrase};
use super::{Img, Tag, Token, collapse_whitespace, decode_entities, img, tag_ids, tokens};
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

/// The shape of the inline HTML `html`: one tag, or else `Other`; a tag of a
/// phrase that `level` does not read is `Other`.
pub(super) fn read_tag(html: &str, level: HtmlLevel) -> Shape {
    let Some(tokens) = tokens(html) else {
        return Shape::Other;
    };
    match tokens.as_slice() {
        [Token::Open(tag)] => shape(tag, false, level),
        [Token::Close(name)] => Phrase::from_name(name, level).map_or(Shape::Other, Shape::End),
        _ => Shape::Other,
    }
}

/// The shape of a tag as it was read: a start tag, or an end tag when `end`.
pub(super) fn shape(tag: &Tag, end: bool, level: HtmlLevel) -> Shape {
    if end {
        return Phrase::from_name(&tag.name, level).map_or(Shape::Other, Shape::End);
    }
    match tag.name.as_str() {
        "br" => Shape::Break,
        "img" => img(tag).map_or(Shape::Other, Shape::Image),
        "a" => Shape::Start {
            phrase: Phrase::A,
            href: tag.get("href").map(str::to_owned),
            ids: tag_ids(tag),
        },
        name => Phrase::from_name(name, level).map_or(Shape::Other, |phrase| Shape::Start {
            phrase,
            href: None,
            ids: Vec::new(),
        }),
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
    /// The level of HTML that is read.
    level: HtmlLevel,
    /// What becomes of the tags that have no effect.
    tags: HtmlTags,
    inlines: Vec<Inline>,
    /// The start tags still open, a stack for each phrase.
    open: [Vec<Start>; PHRASES],
    /// How many start tags are open, of every phrase.
    depth: usize,
}

impl Joined {
    /// An empty join that reads the tags that `level` has, and shows the
    /// ones with no effect as `tags` says.
    pub fn new(level: HtmlLevel, tags: HtmlTags) -> Self {
        Self {
            level,
            tags,
            ..Self::default()
        }
    }

    /// Adds `inline`: a tag of inline HTML may start or end an element.
    pub fn push(&mut self, inline: Inline) {
        match inline {
            Inline::Html(tag) => {
                let shape = read_tag(&tag, self.level);
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
        let shape = read_tag(&tag, self.level);
        self.push_shaped(tag, shape);
    }

    /// Adds one tag of inline HTML, `tag` as written, whose shape is read.
    fn push_shaped(&mut self, tag: String, shape: Shape) {
        match shape {
            Shape::Break => self.inlines.push(Inline::LineBreak),
            Shape::Image(img) => self.inlines.push(image(img)),
            shape => self.push_tag(tag, shape),
        }
    }

    /// The inlines, with the start tags that nothing ended as they are, or
    /// left out when the tags with no effect are hidden.
    pub fn into_inlines(self) -> Vec<Inline> {
        self.finish(false)
    }

    /// The inlines as [`Joined::into_inlines`] gives them. Where a tag that
    /// is left out sat between two spaces, they are one when `collapse`.
    fn finish(self, collapse: bool) -> Vec<Inline> {
        let mut inlines = self.inlines;
        if self.tags == HtmlTags::Hide {
            hide_tags(&mut inlines, collapse);
        }
        inlines
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
        // A `<picture>` is the `<img>` it holds, without its `<source>` tags.
        if phrase == Phrase::Picture {
            self.forget_from(start.at);
            let shown = content
                .into_iter()
                .filter(|inline| !matches!(inline, Inline::Html(tag) if is_tag_of(tag, "source")));
            self.inlines.extend(shown);
            return;
        }
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
        // A `<span>` is transparent: what it holds takes its place.
        if phrase == Phrase::Span {
            self.inlines.extend(content);
            return;
        }
        self.inlines.push(match phrase {
            Phrase::A | Phrase::Picture | Phrase::Span => unreachable!("made above"),
            Phrase::U | Phrase::Ins => Inline::Underline(content),
            Phrase::Kbd => Inline::Kbd(content),
            Phrase::Mark => Inline::Mark(content),
            Phrase::Sub => Inline::Subscript(content),
            Phrase::Sup => Inline::Superscript(content),
            Phrase::B | Phrase::Strong => Inline::Strong(content),
            Phrase::I | Phrase::Em => Inline::Emphasis(content),
            Phrase::S | Phrase::Del | Phrase::Strike => Inline::Strikethrough(content),
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

/// Whether `html` is one start tag or one end tag: not a comment, a
/// declaration or source that is shown as written.
pub(super) fn is_tag(html: &str) -> bool {
    matches!(
        tokens(html).as_deref(),
        Some([Token::Open(_) | Token::Close(_)])
    )
}

/// Leaves the tags out of `inlines` and of what is in them, and joins the
/// text that is left side by side. Where two spaces meet at the join, one is
/// left when `collapse`, as a browser shows them.
fn hide_tags(inlines: &mut Vec<Inline>, collapse: bool) {
    let before = std::mem::take(inlines);
    for mut inline in before {
        match &mut inline {
            Inline::Html(html) if is_tag(html) => continue,
            Inline::Text(text) => {
                if let Some(Inline::Text(last)) = inlines.last_mut() {
                    let trimmed = match collapse && last.ends_with(' ') {
                        true => text.trim_start_matches(' '),
                        false => text,
                    };
                    last.push_str(trimmed);
                    continue;
                }
            }
            Inline::Emphasis(inner)
            | Inline::Strong(inner)
            | Inline::Strikethrough(inner)
            | Inline::Kbd(inner)
            | Inline::Mark(inner)
            | Inline::Subscript(inner)
            | Inline::Superscript(inner)
            | Inline::Underline(inner)
            | Inline::Link { content: inner, .. } => hide_tags(inner, collapse),
            _ => {}
        }
        inlines.push(inline);
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
pub fn html_inlines(content: Vec<Inline>, level: HtmlLevel, tags: HtmlTags) -> Vec<Inline> {
    let mut joined = Joined::new(level, tags);
    for inline in content {
        match inline {
            Inline::Html(tag) => joined.push_html(tag),
            inline => joined.push(inline),
        }
    }
    joined.into_inlines()
}

/// Whether `tag`, a tag as written, is a start tag of the element `name`,
/// in any case. The tag was read when it was added, so its name is where a
/// tag's name is.
fn is_tag_of(tag: &str, name: &str) -> bool {
    let rest = tag.strip_prefix('<').unwrap_or("");
    rest.get(..name.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(name))
        && !rest[name.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-')
}

/// A leaf of a run of text in an HTML block, as the tree of the block read
/// it, with its bytes as written.
pub enum Leaf<'a> {
    /// Text, its entities not decoded yet.
    Text(&'a str),
    Comment,
    /// A tag, a start tag or an end tag when `end`, as read.
    Tag(&'a str, &'a Tag, bool),
    /// An element left as written, such as a `<script>`: a tag of its own.
    Source(&'a str),
}

/// What is left of a run as its text and tags are read, before they are
/// joined.
enum Item {
    Text(String),
    Tag(String, Shape),
}

/// The inlines of a run of `leaves` of an HTML block: its text, entities
/// decoded and whitespace collapsed, with none at its ends and none at the
/// start of a line after a `<br>` or at its end before one, where a browser
/// shows none; and the tags kiri reads in text joined into the elements they
/// make. Each tag is read once, by the tree.
pub fn run_inlines<'a>(
    leaves: impl IntoIterator<Item = Leaf<'a>>,
    level: HtmlLevel,
    tags: HtmlTags,
) -> Vec<Inline> {
    let mut items: Vec<Item> = Vec::new();
    for leaf in leaves {
        match leaf {
            Leaf::Text(raw) => {
                let text = collapse_whitespace(&decode_entities(raw));
                match items.last_mut() {
                    // A comment between two texts shows nothing, so their
                    // spaces at the join are one.
                    Some(Item::Text(last)) => {
                        let text = match last.ends_with(' ') {
                            true => text.trim_start_matches(' '),
                            false => &text,
                        };
                        last.push_str(text);
                    }
                    _ => items.push(Item::Text(text)),
                }
            }
            Leaf::Comment => {}
            Leaf::Tag(written, tag, end) => {
                items.push(Item::Tag(written.to_owned(), shape(tag, end, level)))
            }
            Leaf::Source(written) => items.push(Item::Tag(written.to_owned(), Shape::Other)),
        }
    }
    if let Some(Item::Text(first)) = items.first_mut() {
        *first = first.trim_start_matches(' ').to_owned();
    }
    if let Some(Item::Text(last)) = items.last_mut() {
        *last = last.trim_end_matches(' ').to_owned();
    }
    items.retain(|item| !matches!(item, Item::Text(text) if text.is_empty()));
    for at in 0..items.len() {
        if !matches!(items[at], Item::Tag(_, Shape::Break)) {
            continue;
        }
        let blank = |c: char| c.is_ascii_whitespace();
        if let Some(Item::Text(text)) = items.get_mut(at + 1) {
            *text = text.trim_start_matches(blank).to_owned();
        }
        if let Some(Item::Text(text)) = at.checked_sub(1).and_then(|before| items.get_mut(before)) {
            *text = text.trim_end_matches(blank).to_owned();
        }
    }
    let mut joined = Joined::new(level, tags);
    for item in items {
        match item {
            Item::Text(text) if text.is_empty() => {}
            Item::Text(text) => joined.push(Inline::Text(text)),
            Item::Tag(written, shape) => joined.push_shaped(written, shape),
        }
    }
    let mut inlines = joined.finish(true);
    trim_edge(&mut inlines, true);
    trim_edge(&mut inlines, false);
    inlines
}

/// `inlines` without the white space at their start, or at their end when
/// not `start`, also where it is the first text inside an element such as a
/// link: a browser shows none at the start or the end of a line. A place
/// that an `<a>` names shows nothing, and is passed over.
fn trim_edge(inlines: &mut Vec<Inline>, start: bool) {
    loop {
        let at = match start {
            true => inlines
                .iter()
                .position(|inline| !matches!(inline, Inline::Anchor(_))),
            false => inlines
                .iter()
                .rposition(|inline| !matches!(inline, Inline::Anchor(_))),
        };
        let Some(at) = at else { return };
        match &mut inlines[at] {
            Inline::Text(text) => {
                let trimmed = match start {
                    true => text.trim_start_matches(|c: char| c.is_ascii_whitespace()),
                    false => text.trim_end_matches(|c: char| c.is_ascii_whitespace()),
                };
                if !trimmed.is_empty() {
                    *text = trimmed.to_owned();
                    return;
                }
                inlines.remove(at);
            }
            Inline::Link { content, .. }
            | Inline::Strong(content)
            | Inline::Emphasis(content)
            | Inline::Strikethrough(content)
            | Inline::Underline(content)
            | Inline::Mark(content) => return trim_edge(content, start),
            _ => return,
        }
    }
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
