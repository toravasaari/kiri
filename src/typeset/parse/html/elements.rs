//! The table of HTML elements that kiri knows: one row for each, with its kind, the level of HTML it belongs to, what it holds and how it is treated.
//!
//! What an element is, is said here and nowhere else: the readers of tags
//! (the tree, the reading of it as blocks and the inline join) ask the table
//! instead of keeping a list of names of their own. A row is data; the code
//! that draws an element is named in its `treatment`.

use super::Tag;

/// What sort of element a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Its own block: it may hold blocks, and starts on a line of its own.
    Block,
    /// Part of running text, read by the inline join.
    Text,
    /// Has no end tag and holds nothing.
    Void,
    /// Has no end tag and holds nothing, and is a block of its own, as `<hr>`.
    VoidBlock,
    /// Holds text that is no tags until its own end tag, such as `<pre>`.
    RawText,
}

/// The levels of HTML that kiri reads: what GitHub shows, or all that kiri
/// reads. The second has the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// GitHub's list, and so the other level as well.
    Github,
    /// Only the level "all" has it.
    All,
    /// Neither level has it: a tag of it has no effect.
    Neither,
}

/// The level of HTML that a document is read at, which the setting `html`
/// chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HtmlLevel {
    /// What GitHub shows.
    #[default]
    Github,
    /// All that kiri reads: `<u>`, `<center>` and task boxes as well.
    All,
}

/// What becomes of a tag that has no effect, which the setting `html_tags`
/// chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HtmlTags {
    /// Shown dim, where it is.
    #[default]
    Dim,
    /// Left out. Source that is shown as written is never left out.
    Hide,
}

impl Level {
    /// Whether an element of this level is read at the level `chosen`.
    pub fn is_read_at(self, chosen: HtmlLevel) -> bool {
        match self {
            Self::Github => true,
            Self::All => chosen == HtmlLevel::All,
            Self::Neither => false,
        }
    }
}

/// What an element may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holds {
    /// Nothing: a void element.
    Nothing,
    /// Text and the elements of text.
    Text,
    /// Blocks, and text between them.
    Blocks,
    /// Only elements of these names, as a `<ul>` holds only `<li>`s; white
    /// space and comments are allowed too, and nothing else.
    Only(&'static [&'static str]),
}

/// The start tags that end an element whose end tag may be left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndedBy {
    /// None: the element is ended by its end tag, or never.
    Nothing,
    /// The start tag of any block element, as a `<p>` is.
    AnyBlock,
    /// A start tag of one of these names, as an `<li>` is by another `<li>`.
    Names(&'static [&'static str]),
}

/// Whether the start tag of an element centres what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Centres {
    Always,
    /// When its `align` is `center`.
    ByAlign,
}

/// The enum `Phrase` and the list `Phrase::ALL` of its variants, made from one
/// list of names, so that no variant is missing from the list or from the
/// count `PHRASES` that is its length. The comments of a name go with it.
macro_rules! phrases {
    ($($(#[$doc:meta])* $name:ident,)*) => {
        /// An inline element that kiri reads from its opening and closing
        /// tags, with the inlines between them as its content.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Phrase {
            $($(#[$doc])* $name,)*
        }

        impl Phrase {
            /// Every phrase, in the order of the enum: `ALL[i] as usize == i`.
            pub const ALL: &'static [Phrase] = &[$(Phrase::$name,)*];
        }
    };
}

phrases! {
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
    U,
    /// `<span>`: it draws nothing of its own, so what it holds is read as if
    /// its tags were not there.
    Span,
    /// `<ins>`: inserted text, underlined as a browser draws it.
    Ins,
    /// `<strike>`: struck through, as `<s>` and `<del>` are.
    Strike,
    /// `<picture>`: what it holds, which is its `<img>`, without the
    /// `<source>` tags, which a terminal has no use for.
    Picture,
    /// `<a href>`: its start tag has an address, which the link takes.
    A,
}

impl Phrase {
    /// The phrase that the element named `name` is, from its row, when its
    /// row is read at the level `level`.
    pub fn from_name(name: &str, level: HtmlLevel) -> Option<Self> {
        let row = get(name)?;
        match row.treatment {
            Treatment::Phrase(phrase) if row.level.is_read_at(level) => Some(phrase),
            _ => None,
        }
    }
}

/// What kiri does with an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Treatment {
    /// A wrapper of blocks, which centres what it holds as `Centres` says.
    Wrapper(Centres),
    /// A `<details>` with its `<summary>`.
    Details,
    /// The `<summary>` of a `<details>`.
    Summary,
    /// A heading of this level.
    Heading(u8),
    /// A code block.
    Pre,
    /// An inline element, joined by the inline join.
    Phrase(Phrase),
    /// A line break.
    LineBreak,
    /// An image.
    Image,
    /// A `<ul>` or `<ol>` whose items are read as a list.
    List,
    /// An `<li>`: an item of the list that reads it, and nothing of its own
    /// anywhere else, where its tags are dim.
    Item,
    /// An `<input>`: a task box where a list's item starts with it, at the
    /// level that reads boxes; elsewhere a tag with no effect.
    Input,
    /// Not read, and left as source.
    Source,
}

/// A row of the table.
#[derive(Debug, PartialEq, Eq)]
pub struct Element {
    pub name: &'static str,
    pub kind: Kind,
    /// The lowest level that has it.
    pub level: Level,
    pub holds: Holds,
    pub treatment: Treatment,
    /// Its end tag may be left out: a browser ends it at the end tag of an
    /// element round it and at a start tag that `ended_by` names.
    pub end_optional: bool,
    pub ended_by: EndedBy,
}

impl Element {
    /// Whether the level `chosen` is below the one that has this element:
    /// its tags have no effect. The elements that no level has are not
    /// beyond it; they are left as source at every level.
    pub fn is_beyond(&self, chosen: HtmlLevel) -> bool {
        self.level == Level::All && chosen == HtmlLevel::Github
    }

    /// Whether its start tag `tag` centres what it holds.
    pub(super) fn centres_with(&self, tag: &Tag) -> bool {
        match self.treatment {
            Treatment::Wrapper(Centres::Always) => true,
            Treatment::Wrapper(Centres::ByAlign) => is_centred(tag),
            _ => false,
        }
    }
}

/// A row with the usual values: an element whose end tag is its own.
const fn row(
    name: &'static str,
    kind: Kind,
    level: Level,
    holds: Holds,
    treatment: Treatment,
) -> Element {
    Element {
        name,
        kind,
        level,
        holds,
        treatment,
        end_optional: false,
        ended_by: EndedBy::Nothing,
    }
}

const fn heading(name: &'static str, level: u8) -> Element {
    row(
        name,
        Kind::Block,
        Level::Github,
        Holds::Text,
        Treatment::Heading(level),
    )
}

const fn phrase(name: &'static str, level: Level, phrase: Phrase) -> Element {
    row(
        name,
        Kind::Text,
        level,
        Holds::Text,
        Treatment::Phrase(phrase),
    )
}

/// A block element of GitHub's list that kiri leaves as source for now; a
/// row that is read overrides its treatment, as `ul`, `ol` and `li` do.
const fn source_block(name: &'static str) -> Element {
    row(
        name,
        Kind::Block,
        Level::Github,
        Holds::Blocks,
        Treatment::Source,
    )
}

/// The raw-text elements that are no more than source: CommonMark ends an
/// HTML block that starts with one at its own end tag.
const fn raw(name: &'static str) -> Element {
    row(
        name,
        Kind::RawText,
        Level::Neither,
        Holds::Text,
        Treatment::Source,
    )
}

/// The elements that kiri reads. GitHub's level is the list of its
/// sanitizer (html-pipeline 2.14.3) plus `<picture>` and `<source>`; of
/// these, the rows are those that kiri reads, and the rest of the list is
/// not here yet. The raw-text elements that CommonMark's HTML blocks end
/// at, `<script>`, `<style>` and `<textarea>`, are rows too, at no level.
pub static TABLE: [Element; 47] = [
    Element {
        end_optional: true,
        ended_by: EndedBy::AnyBlock,
        ..row(
            "p",
            Kind::Block,
            Level::Github,
            Holds::Text,
            Treatment::Wrapper(Centres::ByAlign),
        )
    },
    Element {
        ..row(
            "div",
            Kind::Block,
            Level::Github,
            Holds::Blocks,
            Treatment::Wrapper(Centres::ByAlign),
        )
    },
    Element {
        ..row(
            "center",
            Kind::Block,
            Level::All,
            Holds::Blocks,
            Treatment::Wrapper(Centres::Always),
        )
    },
    Element {
        ..row(
            "details",
            Kind::Block,
            Level::Github,
            Holds::Blocks,
            Treatment::Details,
        )
    },
    row(
        "summary",
        Kind::Block,
        Level::Github,
        Holds::Text,
        Treatment::Summary,
    ),
    heading("h1", 1),
    heading("h2", 2),
    heading("h3", 3),
    heading("h4", 4),
    heading("h5", 5),
    heading("h6", 6),
    row(
        "pre",
        Kind::RawText,
        Level::Github,
        Holds::Text,
        Treatment::Pre,
    ),
    raw("script"),
    raw("style"),
    raw("textarea"),
    phrase("kbd", Level::Github, Phrase::Kbd),
    phrase("mark", Level::Github, Phrase::Mark),
    phrase("sub", Level::Github, Phrase::Sub),
    phrase("sup", Level::Github, Phrase::Sup),
    phrase("b", Level::Github, Phrase::B),
    phrase("strong", Level::Github, Phrase::Strong),
    phrase("i", Level::Github, Phrase::I),
    phrase("em", Level::Github, Phrase::Em),
    phrase("s", Level::Github, Phrase::S),
    phrase("del", Level::Github, Phrase::Del),
    phrase("code", Level::Github, Phrase::Code),
    phrase("u", Level::All, Phrase::U),
    phrase("span", Level::Github, Phrase::Span),
    phrase("ins", Level::Github, Phrase::Ins),
    phrase("strike", Level::Github, Phrase::Strike),
    phrase("a", Level::Github, Phrase::A),
    row(
        "br",
        Kind::Void,
        Level::Github,
        Holds::Nothing,
        Treatment::LineBreak,
    ),
    row(
        "img",
        Kind::Void,
        Level::Github,
        Holds::Nothing,
        Treatment::Image,
    ),
    phrase("picture", Level::Github, Phrase::Picture),
    row(
        "source",
        Kind::Void,
        Level::Github,
        Holds::Nothing,
        Treatment::Source,
    ),
    // A task box, read at the level "all" where an item starts with it.
    row(
        "input",
        Kind::Void,
        Level::All,
        Holds::Nothing,
        Treatment::Input,
    ),
    // The other block elements of GitHub's list, which end an open `<p>` and
    // are left as source for now.
    source_block("blockquote"),
    source_block("dl"),
    Element {
        end_optional: true,
        ..source_block("dt")
    },
    Element {
        end_optional: true,
        ..source_block("dd")
    },
    source_block("figure"),
    source_block("figcaption"),
    row(
        "hr",
        Kind::VoidBlock,
        Level::Github,
        Holds::Nothing,
        Treatment::Source,
    ),
    Element {
        holds: Holds::Only(&["li"]),
        treatment: Treatment::List,
        ..source_block("ol")
    },
    Element {
        holds: Holds::Only(&["li"]),
        treatment: Treatment::List,
        ..source_block("ul")
    },
    Element {
        end_optional: true,
        ended_by: EndedBy::Names(&["li"]),
        treatment: Treatment::Item,
        ..source_block("li")
    },
    source_block("table"),
];

/// The row of the element named `name`, in lower case.
pub fn get(name: &str) -> Option<&'static Element> {
    TABLE.iter().find(|element| element.name == name)
}

/// Where the row of the element named `name` is in `TABLE`.
pub fn index(name: &str) -> Option<usize> {
    TABLE.iter().position(|element| element.name == name)
}

/// The level of a tag named `h1` to `h6`.
pub fn heading_level(name: &str) -> Option<u8> {
    match get(name)?.treatment {
        Treatment::Heading(level) => Some(level),
        _ => None,
    }
}

/// Whether the `align` of `tag` is `center`, in any case, as a browser
/// reads it.
pub(super) fn is_centred(tag: &Tag) -> bool {
    (tag.get("align")).is_some_and(|align| {
        align
            .trim_matches(|c: char| c.is_ascii_whitespace())
            .eq_ignore_ascii_case("center")
    })
}
