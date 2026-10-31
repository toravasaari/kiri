//! How the images of a document are loaded, and its diagrams drawn, and the
//! documents shown together whose images are loaded with each other's.

use std::path::Path;

use crate::typeset::diagrams::renderer::{NoRenderer, Renderer};
use crate::typeset::diagrams::request::{FormulaColors, Theme};
use crate::typeset::images::heading::HeadingLook;
use crate::typeset::model::Document;

/// How the images of a document are loaded, and its diagrams drawn.
#[derive(Clone, Copy)]
pub struct LoadOptions<'a> {
    /// What draws the diagrams; by default nothing, so that every code
    /// block stays code.
    pub renderer: &'a dyn Renderer,
    /// The page the diagrams are drawn for, unless they choose their own
    /// colours; by default a light one.
    pub theme: Theme,
    /// The colours formulas are drawn in; by default none, which leaves
    /// them to the page.
    pub formula: FormulaColors,
    /// Whether a warning names a diagram by its line in the document, or
    /// by its own first line, for a document that has no lines the reader
    /// can look up, such as one piped in; by default by its line.
    pub by_line: bool,
    /// Told how many of a document's diagrams are drawn, and of how many:
    /// first none, and then each time one more is.
    pub progress: Option<&'a (dyn Fn(usize, usize) + Sync)>,
    /// Whether a diagram whose colours are for the other page, light or
    /// dark, lies on a panel of its own background; by default it does.
    pub panel: bool,
    /// The most pixels, wide and high, an image can be shown at: a
    /// diagram drawn in pixels is reduced to fit them as it loads. `None`,
    /// the default, keeps it as drawn.
    pub largest: Option<(u32, u32)>,
    /// How headings are drawn as images; `None`, the default, draws none.
    pub headings: Option<HeadingLook>,
}

impl Default for LoadOptions<'_> {
    fn default() -> Self {
        LoadOptions {
            renderer: &NoRenderer,
            theme: Theme::Light,
            formula: FormulaColors::default(),
            by_line: true,
            progress: None,
            panel: true,
            largest: None,
            headings: None,
        }
    }
}

/// One of several documents shown together, whose images are loaded with
/// the others'.
#[derive(Clone, Copy)]
pub struct Part<'a> {
    pub document: &'a Document,
    /// Its number among the documents shown together, which the keys of
    /// its images carry.
    pub number: usize,
    /// The directory that its relative image paths start from.
    pub base: &'a Path,
    /// Whether a warning names a diagram by its line in the document, or
    /// by its own first line, as `LoadOptions::by_line` says.
    pub by_line: bool,
    /// Whether the images in the middle of text are wanted too, as reading
    /// mode draws them; otherwise only an image that is a block of its
    /// own is, as the others show as their alt text.
    pub inline_images: bool,
}
