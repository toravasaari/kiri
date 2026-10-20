//! Diagrams in code blocks: drawn as images where they can be, and code
//! otherwise.

use kiri::caps::Capabilities;
use kiri::images::Images;

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
