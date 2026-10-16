//! Colours and sizes the renderer uses.
//!
//! Colours default to the terminal's own 16-colour palette, so kiri follows
//! the user's kitty theme instead of imposing one.

use crate::render::style::Color;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Maximum text width in cells; long lines are hard to read. 0 = none.
    pub max_width: u16,
    /// Styles for heading levels 1 to 6.
    pub headings: [HeadingTheme; 6],
    pub link: Option<Color>,
    pub inline_code: Option<Color>,
    pub quote_bar: Option<Color>,
    pub rule: Option<Color>,
    pub list_marker: Option<Color>,
    pub table_border: Option<Color>,
    /// Code block colour themes by background; see `highlight::theme_names`.
    pub code_theme_dark: String,
    pub code_theme_light: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingTheme {
    /// Size as a multiple of the normal font, with kitty's text sizing.
    pub scale: u8,
    pub color: Option<Color>,
    pub bold: bool,
    /// Draw a rule under the heading, as GitHub does for levels 1 and 2.
    pub rule: bool,
}

impl HeadingTheme {
    fn new(scale: u8, color: Option<Color>, rule: bool) -> Self {
        Self {
            scale,
            color,
            bold: true,
            rule,
        }
    }
}

const BLUE: Option<Color> = Some(Color::Palette(4));
const MAGENTA: Option<Color> = Some(Color::Palette(5));
const CYAN: Option<Color> = Some(Color::Palette(6));
const GREY: Option<Color> = Some(Color::Palette(8));

impl Default for Theme {
    fn default() -> Self {
        Self {
            max_width: 100,
            headings: [
                HeadingTheme::new(3, BLUE, true),
                HeadingTheme::new(2, BLUE, true),
                HeadingTheme::new(1, CYAN, false),
                HeadingTheme::new(1, None, false),
                HeadingTheme::new(1, None, false),
                HeadingTheme::new(1, GREY, false),
            ],
            link: BLUE,
            inline_code: MAGENTA,
            quote_bar: GREY,
            rule: GREY,
            list_marker: GREY,
            table_border: GREY,
            code_theme_dark: "base16-ocean.dark".into(),
            code_theme_light: "InspiredGitHub".into(),
        }
    }
}

impl Theme {
    pub fn heading(&self, level: u8) -> &HeadingTheme {
        let index = usize::from(level.clamp(1, 6)) - 1;
        &self.headings[index]
    }
}
