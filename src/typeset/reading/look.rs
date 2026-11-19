//! Reading mode's colours: the one look, resolved into the brushes the page
//! is drawn with.

use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
use crate::typeset::layout::CodeColors;
use crate::typeset::model::AlertKind;
use crate::typeset::reading::page::{
    AlertLook, Look as PageLook, MarkColors, Marked, TableStyle as PageTableStyle,
};
use crate::typeset::reading::text::Brush;
use crate::typeset::theme::{Look, SearchColor, TableStyle};

/// Reading mode's zoom in whole percent when nothing sets it.
pub const DEFAULT_ZOOM: u16 = 100;
/// The smallest and the largest zoom, in whole percent.
pub const ZOOM_RANGE: std::ops::RangeInclusive<u16> = 50..=200;
/// The zooms that stepping with `z` and `Z` lands on.
pub const ZOOM_STEPS: [u16; 10] = [50, 67, 80, 90, 100, 110, 125, 150, 175, 200];

/// The zoom one step from `zoom` towards larger or smaller: the next step
/// in that direction, or `zoom` itself at the end.
pub fn zoom_step(zoom: u16, larger: bool) -> u16 {
    let step = match larger {
        true => ZOOM_STEPS.iter().copied().find(|&step| step > zoom),
        false => ZOOM_STEPS.iter().rev().copied().find(|&step| step < zoom),
    };
    step.unwrap_or(zoom)
}

/// The size of reading mode's text in pixels, on a terminal whose cells are
/// `cell_height` high, at `zoom` percent: a fraction of the cell height,
/// rounded once.
pub fn font_size(one: &Look, cell_height: u16, zoom: u16) -> f32 {
    let size = one.pixels.font_size * f32::from(cell_height);
    // The factor is exactly 1 at 100 %, which leaves the size as it was.
    (size * (f32::from(zoom) / 100.0)).round()
}

/// How reading mode draws text in the look `one` on a terminal with `caps`: its
/// size `size` in pixels (see [`font_size`]), and its colours from the
/// theme, or from the terminal where the theme leaves them. Tables shade
/// every second body row when `stripes` is set.
pub fn page_look(
    one: &Look,
    code: &CodeColors,
    stripes: bool,
    caps: &Capabilities,
    size: f32,
) -> PageLook {
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
        size,
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

/// The colours a search's marks have in reading mode: those the theme
/// gives a match, as the text layout lays them over its text; where the
/// theme gives none, or there is no colour, the reverse, a box in the
/// text's colour with the text in the page's.
pub(crate) fn mark_colors(one: &Look, caps: &Capabilities, look: &PageLook) -> MarkColors {
    let dark = caps.dark_background != Some(false);
    let rgb_of = |color: Option<Color>| match color?.rgb_or_xterm(&caps.palette) {
        Color::Rgb(red, green, blue) => Some([red, green, blue]),
        Color::Palette(_) => None,
    };
    let page = (rgb_of(one.background).or_else(|| rgb_of(caps.background))).unwrap_or(if dark {
        [0; 3]
    } else {
        [0xff; 3]
    });
    let reverse = (Some(page), Some(look.text));
    // `auto` is what a browser's find gives where the text layout has
    // reverse video: black on yellow, and black on orange for the current.
    let colors = |foreground, background, browser: (Brush, Brush)| match (foreground, background) {
        _ if !caps.color => reverse,
        (SearchColor::Auto, SearchColor::Auto) => (Some(browser.0), Some(browser.1)),
        (foreground, background) => {
            let pick = |color| match color {
                SearchColor::Color(color) => rgb_of(color),
                SearchColor::Auto => None,
            };
            (pick(foreground), pick(background))
        }
    };
    let search = &one.search;
    MarkColors {
        found: colors(
            search.match_foreground,
            search.match_background,
            ([0, 0, 0], [0xff, 0xff, 0x00]),
        ),
        current: colors(
            search.current_foreground,
            search.current_background,
            ([0, 0, 0], [0xff, 0x96, 0x32]),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_steps_run_through_the_whole_range_in_order() {
        assert_eq!(ZOOM_STEPS[0], *ZOOM_RANGE.start());
        assert_eq!(ZOOM_STEPS[ZOOM_STEPS.len() - 1], *ZOOM_RANGE.end());
        assert!(ZOOM_STEPS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(ZOOM_STEPS.contains(&DEFAULT_ZOOM));
    }

    #[test]
    fn a_zoom_moves_to_the_next_step_in_the_direction_pressed() {
        let up = |zoom| zoom_step(zoom, true);
        let down = |zoom| zoom_step(zoom, false);
        assert_eq!(up(100), 110);
        assert_eq!(down(100), 90);
        // Between two steps: the next one in the direction pressed.
        assert_eq!((up(120), down(120)), (125, 110));
        assert_eq!((up(55), down(55)), (67, 50));
        assert_eq!((up(199), down(199)), (200, 175));
        // On a step, the one beside it.
        let walked: Vec<u16> = std::iter::successors(Some(50), |&zoom| Some(up(zoom)))
            .take(11)
            .collect();
        assert_eq!(walked, [50, 67, 80, 90, 100, 110, 125, 150, 175, 200, 200]);
        let back: Vec<u16> = std::iter::successors(Some(200), |&zoom| Some(down(zoom)))
            .take(11)
            .collect();
        assert_eq!(back, [200, 175, 150, 125, 110, 100, 90, 80, 67, 50, 50]);
    }
}
