//! Document model to terminal output.

pub mod layout;
pub mod output;
pub mod style;
pub mod wrap;

use std::io::{self, Write};

use crate::model::Document;
use crate::term::Capabilities;
use crate::theme::Theme;

pub fn render(
    document: &Document,
    caps: &Capabilities,
    theme: &Theme,
    out: &mut impl Write,
) -> io::Result<()> {
    let lines = layout::layout(document, caps, theme);
    output::write(&lines, caps, out)
}
