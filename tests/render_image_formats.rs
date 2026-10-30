//! Images in the formats only the full build decodes.

use kiri::typeset::caps::Capabilities;

mod common;

use common::{WIDTH, fixture_images, render_fixture, with_images};

#[cfg(feature = "full")]
#[test]
fn kitty_image_formats() {
    use common::readable;
    insta::assert_snapshot!(readable(&render_fixture(
        "images-formats.md",
        &with_images(
            &Capabilities::kitty(WIDTH),
            fixture_images("images-formats.md")
        )
    )));
}

#[cfg(not(feature = "full"))]
#[test]
fn a_minimal_build_shows_alt_text_and_says_why() {
    let images = fixture_images("images-formats.md");
    assert_eq!(
        images.warnings(),
        [
            "images/pattern.bmp: BMP images are not supported in this build",
            "images/pattern.tif: TIFF images are not supported in this build",
            "images/pattern.qoi: QOI images are not supported in this build",
            "images/pattern.ppm: PNM images are not supported in this build",
            "images/pattern.tga: TGA images are not supported in this build",
            "images/transparent.ico: ICO images are not supported in this build",
        ]
    );
    let out = render_fixture(
        "images-formats.md",
        &with_images(&Capabilities::kitty(WIDTH), images),
    );
    assert!(!out.contains("\x1b_G"), "{out:?}");
    for alt in ["BMP", "TIFF", "QOI", "PPM", "TGA"] {
        let alt = format!("[image: {alt} pattern]");
        assert!(out.contains(&alt), "{alt}: {out:?}");
    }
    assert!(out.contains("[image: ICO]"), "{out:?}");
}
