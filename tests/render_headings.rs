//! Headings: sizes, rules, backgrounds and indentation.

use image::{Rgba, RgbaImage};
use kiri::caps::Capabilities;
use kiri::images::{ImageError, ImageKey, Images, Source};
use kiri::render::{Headings, Inputs};
use kiri::theme::Theme;

mod common;

use common::{
    WIDTH, drawn, example_theme, fixture, inputs, readable, render, render_fixture,
    strip_own_sequences, visible,
};

#[test]
fn kitty_headings_with_rules() {
    let theme = example_theme("rules.toml");
    insta::assert_snapshot!(visible(&render_fixture(
        "headings.md",
        &inputs(&Capabilities::kitty(WIDTH), &theme)
    )));
}

#[test]
fn kitty_headings_on_a_background() {
    let theme = example_theme("background.toml");
    insta::assert_snapshot!(visible(&render_fixture(
        "headings.md",
        &inputs(&Capabilities::kitty(WIDTH), &theme)
    )));
}

#[test]
fn heading_background_reaches_the_right_edge() {
    use kiri::render::wrap::width;
    let document = kiri::parse::parse("# One\n\n## Two\n");
    // Indented by one cell, and at the edge on an opaque document.
    let unindented = "background = \"#1c1f26\"\n\
        [h1]\nbackground = \"#2b303b\"\n[h2]\nbackground = \"#232830\"\n";
    let themes = [
        example_theme("background.toml"),
        kiri::theme::parse(unindented).unwrap(),
    ];
    // Scales 3 and 2 leave a remainder at most of these widths.
    for theme in &themes {
        for terminal_width in [46, 47, 48, 49] {
            let inputs = Inputs {
                theme: theme.clone(),
                ..Inputs::new(Capabilities::kitty(terminal_width))
            };
            let lines = kiri::render::layout::layout(&document, &inputs);
            for line in lines.iter().filter(|line| line.rows() > 1) {
                let cells = width(&line.lead) + width(&line.spans) + width(&line.trail);
                assert_eq!(cells, usize::from(terminal_width), "{line:?}");
            }
        }
    }
}

#[test]
fn kitty_headings_on_an_opaque_document() {
    let theme = example_theme("opaque.toml");
    insta::assert_snapshot!(visible(&render_fixture(
        "headings.md",
        &inputs(&Capabilities::kitty(WIDTH), &theme)
    )));
}

#[test]
fn rules_span_the_terminal_past_max_width() {
    let theme = kiri::theme::parse("max_width = 20\n[h1]\nrule = true\n").unwrap();
    let out = render(
        "# Title\n\nText\n\n---\n",
        &inputs(&Capabilities::ansi(30), &theme),
    );
    let rules: Vec<usize> = (out.lines())
        .map(|line| line.matches('─').count())
        .filter(|&count| count > 0)
        .collect();
    // Both the heading rule and the thematic break fill the terminal.
    assert_eq!(rules, vec![30, 30]);
}

#[test]
fn nested_headings_are_not_indented() {
    let theme = example_theme("indent.toml");
    let out = render(
        "# Top\n\n> # Quoted\n\n- # Listed\n",
        &inputs(&Capabilities::ansi(40), &theme),
    );
    // The indent stays on the top-level heading only; nested ones start
    // right after the quote bar or the bullet.
    insta::assert_snapshot!(visible(&out));
}

#[test]
fn empty_headings_disappear() {
    let theme = Theme::default();
    let source = "Before\n\n#\n\n##\n\nAfter\n\n> #\n";
    for caps in [
        Capabilities::plain(WIDTH),
        Capabilities::ansi(WIDTH),
        Capabilities::kitty(WIDTH),
    ] {
        let out = render(source, &inputs(&caps, &theme));
        assert_eq!(
            strip_own_sequences(&out),
            "  Before\n\n  After\n",
            "{caps:?}"
        );
        assert!(!out.contains("\x1b]66;"), "{out:?}");
    }
}

#[test]
fn a_heading_that_renders_nothing_disappears() {
    // An empty link shows nothing when links are clickable.
    let source = "Before\n\n# [](https://example.com)\n\nAfter\n";
    let out = render(source, &Inputs::new(Capabilities::kitty(WIDTH)));
    assert_eq!(strip_own_sequences(&out), "  Before\n\n  After\n");
}

#[test]
fn an_empty_heading_keeps_its_rule() {
    let theme = kiri::theme::parse("[h1]\nrule = true\n").unwrap();
    let out = render(
        "Before\n\n#\n\nAfter\n",
        &inputs(&Capabilities::kitty(20), &theme),
    );
    let rule = "─".repeat(20);
    assert_eq!(
        strip_own_sequences(&out),
        format!("  Before\n\n{rule}\n\n  After\n")
    );
}

/// Kitty without text sizing, as inside tmux, with cells of 10 × 20
/// pixels.
fn no_text_sizing(width: u16) -> Capabilities {
    Capabilities {
        text_sizing: false,
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(width)
    }
}

/// A heading drawn as text in `columns` × `rows` cells of 10 × 20 pixels,
/// with its words after the first starting on the columns `breaks`.
fn text_image(columns: u32, rows: u32, breaks: &[u16]) -> Source {
    let pixels = RgbaImage::from_pixel(columns * 10, rows * 20, Rgba([200, 30, 30, 255]));
    Source::Text {
        image: pixels,
        breaks: breaks.to_vec(),
    }
}

/// Inputs for `caps` with `image` as the level-1 heading `text`, and
/// headings shown as `headings` says.
fn heading_inputs(caps: Capabilities, text: &str, image: Source, headings: Headings) -> Inputs {
    let mut images = Images::new();
    images.insert(ImageKey::heading(1, text), Ok(image));
    Inputs {
        images,
        headings,
        ..Inputs::new(caps)
    }
}

#[test]
fn a_heading_without_text_sizing_is_an_image() {
    let inputs = heading_inputs(
        no_text_sizing(80),
        "Kiri",
        text_image(20, 2, &[]),
        Headings::Auto,
    );
    let out = render("# Kiri\n\nText\n", &inputs);
    assert_eq!(drawn(&out), ["⟦row 0: 20 cells⟧", "⟦row 1: 20 cells⟧"]);
    assert!(!out.contains("\x1b]66;"), "{out:?}");
    insta::assert_snapshot!(readable(&out));
}

#[test]
fn a_heading_image_keeps_its_indent_background_and_rule() {
    let mut theme = example_theme("background.toml");
    theme.headings[0].rule = true;
    let inputs = Inputs {
        theme,
        ..heading_inputs(
            no_text_sizing(40),
            "Kiri",
            text_image(20, 2, &[]),
            Headings::Auto,
        )
    };
    let out = render("# Kiri\n\nText\n", &inputs);
    assert_eq!(drawn(&out), ["⟦row 0: 20 cells⟧", "⟦row 1: 20 cells⟧"]);
    insta::assert_snapshot!(readable(&out));
}

#[test]
fn a_long_heading_goes_on_in_bold_text() {
    let inputs = heading_inputs(
        no_text_sizing(80),
        "One two three four",
        text_image(120, 2, &[30, 60, 90]),
        Headings::Auto,
    );
    let out = render("# One two three four\n\nText\n", &inputs);
    assert_eq!(drawn(&out), ["⟦row 0: 60 cells⟧", "⟦row 1: 60 cells⟧"]);
    assert!(visible(&out).contains("three four"), "{out:?}");
    assert!(!visible(&out).contains("two"), "{out:?}");
    insta::assert_snapshot!(readable(&out));
}

#[test]
fn a_first_word_wider_than_the_room_shrinks_the_image() {
    let inputs = heading_inputs(
        no_text_sizing(80),
        "Onetwothree four",
        text_image(120, 2, &[90]),
        Headings::Auto,
    );
    let out = render("# Onetwothree four\n\nText\n", &inputs);
    // 1200 × 40 pixels shrunk to 800 × 27, in two rows of 80 cells.
    assert_eq!(drawn(&out), ["⟦row 0: 80 cells⟧", "⟦row 1: 80 cells⟧"]);
    assert!(!visible(&out).contains("four"), "{out:?}");
}

#[test]
fn a_heading_that_fits_writes_every_cell() {
    let inputs = heading_inputs(
        no_text_sizing(80),
        "One two",
        text_image(30, 2, &[15]),
        Headings::Auto,
    );
    let lines = kiri::render::layout::layout(&kiri::parse::parse("# One two\n"), &inputs);
    let rows: Vec<_> = lines
        .iter()
        .filter_map(|line| line.image.as_ref())
        .collect();
    assert_eq!(rows.len(), 2, "{lines:?}");
    for row in rows {
        assert_eq!(row.columns, None);
        assert_eq!(row.placement.size.columns, 30);
        assert_eq!(row.placement.text.as_deref(), Some("One two"));
    }
}

#[test]
fn a_heading_without_its_image_is_text() {
    let source = &fixture("headings.md");
    let caps = no_text_sizing(80);
    let text = render(source, &Inputs::new(caps.clone()));
    assert!(!text.contains('\u{10EEEE}'));
    // A heading image that failed, as one that no font draws.
    let mut images = Images::new();
    let failed = ImageError::Diagram {
        name: "heading".into(),
        reason: "no font draws it".into(),
    };
    images.insert(ImageKey::heading(1, "Heading level one"), Err(failed));
    let inputs = Inputs {
        images,
        ..Inputs::new(caps)
    };
    assert_eq!(render(source, &inputs), text);
}

#[test]
fn headings_images_value_wins_over_text_sizing() {
    let caps = Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(80)
    };
    let image = || text_image(20, 2, &[]);
    let auto = heading_inputs(caps.clone(), "Kiri", image(), Headings::Auto);
    let out = render("# Kiri\n", &auto);
    assert!(out.contains("\x1b]66;"), "{out:?}");
    assert!(drawn(&out).is_empty(), "{out:?}");
    let images = heading_inputs(caps, "Kiri", image(), Headings::Images);
    let out = render("# Kiri\n", &images);
    assert!(!out.contains("\x1b]66;"), "{out:?}");
    assert_eq!(drawn(&out), ["⟦row 0: 20 cells⟧", "⟦row 1: 20 cells⟧"]);
}

#[test]
fn sized_and_plain_headings_are_never_images() {
    for headings in [Headings::Sized, Headings::Plain] {
        let inputs = heading_inputs(no_text_sizing(80), "Kiri", text_image(20, 2, &[]), headings);
        let out = render("# Kiri\n", &inputs);
        assert!(drawn(&out).is_empty(), "{headings:?}: {out:?}");
    }
}

#[test]
fn a_heading_in_a_pipe_stays_text() {
    for caps in [Capabilities::plain(80), Capabilities::ansi(80)] {
        let inputs = heading_inputs(
            caps.clone(),
            "Kiri",
            text_image(20, 2, &[]),
            Headings::Images,
        );
        let out = render("# Kiri\n", &inputs);
        assert!(drawn(&out).is_empty(), "{out:?}");
        assert!(visible(&out).contains("Kiri"), "{out:?}");
        if !caps.styling {
            assert!(out.starts_with("# Kiri"), "{out:?}");
        }
    }
}
