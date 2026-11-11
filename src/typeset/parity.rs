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

use crate::render::{Inputs, TableRows};
use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
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
        Block::Table(table) => (table.header.iter())
            .chain(table.rows.iter().flatten())
            .map(Vec::as_slice)
            .collect(),
        Block::Details { summary, .. } => vec![summary],
        _ => Vec::new(),
    };
    (own.into_iter())
        .chain(block.children().flat_map(|(_, child)| inlines(child)))
        .collect()
}

/// Whether `text` is what a list marker reads as: a bullet or a number
/// and a full stop.
pub fn is_marker(text: &str) -> bool {
    let number = text
        .strip_suffix('.')
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()));
    number || ["•", "◦", "▪", "-"].contains(&text)
}

/// A marker as the facts say it: numbers are `N.`.
pub fn marker_value(text: &str) -> String {
    if text.ends_with('.') {
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
        field: Field::HeadingRule,
        at: "level 1",
        cell: "false",
        pixel: "true",
        status: Status::Drift,
        bug: Some("B17"),
        cell_where: "layout/mod.rs:756: the look's heading `rule`, which the built-in theme has off for every level",
        pixel_where: "reading/page/heading.rs:24: a rule when `level` is within the look's `heading_rules`, 2, whatever the theme says",
    },
    Difference {
        field: Field::HeadingRule,
        at: "level 2",
        cell: "false",
        pixel: "true",
        status: Status::Drift,
        bug: Some("B17"),
        cell_where: "layout/mod.rs:756: the look's heading `rule`, which the built-in theme has off for every level",
        pixel_where: "reading/page/heading.rs:24: a rule when `level` is within the look's `heading_rules`, 2, whatever the theme says",
    },
    Difference {
        field: Field::HeadingSize,
        at: "level 1",
        cell: "2.50",
        pixel: "2.00",
        status: Status::Drift,
        bug: Some("B17"),
        cell_where: "theme/mod.rs:377: the theme's size, 2.5, 2.0, then quarters down to 1.0, rounded to a scale kitty draws",
        pixel_where: "theme/look.rs:190: the look's `heading_sizes`, GitHub's 2.0, 1.5, 1.25, 1.0, 0.875, 0.85",
    },
    Difference {
        field: Field::HeadingSize,
        at: "level 2",
        cell: "2.00",
        pixel: "1.50",
        status: Status::Drift,
        bug: Some("B17"),
        cell_where: "theme/mod.rs:377: the theme's size, 2.5, 2.0, then quarters down to 1.0, rounded to a scale kitty draws",
        pixel_where: "theme/look.rs:190: the look's `heading_sizes`, GitHub's 2.0, 1.5, 1.25, 1.0, 0.875, 0.85",
    },
    Difference {
        field: Field::HeadingSize,
        at: "level 3",
        cell: "1.75",
        pixel: "1.25",
        status: Status::Drift,
        bug: Some("B17"),
        cell_where: "theme/mod.rs:377: the theme's size, 2.5, 2.0, then quarters down to 1.0, rounded to a scale kitty draws",
        pixel_where: "theme/look.rs:190: the look's `heading_sizes`, GitHub's 2.0, 1.5, 1.25, 1.0, 0.875, 0.85",
    },
    Difference {
        field: Field::HeadingSize,
        at: "level 4",
        cell: "1.50",
        pixel: "1.00",
        status: Status::Drift,
        bug: Some("B17"),
        cell_where: "theme/mod.rs:377: the theme's size, 2.5, 2.0, then quarters down to 1.0, rounded to a scale kitty draws",
        pixel_where: "theme/look.rs:190: the look's `heading_sizes`, GitHub's 2.0, 1.5, 1.25, 1.0, 0.875, 0.85",
    },
    Difference {
        field: Field::HeadingSize,
        at: "level 5",
        cell: "1.25",
        pixel: "0.88",
        status: Status::Drift,
        bug: Some("B17"),
        cell_where: "theme/mod.rs:377: the theme's size, 2.5, 2.0, then quarters down to 1.0, rounded to a scale kitty draws",
        pixel_where: "theme/look.rs:190: the look's `heading_sizes`, GitHub's 2.0, 1.5, 1.25, 1.0, 0.875, 0.85",
    },
    Difference {
        field: Field::HeadingSize,
        at: "level 6",
        cell: "1.00",
        pixel: "0.85",
        status: Status::Drift,
        bug: Some("B17"),
        cell_where: "theme/mod.rs:377: the theme's size, 2.5, 2.0, then quarters down to 1.0, rounded to a scale kitty draws",
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
mod tests {
    use super::*;
    use crate::typeset::layout::summary as cell;
    use crate::typeset::model::Document;
    use crate::typeset::parse::parse;
    use crate::typeset::reading::page::summary as pixel;
    use std::collections::BTreeSet;
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};

    /// The documents laid out: the examples' and the fixtures'.
    fn documents() -> Vec<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut found = Vec::new();
        for (dir, prefix) in [("examples", "S"), ("tests/fixtures", "")] {
            for entry in std::fs::read_dir(root.join(dir)).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if name.starts_with(prefix) && name.ends_with(".md") {
                    found.push(path);
                }
            }
        }
        found.sort();
        found
    }

    /// What each layout says of each block of each document, in each
    /// setup that matters to it, as a difference where they do not agree.
    struct Found {
        /// The block, as `file#n (kind)`.
        block: String,
        field: Field,
        at: String,
        cell: String,
        pixel: String,
    }

    fn kind(block: &Block) -> &'static str {
        match block {
            Block::FrontMatter(_) => "front matter",
            Block::Heading { .. } => "heading",
            Block::Paragraph(_) => "paragraph",
            Block::CodeBlock { .. } => "code block",
            Block::BlockQuote(_) => "quote",
            Block::Alert { .. } => "alert",
            Block::List(_) => "list",
            Block::Details { .. } => "details",
            Block::Table(_) => "table",
            Block::ThematicBreak => "rule",
            Block::Html(_) => "html",
            Block::Note(_) => "note",
            Block::Error(_) => "error",
            Block::ImageFile(_) => "image file",
        }
    }

    /// The facts the two sides do not agree on, each as its field, its
    /// case and what each side says, `(no fact)` where it says nothing.
    fn pair(cell: &[Fact], pixel: &[Fact]) -> Vec<(Field, String, String, String)> {
        // A fact is the same one on both sides by its field and case; the
        // second `mark` of a block is the second on the other side too.
        let mut rest: Vec<&Fact> = pixel.iter().collect();
        let mut out = Vec::new();
        let none = || "(no fact)".to_owned();
        for a in cell {
            let at = rest
                .iter()
                .position(|b| (b.field, &b.at) == (a.field, &a.at));
            let b = at.map(|at| rest.remove(at));
            if b != Some(a) {
                out.push((
                    a.field,
                    a.at.clone(),
                    a.value.clone(),
                    b.map_or_else(none, |b| b.value.clone()),
                ));
            }
        }
        for b in rest {
            out.push((b.field, b.at.clone(), none(), b.value.clone()));
        }
        out
    }

    fn compare(paths: &[PathBuf]) -> Vec<Found> {
        // One thread per document, and the findings joined in the order of
        // the documents, so the report reads as it did one document at a time.
        std::thread::scope(|scope| {
            let threads: Vec<_> = paths
                .iter()
                .map(|path| scope.spawn(move || compare_document(path)))
                .collect();
            threads
                .into_iter()
                .flat_map(|thread| {
                    // A panic keeps its message, as it did with no threads.
                    thread
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect()
        })
    }

    fn compare_document(path: &Path) -> Vec<Found> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut found = Vec::new();
        let source = std::fs::read_to_string(path).unwrap();
        let name = path.strip_prefix(root).unwrap().display().to_string();
        let Document { blocks, .. } = parse(&source);
        for (number, block) in blocks.iter().enumerate() {
            for setup in Setup::TABLES {
                let (cell, pixel) = (cell::summary(block, &setup), pixel::summary(block, &setup));
                // Only a table's facts change with the setup.
                let kept = |fact: &Fact| {
                    setup.is_default() || matches!(fact.field, Field::TableRows | Field::TableFrame)
                };
                let (cell, pixel): (Vec<_>, Vec<_>) = (
                    cell.into_iter().filter(kept).collect(),
                    pixel.into_iter().filter(kept).collect(),
                );
                let label = format!("{name}#{number} ({})", kind(block));
                for (field, at, cell, pixel) in pair(&cell, &pixel) {
                    found.push(Found {
                        block: label.clone(),
                        field,
                        at,
                        cell,
                        pixel,
                    });
                }
            }
        }
        found
    }

    /// What the test says is wrong with the list, or `None`.
    fn judge(found: &[Found], list: &[Difference]) -> Option<String> {
        judge_refs(found, &list.iter().collect::<Vec<_>>())
    }

    fn judge_refs(found: &[Found], list: &[&Difference]) -> Option<String> {
        let mut report = String::new();
        let listed = |f: &Found| {
            list.iter().position(|d| {
                (d.field, d.at, d.cell, d.pixel)
                    == (f.field, f.at.as_str(), f.cell.as_str(), f.pixel.as_str())
            })
        };
        let mut seen = BTreeSet::new();
        // Each difference not on the list once, with the blocks that show it.
        let mut unlisted: Vec<(&Found, Vec<&str>)> = Vec::new();
        for f in found {
            match listed(f) {
                Some(index) => {
                    seen.insert(index);
                }
                None => match unlisted.iter_mut().find(|(g, _)| {
                    (g.field, &g.at, &g.cell, &g.pixel) == (f.field, &f.at, &f.cell, &f.pixel)
                }) {
                    Some((_, blocks)) => blocks.push(&f.block),
                    None => unlisted.push((f, vec![&f.block])),
                },
            }
        }
        for (f, blocks) in unlisted {
            writeln!(
                report,
                "not on the list: {:?} at {:?}: cell {:?}, pixel {:?}; {} blocks, such as {}",
                f.field,
                f.at,
                f.cell,
                f.pixel,
                blocks.len(),
                blocks[..blocks.len().min(3)].join(", ")
            )
            .unwrap();
        }
        for (index, d) in list.iter().enumerate() {
            if !seen.contains(&index) {
                writeln!(
                    report,
                    "on the list, but no block shows it: {:?} at {:?}: cell {:?}, pixel {:?} \
                     (cell: {}; pixel: {})",
                    d.field, d.at, d.cell, d.pixel, d.cell_where, d.pixel_where
                )
                .unwrap();
            }
        }
        (!report.is_empty()).then_some(report)
    }

    #[test]
    fn the_layouts_differ_only_where_the_list_says() {
        let found = compare(&documents());
        if let Some(report) = judge(&found, DIFFERENCES) {
            panic!("the layouts and the list of differences disagree:\n{report}");
        }
        // Each entry is what keeps its difference from failing the test:
        // without it the message says what each layout decided.
        for (index, entry) in DIFFERENCES.iter().enumerate() {
            let rest: Vec<&Difference> = (DIFFERENCES.iter().enumerate())
                .filter(|(other, _)| *other != index)
                .map(|(_, entry)| entry)
                .collect();
            let report = judge_refs(&found, &rest).expect("a difference without its entry fails");
            let shown = format!(
                "not on the list: {:?} at {:?}: cell {:?}, pixel {:?}",
                entry.field, entry.at, entry.cell, entry.pixel
            );
            assert!(report.contains(&shown), "{shown} not in\n{report}");
        }
    }

    #[test]
    fn the_list_has_no_entry_twice_and_the_known_drifts_are_on_it() {
        let key = |d: &Difference| (d.field, d.at, d.cell, d.pixel);
        let keys: BTreeSet<String> = DIFFERENCES
            .iter()
            .map(|d| format!("{:?}", key(d)))
            .collect();
        assert_eq!(keys.len(), DIFFERENCES.len(), "an entry twice");
        let drift = |field: Field, at: &str| {
            (DIFFERENCES.iter())
                .any(|d| d.field == field && d.at.starts_with(at) && d.status == Status::Drift)
        };
        for (field, at) in [
            (Field::HeadingRule, "level 1"),
            (Field::HeadingSize, "level 1"),
            (Field::ListMarker, "depth 0 numbered task"),
            (Field::QuoteText, "first paragraph"),
            (Field::TableRows, "Lines"),
            (Field::TableFrame, "Grid"),
        ] {
            assert!(
                drift(field, at),
                "{field:?} at {at:?} is not a drift on the list"
            );
        }
    }

    #[test]
    fn a_drift_names_a_bug_the_roadmap_has_and_a_difference_that_is_meant_names_none() {
        let roadmap =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/roadmap.md"))
                .unwrap();
        // Which bug decides which field, as the roadmap sorts them.
        let decided_by = |d: &Difference| match (d.field, d.at) {
            (Field::HeadingRule | Field::HeadingSize | Field::QuoteText, _) => "B17",
            (Field::ListMarker, "depth 0 numbered task item") => "B17",
            (Field::TableRows | Field::TableFrame, _) => "B18",
            (field, at) => panic!("{field:?} at {at:?} is on no bug's list"),
        };
        for d in DIFFERENCES {
            // A table too narrow for its frame is stacked on purpose.
            assert_eq!(
                d.status == Status::Intended,
                d.cell == "stacked",
                "{:?} at {:?}: cell {:?}",
                d.field,
                d.at,
                d.cell
            );
            if d.status == Status::Drift {
                assert_eq!(
                    d.bug,
                    Some(decided_by(d)),
                    "{:?} at {:?}: cell {:?}",
                    d.field,
                    d.at,
                    d.cell
                );
            }
            match (d.status, d.bug) {
                (Status::Drift, Some(bug)) => assert!(
                    roadmap.lines().any(|line| line
                        .strip_prefix("#### ")
                        .is_some_and(|title| title.split(' ').next() == Some(bug))),
                    "{:?} at {:?} names {bug}, which is no heading of the roadmap",
                    d.field,
                    d.at
                ),
                (Status::Intended, None) => {}
                (status, bug) => panic!(
                    "{:?} at {:?} is {status:?} and names {bug:?}",
                    d.field, d.at
                ),
            }
        }
    }

    /// The value of the cell layout's fact of `field` for the one block of
    /// `markdown`.
    fn cell_value(markdown: &str, field: Field) -> String {
        let block = parse(markdown).blocks.remove(0);
        let facts = cell::summary(&block, &Setup::DEFAULT);
        (facts.into_iter().find(|fact| fact.field == field))
            .unwrap_or_else(|| panic!("no {field:?} fact for {markdown:?}"))
            .value
    }

    #[test]
    fn a_quote_is_read_by_its_text_not_by_a_link_or_code_that_opens_it() {
        // The cell layout leaves a quote's text in its own colour.
        for markdown in [
            "> plain text",
            "> [link](https://example.com) text",
            "> `code` text",
        ] {
            assert_eq!(cell_value(markdown, Field::QuoteText), "own", "{markdown}");
        }
    }

    #[test]
    fn a_heading_is_read_by_its_own_size_not_by_a_script_that_opens_it() {
        assert_eq!(
            cell_value("# <sup>2</sup> head", Field::HeadingSize),
            "2.50"
        );
    }

    #[test]
    fn a_heading_has_a_rule_only_where_one_is_drawn_not_where_its_text_has_a_line_glyph() {
        assert_eq!(cell_value("# ─ head", Field::HeadingRule), "false");
        assert_eq!(cell_value("# head", Field::HeadingRule), "false");
    }

    #[test]
    fn a_fact_one_side_lacks_is_reported_as_itself_and_not_shifts_the_rest() {
        let fact = |field, value: &str| Fact::new(field, "level 1", value);
        let cell = [fact(Field::HeadingRule, "false")];
        let pixel = [
            fact(Field::HeadingSize, "2.00"),
            fact(Field::HeadingRule, "true"),
        ];
        let line = |field, cell: &str, pixel: &str| {
            (
                field,
                "level 1".to_owned(),
                cell.to_owned(),
                pixel.to_owned(),
            )
        };
        assert_eq!(
            pair(&cell, &pixel),
            [
                line(Field::HeadingRule, "false", "true"),
                line(Field::HeadingSize, "(no fact)", "2.00"),
            ]
        );
    }

    #[test]
    fn a_heading_with_no_text_has_no_size_in_either_layout() {
        let block = parse("# ![](a.png)").blocks.remove(0);
        for facts in [
            cell::summary(&block, &Setup::DEFAULT),
            pixel::summary(&block, &Setup::DEFAULT),
        ] {
            assert!(
                facts.iter().all(|fact| fact.field != Field::HeadingSize),
                "{facts:?}"
            );
        }
    }
}
