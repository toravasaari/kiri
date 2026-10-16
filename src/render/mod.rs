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

/// Renders `document` with the `images` loaded for it; images that are not
/// there, or that the terminal cannot show, appear as their alt text.
pub fn render(
    document: &Document,
    images: &Images,
    caps: &Capabilities,
    theme: &Theme,
    out: &mut impl Write,
) -> io::Result<()> {
    let lines = layout::layout(document, images, caps, theme);
    output::write(&lines, images, caps, theme.background, out)
}
