//! The table of HTML elements that kiri knows: one row for each, with its kind, the level of HTML it belongs to, what it holds and how it is treated.
//!
//! What an element is, is said here and nowhere else: the readers of tags
//! (`html.rs`, `html/wrapper.rs`, the joiner of wrappers) ask the table
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

/// What an element may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holds {
    /// Nothing: a void element.
    Nothing,
    /// Text and the elements of text.
    Text,
    /// Blocks, and text between them.
    Blocks,
}

/// The start tags that end an element whose end tag may be left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndedBy {
    /// None: the element is ended by its end tag, or never.
    Nothing,
    /// The start tag of any block element, as a `<p>` is.
    AnyBlock,
}

/// Whether the start tag of an element centres what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Centres {
    Always,
    /// When its `align` is `center`.
    ByAlign,
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
    U,
    /// `<a href>`: its start tag has an address, which the link takes.
    A,
}

impl Phrase {
    /// The phrase that the element named `name` is, from its row.
    pub fn from_name(name: &str) -> Option<Self> {
        match get(name)?.treatment {
            Treatment::Phrase(phrase) => Some(phrase),
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
    /// A block may wrap its one `<img>` in it and still be that image. A
    /// browser shows `<picture>` as its `<img>` unless a `<source>` fits,
    /// and an `<a>` with an `href` makes the image a link.
    pub wraps_image: bool,
    /// Its end tag ends the elements still open inside it (the joiner of
    /// wrappers; it goes with the joiner).
    pub ends_inside: bool,
    /// Its start tag opens it when other things share its HTML block too
    /// (the joiner of wrappers; it goes with the joiner).
    pub opens_beside: bool,
}

impl Element {
    /// Whether its start tag `tag` centres what it holds.
    pub(super) fn centres_with(&self, tag: &Tag) -> bool {
        match self.treatment {
            Treatment::Wrapper(Centres::Always) => true,
            Treatment::Wrapper(Centres::ByAlign) => is_centred(tag),
            _ => false,
        }
    }

    /// Whether it is an element whose tags join blocks: a wrapper or a
    /// `<details>`.
    fn joins(&self) -> bool {
        matches!(self.treatment, Treatment::Wrapper(_) | Treatment::Details)
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
        wraps_image: false,
        ends_inside: false,
        opens_beside: false,
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
/// not here yet.
pub static TABLE: [Element; 32] = [
    Element {
        end_optional: true,
        ended_by: EndedBy::AnyBlock,
        wraps_image: true,
        opens_beside: true,
        ..row(
            "p",
            Kind::Block,
            Level::Github,
            Holds::Text,
            Treatment::Wrapper(Centres::ByAlign),
        )
    },
    Element {
        wraps_image: true,
        opens_beside: true,
        ..row(
            "div",
            Kind::Block,
            Level::Github,
            Holds::Blocks,
            Treatment::Wrapper(Centres::ByAlign),
        )
    },
    Element {
        wraps_image: true,
        opens_beside: true,
        ..row(
            "center",
            Kind::Block,
            Level::All,
            Holds::Blocks,
            Treatment::Wrapper(Centres::Always),
        )
    },
    Element {
        ends_inside: true,
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
    Element {
        wraps_image: true,
        ..phrase("a", Level::Github, Phrase::A)
    },
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
    Element {
        wraps_image: true,
        ..row(
            "picture",
            Kind::Text,
            Level::Github,
            Holds::Text,
            Treatment::Source,
        )
    },
    Element {
        wraps_image: true,
        ..row(
            "source",
            Kind::Void,
            Level::Github,
            Holds::Nothing,
            Treatment::Source,
        )
    },
];

/// The row of the element named `name`, in lower case.
pub fn get(name: &str) -> Option<&'static Element> {
    TABLE.iter().find(|element| element.name == name)
}

/// The row of the element named `name` when its tags join blocks: a
/// wrapper or a `<details>`.
pub fn joiner(name: &str) -> Option<&'static Element> {
    get(name).filter(|element| element.joins())
}

/// The level of a tag named `h1` to `h6`.
pub fn heading_level(name: &str) -> Option<u8> {
    match get(name)?.treatment {
        Treatment::Heading(level) => Some(level),
        _ => None,
    }
}

/// Whether the element named `name` holds raw text: CommonMark ends an HTML
/// block that starts with it at its own end tag.
pub fn is_raw_text(name: &str) -> bool {
    get(name).is_some_and(|element| element.kind == Kind::RawText)
}

/// Whether a block may wrap its one `<img>` in the element named `name`.
pub fn wraps_image(name: &str) -> bool {
    get(name).is_some_and(|element| element.wraps_image)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_has_a_lower_case_name_of_its_own() {
        for (at, row) in TABLE.iter().enumerate() {
            assert_eq!(row.name, row.name.to_ascii_lowercase());
            assert!(
                TABLE[..at].iter().all(|other| other.name != row.name),
                "{} twice",
                row.name
            );
        }
    }

    #[test]
    fn the_kind_of_a_row_says_what_it_holds() {
        for row in &TABLE {
            assert_eq!(
                row.kind == Kind::Void,
                row.holds == Holds::Nothing,
                "{}",
                row.name
            );
        }
    }

    #[test]
    fn only_the_rows_that_say_so_are_headings_phrases_and_wrappers() {
        let names = |keep: fn(&Element) -> bool| -> Vec<&str> {
            TABLE
                .iter()
                .filter(|row| keep(row))
                .map(|row| row.name)
                .collect()
        };
        assert_eq!(
            names(|row| matches!(row.treatment, Treatment::Heading(_))),
            ["h1", "h2", "h3", "h4", "h5", "h6"]
        );
        assert_eq!(names(Element::joins), ["p", "div", "center", "details"]);
        assert_eq!(
            names(|row| matches!(row.treatment, Treatment::Phrase(_))),
            [
                "kbd", "mark", "sub", "sup", "b", "strong", "i", "em", "s", "del", "code", "u", "a"
            ]
        );
        assert_eq!(
            names(|row| row.wraps_image),
            ["p", "div", "center", "a", "picture", "source"]
        );
        assert_eq!(
            names(|row| row.kind == Kind::RawText),
            ["pre", "script", "style", "textarea"]
        );
        assert_eq!(heading_level("h3"), Some(3));
        assert_eq!(heading_level("h7"), None);
        assert_eq!(heading_level("hr"), None);
    }

    #[test]
    fn u_and_center_are_only_at_the_level_all() {
        let level = |name| get(name).map(|row| row.level);
        assert_eq!(level("u"), Some(Level::All));
        assert_eq!(level("center"), Some(Level::All));
        assert_eq!(level("div"), Some(Level::Github));
        assert_eq!(level("script"), Some(Level::Neither));
    }
}
