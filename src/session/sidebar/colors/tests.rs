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
    caps.palette[8] = rgb(100, 100, 100);
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
    // The mark's yellow of the default theme, which a palette tells.
    assert_eq!(bar.selected, rgb(130, 177, 255), "the link's colour");
    assert_eq!(bar.border, rgb(48, 53, 63), "the rule's colour");
}

#[test]
fn the_current_row_is_the_marks_colour_and_the_page_mixed_with_it() {
    let mut caps = Capabilities::kitty(120);
    caps.palette[3] = rgb(232, 208, 106);
    let bar = derive(&dark(), &caps).unwrap();
    // The mark's own yellow, still the palette's number, as the default
    // theme has it.
    assert_eq!(bar.current, Some(Color::Palette(3)));
    // 15 % of the way from the bar's background to that yellow.
    assert_eq!(bar.current_background, rgb(59, 58, 47));
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
    assert_eq!(bar.current, Some(Color::Palette(3)));
    assert_eq!(bar.current_background, rgb(53, 47, 23));
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
    assert_eq!(bar.current, Some(Color::Palette(3)));
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
    assert_eq!(bar.current_background, rgb(49, 47, 36));
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
