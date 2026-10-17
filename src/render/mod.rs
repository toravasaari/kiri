//! Document model to terminal output.

pub mod highlight;
pub mod image;
pub mod layout;
pub mod output;
pub mod style;
pub mod table;
pub mod wrap;

use std::io::{self, Write};

use crate::images::Images;
use crate::model::Document;
use crate::term::Capabilities;
use crate::theme::Theme;

/// Everything rendering takes besides the document. A new input gets a
/// default in `new`, so callers that do not need it stay as they are.
#[derive(Debug)]
pub struct Inputs {
    /// The images loaded for the document; images that are not here, or
    /// that the terminal cannot show, appear as their alt text.
    pub images: Images,
    pub caps: Capabilities,
    pub theme: Theme,
}

impl Inputs {
    /// For a terminal with `caps`: the built-in theme and no images.
    pub fn new(caps: Capabilities) -> Self {
        Self {
            images: Images::new(),
            caps,
            theme: Theme::default(),
        }
    }
}

/// Renders `document` with `inputs`.
pub fn render(document: &Document, inputs: &Inputs, out: &mut impl Write) -> io::Result<()> {
    let lines = layout::layout(document, inputs);
    let page = output::Page {
        background: inputs.theme.background,
        foreground: inputs.theme.foreground,
    };
    output::write(&lines, &inputs.images, &inputs.caps, page, out)
}
