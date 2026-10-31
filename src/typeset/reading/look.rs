//! Reading mode's colours: the one look, resolved into the brushes the page
//! is drawn with.

use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
use crate::typeset::images::size::cell_size;
use crate::typeset::layout::CodeColors;
use crate::typeset::model::AlertKind;
use crate::typeset::reading::page::{
    AlertLook, Look as PageLook, Marked, TableStyle as PageTableStyle,
};
use crate::typeset::reading::text::Brush;
use crate::typeset::theme::{Look, TableStyle};

/// How reading mode draws text in the look `one` on a terminal with `caps`: its
/// size from the cell height, and its colours from the theme, or from the
/// terminal where the theme leaves them. Tables shade every second body
/// row when `stripes` is set.
pub fn page_look(one: &Look, code: &CodeColors, stripes: bool, caps: &Capabilities) -> PageLook {
    let (_, cell_height) = cell_size(caps);
    let rgb = |color: Option<Color>| match color?.rgb_or_xterm(&caps.palette) {
        Color::Rgb(red, green, blue) => Some([red, green, blue]),
        Color::Palette(_) => None,
    };
    let dark = caps.dark_background != Some(false);
    let contrast = if dark { [0xff; 3] } else { [0; 3] };
    let text = rgb(one.foreground)
        .or_else(|| rgb(caps.foreground))
        .unwrap_or(contrast);
    // A palette colour the terminal did not report is xterm's, a guess.
    let exact = |color: Option<Color>| match color?.rgb(&caps.palette)? {
        Color::Rgb(red, green, blue) => Some([red, green, blue]),
        Color::Palette(_) => None,
    };
    let page =
        rgb(one.background.or(caps.background)).unwrap_or(if dark { [0; 3] } else { [0xff; 3] });
    let muted = rgb(one.muted).unwrap_or_else(|| mix(text, page, 0.6));
    let border = mix(text, page, 0.25);
    let headings = [1, 2, 3, 4, 5, 6].map(|level| rgb(one.heading(level).color).unwrap_or(text));
    let chip = match one.inline_code.background {
        Some(color) => rgb(Some(color)),
        None => None,
    };
    // An alert is a tint of its colour over the page, and inline code in it
    // a stronger one, when the theme has tints. GitHub's colours on a light
    // page have its own pale backgrounds, which a plain mix with the page
    // does not match: dark yellow and white make beige, not pale yellow.
    // They tint the page as a colour filter does, so on white they are
    // GitHub's, and the stronger one is that tint applied twice.
    let alert = |kind| {
        let mut color = rgb(one.alerts.color(kind, dark)).unwrap_or(text);
        let mut bar = color;
        if let Some((gold, title)) = one.alerts.github_gold(kind, dark) {
            (bar, color) = (
                rgb(Some(gold)).unwrap_or(bar),
                rgb(Some(title)).unwrap_or(color),
            );
        }
        let pale = one.alerts.github_background(kind, dark);
        let tint = |strong: bool| {
            one.alerts.tint.then(|| match pale {
                Some(pale) if strong => filter(filter(page, pale), pale),
                Some(pale) => filter(page, pale),
                None => mix(color, page, if strong { 0.3 } else { 0.15 }),
            })
        };
        AlertLook {
            color,
            bar,
            tint: tint(false),
            chip: tint(true).filter(|_| chip.is_some()),
        }
    };
    // Marked text is black on yellow, or without colours the page's
    // colour on the text's, as a search's marks are.
    let marked = if caps.color {
        Marked {
            foreground: rgb(one.mark.foreground),
            background: rgb(one.mark.background),
        }
    } else {
        Marked {
            foreground: Some(page),
            background: Some(text),
        }
    };
    PageLook {
        spacing: one.pixels,
        marked,
        size: (one.pixels.font_size * f32::from(cell_height)).round(),
        text,
        headings,
        link: rgb(one.link).unwrap_or(text),
        code: rgb(one.inline_code.color).unwrap_or(text),
        chip,
        quote: muted,
        border: rgb(one.rule).unwrap_or(border),
        marker: rgb(one.list_marker).unwrap_or(text),
        code_block: rgb(code.background),
        code_text: rgb(code.text).unwrap_or(text),
        label: rgb(code.label),
        error: rgb(one.error).unwrap_or([0xf8, 0x51, 0x49]),
        table_border: rgb(one.table_border).unwrap_or(border),
        table_style: match one.table_style {
            TableStyle::Frame => PageTableStyle::Frame,
            TableStyle::Grid => PageTableStyle::Grid,
        },
        stripe: stripes.then(|| mix(text, page, one.pixels.stripe)),
        page,
        // The terminal's own background, which may be translucent or an
        // image, shows through a page whose theme has none.
        opaque: exact(one.background).is_some(),
        alerts: [
            AlertKind::Note,
            AlertKind::Tip,
            AlertKind::Important,
            AlertKind::Warning,
            AlertKind::Caution,
        ]
        .map(alert),
    }
}

/// `page` seen through a filter of colour `pale`: each channel scaled by
/// the filter's share of full. White stays `pale`.
fn filter(page: Brush, pale: Brush) -> Brush {
    let channel =
        |index: usize| (f32::from(page[index]) * f32::from(pale[index]) / 255.0).round() as u8;
    [channel(0), channel(1), channel(2)]
}

/// `share` of `color` over `page`.
fn mix(color: Brush, page: Brush, share: f32) -> Brush {
    let channel = |index: usize| {
        let (a, b) = (f32::from(color[index]), f32::from(page[index]));
        (b + (a - b) * share).round() as u8
    };
    [channel(0), channel(1), channel(2)]
}
