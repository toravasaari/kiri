use super::*;
use crate::typeset::theme::{ContentsColor, Paper, parse};

const fn rgb(r: u8, g: u8, b: u8) -> Option<Color> {
    Some(Color::Rgb(r, g, b))
}

/// A dark theme with colours of its own throughout, like the demo's.
fn dark() -> Theme {
    parse(
        "background = \"#111318\"\nforeground = \"#d9dce3\"\nmuted = \"#8b919d\"\n\
         link = \"#82b1ff\"\nrule = \"#30353f\"\n[headings]\ncolor = \"#82b1ff\"\n",
    )
    .unwrap()
}

/// A terminal with colours, and the yellow and the grey a theme without
/// colours of its own would draw with.
fn colored() -> Capabilities {
    let mut caps = Capabilities::kitty(120);
    caps.background = rgb(16, 16, 16);
    caps.foreground = rgb(200, 200, 200);
    caps.palette[3] = rgb(200, 160, 0);
    caps.palette[8] = rgb(120, 120, 120);
    caps
}

#[test]
fn a_dark_theme_with_colours_of_its_own_gets_a_bar_from_them() {
    let bar = derive(&dark(), &Capabilities::kitty(120)).unwrap();
    // The page's #111318 moved 6 % of the way to the text's #d9dce3.
    assert_eq!(bar.background, rgb(29, 31, 36));
    assert_eq!(bar.title, rgb(130, 177, 255), "the headings' colour");
    // Halfway between the text and the muted colour.
    assert_eq!(bar.text, rgb(178, 183, 192));
    assert_eq!(bar.glyph, rgb(139, 145, 157), "the muted colour");
    assert_eq!(bar.selected, rgb(130, 177, 255), "the link's colour");
    // The rule's #30353f hardly shows on the bar, so it is made lighter.
    let border = bar.border.unwrap();
    assert!(border.contrast(bar.background.unwrap()).unwrap() >= 3.0);
    assert_eq!(bar.border, rgb(96, 106, 125));
}

#[test]
fn the_current_row_is_the_inline_code_colour_and_the_bar_mixed_with_it() {
    let mut caps = Capabilities::kitty(120);
    caps.palette[3] = rgb(232, 208, 106);
    let bar = derive(&dark(), &caps).unwrap();
    // The default theme's inline code, still the palette's number.
    assert_eq!(bar.current, Some(Color::Palette(5)));
    // 15 % of the way from the bar's background to the violet, which the
    // terminal did not tell: xterm's (205, 0, 205).
    assert_eq!(bar.current_background, rgb(55, 26, 61));
}

#[test]
fn a_theme_without_colours_of_its_own_gets_the_terminals() {
    let bar = derive(&Theme::default(), &colored()).unwrap();
    assert_eq!(bar.background, rgb(27, 27, 27));
    assert_eq!(bar.title, Some(Color::Palette(4)), "the h1's blue");
    // The text and a grey made from it and the page, where the theme has
    // no muted colour and kiri dims.
    assert_eq!(bar.text, rgb(163, 163, 163));
    assert_eq!(bar.glyph, rgb(126, 126, 126));
    assert_eq!(bar.current, Some(Color::Palette(5)));
    assert_eq!(bar.current_background, rgb(54, 23, 54));
    assert_eq!(bar.selected, Some(Color::Palette(4)), "the link's blue");
    assert_eq!(bar.border, Some(Color::Palette(8)), "the rule's grey");
}

#[test]
fn where_the_terminal_does_not_tell_its_colours_the_bar_has_no_background_of_its_own() {
    let bar = derive(&Theme::default(), &Capabilities::kitty(120)).unwrap();
    assert_eq!(bar.background, None);
    assert_eq!(bar.current_background, None);
    assert_eq!(bar.text, None, "the terminal's own text colour");
    // What needs no mixing is still coloured.
    assert_eq!(bar.title, Some(Color::Palette(4)));
    assert_eq!(bar.current, Some(Color::Palette(5)));
    assert_eq!(bar.selected, Some(Color::Palette(4)));
}

#[test]
fn a_light_theme_has_a_light_bar_and_a_current_mark_that_shows_on_it() {
    let theme = Theme::paper(Paper::Light);
    let bar = derive(&theme, &Capabilities::kitty(120)).unwrap();
    // White moved 6 % of the way to the text's #1f1f1f.
    assert_eq!(bar.background, rgb(242, 242, 242));
    assert_eq!(bar.title, rgb(0, 0, 0));
    assert_eq!(bar.selected, rgb(0, 0, 0));
    // The mark's #dcdcdc is hardly there on that: the edge is darker.
    let (edge, page) = (bar.current.unwrap(), bar.background.unwrap());
    assert!(edge.contrast(page).unwrap() >= 3.0, "{edge:?} on {page:?}");
    let band = bar.current_background.unwrap();
    assert!(band.contrast(page).unwrap() < 1.5, "{band:?} on {page:?}");
}

#[test]
fn the_dark_paper_theme_has_a_dark_bar() {
    let bar = derive(&Theme::paper(Paper::Dark), &Capabilities::kitty(120)).unwrap();
    // #1e1e1e 6 % of the way to #d4d4d4.
    assert_eq!(bar.background, rgb(41, 41, 41));
    assert_eq!(bar.title, rgb(255, 255, 255));
}

#[test]
fn a_key_the_theme_sets_is_used_as_it_is_and_the_others_are_derived() {
    let mut theme = dark();
    theme.contents.title = ContentsColor::Color(Some(Color::Palette(1)));
    theme.contents.background = ContentsColor::Color(None);
    theme.contents.border = ContentsColor::Color(rgb(1, 2, 3));
    let mut caps = Capabilities::kitty(120);
    caps.palette[3] = rgb(232, 208, 106);
    let bar = derive(&theme, &caps).unwrap();
    assert_eq!(bar.title, Some(Color::Palette(1)));
    assert_eq!(bar.background, None, "none is the terminal's own");
    assert_eq!(bar.border, rgb(1, 2, 3));
    // The others come from the rest of the theme as before.
    assert_eq!(bar.text, rgb(178, 183, 192));
    assert_eq!(bar.selected, rgb(130, 177, 255));
    // The current row's band is mixed into the page's #111318, as the bar
    // has no colour of its own.
    assert_eq!(bar.current_background, rgb(45, 16, 51));
    // auto asks for the derived colour too.
    theme.contents.title = ContentsColor::Auto;
    assert_eq!(derive(&theme, &caps).unwrap().title, rgb(130, 177, 255));
}

#[test]
fn without_colours_there_is_no_bar_colour() {
    let mut caps = colored();
    caps.color = false;
    assert_eq!(derive(&dark(), &caps), None);
    assert_eq!(derive(&Theme::default().without_colors(), &caps), None);
}

#[test]
fn a_theme_with_no_colours_of_its_own_still_gets_a_coloured_bar_where_the_terminal_has_them() {
    let theme = Theme::default().without_colors();
    let bar = derive(&theme, &colored()).unwrap();
    assert_eq!(bar.background, rgb(27, 27, 27));
    // Nothing of its own to take: the terminal's blue stands in.
    assert_eq!(bar.selected, Some(Color::Palette(4)));
    assert_eq!(bar.current, Some(Color::Palette(4)));
    assert_eq!(bar.title, None, "the text's own colour");
}

/// `theme` with the TOML `toml` of a theme file on top.
fn with(toml: &str) -> Theme {
    parse(toml).unwrap()
}

#[test]
fn the_edge_is_the_inline_code_colour_and_the_band_is_mixed_towards_it() {
    let theme =
        with("inline_code = \"#f0b67a\"\nbackground = \"#111318\"\nforeground = \"#d9dce3\"\n");
    let bar = derive(&theme, &Capabilities::kitty(120)).unwrap();
    assert_eq!(bar.current, rgb(240, 182, 122));
    // 15 % of the way from the bar's #1d1f24 to the orange.
    assert_eq!(bar.current_background, rgb(61, 54, 49));
}

#[test]
fn the_default_theme_s_edge_is_its_violet_and_the_band_a_tint_of_it() {
    let bar = derive(&Theme::default(), &colored()).unwrap();
    assert_eq!(bar.current, Some(Color::Palette(5)));
    // The palette's violet is not told: xterm's (205, 0, 205) is mixed.
    assert_eq!(bar.current_background, rgb(54, 23, 54));
}

#[test]
fn without_an_inline_code_colour_the_edge_is_the_marks_as_before() {
    let theme = with("inline_code = \"none\"\n");
    let bar = derive(&theme, &colored()).unwrap();
    assert_eq!(bar.current, Some(Color::Palette(3)));
    assert_eq!(bar.current_background, rgb(53, 47, 23));
}

#[test]
fn a_current_colour_the_theme_sets_tints_the_band() {
    let mut theme = dark();
    theme.contents.current = ContentsColor::Color(rgb(255, 136, 0));
    let bar = derive(&theme, &Capabilities::kitty(120)).unwrap();
    assert_eq!(bar.current, rgb(255, 136, 0));
    assert_eq!(bar.current_background, rgb(63, 47, 31));
}

#[test]
fn with_no_current_colour_the_band_is_still_tinted_by_the_derived_one() {
    let mut theme = dark();
    theme.contents.current = ContentsColor::Color(None);
    let bar = derive(&theme, &Capabilities::kitty(120)).unwrap();
    assert_eq!(bar.current, None);
    // The default violet, as xterm has it, in the bar #1d1f24.
    assert_eq!(bar.current_background, rgb(55, 26, 61));
}

#[test]
fn the_band_is_mixed_towards_the_edge_as_it_shows_on_a_light_and_a_dark_bar() {
    for (toml, towards_lighter) in [
        (
            "background = \"#ffffff\"\nforeground = \"#1f1f1f\"\ninline_code = \"#e0e0e0\"\n",
            false,
        ),
        (
            "background = \"#1e1e1e\"\nforeground = \"#d4d4d4\"\ninline_code = \"#202020\"\n",
            true,
        ),
    ] {
        let bar = derive(&with(toml), &Capabilities::kitty(120)).unwrap();
        let (edge, ground) = (bar.current.unwrap(), bar.background.unwrap());
        assert!(
            edge.contrast(ground).unwrap() >= 3.0,
            "{edge:?} on {ground:?}"
        );
        assert_eq!(bar.current_background, ground.mix(edge, 0.15), "{toml}");
        // The raw colour, hardly different from the bar, would give less.
        let Color::Rgb(raw, ..) = (if towards_lighter {
            Color::Rgb(0x20, 0, 0)
        } else {
            Color::Rgb(0xe0, 0, 0)
        }) else {
            unreachable!()
        };
        let raw = Color::Rgb(raw, raw, raw);
        assert_ne!(bar.current_background, ground.mix(raw, 0.15), "{toml}");
    }
}

#[test]
fn a_border_too_near_the_page_is_made_visible_and_one_that_shows_is_kept() {
    // Solarized Dark: the palette's bright black is the page's colour.
    let mut caps = Capabilities::kitty(120);
    caps.background = rgb(0x00, 0x2b, 0x36);
    caps.foreground = rgb(0x83, 0x94, 0x96);
    caps.palette[8] = rgb(0x00, 0x2b, 0x36);
    let bar = derive(&Theme::default(), &caps).unwrap();
    let (border, ground) = (bar.border.unwrap(), bar.background.unwrap());
    assert!(
        border.contrast(ground).unwrap() >= 3.0,
        "{border:?} on {ground:?}"
    );
    // A grey that shows stays the palette's own colour.
    caps.palette[8] = rgb(0x93, 0xa1, 0xa1);
    let bar = derive(&Theme::default(), &caps).unwrap();
    assert_eq!(bar.border, Some(Color::Palette(8)));
    // A border the theme sets is used as it is.
    let mut theme = Theme::default();
    theme.contents.border = ContentsColor::Color(Some(Color::Palette(8)));
    caps.palette[8] = rgb(0x00, 0x2b, 0x36);
    assert_eq!(
        derive(&theme, &caps).unwrap().border,
        Some(Color::Palette(8))
    );
}
