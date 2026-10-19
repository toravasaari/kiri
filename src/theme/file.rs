//! The theme file: TOML that changes the defaults.

use std::path::Path;

use serde::Deserialize;

use super::{
    AlertColor, AlertColors, AlertTheme, CODE_THEMES, CodeBackground, HeadingTheme, LabelColor,
    SearchColor, StatusColor, Theme,
};
use crate::color::{Color, Underline};

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
    use crate::model::AlertKind;
    use crate::theme::{SearchTheme, StatusLineTheme};

    #[test]
    fn the_documented_defaults_are_the_defaults() {
        let docs = include_str!("../../docs/theme.md");
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
    fn search_has_no_flash_colours() {
        assert!(parse("[search]\nflash_foreground = \"black\"\n").is_err());
        assert!(parse("[search]\nflash_background = \"yellow\"\n").is_err());
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
