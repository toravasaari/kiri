//! Front matter: its keys and values as a grid, with the source in a code block
//! when it is shown as source.

use crate::typeset::grid::{Leaves, front_matter_grid};
use crate::typeset::model::{FrontMatter, Inline, Value};
use crate::typeset::reading::page::{Flow, FrontMatterShown};

impl Flow<'_> {
    /// Lays front matter out at `x` in `width` pixels: a table of its keys
    /// and values when it has keys, otherwise, or when it is asked for as
    /// code, a code block; an empty one is nothing. It does not use the
    /// area: its cells wrap and it never scrolls.
    pub(super) fn front_matter(&mut self, front_matter: &FrontMatter, x: f32, width: f32) {
        if front_matter.source.trim().is_empty() {
            return;
        }
        // A blank line above it, so that it does not touch the page's top.
        self.y += self.look.spacing.line_height * self.look.size;
        match (&front_matter.value, self.front_matter) {
            (Some(Ok(Value::Map(entries))), FrontMatterShown::Table) => {
                self.nested(&front_matter_grid(entries, &Inlines), x, width, true);
            }
            _ => self.code(Some(&front_matter.language), &front_matter.source, x, width),
        }
    }
}

/// Reading mode's leaves in front matter's grid: the inlines of a cell.
struct Inlines;

impl Leaves<Vec<Inline>> for Inlines {
    fn key(&self, key: &str) -> Vec<Inline> {
        vec![Inline::Text(key.to_owned())]
    }

    fn text(&self, text: &str) -> Vec<Inline> {
        vec![Inline::Text(text.to_owned())]
    }

    fn empty(&self) -> Vec<Inline> {
        Vec::new()
    }

    fn yaml(&self, yaml: &str) -> Vec<Inline> {
        let mut lines = Vec::new();
        for (number, line) in yaml.lines().enumerate() {
            if number > 0 {
                lines.push(Inline::LineBreak);
            }
            lines.push(Inline::Code(line.to_owned()));
        }
        lines
    }
}
