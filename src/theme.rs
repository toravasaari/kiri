//! Colours and sizes the renderer uses, and the theme file that changes them.
//!
//! Colours default to the terminal's own 16-colour palette, so kiri follows
//! the user's kitty theme instead of imposing one.

use std::path::Path;

use serde::Deserialize;

use crate::color::{Color, Underline};
use crate::model::AlertKind;

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
    /// Size as a multiple of the normal font, with kitty's text sizing.
    pub scale: u8,
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
            scale,
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
            code_padding: true,
            code_theme_dark: "base16-ocean.dark".into(),
            code_theme_light: "InspiredGitHub".into(),
            show_language_label: true,
            language_label: LabelColor::CodeBlock,
            alerts: AlertTheme::default(),
            status_line: StatusLineTheme::default(),
            search: SearchTheme::default(),
        }
    }
}

/// The built-in themes that draw the document as a page of fixed colours,
/// whatever the terminal's palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paper {
    Light,
    Dark,
}

/// The greys of a paper theme, from the page to the text.
struct PaperColors {
    background: Option<Color>,
    text: Option<Color>,
    /// Alt text, raw HTML and link targets.
    muted: Option<Color>,
    /// Headings and links.
    strong: Option<Color>,
    /// The smallest heading.
    quiet: Option<Color>,
    marker: Option<Color>,
    border: Option<Color>,
    /// Quote bars and rules.
    line: Option<Color>,
    code_background: Option<Color>,
    code_theme: &'static str,
    /// The pager's status line and the text on it.
    status: Option<Color>,
    status_text: Option<Color>,
    /// Search matches in the pager and the text on them.
    found: Option<Color>,
    found_text: Option<Color>,
}

impl Theme {
    /// A light or dark page in greys, with colour only in code blocks.
    pub fn paper(paper: Paper) -> Self {
        let grey = |level: u8| Some(Color::Rgb(level, level, level));
        let colors = match paper {
            Paper::Light => PaperColors {
                background: grey(0xff),
                text: grey(0x1f),
                muted: grey(0x6e),
                strong: grey(0x00),
                quiet: grey(0x55),
                marker: grey(0x70),
                border: grey(0xb4),
                line: grey(0xc8),
                code_background: grey(0xeb),
                code_theme: "base16-ocean.light",
                status: grey(0xe0),
                status_text: grey(0x1f),
                found: grey(0xcc),
                found_text: grey(0x00),
            },
            Paper::Dark => PaperColors {
                background: grey(0x1e),
                text: grey(0xd4),
                muted: grey(0x8e),
                strong: grey(0xff),
                quiet: grey(0xa0),
                marker: grey(0x9a),
                border: grey(0x5a),
                line: grey(0x50),
                code_background: grey(0x35),
                code_theme: "base16-eighties.dark",
                status: grey(0x3a),
                status_text: grey(0xe0),
                found: grey(0x60),
                found_text: grey(0xff),
            },
        };
        let mut theme = Self {
            background: colors.background,
            foreground: colors.text,
            muted: colors.muted,
            link: colors.strong,
            inline_code: colors.text,
            inline_code_background: CodeBackground::Color(colors.code_background),
            quote_bar: colors.line,
            rule: colors.line,
            list_marker: colors.marker,
            table_border: colors.border,
            // The page is the same whatever the terminal's background.
            code_theme_dark: colors.code_theme.into(),
            code_theme_light: colors.code_theme.into(),
            // The badge is the page's text and background swapped.
            status_line: StatusLineTheme {
                background: StatusColor::Color(colors.status),
                foreground: StatusColor::Color(colors.status_text),
                badge_background: StatusColor::Color(colors.text),
                badge_foreground: StatusColor::Color(colors.background),
            },
            // The current match is in the badge's colours.
            search: SearchTheme {
                match_foreground: SearchColor::Color(colors.found_text),
                match_background: SearchColor::Color(colors.found),
                current_foreground: SearchColor::Color(colors.background),
                current_background: SearchColor::Color(colors.text),
            },
            ..Self::default()
        };
        for heading in &mut theme.headings {
            heading.color = colors.strong;
        }
        theme.headings[5].color = colors.quiet;
        theme
    }

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
        self
    }
}

/// Reads a theme file. Keys it leaves out keep their default values.
pub fn load(path: &Path) -> Result<Theme, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn parse(text: &str) -> Result<Theme, String> {
    let file: ThemeFile = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut theme = Theme::default();

    if let Some(max_width) = file.max_width {
        theme.max_width = max_width;
    }
    if let Some(margin) = file.text_margin {
        theme.text_margin = margin;
    }
    set_color(&mut theme.background, file.background)?;
    set_color(&mut theme.foreground, file.foreground)?;
    set_color(&mut theme.muted, file.muted)?;
    // [headings] applies to every level, then [h1] to [h6] refine it.
    if let Some(all) = &file.headings {
        for heading in &mut theme.headings {
            apply_heading(heading, all, "headings")?;
        }
    }
    let levels = [file.h1, file.h2, file.h3, file.h4, file.h5, file.h6];
    for (level, (heading, overrides)) in theme.headings.iter_mut().zip(levels).enumerate() {
        if let Some(overrides) = overrides {
            apply_heading(heading, &overrides, &format!("h{}", level + 1))?;
        }
    }
    set_color(&mut theme.link, file.link)?;
    set_color(&mut theme.inline_code, file.inline_code)?;
    set_keyword_or_color(
        &mut theme.inline_code_background,
        file.inline_code_background,
        ("code-block", CodeBackground::CodeBlock),
        CodeBackground::Color,
    )?;
    if let Some(padding) = file.inline_code_padding {
        theme.inline_code_padding = padding;
    }
    if let Some(wrap) = file.inline_code_wrap {
        theme.inline_code_wrap = wrap;
    }
    if let Some(padding) = file.code_padding {
        theme.code_padding = padding;
    }
    set_color(&mut theme.quote_bar, file.quote_bar)?;
    set_color(&mut theme.rule, file.rule)?;
    set_color(&mut theme.list_marker, file.list_marker)?;
    set_color(&mut theme.table_border, file.table_border)?;
    for (target, name) in [
        (&mut theme.code_theme_dark, file.code_theme_dark),
        (&mut theme.code_theme_light, file.code_theme_light),
    ] {
        let Some(name) = name else { continue };
        if !CODE_THEMES.contains(&name.as_str()) {
            return Err(format!(
                "unknown code theme \"{name}\"; available: {}",
                CODE_THEMES.join(", ")
            ));
        }
        *target = name;
    }
    if let Some(show) = file.show_language_label {
        theme.show_language_label = show;
    }
    set_keyword_or_color(
        &mut theme.language_label,
        file.language_label,
        ("code-block", LabelColor::CodeBlock),
        LabelColor::Color,
    )?;
    if let Some(alerts) = file.alerts {
        apply_alerts(&mut theme.alerts, alerts)?;
    }
    if let Some(status_line) = file.status_line {
        let target = &mut theme.status_line;
        for (target, value) in [
            (&mut target.background, status_line.background),
            (&mut target.foreground, status_line.foreground),
            (&mut target.badge_background, status_line.badge_background),
            (&mut target.badge_foreground, status_line.badge_foreground),
        ] {
            set_keyword_or_color(
                target,
                value,
                ("auto", StatusColor::Auto),
                StatusColor::Color,
            )?;
        }
    }
    if let Some(search) = file.search {
        let target = &mut theme.search;
        for (target, value) in [
            (&mut target.match_foreground, search.match_foreground),
            (&mut target.match_background, search.match_background),
            (&mut target.current_foreground, search.current_foreground),
            (&mut target.current_background, search.current_background),
        ] {
            set_keyword_or_color(
                target,
                value,
                ("auto", SearchColor::Auto),
                SearchColor::Color,
            )?;
        }
    }
    Ok(theme)
}

fn apply_alerts(alerts: &mut AlertTheme, file: AlertsFile) -> Result<(), String> {
    if let Some(colors) = file.colors {
        alerts.colors = match colors.as_str() {
            "github" => AlertColors::Github,
            "terminal" => AlertColors::Terminal,
            _ => {
                return Err(format!(
                    "unknown alerts.colors \"{colors}\"; use github or terminal"
                ));
            }
        };
    }
    if let Some(background) = file.background {
        alerts.tint = match background.as_str() {
            "tint" => true,
            "none" => false,
            _ => {
                return Err(format!(
                    "unknown alerts.background \"{background}\"; use tint or none"
                ));
            }
        };
    }
    for (target, value) in [
        (&mut alerts.note, file.note),
        (&mut alerts.tip, file.tip),
        (&mut alerts.important, file.important),
        (&mut alerts.warning, file.warning),
        (&mut alerts.caution, file.caution),
    ] {
        set_keyword_or_color(target, value, ("auto", AlertColor::Auto), AlertColor::Color)?;
    }
    Ok(())
}

fn apply_heading(
    heading: &mut HeadingTheme,
    overrides: &HeadingFile,
    table: &str,
) -> Result<(), String> {
    if let Some(scale) = overrides.scale {
        // The text sizing protocol accepts scales 1 to 7.
        if !(1..=7).contains(&scale) {
            return Err(format!(
                "{table}.scale must be between 1 and 7, not {scale}"
            ));
        }
        heading.scale = scale;
    }
    set_color(&mut heading.color, overrides.color.clone())?;
    if let Some(bold) = overrides.bold {
        heading.bold = bold;
    }
    if let Some(italic) = overrides.italic {
        heading.italic = italic;
    }
    if let Some(rule) = overrides.rule {
        heading.rule = rule;
    }
    if let Some(underline) = &overrides.underline {
        heading.underline = parse_underline(underline)?;
    }
    set_color(&mut heading.background, overrides.background.clone())?;
    if let Some(indent) = overrides.indent {
        heading.indent = indent;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    max_width: Option<u16>,
    background: Option<String>,
    foreground: Option<String>,
    muted: Option<String>,
    text_margin: Option<u8>,
    headings: Option<HeadingFile>,
    h1: Option<HeadingFile>,
    h2: Option<HeadingFile>,
    h3: Option<HeadingFile>,
    h4: Option<HeadingFile>,
    h5: Option<HeadingFile>,
    h6: Option<HeadingFile>,
    link: Option<String>,
    inline_code: Option<String>,
    inline_code_background: Option<String>,
    inline_code_padding: Option<bool>,
    inline_code_wrap: Option<bool>,
    quote_bar: Option<String>,
    rule: Option<String>,
    list_marker: Option<String>,
    table_border: Option<String>,
    code_padding: Option<bool>,
    code_theme_dark: Option<String>,
    code_theme_light: Option<String>,
    show_language_label: Option<bool>,
    language_label: Option<String>,
    alerts: Option<AlertsFile>,
    status_line: Option<StatusLineFile>,
    search: Option<SearchFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchFile {
    match_foreground: Option<String>,
    match_background: Option<String>,
    current_foreground: Option<String>,
    current_background: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusLineFile {
    background: Option<String>,
    foreground: Option<String>,
    badge_background: Option<String>,
    badge_foreground: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AlertsFile {
    colors: Option<String>,
    background: Option<String>,
    note: Option<String>,
    tip: Option<String>,
    important: Option<String>,
    warning: Option<String>,
    caution: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeadingFile {
    scale: Option<u8>,
    color: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
    rule: Option<bool>,
    underline: Option<String>,
    background: Option<String>,
    indent: Option<u8>,
}

fn set_color(target: &mut Option<Color>, value: Option<String>) -> Result<(), String> {
    if let Some(value) = value {
        *target = parse_color(&value)?;
    }
    Ok(())
}

/// Sets `target` from `value` when there is one: `keyword` names its own
/// value, anything else is a colour that `color` makes the value of.
fn set_keyword_or_color<T>(
    target: &mut T,
    value: Option<String>,
    keyword: (&str, T),
    color: impl FnOnce(Option<Color>) -> T,
) -> Result<(), String> {
    if let Some(value) = value {
        let (name, named) = keyword;
        *target = if value == name {
            named
        } else {
            color(parse_color(&value)?)
        };
    }
    Ok(())
}

fn parse_underline(value: &str) -> Result<Underline, String> {
    Ok(match value {
        "none" => Underline::None,
        "single" => Underline::Single,
        "double" => Underline::Double,
        "curly" => Underline::Curly,
        "dotted" => Underline::Dotted,
        "dashed" => Underline::Dashed,
        _ => {
            return Err(format!(
                "unknown underline \"{value}\"; use none, single, double, curly, dotted or dashed"
            ));
        }
    })
}

const COLOR_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

/// A palette colour by name (`blue`, `bright-blue`) or number (`0`-`255`),
/// `#rrggbb`, or `none` for the terminal's default colour.
fn parse_color(value: &str) -> Result<Option<Color>, String> {
    if value == "none" {
        return Ok(None);
    }
    // Digits only: from_str_radix and parse would also take a leading `+`.
    if let Some(hex) = value.strip_prefix('#')
        && hex.len() == 6
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
        && let Ok(rgb) = u32::from_str_radix(hex, 16)
    {
        let [_, r, g, b] = rgb.to_be_bytes();
        return Ok(Some(Color::Rgb(r, g, b)));
    }
    if value.bytes().all(|b| b.is_ascii_digit())
        && let Ok(index) = value.parse::<u8>()
    {
        return Ok(Some(Color::Palette(index)));
    }
    let (bright, name) = match value.strip_prefix("bright-") {
        Some(name) => (8, name),
        None => (0, value),
    };
    match COLOR_NAMES.iter().position(|&known| known == name) {
        Some(index) => Ok(Some(Color::Palette(bright + index as u8))),
        None => Err(format!(
            "unknown colour \"{value}\"; use a name such as blue or bright-blue, 0-255, #rrggbb or none"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_defaults_are_the_defaults() {
        let docs = include_str!("../docs/theme.md");
        let defaults = docs
            .find("## Defaults")
            .expect("docs/theme.md has a Defaults section");
        let start = defaults
            + docs[defaults..]
                .find("```toml\n")
                .expect("the Defaults section has a TOML block")
            + 8;
        let end = start + docs[start..].find("```").expect("the TOML block ends");
        assert_eq!(parse(&docs[start..end]).unwrap(), Theme::default());
    }

    /// The TOML block after `heading` in docs/theme.md.
    fn documented(heading: &str) -> &'static str {
        let docs = include_str!("../docs/theme.md");
        let section = docs
            .find(heading)
            .unwrap_or_else(|| panic!("docs/theme.md has {heading}"));
        let start = section
            + docs[section..]
                .find("```toml\n")
                .expect("the section has a TOML block")
            + 8;
        let end = start + docs[start..].find("```").expect("the TOML block ends");
        &docs[start..end]
    }

    #[test]
    fn the_status_line_takes_colours_none_and_auto() {
        let theme = parse(
            "[status_line]\nbackground = \"#101010\"\nforeground = \"none\"\n\
             badge_background = \"red\"\nbadge_foreground = \"auto\"\n",
        )
        .unwrap();
        assert_eq!(
            theme.status_line,
            StatusLineTheme {
                background: StatusColor::Color(Some(Color::Rgb(16, 16, 16))),
                foreground: StatusColor::Color(None),
                badge_background: StatusColor::Color(Some(Color::Palette(1))),
                badge_foreground: StatusColor::Auto,
            }
        );
        assert!(parse("[status_line]\nbar = \"red\"\n").is_err());
    }

    #[test]
    fn search_matches_take_colours_none_and_auto() {
        let theme = parse(
            "[search]\nmatch_foreground = \"#101010\"\nmatch_background = \"none\"\n\
             current_foreground = \"auto\"\ncurrent_background = \"yellow\"\n",
        )
        .unwrap();
        assert_eq!(
            theme.search,
            SearchTheme {
                match_foreground: SearchColor::Color(Some(Color::Rgb(16, 16, 16))),
                match_background: SearchColor::Color(None),
                current_foreground: SearchColor::Auto,
                current_background: SearchColor::Color(Some(Color::Palette(3))),
            }
        );
        assert!(parse("[search]\nbackground = \"red\"\n").is_err());
        assert!(parse("[search]\nmatch_background = \"pink\"\n").is_err());
    }

    #[test]
    fn paper_themes_mark_matches_in_greys_and_no_colour_in_reverse_video() {
        let grey = |level| SearchColor::Color(Some(Color::Rgb(level, level, level)));
        let light = Theme::paper(Paper::Light).search;
        assert_eq!(
            [light.match_foreground, light.match_background],
            [grey(0x00), grey(0xcc)]
        );
        assert_eq!(
            [light.current_foreground, light.current_background],
            [grey(0xff), grey(0x1f)]
        );
        let dark = Theme::paper(Paper::Dark).search;
        assert_eq!(
            [dark.match_foreground, dark.match_background],
            [grey(0xff), grey(0x60)]
        );
        assert_eq!(
            [dark.current_foreground, dark.current_background],
            [grey(0x1e), grey(0xd4)]
        );
        let plain = Theme::paper(Paper::Dark).without_colors().search;
        assert_eq!(plain, SearchTheme::default());
    }

    #[test]
    fn search_has_no_flash_colours() {
        assert!(parse("[search]\nflash_foreground = \"black\"\n").is_err());
        assert!(parse("[search]\nflash_background = \"yellow\"\n").is_err());
    }

    #[test]
    fn the_documented_paper_themes_are_the_paper_themes() {
        assert_eq!(
            parse(documented("### Light paper")).unwrap(),
            Theme::paper(Paper::Light)
        );
        assert_eq!(
            parse(documented("### Dark paper")).unwrap(),
            Theme::paper(Paper::Dark)
        );
    }

    /// Every colour of `theme` outside code blocks.
    fn colors(theme: &Theme) -> Vec<Option<Color>> {
        let mut colors = vec![
            theme.background,
            theme.foreground,
            theme.muted,
            theme.link,
            theme.inline_code,
            theme.quote_bar,
            theme.rule,
            theme.list_marker,
            theme.table_border,
        ];
        match theme.inline_code_background {
            CodeBackground::Color(color) => colors.push(color),
            CodeBackground::CodeBlock => panic!("inline code takes the code block's colour"),
        }
        for heading in &theme.headings {
            colors.push(heading.color);
            colors.push(heading.background);
        }
        colors
    }

    fn lightness(color: Option<Color>) -> u8 {
        match color {
            Some(Color::Rgb(r, g, b)) if r == g && g == b => r,
            _ => panic!("{color:?} is not a fixed grey"),
        }
    }

    #[test]
    fn paper_themes_are_fixed_greys_outside_code_blocks() {
        for paper in [Paper::Light, Paper::Dark] {
            let theme = Theme::paper(paper);
            // Anything else may be none: the page's colours show then.
            for (index, color) in colors(&theme).into_iter().enumerate() {
                if color.is_some() || index < 2 {
                    lightness(color);
                }
            }
        }
    }

    #[test]
    fn light_paper_is_dark_text_on_a_light_page_and_dark_paper_the_reverse() {
        let light = Theme::paper(Paper::Light);
        assert!(lightness(light.background) > 0xf0);
        assert!(lightness(light.foreground) < 0x40);
        let dark = Theme::paper(Paper::Dark);
        assert!(lightness(dark.background) < 0x30);
        assert!(lightness(dark.foreground) > 0xc0);
    }

    /// The WCAG contrast ratio of two greys.
    fn contrast(a: Option<Color>, b: Option<Color>) -> f64 {
        let luminance = |color| {
            let c = f64::from(lightness(color)) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn paper_themes_mute_text_in_a_readable_grey() {
        for paper in [Paper::Light, Paper::Dark] {
            let theme = Theme::paper(paper);
            let ratio = contrast(theme.muted, theme.background);
            assert!(ratio >= 4.5, "{paper:?}: {ratio}");
            // Muted, but not as strong as the text.
            assert!(ratio < contrast(theme.foreground, theme.background));
        }
    }

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
    fn paper_themes_highlight_code_to_match_the_page() {
        let light = Theme::paper(Paper::Light);
        assert_eq!(light.code_theme_light, "base16-ocean.light");
        assert_eq!(light.code_theme_dark, light.code_theme_light);
        let dark = Theme::paper(Paper::Dark);
        assert_eq!(dark.code_theme_dark, "base16-eighties.dark");
        assert_eq!(dark.code_theme_light, dark.code_theme_dark);
    }

    #[test]
    fn an_empty_file_gives_the_defaults() {
        assert_eq!(parse("").unwrap(), Theme::default());
    }

    #[test]
    fn overrides_only_the_given_keys() {
        let theme =
            parse("max_width = 0\nlink = \"#ff8000\"\n[h2]\nscale = 1\nrule = false\n").unwrap();
        assert_eq!(theme.max_width, 0);
        assert_eq!(theme.link, Some(Color::Rgb(255, 128, 0)));
        assert_eq!(theme.heading(2).scale, 1);
        assert!(!theme.heading(2).rule);
        assert_eq!(theme.heading(2).color, Theme::default().heading(2).color);
        assert_eq!(theme.heading(1), Theme::default().heading(1));
    }

    #[test]
    fn inline_code_can_stay_on_one_line() {
        assert!(Theme::default().inline_code_wrap);
        assert!(
            !parse("inline_code_wrap = false\n")
                .unwrap()
                .inline_code_wrap
        );
    }

    #[test]
    fn the_language_label_can_be_hidden_or_coloured() {
        let theme = Theme::default();
        assert!(theme.show_language_label);
        assert_eq!(theme.language_label, LabelColor::CodeBlock);
        assert!(
            !parse("show_language_label = false\n")
                .unwrap()
                .show_language_label
        );
        let parsed = |value: &str| parse(&format!("language_label = \"{value}\"\n"));
        assert_eq!(
            parsed("code-block").unwrap().language_label,
            LabelColor::CodeBlock
        );
        assert_eq!(
            parsed("red").unwrap().language_label,
            LabelColor::Color(Some(Color::Palette(1)))
        );
        assert_eq!(
            parsed("none").unwrap().language_label,
            LabelColor::Color(None)
        );
        assert!(parsed("purple").is_err());
    }

    #[test]
    fn alerts_take_github_colours_unless_the_theme_says_otherwise() {
        let alerts = Theme::default().alerts;
        assert!(alerts.tint);
        assert_eq!(
            alerts.color(AlertKind::Note, true),
            Some(Color::Rgb(0x44, 0x93, 0xf8))
        );
        assert_eq!(
            alerts.color(AlertKind::Caution, false),
            Some(Color::Rgb(0xd1, 0x24, 0x2f))
        );
        let alerts = parse(
            "[alerts]\ncolors = \"terminal\"\nbackground = \"none\"\n\
             tip = \"#123456\"\nwarning = \"none\"\nnote = \"auto\"\n",
        )
        .unwrap()
        .alerts;
        assert!(!alerts.tint);
        assert_eq!(alerts.color(AlertKind::Note, true), Some(Color::Palette(4)));
        assert_eq!(
            alerts.color(AlertKind::Caution, true),
            Some(Color::Palette(1))
        );
        assert_eq!(
            alerts.color(AlertKind::Tip, true),
            Some(Color::Rgb(0x12, 0x34, 0x56))
        );
        assert_eq!(alerts.color(AlertKind::Warning, true), None);
        for mistake in [
            "colors = \"pink\"",
            "background = \"blur\"",
            "note = \"nope\"",
        ] {
            assert!(
                parse(&format!("[alerts]\n{mistake}\n")).is_err(),
                "{mistake}"
            );
        }
        // Without colours, alerts have neither colours nor a tint.
        let plain = Theme::default().without_colors().alerts;
        assert!(!plain.tint);
        assert_eq!(plain.color(AlertKind::Important, true), None);
    }

    #[test]
    fn headings_table_applies_to_all_levels_before_each_level() {
        let theme = parse("[headings]\nindent = 1\nrule = false\n[h2]\nindent = 2\n").unwrap();
        let indents: Vec<u8> = theme.headings.iter().map(|h| h.indent).collect();
        assert_eq!(indents, vec![1, 2, 1, 1, 1, 1]);
        assert!(theme.headings.iter().all(|h| !h.rule));
    }

    #[test]
    fn parses_colour_names_and_numbers() {
        assert_eq!(parse_color("cyan"), Ok(Some(Color::Palette(6))));
        assert_eq!(parse_color("bright-black"), Ok(Some(Color::Palette(8))));
        assert_eq!(parse_color("208"), Ok(Some(Color::Palette(208))));
        assert_eq!(parse_color("none"), Ok(None));
        assert_eq!(
            parse_color("#1a2B3c"),
            Ok(Some(Color::Rgb(0x1a, 0x2b, 0x3c)))
        );
        assert!(parse_color("#12345").is_err());
        assert!(parse_color("#+12345").is_err());
        assert!(parse_color("+12").is_err());
        assert!(parse_color("purple").is_err());
    }

    #[test]
    fn accepts_every_bundled_code_theme_and_lists_them_on_a_mistake() {
        let names = [
            "InspiredGitHub",
            "Solarized (dark)",
            "Solarized (light)",
            "base16-eighties.dark",
            "base16-mocha.dark",
            "base16-ocean.dark",
            "base16-ocean.light",
        ];
        for name in names {
            let theme = parse(&format!("code_theme_light = \"{name}\"\n")).unwrap();
            assert_eq!(theme.code_theme_light, name);
        }
        assert_eq!(
            parse("code_theme_dark = \"nope\"\n").unwrap_err(),
            format!(
                "unknown code theme \"nope\"; available: {}",
                names.join(", ")
            )
        );
    }

    #[test]
    fn rejects_mistakes_with_a_reason() {
        assert!(
            parse("[h1]\nscale = 9\n")
                .unwrap_err()
                .contains("between 1 and 7")
        );
        assert!(
            parse("code_theme_dark = \"nope\"\n")
                .unwrap_err()
                .contains("InspiredGitHub")
        );
        assert!(parse("colour = \"blue\"\n").is_err());
    }

    #[test]
    fn inline_code_takes_the_code_block_background_or_a_colour() {
        let parsed = |value: &str| {
            parse(&format!("inline_code_background = \"{value}\"\n"))
                .unwrap()
                .inline_code_background
        };
        assert_eq!(parsed("code-block"), CodeBackground::CodeBlock);
        assert_eq!(
            parsed("blue"),
            CodeBackground::Color(Some(Color::Palette(4)))
        );
        assert_eq!(parsed("none"), CodeBackground::Color(None));
    }

    #[test]
    fn a_keyword_or_colour_field_tells_why_a_value_is_wrong() {
        let unknown = "unknown colour \"pink\"; use a name such as blue or bright-blue, \
                       0-255, #rrggbb or none";
        for file in [
            "inline_code_background = \"pink\"",
            "language_label = \"pink\"",
            "[alerts]\nnote = \"pink\"",
            "[status_line]\nbackground = \"pink\"",
            "[search]\nmatch_background = \"pink\"",
        ] {
            assert_eq!(parse(&format!("{file}\n")).unwrap_err(), unknown, "{file}");
        }
    }
}
