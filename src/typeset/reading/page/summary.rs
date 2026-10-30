//! Reading mode's side of the parity test: what the pixel layout decided
//! for a block, as facts the cell layout's summary gives too.

use crate::render::TableRows;
use crate::typeset::images::size::cell_size;
use crate::typeset::layout::CodeColors;
use crate::typeset::model::Block;
use crate::typeset::parity::{self, Fact, Field, Setup};
use crate::typeset::reading::engine::{self, Area};
use crate::typeset::reading::fonts::Fonts;
use crate::typeset::reading::page::{
    Frame, FrontMatterShown, Ink, Look, Page, Plain, Shape, TableStyle, Wide, lay_out, runs,
};

/// The facts of `block`, laid out alone in the pixel layout of `setup`.
pub(crate) fn summary(block: &Block, setup: &Setup) -> Vec<Fact> {
    let caps = setup.caps();
    let look = engine::look(
        &setup.theme(),
        &CodeColors::default(),
        setup.table_rows != TableRows::Off,
        &caps,
    );
    let (cell_width, cell_height) = cell_size(&caps);
    let area = Area::of(&caps, usize::from(caps.width));
    let fonts = Fonts::fixture();
    let page = lay_out(
        &[block],
        &look,
        &fonts,
        (&Plain, &Plain),
        Frame {
            width: area.text,
            area: f32::from(area.area) * f32::from(cell_width),
            row: f32::from(cell_height),
            wide: Wide::Wrap,
            front_matter: FrontMatterShown::Table,
        },
    );
    let mut facts = Vec::new();
    match block {
        Block::Heading { level, content } => {
            let rule = page
                .items
                .iter()
                .any(|item| matches!(item.shape, Shape::Rect { .. }));
            let at = format!("level {level}");
            // The cell layout has no text to tell an empty heading's size by.
            if let Some(text) = page.paragraphs().first().filter(|_| !content.is_empty()) {
                let size = format!("{:.2}", text.look.size / look.size);
                facts.push(Fact::new(Field::HeadingSize, &at, size));
            }
            facts.push(Fact::new(Field::HeadingRule, at, rule.to_string()));
        }
        Block::BlockQuote(blocks) if matches!(blocks.first(), Some(Block::Paragraph(_))) => {
            let dimmed = (page.paragraphs().first()).is_some_and(|p| p.look.color != look.text);
            let value = if dimmed { "dimmed" } else { "own" };
            facts.push(Fact::new(Field::QuoteText, "first paragraph", value));
        }
        Block::Table(_) => {
            let rows = if page.look().stripe.is_some() {
                "stripes"
            } else {
                "plain"
            };
            let frame = match page.look().table_style {
                TableStyle::Frame => "frame",
                TableStyle::Grid => "grid",
            };
            let (rows_at, frame_at) = (
                format!("{:?}", setup.table_rows),
                format!("{:?}", setup.table_style),
            );
            facts.push(Fact::new(Field::TableRows, rows_at, rows));
            facts.push(Fact::new(Field::TableFrame, frame_at, frame));
        }
        _ => {}
    }
    facts.extend(markers(block, &page, &look));
    for (kind, content) in parity::apart(block) {
        // Set apart if it makes the text's runs other than they are without.
        let ink = Ink {
            pictures: &Plain,
            room: area.text,
        };
        let apart = runs(&[kind.wrap(content.clone())], &look, ink) != runs(&content, &look, ink);
        let value = if apart { "styled" } else { "unstyled" };
        facts.push(Fact::new(Field::InlineStyle, kind.name(), value));
    }
    facts
}

/// The markers and task boxes of the list items in `block`, read off its
/// `page` from the top down: a text in the marker colour that reads as
/// one, and a task's box.
fn markers(block: &Block, page: &Page, look: &Look) -> Vec<Fact> {
    let slots = parity::slots(block);
    if slots.is_empty() {
        return Vec::new();
    }
    let mut read: Vec<(f32, (String, Option<String>))> = Vec::new();
    for item in &page.items {
        match &item.shape {
            Shape::Text { paragraph, y, .. }
                if paragraph.look.color == look.marker && parity::is_marker(paragraph.text()) =>
            {
                read.push((*y, (parity::marker_value(paragraph.text()), None)));
            }
            Shape::Task { y, checked, .. } => {
                let task_box = if *checked { "☑" } else { "☐" };
                read.push((*y, ("none".to_owned(), Some(task_box.to_owned()))));
            }
            _ => {}
        }
    }
    read.sort_by(|a, b| a.0.total_cmp(&b.0));
    let read: Vec<_> = read.into_iter().map(|(_, shown)| shown).collect();
    parity::marker_facts(&slots, &read)
}

impl Page {
    /// The texts on the page, from the top down.
    fn paragraphs(&self) -> Vec<&crate::typeset::reading::text::Paragraph> {
        let mut texts: Vec<_> = (self.items.iter())
            .filter_map(|item| match &item.shape {
                Shape::Text { paragraph, y, .. } => Some((*y, &**paragraph)),
                _ => None,
            })
            .collect();
        texts.sort_by(|a, b| a.0.total_cmp(&b.0));
        texts.into_iter().map(|(_, paragraph)| paragraph).collect()
    }
}
