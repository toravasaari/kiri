//! The colours of the contents bar: the ones the theme's `[contents]`
//! table gives, and for each it leaves out, one derived from the rest of
//! the theme, so that every theme has a coloured bar.

use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
use crate::typeset::theme::{ContentsColor, Theme, VISIBLE};

/// The colours the bar is drawn in, each `None` for the terminal's own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BarColors {
    pub(crate) background: Option<Color>,
    pub(crate) title: Option<Color>,
    pub(crate) text: Option<Color>,
    pub(crate) glyph: Option<Color>,
    pub(crate) current: Option<Color>,
    pub(crate) current_background: Option<Color>,
    pub(crate) selected: Option<Color>,
    pub(crate) border: Option<Color>,
}

/// How far the bar's background is moved from the page's towards the
/// text's, as a share of the way.
const BACKGROUND_SHARE: f32 = 0.06;

/// How far the text of the deeper headings is moved from the page's text
/// colour towards the muted one.
const TEXT_SHARE: f32 = 0.5;

/// How far the dimmed grey of a theme with no muted colour is from the
/// text towards the page, as kiri's dimming leaves it about.
const DIM_SHARE: f32 = 0.4;

/// How far the current row's background is from the bar's background
/// towards the colour of the row's edge.
const CURRENT_SHARE: f32 = 0.2;

/// The bar's colours for `theme` on a terminal with `caps`; `None` where
/// the terminal has no colours, and the bar is plain.
///
/// A key of the theme's `[contents]` table that is a colour, or `none`, is
/// used as it is. One that is left out, or `auto`, is derived:
///
/// - `background`: the page's background, from the theme or else the
///   terminal's, moved a little towards the text's colour (towards white or
///   black where that is not known), so the bar is a shade apart from the
///   page; none where the page's colour is not known.
/// - `title`: the colour of the headings, or the text's own.
/// - `text`: the text's colour mixed halfway with the muted colour, where
///   both are known as RGB; the text's colour otherwise.
/// - `glyph`: the muted colour, or the grey that dimming makes where the
///   theme has none and the page's colours are known.
/// - `current`: the background of `[mark]`, made darker or lighter, in its
///   own hue, until it shows on the bar; or the mark's text, or the link's
///   colour, where the mark has no background.
/// - `current_background`: the bar's background a fifth of the way to
///   `current`; none where either is not known as RGB.
/// - `selected`: the link's colour, or the headings'.
/// - `border`: the colour of rules.
///
/// A palette colour stays a palette colour, so it follows the terminal's
/// scheme, except where it must be mixed: the colours the terminal told
/// are used then, or xterm's.
pub(crate) fn derive(theme: &Theme, caps: &Capabilities) -> Option<BarColors> {
    if !caps.color {
        return None;
    }
    let given = &theme.contents;
    let rgb = |color: Color| color.rgb_or_xterm(&caps.palette);
    let known = |color: Option<Color>| color?.rgb(&caps.palette);
    let page = known(theme.background.or(caps.background));
    let text = known(theme.foreground.or(caps.foreground));

    let background = match given.background {
        ContentsColor::Color(color) => color,
        ContentsColor::Auto => page.and_then(|page| {
            // Towards the text, or towards the end of the scale that the
            // page is not at.
            let (white, black) = (Color::Rgb(255, 255, 255), Color::Rgb(0, 0, 0));
            let dark = page.contrast(white) > page.contrast(black);
            let towards = text.unwrap_or(if dark { white } else { black });
            page.mix(towards, BACKGROUND_SHARE)
        }),
    };
    // What the bar's colours are mixed into: its own background, else the
    // page's.
    let ground = known(background).or(page);

    let title = theme.headings[0].color.or(theme.foreground);
    // The muted colour, or where there is none the text dimmed.
    let muted = theme
        .muted
        .and_then(|muted| known(Some(muted)))
        .or_else(|| text?.mix(page?, DIM_SHARE));
    let current = theme
        .mark
        .background
        .or(theme.mark.foreground)
        .or(theme.link)
        .unwrap_or(Color::Palette(4));
    // The edge shows on the bar: where it can be told, a colour that is
    // too near the bar's is moved away from it, in its own hue.
    let edge = match (known(Some(current)), ground) {
        (Some(color), Some(ground)) => match color.readable_on(ground, VISIBLE) {
            shown if shown == color => current,
            shown => shown,
        },
        _ => current,
    };
    let derived = BarColors {
        background,
        title,
        text: match (text, muted) {
            (Some(text), Some(muted)) => text.mix(muted, TEXT_SHARE),
            _ => theme.foreground,
        },
        glyph: theme.muted.or(muted),
        current: Some(edge),
        current_background: ground.and_then(|ground| ground.mix(rgb(current), CURRENT_SHARE)),
        selected: Some(
            theme
                .link
                .or(theme.headings[0].color)
                .unwrap_or(Color::Palette(4)),
        ),
        border: theme.rule,
    };
    let pick = |given: ContentsColor, derived: Option<Color>| match given {
        ContentsColor::Auto => derived,
        ContentsColor::Color(color) => color,
    };
    Some(BarColors {
        background,
        title: pick(given.title, derived.title),
        text: pick(given.text, derived.text),
        glyph: pick(given.glyph, derived.glyph),
        current: pick(given.current, derived.current),
        current_background: pick(given.current_background, derived.current_background),
        selected: pick(given.selected, derived.selected),
        border: pick(given.border, derived.border),
    })
}

#[cfg(test)]
mod tests;
