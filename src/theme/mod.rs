//! Colours and sizes the renderer uses, and the theme file that changes them.
//!
//! Colours default to the terminal's own 16-colour palette, so kiri follows
//! the user's kitty theme instead of imposing one.

use crate::color::{Color, Underline};
use crate::model::AlertKind;
use crate::size::TextSize;

mod file;
mod paper;

pub use file::{load, parse};
pub use paper::Paper;

/// The code block colour themes bundled with the highlighter.
pub const CODE_THEMES: [&str; 7] = [
    "InspiredGitHub",
    "Solarized (dark)",
    "Solarized (light)",
    "base16-eighties.dark",
    "base16-mocha.dark",
    "base16-ocean.dark",
    "base16-ocean.light",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Maximum text width in cells; long lines are hard to read. 0 = none.
    pub max_width: u16,
    /// Painted behind the whole document; `None` shows the terminal's own.
    pub background: Option<Color>,
    /// Text that has no colour of its own; `None` uses the terminal's.
    pub foreground: Option<Color>,
    /// Text kiri plays down, such as image alt text, raw HTML and link
    /// targets; `None` dims it instead.
    pub muted: Option<Color>,
    /// Cells kept free on both sides of everything but headings and rules.
    pub text_margin: u8,
    /// Styles for heading levels 1 to 6.
    pub headings: [HeadingTheme; 6],
    pub link: Option<Color>,
    pub inline_code: Option<Color>,
    pub inline_code_background: CodeBackground,
    /// A space of background colour on each side of inline code.
    pub inline_code_padding: bool,
    /// Inline code may wrap at its spaces. Without it, code moves to the
    /// next line instead, unless it is longer than a line.
    pub inline_code_wrap: bool,
    pub quote_bar: Option<Color>,
    pub rule: Option<Color>,
    pub list_marker: Option<Color>,
    pub table_border: Option<Color>,
    /// Messages kiri writes into the document, such as why a diagram
    /// could not be drawn.
    pub error: Option<Color>,
    /// A box of background colour around code blocks, with one cell and one
    /// line of padding. Without it, code starts at the margin.
    pub code_padding: bool,
    /// Code block colour themes by background, from `CODE_THEMES`.
    pub code_theme_dark: String,
    pub code_theme_light: String,
    /// The language of a code block named in its top right corner, on the
    /// padding line above the code. It needs `code_padding` and colours.
    pub show_language_label: bool,
    pub language_label: LabelColor,
    pub alerts: AlertTheme,
    pub status_line: StatusLineTheme,
    pub search: SearchTheme,
    pub mark: MarkTheme,
    pub file_header: FileHeaderTheme,
    pub formula: FormulaTheme,
}

/// The colours of a formula drawn as an image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FormulaTheme {
    /// Its letters and lines; `None` for the colour of the text.
    pub text: Option<Color>,
    /// Behind it; `None` for no background of its own.
    pub background: Option<Color>,
}

/// The colours of the header above each of several files: a band across
/// the text with a bar down its left edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileHeaderTheme {
    pub background: BandColor,
    pub bar: Option<Color>,
}

impl Default for FileHeaderTheme {
    fn default() -> Self {
        Self {
            background: BandColor::Auto,
            bar: BLUE,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BandColor {
    /// A little of the text colour mixed into the page.
    #[default]
    Auto,
    Color(Option<Color>),
}

/// The colours of marked text, HTML's `<mark>`. Without colours, marked
/// text is in reverse video.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarkTheme {
    pub foreground: Option<Color>,
    pub background: Option<Color>,
}

/// The colours of the pager's status line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusLineTheme {
    pub background: StatusColor,
    pub foreground: StatusColor,
    pub badge_background: StatusColor,
    pub badge_foreground: StatusColor,
}

/// The colours of the search matches in the pager: every match shown, and
/// the one the reader is at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchTheme {
    pub match_foreground: SearchColor,
    pub match_background: SearchColor,
    pub current_foreground: SearchColor,
    pub current_background: SearchColor,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchColor {
    /// Reverse video when both colours of a match are auto; otherwise the
    /// text's own colour.
    #[default]
    Auto,
    Color(Option<Color>),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StatusColor {
    /// Chosen from the page and the terminal's colours.
    #[default]
    Auto,
    Color(Option<Color>),
}

/// How alert blocks, such as `> [!NOTE]`, are coloured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertTheme {
    /// Where the colours of the kinds come from.
    pub colors: AlertColors,
    /// A background mixed from the page and the kind's colour.
    pub tint: bool,
    pub note: AlertColor,
    pub tip: AlertColor,
    pub important: AlertColor,
    pub warning: AlertColor,
    pub caution: AlertColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertColors {
    /// GitHub's colours for a dark or a light background.
    Github,
    /// The terminal's palette: blue, green, magenta, yellow and red.
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertColor {
    /// The colour `AlertTheme::colors` gives the kind.
    Auto,
    Color(Option<Color>),
}

impl AlertTheme {
    /// The colour of alerts of `kind` on a dark background if `dark`.
    pub fn color(&self, kind: AlertKind, dark: bool) -> Option<Color> {
        let color = match kind {
            AlertKind::Note => self.note,
            AlertKind::Tip => self.tip,
            AlertKind::Important => self.important,
            AlertKind::Warning => self.warning,
            AlertKind::Caution => self.caution,
        };
        match (color, self.colors) {
            (AlertColor::Color(color), _) => color,
            (AlertColor::Auto, AlertColors::Github) => Some(github_alert_color(kind, dark)),
            (AlertColor::Auto, AlertColors::Terminal) => Some(Color::Palette(match kind {
                AlertKind::Note => 4,
                AlertKind::Tip => 2,
                AlertKind::Important => 5,
                AlertKind::Warning => 3,
                AlertKind::Caution => 1,
            })),
        }
    }
}

impl Default for AlertTheme {
    fn default() -> Self {
        Self {
            colors: AlertColors::Github,
            tint: true,
            note: AlertColor::Auto,
            tip: AlertColor::Auto,
            important: AlertColor::Auto,
            warning: AlertColor::Auto,
            caution: AlertColor::Auto,
        }
    }
}

/// The colours GitHub gives alerts in its dark and light themes.
fn github_alert_color(kind: AlertKind, dark: bool) -> Color {
    let rgb = |hex: u32| {
        let [_, r, g, b] = hex.to_be_bytes();
        Color::Rgb(r, g, b)
    };
    rgb(match (kind, dark) {
        (AlertKind::Note, true) => 0x4493f8,
        (AlertKind::Tip, true) => 0x3fb950,
        (AlertKind::Important, true) => 0xab7df8,
        (AlertKind::Warning, true) => 0xd29922,
        (AlertKind::Caution, true) => 0xf85149,
        (AlertKind::Note, false) => 0x0969da,
        (AlertKind::Tip, false) => 0x1a7f37,
        (AlertKind::Important, false) => 0x8250df,
        (AlertKind::Warning, false) => 0x9a6700,
        (AlertKind::Caution, false) => 0xd1242f,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingTheme {
    /// The heading's size, drawn with kitty's text sizing.
    pub size: TextSize,
    pub color: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    /// Draw a rule under the heading, as GitHub does for levels 1 and 2.
    pub rule: bool,
    pub underline: Underline,
    /// Filled across the whole width.
    pub background: Option<Color>,
    /// Indent in normal-size cells.
    pub indent: u8,
}

/// The colour of a code block's language label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelColor {
    /// Halfway between the code block colour theme's text and background.
    CodeBlock,
    Color(Option<Color>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeBackground {
    /// The background of the code block colour theme.
    CodeBlock,
    Color(Option<Color>),
}

impl HeadingTheme {
    fn new(scale: u8, color: Option<Color>, bold: bool, underline: Underline) -> Self {
        Self {
            size: TextSize::new(scale).expect("a default heading scale is 1 to 7"),
            color,
            bold,
            italic: false,
            rule: false,
            underline,
            background: None,
            indent: 0,
        }
    }
}

const RED: Option<Color> = Some(Color::Palette(1));
const BLUE: Option<Color> = Some(Color::Palette(4));
const MAGENTA: Option<Color> = Some(Color::Palette(5));
const CYAN: Option<Color> = Some(Color::Palette(6));
const GREY: Option<Color> = Some(Color::Palette(8));

impl Default for Theme {
    fn default() -> Self {
        Self {
            max_width: 100,
            background: None,
            foreground: None,
            muted: None,
            text_margin: 2,
            // Size carries the hierarchy; bold and the underline style tell
            // apart levels of the same size.
            headings: [
                HeadingTheme::new(3, BLUE, true, Underline::Single),
                HeadingTheme::new(2, BLUE, true, Underline::Single),
                HeadingTheme::new(2, BLUE, false, Underline::Dotted),
                HeadingTheme::new(1, BLUE, true, Underline::Single),
                HeadingTheme::new(1, BLUE, false, Underline::Dotted),
                HeadingTheme {
                    italic: true,
                    ..HeadingTheme::new(1, CYAN, false, Underline::Dotted)
                },
            ],
            link: BLUE,
            inline_code: MAGENTA,
            inline_code_background: CodeBackground::CodeBlock,
            inline_code_padding: true,
            inline_code_wrap: true,
            quote_bar: GREY,
            rule: GREY,
            list_marker: GREY,
            table_border: GREY,
            error: RED,
            code_padding: true,
            code_theme_dark: "base16-ocean.dark".into(),
            code_theme_light: "InspiredGitHub".into(),
            show_language_label: true,
            language_label: LabelColor::CodeBlock,
            alerts: AlertTheme::default(),
            status_line: StatusLineTheme::default(),
            search: SearchTheme::default(),
            // Black on yellow, as a browser shows it.
            mark: MarkTheme {
                foreground: Some(Color::Palette(0)),
                background: Some(Color::Palette(3)),
            },
            file_header: FileHeaderTheme::default(),
            formula: FormulaTheme::default(),
        }
    }
}

impl Theme {
    pub fn heading(&self, level: u8) -> &HeadingTheme {
        let index = usize::from(level.clamp(1, 6)) - 1;
        &self.headings[index]
    }

    /// Drops every margin, indent and code padding, so text and code start
    /// at the left edge and copy cleanly. List nesting stays. Inline code
    /// loses its padding too: copied no-break spaces break shell commands.
    pub fn without_margins(mut self) -> Self {
        self.text_margin = 0;
        self.code_padding = false;
        self.inline_code_padding = false;
        for heading in &mut self.headings {
            heading.indent = 0;
        }
        self
    }

    /// Every colour set to the terminal's default, for `NO_COLOR`.
    pub fn without_colors(mut self) -> Self {
        self.background = None;
        self.foreground = None;
        self.muted = None;
        self.link = None;
        self.inline_code = None;
        self.inline_code_background = CodeBackground::Color(None);
        self.quote_bar = None;
        self.rule = None;
        self.list_marker = None;
        self.table_border = None;
        self.error = None;
        self.language_label = LabelColor::Color(None);
        self.alerts = AlertTheme {
            tint: false,
            note: AlertColor::Color(None),
            tip: AlertColor::Color(None),
            important: AlertColor::Color(None),
            warning: AlertColor::Color(None),
            caution: AlertColor::Color(None),
            ..self.alerts
        };
        for heading in &mut self.headings {
            heading.color = None;
            heading.background = None;
        }
        self.status_line = StatusLineTheme::default();
        self.search = SearchTheme::default();
        self.mark = MarkTheme::default();
        self.file_header = FileHeaderTheme {
            background: BandColor::Color(None),
            bar: None,
        };
        self.formula = FormulaTheme::default();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn muted_text_is_dimmed_unless_the_theme_gives_it_a_colour() {
        assert_eq!(Theme::default().muted, None);
        assert_eq!(
            parse("muted = \"#808080\"\n").unwrap().muted,
            Some(Color::Rgb(0x80, 0x80, 0x80))
        );
        assert_eq!(Theme::paper(Paper::Light).without_colors().muted, None);
    }

    #[test]
    fn errors_are_red_on_every_page() {
        // The terminal's own red, unless the page has colours of its own.
        assert_eq!(Theme::default().error, Some(Color::Palette(1)));
        // A dark red on the light paper page, a light one on the dark.
        let light = Theme::paper(Paper::Light).error;
        assert_eq!(light, Some(Color::Rgb(0xb3, 0x26, 0x1e)));
        let dark = Theme::paper(Paper::Dark).error;
        assert_eq!(dark, Some(Color::Rgb(0xff, 0x78, 0x6e)));
        assert_eq!(
            parse("error = \"#00ff00\"\n").unwrap().error,
            Some(Color::Rgb(0, 0xff, 0))
        );
        assert_eq!(parse("error = \"none\"\n").unwrap().error, None);
        assert_eq!(Theme::default().without_colors().error, None);
    }

    #[test]
    fn without_colors_leaves_not_one_colour() {
        // Every field is named, so a new one does not compile until it is
        // sorted here into the colours, which must go, and the rest.
        for theme in [
            Theme::default(),
            Theme::paper(Paper::Light),
            Theme::paper(Paper::Dark),
        ] {
            let Theme {
                max_width: _,
                background,
                foreground,
                muted,
                text_margin: _,
                headings,
                link,
                inline_code,
                inline_code_background,
                inline_code_padding: _,
                inline_code_wrap: _,
                quote_bar,
                rule,
                list_marker,
                table_border,
                error,
                code_padding: _,
                code_theme_dark: _,
                code_theme_light: _,
                show_language_label: _,
                language_label,
                alerts,
                status_line,
                search,
                mark,
                file_header,
                formula,
            } = theme.without_colors();
            let colors = [
                background,
                foreground,
                muted,
                link,
                inline_code,
                quote_bar,
                rule,
                list_marker,
                table_border,
                error,
            ];
            assert_eq!(colors, [None; 10]);
            assert_eq!(inline_code_background, CodeBackground::Color(None));
            assert_eq!(language_label, LabelColor::Color(None));
            for heading in headings {
                let HeadingTheme {
                    size: _,
                    color,
                    bold: _,
                    italic: _,
                    rule: _,
                    underline: _,
                    background,
                    indent: _,
                } = heading;
                assert_eq!((color, background), (None, None));
            }
            let AlertTheme {
                colors: _,
                tint,
                note,
                tip,
                important,
                warning,
                caution,
            } = alerts;
            assert!(!tint);
            let none = AlertColor::Color(None);
            assert_eq!([note, tip, important, warning, caution], [none; 5]);
            let StatusLineTheme {
                background,
                foreground,
                badge_background,
                badge_foreground,
            } = status_line;
            let status = [background, foreground, badge_background, badge_foreground];
            assert_eq!(status, [StatusColor::Auto; 4]);
            let SearchTheme {
                match_foreground,
                match_background,
                current_foreground,
                current_background,
            } = search;
            let search = [
                match_foreground,
                match_background,
                current_foreground,
                current_background,
            ];
            assert_eq!(search, [SearchColor::Auto; 4]);
            let MarkTheme {
                foreground,
                background,
            } = mark;
            assert_eq!([foreground, background], [None; 2]);
            let FileHeaderTheme { background, bar } = file_header;
            assert_eq!((background, bar), (BandColor::Color(None), None));
            let FormulaTheme { text, background } = formula;
            assert_eq!([text, background], [None; 2]);
        }
    }
}
