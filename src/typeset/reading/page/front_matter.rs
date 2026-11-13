//! Front matter: its keys and values as a grid, with the source in a code block
//! when it is shown as source.

use crate::typeset::grid::{Leaves, front_matter_grid};
use crate::typeset::model::{Block, FrontMatter, Inline};
use crate::typeset::reading::page::{Flow, FrontMatterShown, Leaf};
use crate::typeset::text::{self, FrontMatterTexts, Part};

impl Flow<'_> {
    /// Lays front matter out at `x` in `width` pixels: a table of its keys
    /// and values when it has keys, otherwise, or when it is asked for as
    /// code, a code block; an empty one is nothing. It does not use the
    /// area: its cells wrap and it never scrolls.
    pub(super) fn front_matter(
        &mut self,
        block: &Block,
        front_matter: &FrontMatter,
        x: f32,
        width: f32,
    ) {
        let texts = text::front_matter_texts(front_matter);
        if texts == FrontMatterTexts::None {
            return;
        }
        // A blank line above it, so that it does not touch the page's top.
        self.y += self.look.spacing.line_height * self.look.size;
        match (texts, self.front_matter) {
            (FrontMatterTexts::Leaves(entries), FrontMatterShown::Table) => {
                self.nested(&front_matter_grid(entries, &Inlines), x, width, true);
            }
            _ => {
                // The source is the text of front matter that has no keys
                // and values; shown in place of them, it shows none.
                self.text_is(text::number(block, Part::Source));
                self.code(Some(&front_matter.language), &front_matter.source, x, width);
            }
        }
    }
}

/// Reading mode's leaves in front matter's grid: the inlines of a cell, and
/// its number among the texts of the block, which the grid gave it.
struct Inlines;

impl Inlines {
    fn leaf(number: usize, content: Vec<Inline>) -> Leaf {
        Leaf {
            text: Some(number),
            content,
        }
    }
}

impl Leaves<Leaf> for Inlines {
    fn key(&self, number: usize, key: &str) -> Leaf {
        Self::leaf(number, vec![Inline::Text(key.to_owned())])
    }

    fn text(&self, number: usize, text: &str) -> Leaf {
        Self::leaf(number, vec![Inline::Text(text.to_owned())])
    }

    fn empty(&self, number: usize) -> Leaf {
        Self::leaf(number, Vec::new())
    }

    fn yaml(&self, number: usize, yaml: &str) -> Leaf {
        let mut lines = Vec::new();
        for (index, line) in yaml.lines().enumerate() {
            if index > 0 {
                lines.push(Inline::LineBreak);
            }
            lines.push(Inline::Code(line.to_owned()));
        }
        Self::leaf(number, lines)
    }
}
