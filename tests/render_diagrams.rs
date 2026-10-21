//! Diagrams in code blocks: drawn as images where they can be, and code
//! otherwise.

use image::RgbaImage;
use kiri::caps::Capabilities;
use kiri::images::{DEFAULT_MAX_FILE_SIZE, Fonts, ImageKey, Images, Source};

mod common;

use common::{
    WIDTH, drawn, fixture_images_with, readable, render, render_fixture, sent, stub_diagram_images,
    stub_diagrams, with_images,
};

const FLOWCHART: &str = "Before.\n\n```mermaid\nflowchart TD\n  A --> B\n```\n\nAfter.\n";

#[test]
fn kitty_mermaid() {
    let images = fixture_images_with("mermaid.md", &stub_diagrams());
    insta::assert_snapshot!(readable(&render_fixture(
        "mermaid.md",
        &with_images(&Capabilities::kitty(WIDTH), images)
    )));
}

#[test]
fn a_diagram_is_drawn_in_place_of_its_code() {
    let images = stub_diagram_images(FLOWCHART);
    let out = render(FLOWCHART, &with_images(&Capabilities::kitty(WIDTH), images));
    assert_eq!(sent(&out).len(), 1, "{out:?}");
    // Two lines and 23 characters of source, 330 × 40 CSS pixels, in
    // cells of 10 × 20.
    assert_eq!(drawn(&out), ["⟦row 0: 33 cells⟧", "⟦row 1: 33 cells⟧"]);
    assert!(!out.contains("flowchart"), "{out:?}");
    assert!(out.contains("Before.") && out.contains("After."), "{out:?}");
}

#[test]
fn a_diagram_that_did_not_draw_stays_code() {
    let source = "```mermaid\nflowchart TD\n  A[open --> B\n```\n";
    let images = stub_diagram_images(source);
    let out = render(source, &with_images(&Capabilities::kitty(WIDTH), images));
    assert!(sent(&out).is_empty(), "{out:?}");
    assert!(out.contains("A[open --> B"), "{out:?}");
}

#[test]
fn diagrams_stay_code_without_images_or_graphics() {
    // No images loaded, as with --no-images.
    let out = render(
        FLOWCHART,
        &with_images(&Capabilities::kitty(WIDTH), Images::new()),
    );
    assert!(sent(&out).is_empty(), "{out:?}");
    assert!(out.contains("flowchart TD"), "{out:?}");
    // Loaded, but the terminal cannot draw them.
    for caps in [Capabilities::ansi(WIDTH), Capabilities::plain(WIDTH)] {
        let images = stub_diagram_images(FLOWCHART);
        let out = render(FLOWCHART, &with_images(&caps, images));
        assert!(drawn(&out).is_empty(), "{out:?}");
        assert!(out.contains("flowchart TD"), "{out:?}");
    }
}

/// What `out` sent of its one image: its size in pixels and in cells,
/// which are whole cells, and how much of it the image covers.
fn placed(out: &str) -> ([String; 4], (u32, u32)) {
    let sent = sent(out);
    assert_eq!(sent.len(), 1, "{out:?}");
    let keys = ["s", "v", "c", "r"].map(|key| sent[0].keys[key].clone());
    let width: u32 = keys[0].parse().unwrap();
    let height = sent[0].rgba.len() as u32 / 4 / width;
    let image = RgbaImage::from_raw(width, height, sent[0].rgba.clone()).unwrap();
    let covered = (image.enumerate_pixels())
        .filter(|(_, _, pixel)| pixel.0[3] > 0)
        .fold((0, 0), |(w, h), (x, y, _)| (w.max(x + 1), h.max(y + 1)));
    (keys, covered)
}

#[test]
fn a_raster_diagram_is_placed_at_the_size_of_its_css_pixels() {
    // As a browser draws it at three pixels to the CSS pixel: 60 × 30
    // pixels are 20 × 10 CSS pixels, as large as such an SVG.
    let dense = Source::Dense {
        image: RgbaImage::from_pixel(60, 30, image::Rgba([0, 0, 255, 255])),
        scale: 3,
    };
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">
        <rect width="20" height="10" fill="#0000ff"/></svg>"##;
    let svg = kiri::images::decode(svg, DEFAULT_MAX_FILE_SIZE, &Fonts::none()).unwrap();
    assert!(matches!(svg, Source::Svg(_)));
    // Cells 40 pixels high, two terminal pixels to the CSS pixel.
    let caps = Capabilities {
        height: Some(24),
        cell_pixels: Some((10, 40)),
        ..Capabilities::kitty(WIDTH)
    };
    let source = "```mermaid\nflowchart TD\n```\n";
    let key = ImageKey::Block {
        language: "mermaid".to_owned(),
        source: "flowchart TD\n".to_owned(),
    };
    let in_document = |image: &Source| {
        let mut images = Images::new();
        images.insert(key.clone(), Ok(image.clone()));
        placed(&render(source, &with_images(&caps, images)))
    };
    let alone = |image: &Source| {
        let mut out = Vec::new();
        kiri::render::render_image(image.clone(), &caps, &mut out).unwrap();
        placed(&String::from_utf8(out).unwrap())
    };
    // 40 × 20 pixels, in a sent image of whole cells.
    let expected = (["40", "40", "4", "1"].map(String::from), (40, 20));
    assert_eq!(in_document(&svg), expected);
    assert_eq!(in_document(&dense), expected);
    assert_eq!(alone(&svg), expected);
    assert_eq!(alone(&dense), expected);
}
