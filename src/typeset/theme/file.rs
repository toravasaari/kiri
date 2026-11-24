//! The theme file: TOML that changes the defaults. This file parses its
//! text; reading the file is `src/load/theme.rs`.

use serde::Deserialize;

use crate::typeset::color::{Color, Underline};
use crate::typeset::size::{TextSize, VerticalAlign};
use crate::typeset::theme::{
    AlertColor, AlertColors, AlertTheme, BarColor, CODE_THEMES, CodeBackground, ContentsColor,
    HeadingTheme, LabelColor, SearchColor, TableStyle, Theme,
};

#[expect(clippy::too_many_lines)]
pub fn parse(text: &str) -> Result<Theme, String> {
    let file: ThemeFile = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut theme = Theme::default();

    if let Some(max_width) = file.max_width {
        theme.max_width = max_width;
    }
    if let Some(margin) = file.text_margin {
        theme.text_margin = margin;
    }
    set_color(&mut theme.background, file.background, "background")?;
    set_color(&mut theme.foreground, file.foreground, "foreground")?;
    set_color(&mut theme.muted, file.muted, "muted")?;
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
    set_color(&mut theme.link, file.link, "link")?;
    set_color(&mut theme.inline_code, file.inline_code, "inline_code")?;
    set_keyword_or_color(
        &mut theme.inline_code_background,
        file.inline_code_background,
        ("code-block", CodeBackground::CodeBlock),
        CodeBackground::Color,
        "inline_code_background",
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
    set_color(&mut theme.quote_bar, file.quote_bar, "quote_bar")?;
    set_color(&mut theme.rule, file.rule, "rule")?;
    set_color(&mut theme.list_marker, file.list_marker, "list_marker")?;
    set_color(&mut theme.table_border, file.table_border, "table_border")?;
    if let Some(style) = file.table_style {
        theme.table_style = match style.as_str() {
            "frame" => TableStyle::Frame,
            "grid" => TableStyle::Grid,
            _ => {
                return Err(format!(
                    "unknown table_style \"{style}\"; use frame or grid"
                ));
            }
        };
    }
    set_color(&mut theme.error, file.error, "error")?;
    for (target, name, key) in [
        (
            &mut theme.code_theme_dark,
            file.code_theme_dark,
            "code_theme_dark",
        ),
        (
            &mut theme.code_theme_light,
            file.code_theme_light,
            "code_theme_light",
        ),
    ] {
        let Some(name) = name else { continue };
        if !CODE_THEMES.contains(&name.as_str()) {
            return Err(format!(
                "unknown code theme \"{name}\" for {key}; available: {}",
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
        "language_label",
    )?;
    if let Some(alerts) = file.alerts {
        apply_alerts(&mut theme.alerts, alerts)?;
    }
    set_keyword_or_color(
        &mut theme.bar_background,
        file.bar_background,
        ("auto", BarColor::Auto),
        BarColor::Color,
        "bar_background",
    )?;
    if let Some(status_line) = file.status_line {
        let target = &mut theme.status_line;
        for (target, value, key) in [
            (&mut target.background, status_line.background, "background"),
            (&mut target.foreground, status_line.foreground, "foreground"),
            (
                &mut target.badge_background,
                status_line.badge_background,
                "badge_background",
            ),
            (
                &mut target.badge_foreground,
                status_line.badge_foreground,
                "badge_foreground",
            ),
        ] {
            set_keyword_or_color(
                target,
                value,
                ("auto", BarColor::Auto),
                BarColor::Color,
                &format!("status_line.{key}"),
            )?;
        }
    }
    if let Some(search) = file.search {
        let target = &mut theme.search;
        for (target, value, key) in [
            (
                &mut target.match_foreground,
                search.match_foreground,
                "match_foreground",
            ),
            (
                &mut target.match_background,
                search.match_background,
                "match_background",
            ),
            (
                &mut target.current_foreground,
                search.current_foreground,
                "current_foreground",
            ),
            (
                &mut target.current_background,
                search.current_background,
                "current_background",
            ),
        ] {
            set_keyword_or_color(
                target,
                value,
                ("auto", SearchColor::Auto),
                SearchColor::Color,
                &format!("search.{key}"),
            )?;
        }
    }
    if let Some(header) = file.file_header {
        set_keyword_or_color(
            &mut theme.file_header.background,
            header.background,
            ("auto", BarColor::Auto),
            BarColor::Color,
            "file_header.background",
        )?;
        set_color(&mut theme.file_header.bar, header.bar, "file_header.bar")?;
    }
    if let Some(contents) = file.contents {
        let target = &mut theme.contents;
        for (target, value, key) in [
            (&mut target.background, contents.background, "background"),
            (&mut target.title, contents.title, "title"),
            (&mut target.text, contents.text, "text"),
            (&mut target.glyph, contents.glyph, "glyph"),
            (&mut target.current, contents.current, "current"),
            (
                &mut target.current_background,
                contents.current_background,
                "current_background",
            ),
            (&mut target.selected, contents.selected, "selected"),
            (&mut target.border, contents.border, "border"),
        ] {
            set_keyword_or_color(
                target,
                value,
                ("auto", ContentsColor::Auto),
                ContentsColor::Color,
                &format!("contents.{key}"),
            )?;
        }
    }
    if let Some(formula) = file.formula {
        set_color(&mut theme.formula.text, formula.text, "formula.text")?;
        let background = formula.background;
        set_color(
            &mut theme.formula.background,
            background,
            "formula.background",
        )?;
    }
    if let Some(mark) = file.mark {
        set_color(
            &mut theme.mark.foreground,
            mark.foreground,
            "mark.foreground",
        )?;
        set_color(
            &mut theme.mark.background,
            mark.background,
            "mark.background",
        )?;
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
    for (target, value, key) in [
        (&mut alerts.note, file.note, "note"),
        (&mut alerts.tip, file.tip, "tip"),
        (&mut alerts.important, file.important, "important"),
        (&mut alerts.warning, file.warning, "warning"),
        (&mut alerts.caution, file.caution, "caution"),
    ] {
        set_keyword_or_color(
            target,
            value,
            ("auto", AlertColor::Auto),
            AlertColor::Color,
            &format!("alerts.{key}"),
        )?;
    }
    Ok(())
}

fn apply_heading(
    heading: &mut HeadingTheme,
    overrides: &HeadingFile,
    table: &str,
) -> Result<(), String> {
    if let Some(scale) = overrides.scale {
        heading.size = TextSize::nearest(scale, VerticalAlign::Bottom)
            .ok_or_else(|| format!("{table}.scale must be between 0.5 and 7, not {scale}"))?;
        heading.written_scale =
            ((heading.size.value() - scale).abs() > 1e-9).then(|| scale.to_string());
    }
    set_color(
        &mut heading.color,
        overrides.color.clone(),
        &format!("{table}.color"),
    )?;
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
        heading.underline = parse_underline(underline, &format!("{table}.underline"))?;
    }
    set_color(
        &mut heading.background,
        overrides.background.clone(),
        &format!("{table}.background"),
    )?;
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
    table_style: Option<String>,
    error: Option<String>,
    code_padding: Option<bool>,
    code_theme_dark: Option<String>,
    code_theme_light: Option<String>,
    show_language_label: Option<bool>,
    language_label: Option<String>,
    alerts: Option<AlertsFile>,
    bar_background: Option<String>,
    status_line: Option<StatusLineFile>,
    search: Option<SearchFile>,
    mark: Option<MarkFile>,
    file_header: Option<FileHeaderFile>,
    contents: Option<ContentsFile>,
    formula: Option<FormulaFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContentsFile {
    background: Option<String>,
    title: Option<String>,
    text: Option<String>,
    glyph: Option<String>,
    current: Option<String>,
    current_background: Option<String>,
    selected: Option<String>,
    border: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormulaFile {
    text: Option<String>,
    background: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileHeaderFile {
    background: Option<String>,
    bar: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarkFile {
    foreground: Option<String>,
    background: Option<String>,
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
    /// A whole number or a decimal; TOML integers read as decimals too.
    scale: Option<f64>,
    color: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
    rule: Option<bool>,
    underline: Option<String>,
    background: Option<String>,
    indent: Option<u8>,
}

/// Sets `target` from the colour `value` of `key` when there is one.
fn set_color(target: &mut Option<Color>, value: Option<String>, key: &str) -> Result<(), String> {
    if let Some(value) = value {
        *target = color(&value, key)?;
    }
    Ok(())
}

/// Sets `target` from the `value` of `key` when there is one: `keyword`
/// names its own value, anything else is a colour that `color` makes the
/// value of.
fn set_keyword_or_color<T>(
    target: &mut T,
    value: Option<String>,
    keyword: (&str, T),
    color: impl FnOnce(Option<Color>) -> T,
    key: &str,
) -> Result<(), String> {
    if let Some(value) = value {
        let (name, named) = keyword;
        *target = if value == name {
            named
        } else {
            color(self::color(&value, key)?)
        };
    }
    Ok(())
}

/// The underline style `value` of `key`, or an error that names the key.
fn parse_underline(value: &str, key: &str) -> Result<Underline, String> {
    Ok(match value {
        "none" => Underline::None,
        "single" => Underline::Single,
        "double" => Underline::Double,
        "curly" => Underline::Curly,
        "dotted" => Underline::Dotted,
        "dashed" => Underline::Dashed,
        _ => {
            return Err(format!(
                "unknown underline \"{value}\" for {key}; use none, single, double, curly, dotted \
                 or dashed"
            ));
        }
    })
}

const COLOR_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

/// The colour `value` of `key`, or an error that names the key.
fn color(value: &str, key: &str) -> Result<Option<Color>, String> {
    parse_color(value).ok_or_else(|| {
        format!(
            "unknown colour \"{value}\" for {key}; use a name such as blue or bright-blue, \
             0-255, #rrggbb or none"
        )
    })
}

/// A palette colour by name (`blue`, `bright-blue`) or number (`0`-`255`),
/// `#rrggbb`, or `none` for the terminal's default colour; `None` when
/// `value` is none of them.
fn parse_color(value: &str) -> Option<Option<Color>> {
    if value == "none" {
        return Some(None);
    }
    // Digits only: from_str_radix and parse would also take a leading `+`.
    if let Some(hex) = value.strip_prefix('#')
        && hex.len() == 6
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
        && let Ok(rgb) = u32::from_str_radix(hex, 16)
    {
        let [_, r, g, b] = rgb.to_be_bytes();
        return Some(Some(Color::Rgb(r, g, b)));
    }
    if value.bytes().all(|b| b.is_ascii_digit())
        && let Ok(index) = value.parse::<u8>()
    {
        return Some(Some(Color::Palette(index)));
    }
    let (bright, name) = match value.strip_prefix("bright-") {
        Some(name) => (8, name),
        None => (0, value),
    };
    let index = COLOR_NAMES.iter().position(|&known| known == name)?;
    Some(Some(Color::Palette(bright + index as u8)))
}

#[cfg(test)]
mod tests;
