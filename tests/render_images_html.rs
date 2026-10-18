//! Images in HTML `<img>` tags.

use kiri::images::{ImageError, Images};
use kiri::term::Capabilities;

mod common;

use common::{
    WIDTH, drawn, fixture_images, one_image, quadrants, readable, render, render_fixture, sent,
    visible, with_images,
};

#[test]
fn kitty_html_images() {
    insta::assert_snapshot!(readable(&render_fixture(
        "images-html.md",
        &with_images(
            &Capabilities::kitty(WIDTH),
            fixture_images("images-html.md")
        )
    )));
}

#[test]
fn ansi_html_images() {
    insta::assert_snapshot!(visible(&render_fixture(
        "images-html.md",
        &with_images(&Capabilities::ansi(WIDTH), fixture_images("images-html.md"))
    )));
}

#[test]
fn plain_html_images() {
    insta::assert_snapshot!(render_fixture(
        "images-html.md",
        &with_images(
            &Capabilities::plain(WIDTH),
            fixture_images("images-html.md")
        )
    ));
}

#[test]
fn the_fixture_loads_the_images_its_blocks_show() {
    let images = fixture_images("images-html.md");
    let mut urls: Vec<&str> = images.iter().map(|(url, _)| url).collect();
    urls.sort();
    assert_eq!(
        urls,
        [
            "images/icon.png",
            "images/no-such-file.png",
            "images/pattern.png",
            "images/transparent.png",
        ]
    );
    let warnings = images.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].ends_with("images/no-such-file.png: file not found"),
        "{warnings:?}"
    );
}

#[test]
fn an_img_alone_is_drawn() {
    for source in [
        "<img src=\"p.png\" alt=\"alone\">\n",
        "<p align=\"center\">\n  <img src=\"p.png\" alt=\"alone\">\n</p>\n",
        // Over two lines it is inline HTML alone in a paragraph.
        "<img src=\"p.png\"\n  alt=\"alone\">\n",
    ] {
        let images = one_image("p.png", quadrants(40, 30));
        let out = render(source, &with_images(&Capabilities::kitty(WIDTH), images));
        assert_eq!(sent(&out).len(), 1, "{source:?}: {out:?}");
        assert_eq!(
            drawn(&out),
            ["⟦row 0: 4 cells⟧", "⟦row 1: 4 cells⟧"],
            "{source:?}"
        );
        assert!(!out.contains("alone"), "{source:?}: {out:?}");
    }
}

#[test]
fn an_img_in_running_text_is_alt_text() {
    let images = one_image("p.png", quadrants(40, 30));
    let source = "Text <img src=\"p.png\" alt=\"Fish &amp; chips\"> here.\n";
    let out = render(source, &with_images(&Capabilities::kitty(WIDTH), images));
    assert!(sent(&out).is_empty(), "{out:?}");
    assert!(out.contains("[image: Fish & chips]"), "{out:?}");
    assert!(!out.contains("<img"), "{out:?}");
}

#[test]
fn an_img_is_alt_text_without_graphics_or_its_image() {
    let source = "<img src=\"p.png\" alt=\"alone\">\n";
    let loaded = || one_image("p.png", quadrants(40, 30));
    let mut missing = Images::new();
    missing.insert("p.png", Err(ImageError::Missing("p.png".into())));
    let cases = [
        (Capabilities::plain(WIDTH), loaded()),
        (Capabilities::ansi(WIDTH), loaded()),
        (Capabilities::kitty(WIDTH), Images::new()),
        (Capabilities::kitty(WIDTH), missing),
    ];
    for (caps, images) in cases {
        let out = render(source, &with_images(&caps, images));
        assert!(out.contains("[image: alone]"), "{caps:?}: {out:?}");
        assert!(!out.contains("<img"), "{caps:?}: {out:?}");
    }
}

#[test]
fn html_around_more_than_an_img_stays_source() {
    let images = one_image("p.png", quadrants(40, 30));
    let source = "<p>Text and <img src=\"p.png\" alt=\"x\"></p>\n";
    let out = render(source, &with_images(&Capabilities::kitty(WIDTH), images));
    assert!(sent(&out).is_empty(), "{out:?}");
    assert!(
        out.contains("<p>Text and <img src=\"p.png\" alt=\"x\"></p>"),
        "{out:?}"
    );
}

#[test]
fn a_linked_img_is_drawn_but_cannot_be_clicked() {
    for source in [
        "<a href=\"https://example.com\"><img src=\"p.png\" alt=\"linked\"></a>\n",
        "<p align=\"center\">\n  <a href=\"https://example.com\"><img src=\"p.png\"></a>\n</p>\n",
    ] {
        let images = one_image("p.png", quadrants(40, 30));
        let out = render(source, &with_images(&Capabilities::kitty(WIDTH), images));
        assert_eq!(
            drawn(&out),
            ["⟦row 0: 4 cells⟧", "⟦row 1: 4 cells⟧"],
            "{source:?}: {out:?}"
        );
        assert!(!out.contains("\x1b]8;"), "{out:?}");
    }
}

#[test]
fn a_linked_img_in_running_text_is_a_link_with_alt_text() {
    let source = "See <a href=\"https://example.com\"><img src=\"p.png\" alt=\"logo\"></a>.\n";
    let out = render(
        source,
        &with_images(&Capabilities::kitty(WIDTH), Images::new()),
    );
    assert!(out.contains("\x1b]8;;https://example.com\x1b\\"), "{out:?}");
    assert!(out.contains("[image: logo]"), "{out:?}");
    assert!(!out.contains("<a"), "{out:?}");
}
