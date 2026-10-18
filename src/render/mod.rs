//! Document model to terminal output.

pub mod highlight;
pub mod image;
pub mod layout;
pub mod output;
pub mod style;
pub mod table;
pub mod wrap;

use std::io::{self, Write};
use std::rc::Rc;

use crate::images::{Images, Source};
use crate::model::Document;
use crate::render::style::Line;
use crate::term::Capabilities;
use crate::theme::Theme;

/// Everything rendering takes besides the document. A new input gets a
/// default in `new`, so callers that do not need it stay as they are.
#[derive(Debug)]
pub struct Inputs {
    /// The images loaded for the document; images that are not here, or
    /// that the terminal cannot show, appear as their alt text.
    pub images: Images,
    /// What the terminal can show, and how wide it is.
    pub caps: Capabilities,
    /// Colours, sizes and margins.
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

/// Shows `image` on its own, as large as fits the window and no larger
/// than its natural size: a raster image one pixel per terminal pixel, an
/// SVG in CSS pixels. A row is left below it for the shell's prompt. Does
/// nothing when `caps` cannot draw images.
pub fn render_image(image: Source, caps: &Capabilities, out: &mut impl Write) -> io::Result<()> {
    if !caps.draws_images() {
        return Ok(());
    }
    let unit = match &image {
        Source::Raster(_) => image::Unit::Pixel,
        Source::Svg(_) => image::Unit::Css,
    };
    let max_cells = caps.max_image_cells;
    let max_rows = caps.height.map_or(max_cells, |rows| {
        rows.saturating_sub(1).max(1).min(max_cells)
    });
    let Some(size) = image::size_in(
        image.size(),
        unit,
        image::cell_size(caps),
        caps.width.min(max_cells),
        max_rows,
    ) else {
        return Ok(());
    };
    let placement = Rc::new(image::Placement {
        url: String::new(),
        size,
    });
    let lines: Vec<Line> = (0..size.rows)
        .map(|row| Line::image(Rc::clone(&placement), row))
        .collect();
    let mut images = Images::new();
    images.insert(String::new(), Ok(image));
    output::write(&lines, &images, caps, output::Page::default(), out)
}
