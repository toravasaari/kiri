//! Images: where they are drawn, their sizes and what is sent.

use image::RgbaImage;
use kiri::images::{DEFAULT_MAX_FILE_SIZE, Fonts, ImageError, Images, Source};
use kiri::kitty::graphics::{DIACRITICS, PLACEHOLDER};
use kiri::render::Inputs;
use kiri::typeset::caps::Capabilities;
use kiri::typeset::theme::{Paper, Theme};

mod common;

use common::{
    WIDTH, colour_codes, drawn, example_theme, fixture_images, one_image, quadrants, readable,
    render, render_fixture, sent, visible, with_images,
};

#[test]
fn kitty_images() {
    insta::assert_snapshot!(readable(&render_fixture(
        "images.md",
        &with_images(&Capabilities::kitty(WIDTH), fixture_images("images.md"))
    )));
}

#[test]
fn ansi_images() {
    insta::assert_snapshot!(visible(&render_fixture(
        "images.md",
        &with_images(&Capabilities::ansi(WIDTH), fixture_images("images.md"))
    )));
}

#[test]
fn plain_images() {
    insta::assert_snapshot!(render_fixture(
        "images.md",
        &with_images(&Capabilities::plain(WIDTH), fixture_images("images.md"))
    ));
}

#[test]
fn images_for_tmux_are_wrapped_on_the_way_out() {
    let caps = Capabilities {
        tmux_passthrough: true,
        ..Capabilities::kitty(WIDTH)
    };
    let inputs = with_images(&caps, one_image("p.png", quadrants(40, 30)));
    let document = kiri::typeset::parse::parse("![alone](p.png)\n");
    let mut out = kiri::term::wrapped(Vec::new(), &caps);
    kiri::render::render(&document, &inputs, &mut out).unwrap();
    let out = String::from_utf8(out.into_inner()).unwrap();
    let graphics = out.matches("_G").count();
    assert!(graphics > 0, "{out:?}");
    // Each graphics sequence, inside the passthrough, has its ESC doubled.
    assert_eq!(
        out.matches("\x1bPtmux;\x1b\x1b_G").count(),
        graphics,
        "{out:?}"
    );
    assert_eq!(out.matches("\x1b\x1b\\\x1b\\").count(), graphics, "{out:?}");
    // Unwrapped, it is what kitty gets.
    let unwrapped = render(
        "![alone](p.png)\n",
        &with_images(&caps, one_image("p.png", quadrants(40, 30))),
    );
    assert_eq!(sent(&unwrapped).len(), 1);
}

#[test]
fn without_tmux_images_go_out_as_they_are() {
    let caps = Capabilities::kitty(WIDTH);
    let inputs = with_images(&caps, one_image("p.png", quadrants(40, 30)));
    let document = kiri::typeset::parse::parse("![alone](p.png)\n");
    let mut out = kiri::term::wrapped(Vec::new(), &caps);
    kiri::render::render(&document, &inputs, &mut out).unwrap();
    let out = String::from_utf8(out.into_inner()).unwrap();
    assert_eq!(out, render("![alone](p.png)\n", &inputs));
}

#[test]
fn an_image_alone_in_a_paragraph_is_drawn() {
    let images = one_image("p.png", quadrants(40, 30));
    let out = render(
        "![alone](p.png)\n",
        &with_images(&Capabilities::kitty(WIDTH), images),
    );
    assert_eq!(sent(&out).len(), 1);
    assert_eq!(drawn(&out), ["⟦row 0: 4 cells⟧", "⟦row 1: 4 cells⟧"]);
    assert!(!out.contains("[image: alone]"), "{out:?}");
}

#[test]
fn a_linked_image_is_drawn_but_cannot_be_clicked() {
    let images = one_image("p.png", quadrants(40, 30));
    let source = "[![linked](p.png)](https://example.com)\n";
    let out = render(source, &with_images(&Capabilities::kitty(WIDTH), images));
    assert_eq!(drawn(&out), ["⟦row 0: 4 cells⟧", "⟦row 1: 4 cells⟧"]);
    assert!(!out.contains("\x1b]8;"), "{out:?}");
}

#[test]
fn images_in_running_text_tables_and_headings_stay_alt_text() {
    let images = one_image("p.png", quadrants(40, 30));
    let source = "Text ![inline](p.png) here.\n\n\
        [![with words](p.png) and words](https://example.com)\n\n\
        | ![cell](p.png) |\n|---|\n| x |\n\n\
        ## ![heading](p.png)\n";
    let out = render(source, &with_images(&Capabilities::kitty(WIDTH), images));
    assert!(sent(&out).is_empty(), "{out:?}");
    assert!(drawn(&out).is_empty(), "{out:?}");
    for alt in ["inline", "with words", "cell", "heading"] {
        assert!(out.contains(&format!("[image: {alt}]")), "{alt}: {out:?}");
    }
}

#[test]
fn images_stay_alt_text_without_graphics_placeholders_or_pixels() {
    let source = "![alone](p.png)\n";
    let loaded = || one_image("p.png", quadrants(40, 30));
    let mut missing = Images::new();
    missing.insert("p.png", Err(ImageError::Missing("p.png".into())));
    let cases = [
        (Capabilities::plain(WIDTH), loaded()),
        (Capabilities::ansi(WIDTH), loaded()),
        (
            Capabilities {
                max_image_cells: 0,
                ..Capabilities::kitty(WIDTH)
            },
            loaded(),
        ),
        (Capabilities::kitty(WIDTH), Images::new()),
        (Capabilities::kitty(WIDTH), missing),
    ];
    for (caps, images) in cases {
        let out = render(source, &with_images(&caps, images));
        assert!(out.contains("[image: alone]"), "{caps:?}: {out:?}");
        assert!(
            !out.contains('\u{10EEEE}') && !out.contains("\x1b_G"),
            "{out:?}"
        );
    }
}

#[test]
fn images_sit_on_the_document_background() {
    let images = one_image("p.png", quadrants(40, 30));
    let out = render(
        "Text\n\n![p](p.png)\n",
        &Inputs {
            images,
            theme: example_theme("opaque.toml"),
            ..Inputs::new(Capabilities::kitty(WIDTH))
        },
    );
    assert_eq!(drawn(&out), ["⟦row 0: 4 cells⟧", "⟦row 1: 4 cells⟧"]);
    // The background of opaque.toml, #1c1f26, behind the placeholders and
    // the rest of each row, where the image is transparent or ends.
    let background = "\x1b[0;48;2;28;31;38m";
    let rows: Vec<&str> = (out.lines())
        .filter(|line| line.contains(PLACEHOLDER))
        .collect();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let painted = row.find(background).expect("background on the row");
        assert!(painted < row.find(PLACEHOLDER).unwrap(), "{row:?}");
        assert!(row.ends_with("\x1b[39m\x1b[K\x1b[0m"), "{row:?}");
    }
}

#[test]
fn text_after_an_image_takes_the_paper_foreground_again() {
    let images = one_image("p.png", quadrants(40, 30));
    let out = render(
        "![p](p.png)\n\nText after the image\n",
        &Inputs {
            images,
            theme: Theme::paper(Paper::Light),
            ..Inputs::new(Capabilities::kitty(WIDTH))
        },
    );
    assert_eq!(drawn(&out), ["⟦row 0: 4 cells⟧", "⟦row 1: 4 cells⟧"]);
    // The placeholders carry the image id in their foreground; the text
    // after them is in the paper's grey #1f1f1f again.
    let line = (out.lines())
        .find(|line| line.contains("Text after"))
        .expect("the text line");
    let before = &line[..line.find("Text after").unwrap()];
    let last = before.rfind("\x1b[").map_or("", |start| &before[start..]);
    assert!(last.contains("38;2;31;31;31"), "{line:?}");
}

#[test]
fn no_color_draws_images_without_other_colours() {
    let caps = Capabilities {
        color: false,
        ..Capabilities::kitty(WIDTH)
    };
    for theme in [Theme::default(), example_theme("opaque.toml")] {
        let out = render(
            "Text\n\n![p](p.png)\n",
            &Inputs {
                images: one_image("p.png", quadrants(40, 30)),
                theme: theme.clone(),
                ..Inputs::new(caps.clone())
            },
        );
        let sent = sent(&out);
        assert_eq!(sent.len(), 1);
        assert_eq!(drawn(&out), ["⟦row 0: 4 cells⟧", "⟦row 1: 4 cells⟧"]);
        // The only colour is the image id that kitty reads from the
        // placeholders' foreground.
        let id: u32 = sent[0].keys["i"].parse().unwrap();
        let [_, red, green, blue] = id.to_be_bytes();
        let id_colour = format!("38;2;{red};{green};{blue}");
        let codes = colour_codes(&out);
        assert_eq!(codes, [id_colour.clone(), id_colour], "{out:?}");
    }
}

#[test]
fn images_indent_with_margins_lists_and_quotes() {
    use kiri::typeset::wrap::width;
    let images = one_image("p.png", quadrants(40, 30));
    let source = "![top](p.png)\n\n- ![listed](p.png)\n\n> ![quoted](p.png)\n";
    let inputs = with_images(&Capabilities::kitty(WIDTH), images);
    let lines = kiri::typeset::layout::layout(&kiri::typeset::parse::parse(source), &inputs);
    let rows: Vec<(String, u16, u16)> = (lines.iter())
        .filter_map(|line| {
            let image = line.image.as_ref()?;
            let before: String = (line.lead.iter().chain(&line.spans))
                .map(|span| span.text.as_str())
                .collect();
            assert_eq!(width(&line.trail), 0);
            Some((before, image.row, image.placement.size.columns))
        })
        .collect();
    let expected = [
        ("  ", 0, 4),
        ("  ", 1, 4),
        ("  • ", 0, 4),
        ("    ", 1, 4),
        ("  │ ", 0, 4),
        ("  │ ", 1, 4),
    ]
    .map(|(before, row, columns)| (before.to_owned(), row, columns));
    assert_eq!(rows, expected);
}

#[test]
fn images_never_reach_past_the_width() {
    use kiri::typeset::wrap::width;
    let images = one_image("p.png", quadrants(2000, 100));
    let source = "![top](p.png)\n\n- ![listed](p.png)\n\n> > ![quoted](p.png)\n";
    let document = kiri::typeset::parse::parse(source);
    let mut inputs = with_images(&Capabilities::kitty(WIDTH), images);
    for terminal_width in [12, 16, 24, 40, 80] {
        inputs.caps = Capabilities::kitty(terminal_width);
        let lines = kiri::typeset::layout::layout(&document, &inputs);
        let image_rows: Vec<_> = lines.iter().filter(|line| line.image.is_some()).collect();
        let first_rows = image_rows
            .iter()
            .filter(|line| line.image.as_ref().unwrap().row == 0);
        assert_eq!(first_rows.count(), 3, "{lines:?}");
        for line in image_rows {
            let columns = line.image.as_ref().unwrap().placement.size.columns;
            let cells = width(&line.lead) + width(&line.spans) + usize::from(columns);
            assert!(cells <= usize::from(terminal_width), "{line:?}");
        }
    }
}

#[test]
fn images_taller_than_the_cell_limit_are_clamped() {
    // 2000 CSS pixels high, a hundred rows at their natural size.
    let tall = || one_image("p.png", quadrants(400, 2000));
    let document = kiri::typeset::parse::parse("![tall](p.png)\n");
    let image_sizes = |caps: Capabilities, images: Images| -> Vec<(u16, u16)> {
        let inputs = with_images(&caps, images);
        let lines = kiri::typeset::layout::layout(&document, &inputs);
        (lines.iter())
            .filter_map(|line| line.image.as_ref())
            .filter(|image| image.row == 0)
            .map(|image| (image.placement.size.columns, image.placement.size.rows))
            .collect()
    };
    let limited = Capabilities {
        max_image_cells: 10,
        ..Capabilities::kitty(WIDTH)
    };
    // Ten rows of 20 pixels, and the width shrinks with them: 40 pixels.
    assert_eq!(image_sizes(limited, tall()), [(4, 10)]);
    assert_eq!(image_sizes(Capabilities::kitty(WIDTH), tall()), [(40, 100)]);
    // kitty's own limit, one row for each placeholder diacritic.
    let taller = one_image("p.png", quadrants(10, 8000));
    let max = DIACRITICS.len() as u16;
    assert_eq!(image_sizes(Capabilities::kitty(WIDTH), taller), [(1, max)]);
    let none = Capabilities {
        max_image_cells: 0,
        ..Capabilities::kitty(WIDTH)
    };
    assert_eq!(image_sizes(none, tall()), []);
}

#[test]
fn an_image_is_sent_once_for_each_size_it_is_shown_at() {
    // As wide as the text at the top level, so it shrinks in the quote.
    let images = one_image("p.png", quadrants(440, 40));
    let source = "![a](p.png)\n\n![again](p.png)\n\n> ![quoted](p.png)\n";
    let out = render(source, &with_images(&Capabilities::kitty(WIDTH), images));
    let sent = sent(&out);
    let sizes: Vec<(&str, &str)> = (sent.iter())
        .map(|image| (image.keys["c"].as_str(), image.keys["r"].as_str()))
        .collect();
    assert_eq!(sizes, [("44", "2"), ("42", "2")]);
    assert_ne!(sent[0].keys["i"], sent[1].keys["i"]);
    assert_eq!(drawn(&out).len(), 6);
}

#[test]
fn sent_pixels_are_the_image_at_its_shown_size() {
    let images = one_image("p.png", quadrants(50, 30));
    // Cells 40 pixels high double the CSS pixels: 100 × 60 pixels in ten
    // columns and two rows, the last 20 pixels of which stay transparent.
    let caps = Capabilities {
        cell_pixels: Some((10, 40)),
        ..Capabilities::kitty(WIDTH)
    };
    let out = render("![p](p.png)\n", &with_images(&caps, images));
    let sent = sent(&out);
    assert_eq!(sent.len(), 1);
    let keys = &sent[0].keys;
    for (key, value) in [
        ("a", "T"),
        ("f", "32"),
        ("o", "z"),
        ("s", "100"),
        ("v", "80"),
        ("c", "10"),
        ("r", "2"),
    ] {
        assert_eq!(keys[key], value, "{key}");
    }
    let image = RgbaImage::from_raw(100, 80, sent[0].rgba.clone()).expect("100 × 80 RGBA");
    assert_eq!(image.get_pixel(25, 15).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(75, 15).0, [0, 255, 0, 255]);
    assert_eq!(image.get_pixel(25, 45).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(75, 45).0, [255, 255, 0, 255]);
    assert_eq!(image.get_pixel(50, 70).0[3], 0);
    assert_eq!(image.get_pixel(99, 79).0[3], 0);
    // The placeholders carry the id as their colour.
    let id: u32 = keys["i"].parse().unwrap();
    let [_, red, green, blue] = id.to_be_bytes();
    let colour = format!("\x1b[38;2;{red};{green};{blue}m{PLACEHOLDER}");
    assert_eq!(out.matches(&colour).count(), 2, "{out:?}");
}

#[test]
fn an_svg_is_drawn_at_its_shown_size_not_scaled() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="30">
        <rect width="25" height="30" fill="#ff0000"/>
        <rect x="25" width="25" height="30" fill="#0000ff"/></svg>"##;
    let source = kiri::images::decode(svg, DEFAULT_MAX_FILE_SIZE, &Fonts::none()).unwrap();
    let mut images = Images::new();
    images.insert("p.svg", Ok(source));
    // Cells 40 pixels high double the CSS pixels: 100 × 60 pixels. Drawn at
    // that size, the edge between the halves stays sharp; a scaled raster
    // would blur it.
    let caps = Capabilities {
        cell_pixels: Some((10, 40)),
        ..Capabilities::kitty(WIDTH)
    };
    let out = render("![p](p.svg)\n", &with_images(&caps, images));
    let sent = sent(&out);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].keys["s"].as_str(), sent[0].keys["v"].as_str()),
        ("100", "80")
    );
    let image = RgbaImage::from_raw(100, 80, sent[0].rgba.clone()).expect("100 × 80 RGBA");
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(49, 30).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(50, 30).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(99, 59).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(50, 60).0[3], 0);
    assert_eq!(drawn(&out), ["⟦row 0: 10 cells⟧", "⟦row 1: 10 cells⟧"]);
}

/// `image` shown on its own with `caps`.
fn alone(image: &Source, caps: &Capabilities) -> String {
    let image = image.clone();
    let mut out = Vec::new();
    kiri::render::render_image(image, caps, &mut out).expect("rendering to memory succeeds");
    String::from_utf8(out).expect("output is UTF-8")
}

/// kitty in a window of `WIDTH` columns and `rows` rows of 10 × 40 pixels.
fn window(rows: Option<u16>) -> Capabilities {
    Capabilities {
        height: rows,
        cell_pixels: Some((10, 40)),
        ..Capabilities::kitty(WIDTH)
    }
}

#[test]
fn an_image_on_its_own() {
    let image = Source::Raster(quadrants(50, 30));
    insta::assert_snapshot!(readable(&alone(&image, &window(Some(24)))));
}

#[test]
fn an_image_on_its_own_is_shown_pixel_for_pixel() {
    // In a document, cells 40 pixels high would double it.
    let image = Source::Raster(quadrants(50, 30));
    let out = alone(&image, &window(Some(24)));
    let sent = sent(&out);
    assert_eq!(sent.len(), 1);
    let keys = &sent[0].keys;
    assert_eq!((keys["s"].as_str(), keys["v"].as_str()), ("50", "40"));
    let image = RgbaImage::from_raw(50, 40, sent[0].rgba.clone()).expect("50 × 40 RGBA");
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(49, 29).0, [255, 255, 0, 255]);
    assert_eq!(image.get_pixel(49, 30).0[3], 0);
    assert_eq!(drawn(&out), ["⟦row 0: 5 cells⟧"]);
}

#[test]
fn an_image_on_its_own_shrinks_to_the_window_leaving_a_row_for_the_prompt() {
    // 480 × 400 pixels of window above the last row: the height decides.
    let image = Source::Raster(quadrants(1000, 2000));
    let out = alone(&image, &window(Some(11)));
    let keys = &sent(&out)[0].keys;
    assert_eq!((keys["s"].as_str(), keys["v"].as_str()), ("200", "400"));
    assert_eq!((keys["c"].as_str(), keys["r"].as_str()), ("20", "10"));
    // A tall window: the width decides.
    let out = alone(&image, &window(Some(100)));
    let keys = &sent(&out)[0].keys;
    assert_eq!((keys["s"].as_str(), keys["v"].as_str()), ("480", "960"));
    assert_eq!((keys["c"].as_str(), keys["r"].as_str()), ("48", "24"));
}

#[test]
fn an_image_on_its_own_shrinks_to_the_width_when_the_height_is_unknown() {
    let image = Source::Raster(quadrants(1000, 200));
    let out = alone(&image, &window(None));
    let keys = &sent(&out)[0].keys;
    assert_eq!((keys["c"].as_str(), keys["r"].as_str()), ("48", "3"));
    // Still no more rows than the placeholders can count.
    let tall = Source::Raster(quadrants(10, 40_000));
    let rows = sent(&alone(&tall, &window(None)))[0].keys["r"].clone();
    assert_eq!(rows, kiri::kitty::graphics::MAX_CELLS.to_string());
}

#[test]
fn an_svg_on_its_own_is_shown_in_css_pixels() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="30">
        <rect width="50" height="30" fill="#ff0000"/></svg>"##;
    let image = kiri::images::decode(svg, DEFAULT_MAX_FILE_SIZE, &Fonts::none()).unwrap();
    let keys = sent(&alone(&image, &window(Some(24))))[0].keys.clone();
    assert_eq!((keys["s"].as_str(), keys["v"].as_str()), ("100", "80"));
}

#[test]
fn an_image_on_its_own_is_not_drawn_without_graphics() {
    let image = Source::Raster(quadrants(50, 30));
    assert_eq!(alone(&image, &Capabilities::ansi(WIDTH)), "");
}
