use crate::typeset::driver;
use crate::typeset::images::{ImageKey, Source};

impl Inputs {
    /// `key` as the layout of these inputs makes it: a diagram's key
    /// with the fingerprint of how diagrams are drawn for them, and a
    /// heading's with that of the look of its level.
    pub(crate) fn drawn(&self, key: impl Into<ImageKey>) -> ImageKey {
        let (theme, caps) = (&self.theme, &self.caps);
        let looks = crate::typeset::images::Looks {
            diagram_base: crate::typeset::images::diagram_look_for(theme, caps, self.diagram_panel),
            headings: heading_look(self.headings, false, theme, caps),
            fonts: self.fonts.clone(),
        };
        match key.into() {
            key @ ImageKey::Heading { level, .. } => key.with_look(looks.heading_loading(level)),
            key => key.with_look(looks.diagram_look_loading()),
        }
    }
}

use super::*;

/// Kitty without text sizing, as inside tmux, with cells of 8 × 16
/// pixels.
fn no_text_sizing() -> Capabilities {
    Capabilities {
        text_sizing: false,
        cell_pixels: Some((8, 16)),
        ..Capabilities::kitty(80)
    }
}

/// The colour the layout gives the bullet of a list with `theme`, on a
/// terminal whose bright black is its background.
fn bullet_on_hidden_grey(theme: Theme) -> Option<Color> {
    let background = Color::Rgb(0x19, 0x19, 0x19);
    let mut caps = Capabilities::kitty(80);
    caps.background = Some(background);
    caps.palette[8] = Some(background);
    let inputs = Inputs {
        theme,
        ..Inputs::new(caps)
    };
    let lines = driver::layout(&crate::typeset::parse::parse("- item\n"), &inputs);
    let bullet = (lines[0].spans.iter()).find(|span| span.text.contains('•'));
    bullet.expect("the bullet").style.fg
}

#[test]
fn the_built_in_theme_shows_list_markers_the_palette_would_hide() {
    let bullet = bullet_on_hidden_grey(Theme::default()).expect("a colour");
    let contrast = bullet.contrast(Color::Rgb(0x19, 0x19, 0x19));
    assert!(contrast.is_some_and(|ratio| ratio >= 3.0), "{bullet:?}");
}

#[test]
fn a_chosen_theme_keeps_its_greys() {
    let theme = Theme {
        built_in: false,
        ..Theme::default()
    };
    assert_eq!(bullet_on_hidden_grey(theme), Some(Color::Palette(8)));
}

#[test]
fn heading_images_keep_no_colour_of_the_theme_without_colours() {
    let theme = Theme::paper(crate::typeset::theme::Paper::Light);
    assert!(theme.foreground.is_some());
    let caps = Capabilities {
        color: false,
        ..no_text_sizing()
    };
    let look = heading_look(Headings::Images, false, &theme, &caps).expect("images");
    // Without a colour of its own the text is the terminal's: light on
    // a dark page, as the layouts take it.
    assert_eq!(look.levels[0].color, [0xff; 3]);
}

#[test]
fn auto_uses_images_only_without_text_sizing() {
    let theme = Theme::default();
    let look = heading_look(Headings::Auto, false, &theme, &no_text_sizing());
    let look = look.expect("images without text sizing");
    assert_eq!(look.cell, (8, 16));
    let h1 = theme.heading(1);
    assert_eq!(look.levels[0].rows, h1.size.scale());
    let px = h1.size.value() as f32 * 16.0 / 1.2;
    assert!((look.levels[0].font_px - px).abs() < 0.01, "{look:?}");
    assert_eq!(look.levels[0].bold, h1.bold);
    let kitty = Capabilities::kitty(80);
    assert_eq!(heading_look(Headings::Auto, false, &theme, &kitty), None);
    let images = heading_look(Headings::Images, false, &theme, &kitty);
    assert!(images.is_some());
}

/// The colours, as RGB, that the text layout gives the inline code in
/// a level-1 heading on a terminal with `caps`: its text's and its
/// background's.
fn laid_out_code(caps: &Capabilities) -> (Option<[u8; 3]>, Option<[u8; 3]>) {
    let inputs = Inputs::new(caps.clone());
    let document = crate::typeset::parse::parse("# a `b`\n");
    let lines = driver::layout(&document, &inputs);
    let span = (lines[0].spans.iter()).find(|span| span.text.contains('b'));
    let style = &span.expect("the code").style;
    (rgb(style.fg, caps), rgb(style.bg, caps))
}

#[test]
fn heading_images_draw_code_in_the_colours_the_text_layout_gives_it() {
    let mut caps = no_text_sizing();
    // The terminal's magenta, the default theme's inline code.
    caps.palette[5] = Some(Color::Rgb(0xc0, 0x30, 0xc0));
    let theme = Theme::default();
    let look = heading_look(Headings::Auto, false, &theme, &caps).unwrap();
    let code = look.levels[0].code;
    let (color, background) = laid_out_code(&caps);
    assert_eq!(Some(code.color), color);
    assert!(background.is_some());
    assert_eq!(code.background, background);
    assert!(code.padding);
}

#[test]
fn heading_code_without_a_background_of_its_own_is_padded_on_the_heading_s() {
    let mut theme = Theme::default();
    theme.inline_code_background = crate::typeset::theme::CodeBackground::Color(None);
    theme.headings[0].background = Some(Color::Rgb(0x20, 0x20, 0x30));
    let look = heading_look(Headings::Auto, false, &theme, &no_text_sizing()).unwrap();
    let code = look.levels[0].code;
    // The heading's background shows through; the padding still
    // stands, as the text layout pads it on the heading's.
    assert_eq!(code.background, None);
    assert!(code.padding);
    theme.headings[0].background = None;
    let look = heading_look(Headings::Auto, false, &theme, &no_text_sizing()).unwrap();
    assert!(!look.levels[0].code.padding);
}

#[test]
fn heading_images_take_xterm_s_colours_where_the_terminal_tells_none() {
    let caps = Capabilities {
        palette: [None; 16],
        ..no_text_sizing()
    };
    let look = heading_look(Headings::Auto, false, &Theme::default(), &caps).unwrap();
    // The default theme's blue headings and magenta code.
    let level = look.levels[0];
    assert_eq!(level.color, [0, 0, 0xee]);
    assert_eq!(level.code.color, [0xcd, 0, 0xcd]);
}

#[test]
fn without_colours_heading_code_has_no_background() {
    let caps = Capabilities {
        color: false,
        ..no_text_sizing()
    };
    let look = heading_look(Headings::Auto, false, &Theme::default(), &caps).unwrap();
    let level = look.levels[0];
    assert_eq!(level.code.color, level.color);
    assert_eq!(level.code.background, None);
    assert!(!level.code.padding);
}

#[test]
fn plain_and_sized_never_use_images() {
    let theme = Theme::default();
    for headings in [Headings::Plain, Headings::Sized] {
        let look = heading_look(headings, false, &theme, &no_text_sizing());
        assert_eq!(look, None, "{headings:?}");
    }
}

#[test]
fn no_images_means_no_heading_images() {
    let theme = Theme::default();
    for headings in [Headings::Auto, Headings::Images] {
        let look = heading_look(headings, true, &theme, &no_text_sizing());
        assert_eq!(look, None, "{headings:?}");
    }
}

#[test]
fn without_images_headings_show_as_sized_says() {
    assert_eq!(Headings::Auto.with_images(false), Headings::Sized);
    assert_eq!(Headings::Images.with_images(false), Headings::Sized);
    assert_eq!(Headings::Plain.with_images(false), Headings::Plain);
    assert_eq!(Headings::Images.with_images(true), Headings::Images);
}

#[test]
fn images_need_a_terminal_that_draws_them() {
    let theme = Theme::default();
    let ansi = Capabilities::ansi(80);
    assert_eq!(heading_look(Headings::Images, false, &theme, &ansi), None);
    assert_eq!(heading_look(Headings::Auto, false, &theme, &ansi), None);
}

#[test]
fn heading_images_take_the_heading_colour_or_else_the_text_colour() {
    let mut theme = Theme::default();
    let caps = no_text_sizing();
    theme.headings[0].color = Some(Color::Rgb(9, 8, 7));
    let look = heading_look(Headings::Auto, false, &theme, &caps).unwrap();
    assert_eq!(look.levels[0].color, [9, 8, 7]);
    // A colour of the palette, as the terminal tells it.
    theme.headings[0].color = Some(Color::Palette(4));
    let mut palette = caps.palette;
    palette[4] = Some(Color::Rgb(0, 0, 0xee));
    let caps = Capabilities { palette, ..caps };
    let look = heading_look(Headings::Auto, false, &theme, &caps).unwrap();
    assert_eq!(look.levels[0].color, [0, 0, 0xee]);
    // Without a colour of its own, in the text's: the theme's, or else
    // the terminal's, or else white on a dark page.
    theme
        .headings
        .iter_mut()
        .for_each(|heading| heading.color = None);
    theme.foreground = Some(Color::Rgb(1, 2, 3));
    let look = heading_look(Headings::Auto, false, &theme, &caps).unwrap();
    assert_eq!(look.levels[0].color, [1, 2, 3]);
    theme.foreground = None;
    let caps = Capabilities {
        foreground: None,
        dark_background: Some(true),
        ..caps
    };
    let look = heading_look(Headings::Auto, false, &theme, &caps).unwrap();
    assert_eq!(look.levels[0].color, [0xff, 0xff, 0xff]);
}

#[test]
fn a_diagram_is_laid_out_as_the_picture_that_was_drawn_in_the_same_fonts() {
    use crate::typeset::fonts::Fonts;
    use std::sync::Arc;
    let document = crate::typeset::parse::parse("```mermaid\nflowchart TD\n```\n");
    let inputs = |fonts: Option<Fonts>| Inputs {
        fonts: fonts.map(Arc::new),
        ..Inputs::new(Capabilities::kitty(60))
    };
    let picture = ImageKey::rendered("mermaid", "flowchart TD\n");
    // What a load in the fixture fonts holds.
    let mut inputs_in_fixture = inputs(Some(Fonts::fixture()));
    let key = inputs_in_fixture.drawn(picture);
    inputs_in_fixture.images.insert(
        key.clone(),
        Ok(Source::Raster(image::RgbaImage::new(40, 40))),
    );
    let pictures = |inputs: &Inputs| {
        let lines = driver::layout(&document, inputs);
        lines.iter().filter(|line| line.image.is_some()).count()
    };
    assert!(pictures(&inputs_in_fixture) > 0);
    // The same picture is not the one that was drawn in other fonts.
    let mut other = inputs(Some(Fonts::reading_fixture()));
    other
        .images
        .insert(key, Ok(Source::Raster(image::RgbaImage::new(40, 40))));
    assert_eq!(pictures(&other), 0);
}

#[test]
fn laying_out_a_document_with_pictures_and_code_does_not_load_the_fonts() {
    use crate::typeset::fonts::Fonts;
    use std::sync::Arc;
    let fonts = Arc::new(Fonts::fixture());
    let mut inputs = Inputs {
        fonts: Some(Arc::clone(&fonts)),
        ..Inputs::new(Capabilities::kitty(60))
    };
    inputs.images.insert(
        ImageKey::from("a.png"),
        Ok(Source::Raster(image::RgbaImage::new(40, 40))),
    );
    let source = "![a](a.png)\n\n```rust\nfn main() {}\n```\n\n$x$ in text.\n";
    driver::layout(&crate::typeset::parse::parse(source), &inputs);
    assert!(!fonts.loaded());
}
