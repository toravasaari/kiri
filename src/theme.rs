//! Colours and sizes the renderer uses, and the theme file that changes them.
//!
//! Colours default to the terminal's own 16-colour palette, so kiri follows
//! the user's kitty theme instead of imposing one.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::render::highlight;
use crate::render::style::{Color, Underline};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Maximum text width in cells; long lines are hard to read. 0 = none.
    pub max_width: u16,
    /// Painted behind the whole document; `None` shows the terminal's own.
    pub background: Option<Color>,
    /// Cells kept free on both sides of everything but headings and rules.
    pub text_margin: u8,
    /// Styles for heading levels 1 to 6.
    pub headings: [HeadingTheme; 6],
    pub link: Option<Color>,
    pub inline_code: Option<Color>,
    pub inline_code_background: CodeBackground,
    /// A space of background colour on each side of inline code.
    pub inline_code_padding: bool,
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
    pub underline: Underline,
    /// Filled across the whole width.
    pub background: Option<Color>,
    /// Indent in normal-size cells.
    pub indent: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeBackground {
    /// The background of the code block colour theme.
    CodeBlock,
    Color(Option<Color>),
}

impl HeadingTheme {
    fn new(scale: u8, color: Option<Color>, rule: bool) -> Self {
        Self {
            scale,
            color,
            bold: true,
            rule,
            underline: Underline::None,
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
            text_margin: 0,
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
            inline_code_background: CodeBackground::CodeBlock,
            inline_code_padding: true,
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

/// `$XDG_CONFIG_HOME/kiri/theme.toml`, or `~/.config/kiri/theme.toml`.
pub fn default_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("kiri").join("theme.toml"))
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
    if let Some(background) = file.inline_code_background {
        theme.inline_code_background = match background.as_str() {
            "code-block" => CodeBackground::CodeBlock,
            color => CodeBackground::Color(parse_color(color)?),
        };
    }
    if let Some(padding) = file.inline_code_padding {
        theme.inline_code_padding = padding;
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
        let known = highlight::theme_names();
        if !known.contains(&name.as_str()) {
            return Err(format!(
                "unknown code theme \"{name}\"; available: {}",
                known.join(", ")
            ));
        }
        *target = name;
    }
    Ok(theme)
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
    quote_bar: Option<String>,
    rule: Option<String>,
    list_marker: Option<String>,
    table_border: Option<String>,
    code_theme_dark: Option<String>,
    code_theme_light: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeadingFile {
    scale: Option<u8>,
    color: Option<String>,
    bold: Option<bool>,
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
    if let Some(hex) = value.strip_prefix('#')
        && hex.len() == 6
        && let Ok(rgb) = u32::from_str_radix(hex, 16)
    {
        let [_, r, g, b] = rgb.to_be_bytes();
        return Ok(Some(Color::Rgb(r, g, b)));
    }
    if let Ok(index) = value.parse::<u8>() {
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
        assert!(parse_color("#12345").is_err());
        assert!(parse_color("purple").is_err());
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
}
