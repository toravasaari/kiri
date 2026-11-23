//! `Inputs`: the options that a layout takes besides the document, such as the
//! images, the capabilities and the theme. It also takes the choices of how
//! front matter, top-level headings, table rows and diagram errors show, the
//! looks of headings drawn as images, and the front matter error that a layout
//! shows in place of the table.
//!
//! Nothing here lays a document out or writes it; the layout reads these
//! options.
//!
//! The choices are `FrontMatterStyle`, `Headings`, `TableRows` and
//! `RenderErrors`. The helpers are `heading_look`, `heading_images`, which says
//! whether headings take images given the capabilities, and
//! `front_matter_error`.

use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
use crate::typeset::images::size;
use crate::typeset::images::{CodeLook, HeadingLook, Images, LevelLook};
use crate::typeset::model::{Block, Document, FrontMatter, FrontMatterError};
use crate::typeset::reading::engine;
use crate::typeset::theme::{Look, Theme};

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
    /// How top-level headings show.
    pub headings: Headings,
    /// How the rows of a table are told apart.
    pub table_rows: TableRows,
    /// Whether a diagram that could not be drawn tells why in the
    /// document.
    pub render_errors: RenderErrors,
    /// Reading mode, where the terminal draws images: the document's text
    /// in a proportional font. `None` for the text layout.
    pub reading: Option<engine::Reading>,
    /// Whether a diagram for the other page lies on a panel.
    pub diagram_panel: bool,
    /// How the cell layout lays out a long table: in stretches, in the
    /// pager. `None` lays every table out whole and caps a large one.
    pub tables: Option<crate::typeset::stretch::Tables>,
    /// The fonts the loader drew the images' text with, named in their keys.
    pub fonts: Option<std::sync::Arc<crate::typeset::fonts::Fonts>>,
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

/// How top-level headings show; headings inside quotes, lists and
/// alerts are always one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Headings {
    /// Large with text sizing where the terminal has it, else as images
    /// where images are drawn, else one row.
    #[default]
    Auto,
    /// Large with text sizing where the terminal has it, else one row.
    Sized,
    /// As images where images are drawn, else as `Auto`.
    Images,
    /// One row, bold and coloured, even with text sizing.
    Plain,
}

/// How the top-level headings are drawn as images, for `headings` and a
/// terminal with `caps`, in `theme`'s sizes and colours; `None` when they
/// are not, as with `no_images`.
pub fn heading_look(
    headings: Headings,
    no_images: bool,
    theme: &Theme,
    caps: &Capabilities,
) -> Option<HeadingLook> {
    if no_images || !heading_images(headings, caps) {
        return None;
    }
    let (look, cell) = (Look::new(theme, caps), size::cell_size(caps));
    // Light text on a dark page, as most terminals have it.
    let contrast = match caps.dark_background {
        Some(true) | None => [0xff; 3],
        Some(false) => [0; 3],
    };
    let text = rgb(look.foreground.filter(|_| caps.color), caps)
        .or_else(|| rgb(caps.foreground, caps))
        .unwrap_or(contrast);
    let level = |level: u8| {
        let heading = look.heading(level);
        let color = if caps.color { heading.color } else { None };
        let color = rgb(color, caps).unwrap_or(text);
        LevelLook {
            rows: heading.size.scale(),
            font_px: heading.image_font_px(cell.1),
            bold: heading.bold,
            italic: heading.italic,
            color,
            code: code_look(&look, caps, heading.background, color),
        }
    };
    Some(HeadingLook {
        cell,
        levels: [1, 2, 3, 4, 5, 6].map(level),
    })
}

/// `color` as RGB on a terminal with `caps`, a palette colour as the
/// terminal tells it, or else as xterm's default palette has it.
fn rgb(color: Option<Color>, caps: &Capabilities) -> Option<[u8; 3]> {
    match color?.rgb_or_xterm(&caps.palette) {
        Color::Rgb(red, green, blue) => Some([red, green, blue]),
        Color::Palette(_) => None,
    }
}

/// How inline code in a heading in `background` and in `color` is drawn
/// in its image, as the text layout draws it: in the theme's colour for
/// inline code on its background, padded by a space on each side where it
/// has one; in the heading's colour on the heading's background without
/// colours.
fn code_look(
    look: &Look,
    caps: &Capabilities,
    background: Option<Color>,
    color: [u8; 3],
) -> CodeLook {
    if !(caps.styling && caps.color) {
        return CodeLook {
            color,
            background: None,
            padding: false,
        };
    }
    let code_background = look.inline_code.background;
    CodeLook {
        color: rgb(look.inline_code.color, caps).unwrap_or(color),
        background: rgb(code_background, caps),
        padding: look.inline_code.padding && code_background.or(background).is_some(),
    }
}

impl Headings {
    /// How headings show with images off when `images` is false: as
    /// `auto`, `sized` or `images` say without images, which is as
    /// `sized` says; `plain` stays.
    pub fn with_images(self, images: bool) -> Self {
        match self {
            Self::Auto | Self::Images if !images => Self::Sized,
            headings => headings,
        }
    }
}

/// Whether the top-level headings show as images, where theirs loaded,
/// for `headings` on a terminal with `caps`.
pub(crate) fn heading_images(headings: Headings, caps: &Capabilities) -> bool {
    let wanted = match headings {
        Headings::Auto => !caps.text_sizing,
        Headings::Images => true,
        Headings::Sized | Headings::Plain => false,
    };
    wanted && caps.draws_images()
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

impl Inputs {
    /// For a terminal with `caps`: the built-in theme and no images.
    pub fn new(caps: Capabilities) -> Self {
        Self {
            images: Images::new(),
            caps,
            theme: Theme::default(),
            front_matter: FrontMatterStyle::default(),
            headings: Headings::default(),
            table_rows: TableRows::default(),
            render_errors: RenderErrors::default(),
            reading: None,
            diagram_panel: true,
            tables: None,
            fonts: None,
        }
    }
}

#[cfg(test)]
mod tests;
