//! Document model to terminal output.

pub mod block;
pub mod encoder;
pub mod highlight;
pub mod image;
pub mod layout;
pub mod loading;
pub mod output;
pub mod script;
pub mod style;
pub mod table;
pub mod wrap;

use std::io::{self, Write};

use crate::caps::Capabilities;
use crate::images::{ImageKey, Images, Source};
use crate::model::{Block, Document, File, FrontMatter, FrontMatterError};
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
    /// How front matter shows.
    pub front_matter: FrontMatterStyle,
    /// How the rows of a table are told apart.
    pub table_rows: TableRows,
    /// Whether a diagram that could not be drawn tells why in the
    /// document.
    pub render_errors: RenderErrors,
}

/// Where kiri tells why something it draws from source, such as a
/// diagram, could not be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderErrors {
    /// In the document, below the source.
    Inline,
    /// In a warning on standard error.
    Stderr,
    /// In both.
    #[default]
    Both,
}

impl RenderErrors {
    /// Whether the document tells.
    pub fn inline(self) -> bool {
        matches!(self, Self::Inline | Self::Both)
    }

    /// Whether a warning on standard error tells.
    pub fn stderr(self) -> bool {
        matches!(self, Self::Stderr | Self::Both)
    }
}

/// How the rows of a table drawn as a box are told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableRows {
    /// Not at all.
    Off,
    /// Every second row on a background a little towards the text colour;
    /// nothing where that colour cannot be known or shown.
    #[default]
    Zebra,
    /// A rule between the rows.
    Lines,
    /// The lines after the first of a wrapped value start two cells in, so
    /// that a new row is one that starts at the edge of its cell.
    Indent,
    /// The last line of each row but the last is underlined, dashed, in
    /// the border colour; nothing where the terminal has no underline
    /// colours, as each cell's underline would take its own text colour.
    Underline,
}

/// How front matter shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrontMatterStyle {
    /// As GitHub shows it: a table of keys and values, with tables in it
    /// for nested values.
    #[default]
    Table,
    /// As GitLab shows it: a code block in its language.
    Code,
    /// Not at all.
    Hide,
}

/// Why `document`'s front matter, shown as `style`, could not be read: the
/// parser's error when its YAML, TOML or JSON does not parse and it is to
/// be a table.
pub fn front_matter_error(
    document: &Document,
    style: FrontMatterStyle,
) -> Option<&FrontMatterError> {
    match document.blocks.first() {
        Some(Block::FrontMatter(FrontMatter {
            value: Some(Err(error)),
            ..
        })) if style == FrontMatterStyle::Table => Some(error),
        _ => None,
    }
}

/// The warning for `document`'s front matter shown as `style`: the
/// parser's message when its YAML, TOML or JSON does not parse and it is
/// to be a table.
pub fn front_matter_warning(document: &Document, style: FrontMatterStyle) -> Option<String> {
    front_matter_error(document, style).map(ToString::to_string)
}

impl Inputs {
    /// For a terminal with `caps`: the built-in theme and no images.
    pub fn new(caps: Capabilities) -> Self {
        Self {
            images: Images::new(),
            caps,
            theme: Theme::default(),
            front_matter: FrontMatterStyle::default(),
            table_rows: TableRows::default(),
            render_errors: RenderErrors::default(),
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

/// Renders `files` with `inputs`, one after another, each under a header
/// that names it.
pub fn render_files(files: &[File], inputs: &Inputs, out: &mut impl Write) -> io::Result<()> {
    let stream = layout::files(files, inputs);
    let page = output::Page {
        background: inputs.theme.background,
        foreground: inputs.theme.foreground,
    };
    output::write(&stream.lines, &inputs.images, &inputs.caps, page, out)
}

/// Renders `file`, numbered `number` among the files shown together, under
/// its header, as `render_files` renders each of them, after the blank
/// line that comes between two files when `apart`.
pub fn render_file(
    file: &File,
    number: usize,
    apart: bool,
    inputs: &Inputs,
    out: &mut impl Write,
) -> io::Result<()> {
    let mut lines = Vec::new();
    if apart {
        lines.push(style::Line::blank());
    }
    lines.extend(layout::file(file, number, inputs));
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
    render_image_on(image, caps, output::Page::default(), out)
}

/// `render_image` on `page`, whose colours fill the rows beside the image,
/// as they do beside a document's lines.
pub fn render_image_on(
    image: Source,
    caps: &Capabilities,
    page: output::Page,
    out: &mut impl Write,
) -> io::Result<()> {
    if !caps.draws_images() {
        return Ok(());
    }
    let Some(size) = image::on_its_own(&image, caps, 1) else {
        return Ok(());
    };
    let key = ImageKey::from("");
    let lines = layout::image_rows(image::Placement {
        key: key.clone(),
        size,
        shows: image::Shows::Image,
    });
    let mut images = Images::new();
    images.insert(key, Ok(image));
    output::write(&lines, &images, caps, page, out)
}
