//! The one look: what the theme and the terminal's capabilities decide
//! about how a document looks, resolved once, for both layouts to read.
//!
//! The theme is the file format: what a user may write. The look is what
//! the layouts need from it: colours with the terminal's fallbacks in,
//! heading styles, and the spacing of each medium. The text layout counts
//! in cells, so its table is `CellTable`; reading mode counts in pixels, as
//! GitHub does, so its table is `PixelTable`, with GitHub's values.
//!
//! A layout reads a `Look`, never the `Theme`, so that a decision about
//! how something looks is taken here and nowhere else.

use crate::typeset::caps::Capabilities;
use crate::typeset::color::{Color, Underline};
use crate::typeset::size::TextSize;
use crate::typeset::theme::{
    AlertTheme, FileHeaderTheme, LabelColor, MarkTheme, SearchTheme, TableStyle, Theme,
};

/// A resolved look for one theme on one terminal.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// The widest the text is in cells; 0 for no limit.
    pub max_width: u16,
    pub background: Option<Color>,
    pub foreground: Option<Color>,
    pub muted: Option<Color>,
    pub error: Option<Color>,
    pub link: Option<Color>,
    pub inline_code: InlineCode,
    pub mark: MarkTheme,
    /// The colours of a search's matches.
    pub search: SearchTheme,
    /// Headings of levels 1 to 6.
    pub headings: [HeadingLook; 6],
    pub quote_bar: Option<Color>,
    pub rule: Option<Color>,
    pub list_marker: Option<Color>,
    pub table_border: Option<Color>,
    pub table_style: TableStyle,
    pub code: CodeLook,
    pub alerts: AlertTheme,
    pub file_header: FileHeaderTheme,
    pub cells: CellTable,
    pub pixels: PixelTable,
}

/// How inline code looks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineCode {
    pub color: Option<Color>,
    /// Behind it, as the theme and the page make it; `None` for none.
    pub background: Option<Color>,
    /// A space of background on each side.
    pub padding: bool,
    /// It may break at its spaces.
    pub wrap: bool,
}

/// How a code block looks, apart from its colours, which the highlighter
/// gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeLook {
    pub padding: bool,
    pub show_language_label: bool,
    pub language_label: LabelColor,
    /// The colour themes of the highlighter, by the page's lightness.
    pub theme_dark: String,
    pub theme_light: String,
}

/// How one level of heading looks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingLook {
    /// In the text layout, the size kitty draws it at.
    pub size: TextSize,
    pub color: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: Underline,
    /// Filled across the whole width.
    pub background: Option<Color>,
    /// Indent in cells of normal size.
    pub indent: u8,
    /// Whether the text layout draws a rule under it.
    pub rule: bool,
}

impl HeadingLook {
    /// The font size in pixels of this heading drawn as an image, on a
    /// terminal whose cells are `cell_height` pixels high: the size the
    /// text layout gives it in rows, as a font size of 1.2 rows to a row.
    pub fn image_font_px(&self, cell_height: u16) -> f32 {
        (self.size.value() * f64::from(cell_height) / 1.2) as f32
    }
}

/// The spacing of the text layout, in cells, and the shares it mixes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellTable {
    /// Cells kept free on both sides of everything but headings and rules.
    pub text_margin: u8,
    /// How far code is indented in a block without a box, in cells.
    pub plain_code_indent: usize,
    /// How much of the text colour a striped table row mixes into the page.
    pub stripe: f32,
    /// How much of the text colour a file's header band mixes into a dark
    /// page, and into a light one.
    pub band_dark: f32,
    pub band_light: f32,
    pub bullets: Bullets,
}

/// The bullets of a list by its depth, outermost first; a list deeper than
/// the glyphs keeps the last, as GitHub does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bullets {
    pub glyphs: [&'static str; 3],
}

impl Bullets {
    /// The bullet of a list `depth` lists deep, the outermost being 0.
    pub fn at(&self, depth: usize) -> &'static str {
        self.glyphs[depth.min(self.glyphs.len() - 1)]
    }
}

/// The glyphs of bullets: a disc, a circle, a square.
const GLYPHS: [&str; 3] = ["•", "◦", "▪"];

/// The spacing of reading mode, in font sizes of the text unless a field
/// says pixels, which are GitHub's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelTable {
    /// The text's font size as a share of the cell height: 16 pixels in
    /// rows of 20, as a browser sets text in rows of that height.
    pub font_size: f32,
    /// How much of the text colour a shaded table row mixes into the page.
    pub stripe: f32,
    /// The size of each heading level, `h1` first, in font sizes of the
    /// text, and the line height of headings.
    pub heading_sizes: [f32; 6],
    pub heading_line_height: f32,
    /// How many heading levels have a rule below them, from the first.
    pub heading_rules: u8,
    /// How far below a large heading its rule is, in its own font sizes.
    pub rule_below: f32,
    /// The line height of text, and the weight of headings and strong text.
    pub line_height: f32,
    pub bold: f32,
    /// How far list items are indented and how far apart they are, and how
    /// far a marker ends before its item's text.
    pub list_indent: f32,
    pub item_gap: f32,
    pub marker_gap: f32,
    pub bullets: Bullets,
    /// How far a quote's text is indented, and how wide its bar is.
    pub quote_indent: f32,
    pub quote_bar: f32,
    /// An alert's room above its title and below its text, the gap between
    /// its title and text, and the one between its icon and its title.
    pub alert_padding: f32,
    pub alert_title_gap: f32,
    pub icon_gap: f32,
    /// A code block's padding and the radius of its corners; its line
    /// height, and its label's size.
    pub code_padding: f32,
    pub code_radius: f32,
    pub code_line_height: f32,
    pub label_size: f32,
    /// How far a table cell's text is from its borders, across and down.
    pub cell_across: f32,
    pub cell_down: f32,
}

/// GitHub's spacing, which reading mode follows.
pub const GITHUB: PixelTable = PixelTable {
    font_size: 0.8,
    stripe: 0.06,
    heading_sizes: [2.0, 1.5, 1.25, 1.0, 0.875, 0.85],
    heading_line_height: 1.25,
    heading_rules: 2,
    rule_below: 0.3,
    line_height: 1.5,
    bold: 600.0,
    list_indent: 2.0,
    item_gap: 0.5,
    marker_gap: 0.5,
    bullets: Bullets { glyphs: GLYPHS },
    quote_indent: 1.0,
    quote_bar: 0.25,
    alert_padding: 0.5,
    alert_title_gap: 0.25,
    icon_gap: 0.5,
    code_padding: 1.0,
    code_radius: 0.375,
    code_line_height: 1.45,
    label_size: 0.75,
    cell_across: 13.0 / 16.0,
    cell_down: 6.0 / 16.0,
};

impl Look {
    /// The look of `theme` on a terminal with `caps`: the theme as the
    /// layouts are to read it, so after the adjustments for a terminal
    /// without colours or with a palette that hides the greys.
    pub fn new(theme: &Theme, caps: &Capabilities) -> Self {
        let heading = |level: u8| {
            let heading = theme.heading(level);
            HeadingLook {
                size: heading.size,
                color: heading.color,
                bold: heading.bold,
                italic: heading.italic,
                underline: heading.underline,
                background: heading.background,
                indent: heading.indent,
                rule: heading.rule,
            }
        };
        Self {
            max_width: theme.max_width,
            background: theme.background,
            foreground: theme.foreground,
            muted: theme.muted,
            error: theme.error,
            link: theme.link,
            inline_code: InlineCode {
                color: theme.inline_code,
                background: crate::typeset::layout::inline_code_background(theme, caps),
                padding: theme.inline_code_padding,
                wrap: theme.inline_code_wrap,
            },
            mark: theme.mark,
            search: theme.search,
            headings: [1, 2, 3, 4, 5, 6].map(heading),
            quote_bar: theme.quote_bar,
            rule: theme.rule,
            list_marker: theme.list_marker,
            table_border: theme.table_border,
            table_style: theme.table_style,
            code: CodeLook {
                padding: theme.code_padding,
                show_language_label: theme.show_language_label,
                language_label: theme.language_label,
                theme_dark: theme.code_theme_dark.clone(),
                theme_light: theme.code_theme_light.clone(),
            },
            alerts: theme.alerts.clone(),
            file_header: theme.file_header,
            cells: CellTable {
                text_margin: theme.text_margin,
                plain_code_indent: 2,
                stripe: 0.06,
                band_dark: 0.3,
                band_light: 0.12,
                bullets: Bullets { glyphs: GLYPHS },
            },
            pixels: GITHUB,
        }
    }

    /// The look of heading `level`, 1 to 6.
    pub fn heading(&self, level: u8) -> &HeadingLook {
        &self.headings[usize::from(level.clamp(1, 6)) - 1]
    }
}
