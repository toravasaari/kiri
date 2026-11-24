//! The two layouts side by side: the cell layout and reading mode's pixel
//! layout lay out the same blocks, and the test compares what each decided
//! for them, field by field, against the list of differences known.
//!
//! Each layout gives a summary of a block as `Fact`s, from its own side:
//! `layout::summary` for the cell layout, `reading::page::summary` for the
//! pixel layout. Neither knows the other. A block is laid out alone, as a
//! document of one block, so that what is compared is the block's own
//! decisions and not where it falls on a page.
//!
//! A difference nobody has chosen is `Drift`; it stays on the list, naming
//! the roadmap bug that decides it. A difference that is meant is `Intended`.
//! The test fails on a difference that is not on the list, and on a listed
//! one that no block shows any more, so the list stays the truth.

use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
use crate::typeset::inputs::{Inputs, TableRows};
use crate::typeset::model::{Block, Contents, Inline, ListItem};
use crate::typeset::theme::{TableStyle, Theme};

/// What a summary says something about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The glyph or number before a list item; `none` when the item shows
    /// only a box. Numbers are all `N.`: that they count is not compared.
    ListMarker,
    /// The box of a task item: `☑` or `☐`.
    TaskBox,
    /// A heading's size, in sizes of the body text.
    HeadingSize,
    /// Whether a rule is drawn under a heading.
    HeadingRule,
    /// Whether a quote's text is in its own colour or dimmed.
    QuoteText,
    /// How a table's body rows are told apart.
    TableRows,
    /// The frame a table is drawn in.
    TableFrame,
    /// Whether `<mark>`, `<sub>` and `<sup>` change how their text looks.
    InlineStyle,
}

/// One decision of a layout about a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub field: Field,
    /// Which case of the field it is, such as `level 2` for a heading or
    /// `depth 3 bullet item` for a marker: the same on both sides.
    pub at: String,
    pub value: String,
}

impl Fact {
    pub fn new(field: Field, at: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            field,
            at: at.into(),
            value: value.into(),
        }
    }
}

/// How the layouts are set, apart from the default: tables are the one
/// block whose looks the settings change.
#[derive(Debug, Clone, Copy)]
pub struct Setup {
    pub table_rows: TableRows,
    pub table_style: TableStyle,
}

impl Setup {
    /// The built-in theme and the settings kiri starts with.
    pub const DEFAULT: Setup = Setup {
        table_rows: TableRows::Zebra,
        table_style: TableStyle::Frame,
    };

    /// Every setting a table can be laid out in: each way of telling rows
    /// apart, and reading mode's other frame.
    pub const TABLES: [Setup; 6] = [
        Self::DEFAULT,
        Setup {
            table_rows: TableRows::Off,
            ..Self::DEFAULT
        },
        Setup {
            table_rows: TableRows::Lines,
            ..Self::DEFAULT
        },
        Setup {
            table_rows: TableRows::Indent,
            ..Self::DEFAULT
        },
        Setup {
            table_rows: TableRows::Underline,
            ..Self::DEFAULT
        },
        Setup {
            table_style: TableStyle::Grid,
            ..Self::DEFAULT
        },
    ];

    pub fn is_default(&self) -> bool {
        self.table_rows == Self::DEFAULT.table_rows && self.table_style == Self::DEFAULT.table_style
    }

    pub fn theme(&self) -> Theme {
        Theme {
            table_style: self.table_style,
            ..Theme::default()
        }
    }

    /// A kitty window 80 columns wide, with cells of 10 by 20 pixels, on
    /// a dark page whose colours it tells, so that shading shows.
    pub fn caps(&self) -> Capabilities {
        Capabilities {
            cell_pixels: Some((10, 20)),
            dark_background: Some(true),
            foreground: Some(Color::Rgb(0xe6, 0xed, 0xf3)),
            background: Some(Color::Rgb(0x0d, 0x11, 0x17)),
            ..Capabilities::kitty(80)
        }
    }

    pub fn inputs(&self) -> Inputs {
        Inputs {
            theme: self.theme(),
            table_rows: self.table_rows,
            ..Inputs::new(self.caps())
        }
    }
}

/// One list item of a block, in document order, and where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// How many lists it is inside, itself included, less one.
    pub depth: usize,
    pub numbered: bool,
    pub checked: Option<bool>,
}

impl Slot {
    /// What the slot is called in `Fact::at`.
    pub fn name(&self) -> String {
        let kind = match (self.numbered, self.checked) {
            (false, None) => "bullet",
            (true, None) => "numbered",
            (false, Some(_)) => "task",
            (true, Some(_)) => "numbered task",
        };
        format!("depth {} {kind} item", self.depth)
    }

    /// The facts of `Field::TaskBox`, which only a task has.
    pub fn task_box(&self) -> Option<&'static str> {
        self.checked.map(|checked| if checked { "☑" } else { "☐" })
    }
}

/// The items of every list in `block`, a list's item before the items of
/// the lists inside it.
pub fn slots(block: &Block) -> Vec<Slot> {
    let mut out = Vec::new();
    collect_slots(std::slice::from_ref(block), 0, &mut out);
    out
}

fn collect_slots(blocks: &[Block], depth: usize, out: &mut Vec<Slot>) {
    for block in blocks {
        match block.contents() {
            Contents::None => {}
            Contents::Blocks(blocks) => collect_slots(blocks, depth, out),
            Contents::Items(items) => {
                let numbered = matches!(block, Block::List(list) if list.start.is_some());
                for ListItem { checked, blocks } in items {
                    out.push(Slot {
                        depth,
                        numbered,
                        checked: *checked,
                    });
                    collect_slots(blocks, depth + 1, out);
                }
            }
        }
    }
}

/// An inline that sets its text apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Apart {
    Mark,
    Sub,
    Sup,
}

impl Apart {
    pub fn name(self) -> &'static str {
        match self {
            Self::Mark => "mark",
            Self::Sub => "sub",
            Self::Sup => "sup",
        }
    }

    /// This kind wrapped round `content`.
    pub fn wrap(self, content: Vec<Inline>) -> Inline {
        match self {
            Self::Mark => Inline::Mark(content),
            Self::Sub => Inline::Subscript(content),
            Self::Sup => Inline::Superscript(content),
        }
    }
}

/// Every `<mark>`, `<sub>` and `<sup>` in `block`, with what is inside it.
pub fn apart(block: &Block) -> Vec<(Apart, Vec<Inline>)> {
    let mut out = Vec::new();
    for content in inlines(block) {
        collect_apart(content, &mut out);
    }
    out
}

fn collect_apart(content: &[Inline], out: &mut Vec<(Apart, Vec<Inline>)>) {
    for inline in content {
        let (kind, inner) = match inline {
            Inline::Mark(inner) => (Some(Apart::Mark), inner),
            Inline::Subscript(inner) => (Some(Apart::Sub), inner),
            Inline::Superscript(inner) => (Some(Apart::Sup), inner),
            Inline::Emphasis(inner)
            | Inline::Strong(inner)
            | Inline::Strikethrough(inner)
            | Inline::Kbd(inner)
            | Inline::Link { content: inner, .. } => (None, inner),
            _ => continue,
        };
        if let Some(kind) = kind {
            out.push((kind, inner.clone()));
        }
        collect_apart(inner, out);
    }
}

/// The runs of inlines directly in `block` and the blocks inside it.
fn inlines(block: &Block) -> Vec<&[Inline]> {
    let own: Vec<&[Inline]> = match block {
        Block::Heading { content, .. } | Block::Paragraph(content) => vec![content],
        Block::Table(table) => table.parsed_cells().into_iter().flatten().collect(),
        Block::Details { summary, .. } => vec![summary],
        _ => Vec::new(),
    };
    (own.into_iter())
        .chain(block.children().flat_map(|(_, child)| inlines(child)))
        .collect()
}

/// Whether `text` is what a list marker reads as: a bullet, or a number
/// and a full stop or in brackets.
pub fn is_marker(text: &str) -> bool {
    let digits = |digits: &str| !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit());
    let number = text.strip_suffix('.').is_some_and(digits)
        || text
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            .is_some_and(digits);
    number || ["•", "◦", "▪", "-"].contains(&text)
}

/// A marker as the facts say it: numbers are `N.`.
pub fn marker_value(text: &str) -> String {
    if text.ends_with('.') || text.ends_with(']') {
        "N.".to_owned()
    } else {
        text.to_owned()
    }
}

/// The facts of `slots`, each with what a layout showed for it: its marker,
/// and its box if it has one.
pub fn marker_facts(slots: &[Slot], shown: &[(String, Option<String>)]) -> Vec<Fact> {
    if slots.len() != shown.len() {
        return vec![Fact::new(
            Field::ListMarker,
            "items",
            format!("{} of {} shown", shown.len(), slots.len()),
        )];
    }
    let mut facts = Vec::new();
    for (slot, (marker, task_box)) in slots.iter().zip(shown) {
        facts.push(Fact::new(Field::ListMarker, slot.name(), marker));
        if slot.task_box().is_some() {
            let task_box = task_box.clone().unwrap_or_else(|| "none".to_owned());
            facts.push(Fact::new(Field::TaskBox, slot.name(), task_box));
        }
    }
    facts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Nobody chose it; a roadmap bug decides.
    Drift,
    /// Meant: the media differ on purpose.
    Intended,
}

/// A difference between the layouts that the test lets stand.
pub struct Difference {
    pub field: Field,
    pub at: &'static str,
    /// What each layout decides, as its facts say.
    pub cell: &'static str,
    pub pixel: &'static str,
    pub status: Status,
    /// The roadmap bug that decides a drift, such as `B17`; none for a
    /// difference that is meant.
    pub bug: Option<&'static str>,
    /// Where each layout decides it, and what it does.
    pub cell_where: &'static str,
    pub pixel_where: &'static str,
}

/// The differences the layouts have today, each shown by at least one block.
const DIFFERENCES: &[Difference] = &[
    Difference {
        field: Field::HeadingSize,
        at: "level 5",
        cell: "1.00",
        pixel: "0.88",
        status: Status::Drift,
        bug: Some("B17"),
        cell_where: "theme/mod.rs:408: the built-in theme's size, the look's `heading_sizes` but never below 1.0, rounded to a scale kitty draws",
        pixel_where: "theme/look.rs:190: the look's `heading_sizes`, GitHub's 2.0, 1.5, 1.25, 1.0, 0.875, 0.85",
    },
    Difference {
        field: Field::HeadingSize,
        at: "level 6",
        cell: "1.00",
        pixel: "0.85",
        status: Status::Drift,
        bug: Some("B17"),
        cell_where: "theme/mod.rs:408: the built-in theme's size, the look's `heading_sizes` but never below 1.0, rounded to a scale kitty draws",
        pixel_where: "theme/look.rs:190: the look's `heading_sizes`, GitHub's 2.0, 1.5, 1.25, 1.0, 0.875, 0.85",
    },
    Difference {
        field: Field::ListMarker,
        at: "depth 0 numbered task item",
        cell: "N.",
        pixel: "none",
        status: Status::Drift,
        bug: Some("B17"),
        cell_where: "layout/list.rs:15,46: the number, and the box after it",
        pixel_where: "reading/page/list.rs:44: a task item gets its box and no number",
    },
    Difference {
        field: Field::QuoteText,
        at: "first paragraph",
        cell: "own",
        pixel: "dimmed",
        status: Status::Drift,
        bug: Some("B17"),
        cell_where: "layout/quote.rs:12: the bar only; the text keeps its own colour",
        pixel_where: "reading/page/quote.rs:21: the text in the quote colour, the muted one",
    },
    Difference {
        field: Field::TableRows,
        at: "Lines",
        cell: "lines",
        pixel: "stripes",
        status: Status::Drift,
        bug: Some("B18"),
        cell_where: "layout/mod.rs:1088: a row style of five, by the `table_rows` setting",
        pixel_where: "reading/look.rs:109, reading/page.rs:86: stripes on or off; any setting but Off is on",
    },
    Difference {
        field: Field::TableRows,
        at: "Indent",
        cell: "indent",
        pixel: "stripes",
        status: Status::Drift,
        bug: Some("B18"),
        cell_where: "layout/mod.rs:1088: a row style of five, by the `table_rows` setting",
        pixel_where: "reading/look.rs:109, reading/page.rs:86: stripes on or off; any setting but Off is on",
    },
    Difference {
        field: Field::TableRows,
        at: "Underline",
        cell: "underline",
        pixel: "stripes",
        status: Status::Drift,
        bug: Some("B18"),
        cell_where: "layout/mod.rs:1088: a row style of five, by the `table_rows` setting",
        pixel_where: "reading/look.rs:109, reading/page.rs:86: stripes on or off; any setting but Off is on",
    },
    Difference {
        field: Field::TableFrame,
        at: "Grid",
        cell: "frame",
        pixel: "grid",
        status: Status::Drift,
        bug: Some("B18"),
        cell_where: "layout/table.rs:221: always a rounded box; `table_style` is not read",
        pixel_where: "reading/page/table.rs:262: the look's `table_style`, a rounded frame or a grid",
    },
    Difference {
        field: Field::TableFrame,
        at: "Frame",
        cell: "stacked",
        pixel: "frame",
        status: Status::Intended,
        bug: None,
        cell_where: "layout/table.rs:212: a table whose words do not fit the width is stacked, with no frame",
        pixel_where: "reading/page/table.rs:262: a table is always drawn as a frame or a grid, its cells wrapping",
    },
    Difference {
        field: Field::TableFrame,
        at: "Grid",
        cell: "stacked",
        pixel: "grid",
        status: Status::Intended,
        bug: None,
        cell_where: "layout/table.rs:212: a table whose words do not fit the width is stacked, with no frame",
        pixel_where: "reading/page/table.rs:262: a table is always drawn as a frame or a grid, its cells wrapping",
    },
];

#[cfg(test)]
mod tests;
