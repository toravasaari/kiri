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
use crate::color::Color;
use crate::images::{HeadingLook, ImageKey, Images, LevelLook, Source};
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
    /// How top-level headings show.
    pub headings: Headings,
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
    let cell = image::cell_size(caps);
    let rgb = |color: Option<Color>| match color?.rgb(&caps.palette)? {
        Color::Rgb(red, green, blue) => Some([red, green, blue]),
        Color::Palette(_) => None,
    };
    // Light text on a dark page, as most terminals have it.
    let contrast = match caps.dark_background {
        Some(true) | None => [0xff; 3],
        Some(false) => [0; 3],
    };
    let text = rgb(theme.foreground)
        .or_else(|| rgb(caps.foreground))
        .unwrap_or(contrast);
    let level = |level: u8| {
        let heading = theme.heading(level);
        let color = if caps.color { heading.color } else { None };
        LevelLook {
            rows: heading.size.scale(),
            font_px: (heading.size.value() * f64::from(cell.1) / 1.2) as f32,
            bold: heading.bold,
            italic: heading.italic,
            color: rgb(color).unwrap_or(text),
        }
    };
    Some(HeadingLook {
        cell,
        levels: [1, 2, 3, 4, 5, 6].map(level),
    })
}

/// Whether the top-level headings show as images, where theirs loaded,
/// for `headings` on a terminal with `caps`.
fn heading_images(headings: Headings, caps: &Capabilities) -> bool {
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
            headings: Headings::default(),
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
        text: None,
    });
    let mut images = Images::new();
    images.insert(key, Ok(image));
    output::write(&lines, &images, caps, page, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kitty without text sizing, as inside tmux, with cells of 8 × 16
    /// pixels.
    fn no_text_sizing() -> Capabilities {
        Capabilities {
            text_sizing: false,
            cell_pixels: Some((8, 16)),
            ..Capabilities::kitty(80)
        }
    }

    #[test]
    fn auto_uses_images_only_without_text_sizing() {
        let theme = Theme::default();
        let look = heading_look(Headings::Auto, false, &theme, &no_text_sizing());
        let look = look.expect("images without text sizing");
        assert_eq!(look.cell, (8, 16));
        let h1 = theme.heading(1);
        assert_eq!(look.levels[0].rows, h1.size.scale());
        let px = h1.size.value() as f32 * 16.0 / 1.2;
        assert!((look.levels[0].font_px - px).abs() < 0.01, "{look:?}");
        assert_eq!(look.levels[0].bold, h1.bold);
        let kitty = Capabilities::kitty(80);
        assert_eq!(heading_look(Headings::Auto, false, &theme, &kitty), None);
        let images = heading_look(Headings::Images, false, &theme, &kitty);
        assert!(images.is_some());
    }

    #[test]
    fn plain_and_sized_never_use_images() {
        let theme = Theme::default();
        for headings in [Headings::Plain, Headings::Sized] {
            let look = heading_look(headings, false, &theme, &no_text_sizing());
            assert_eq!(look, None, "{headings:?}");
        }
    }

    #[test]
    fn no_images_means_no_heading_images() {
        let theme = Theme::default();
        for headings in [Headings::Auto, Headings::Images] {
            let look = heading_look(headings, true, &theme, &no_text_sizing());
            assert_eq!(look, None, "{headings:?}");
        }
    }

    #[test]
    fn images_need_a_terminal_that_draws_them() {
        let theme = Theme::default();
        let ansi = Capabilities::ansi(80);
        assert_eq!(heading_look(Headings::Images, false, &theme, &ansi), None);
        assert_eq!(heading_look(Headings::Auto, false, &theme, &ansi), None);
    }

    #[test]
    fn heading_images_take_the_heading_colour_or_else_the_text_colour() {
        let mut theme = Theme::default();
        let caps = no_text_sizing();
        theme.headings[0].color = Some(Color::Rgb(9, 8, 7));
        let look = heading_look(Headings::Auto, false, &theme, &caps).unwrap();
        assert_eq!(look.levels[0].color, [9, 8, 7]);
        // A colour of the palette, as the terminal tells it.
        theme.headings[0].color = Some(Color::Palette(4));
        let mut palette = caps.palette;
        palette[4] = Some(Color::Rgb(0, 0, 0xee));
        let caps = Capabilities { palette, ..caps };
        let look = heading_look(Headings::Auto, false, &theme, &caps).unwrap();
        assert_eq!(look.levels[0].color, [0, 0, 0xee]);
        // Without a colour of its own, in the text's: the theme's, or else
        // the terminal's, or else white on a dark page.
        theme
            .headings
            .iter_mut()
            .for_each(|heading| heading.color = None);
        theme.foreground = Some(Color::Rgb(1, 2, 3));
        let look = heading_look(Headings::Auto, false, &theme, &caps).unwrap();
        assert_eq!(look.levels[0].color, [1, 2, 3]);
        theme.foreground = None;
        let caps = Capabilities {
            foreground: None,
            dark_background: Some(true),
            ..caps
        };
        let look = heading_look(Headings::Auto, false, &theme, &caps).unwrap();
        assert_eq!(look.levels[0].color, [0xff, 0xff, 0xff]);
    }
}
