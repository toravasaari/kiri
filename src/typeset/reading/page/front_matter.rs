//! Front matter: its keys and values as a grid, with the source in a code block
//! when it is shown as source.

use crate::typeset::grid::{Leaves, front_matter_grid};
use crate::typeset::model::{FrontMatter, Inline, Value};
use crate::typeset::reading::page::{Flow, FrontMatterShown, Leaf};

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
                let leaves = Inlines {
                    next: std::cell::Cell::new(0),
                };
                self.nested(&front_matter_grid(entries, &leaves), x, width, true);
            }
            _ => {
                // The source is the text of front matter that has no keys
                // and values; shown in place of them, it shows none.
                let keys_and_values = matches!(&front_matter.value, Some(Ok(Value::Map(_))));
                self.text_is((!keys_and_values).then_some(0));
                self.code(Some(&front_matter.language), &front_matter.source, x, width);
            }
        }
    }
}

/// Reading mode's leaves in front matter's grid: the inlines of a cell, and
/// its number among the texts of the block, which they are made in the order
/// of.
struct Inlines {
    next: std::cell::Cell<usize>,
}

impl Inlines {
    fn leaf(&self, content: Vec<Inline>) -> Leaf {
        Leaf {
            text: Some(self.next.replace(self.next.get() + 1)),
            content,
        }
    }
}

impl Leaves<Leaf> for Inlines {
    fn key(&self, key: &str) -> Leaf {
        self.leaf(vec![Inline::Text(key.to_owned())])
    }

    fn text(&self, text: &str) -> Leaf {
        self.leaf(vec![Inline::Text(text.to_owned())])
    }

    fn empty(&self) -> Leaf {
        self.leaf(Vec::new())
    }

    fn yaml(&self, yaml: &str) -> Leaf {
        let mut lines = Vec::new();
        for (number, line) in yaml.lines().enumerate() {
            if number > 0 {
                lines.push(Inline::LineBreak);
            }
            lines.push(Inline::Code(line.to_owned()));
        }
        self.leaf(lines)
    }
}
