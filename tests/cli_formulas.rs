//! The kiri binary end to end with formulas.

mod common;

use common::{Scratch, kiri, sent, text, words};

const FORMULAS: &str = "# Sums\n\n```math\n\\sum_{i=1}^n i\n```\n\nAnd\n\n$$\\frac{a}{b}$$\n";
const BROKEN: &str = "# Sums\n\n$$\n\\frac{a\n$$\n";

/// What kiri tells about `BROKEN`'s formula.
const REASON: &str = "ParseError at position 7: Unexpected end of input in a macro argument";

#[test]
fn draws_formula_blocks_as_images() {
    let dir = Scratch::new("formulas");
    let out = kiri(&dir, FORMULAS, &["--mode", "kitty"]);
    assert!(out.status.success());
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    assert_eq!(sent(&stdout).len(), 2, "{stdout:?}");
    assert!(!stdout.contains("\\sum"), "{stdout:?}");
    assert!(!stdout.contains("$$"), "{stdout:?}");
}

#[test]
fn a_formula_that_does_not_draw_stays_source_with_a_warning() {
    let dir = Scratch::new("formula-broken");
    let out = kiri(&dir, BROKEN, &["--mode", "kitty"]);
    assert!(out.status.success());
    assert_eq!(
        text(&out.stderr),
        format!("kiri: formula on line 3: {REASON}\n")
    );
    let shown = words(&common::strip_own_sequences(&text(&out.stdout)));
    assert!(shown.contains("\\frac{a"), "{shown:?}");
    let inline = format!("✗ formula could not be drawn: {REASON}");
    assert!(shown.contains(&inline), "{shown:?}");
    // --render-errors chooses where the reason goes.
    let out = kiri(
        &dir,
        BROKEN,
        &["--mode", "kitty", "--render-errors", "stderr"],
    );
    let shown = words(&common::strip_own_sequences(&text(&out.stdout)));
    assert!(!shown.contains(&inline), "{shown:?}");
    assert_eq!(
        text(&out.stderr),
        format!("kiri: formula on line 3: {REASON}\n")
    );
    let out = kiri(
        &dir,
        BROKEN,
        &["--mode", "kitty", "--render-errors", "inline"],
    );
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn formulas_are_code_without_images_or_diagrams() {
    let dir = Scratch::new("formulas-off");
    for args in [
        &["--mode", "kitty", "--no-images"][..],
        &["--mode", "kitty", "--diagrams", "off"],
        &["--mode", "ansi"],
    ] {
        let out = kiri(&dir, BROKEN, args);
        assert!(out.status.success());
        assert_eq!(text(&out.stderr), "", "{args:?}");
        let out = kiri(&dir, FORMULAS, args);
        let stdout = text(&out.stdout);
        assert!(sent(&stdout).is_empty(), "{args:?}: {stdout:?}");
        assert!(stdout.contains("\\sum_{i=1}^n i"), "{args:?}: {stdout:?}");
        assert!(stdout.contains("\\frac{a}{b}"), "{args:?}: {stdout:?}");
    }
}

/// The colours of the pixels of `rgba` that are fully opaque.
fn opaque_colours(rgba: &[u8]) -> Vec<[u8; 3]> {
    let mut colours: Vec<[u8; 3]> = (rgba.chunks(4))
        .filter(|pixel| pixel[3] == 255)
        .map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect();
    colours.sort();
    colours.dedup();
    colours
}

#[test]
fn a_formula_is_in_the_colour_of_the_text() {
    let dir = Scratch::new("formula-colour");
    let theme = dir.join("theme.toml");
    std::fs::write(&theme, "foreground = \"#123456\"\n").unwrap();
    let theme = theme.to_str().unwrap();
    for (args, colour) in [
        // The paper themes' text.
        (&["--light"][..], [0x1f, 0x1f, 0x1f]),
        (&["--dark"], [0xd4, 0xd4, 0xd4]),
        (&["--theme", theme], [0x12, 0x34, 0x56]),
        // Nothing tells the text's colour: white, as on an unknown page,
        // which counts as dark.
        (&[], [0xff, 0xff, 0xff]),
    ] {
        let args = [&["--mode", "kitty"][..], args].concat();
        let out = kiri(&dir, "$$\\rule{2em}{1em}$$\n", &args);
        assert_eq!(text(&out.stderr), "", "{args:?}");
        let images = sent(&text(&out.stdout));
        assert_eq!(images.len(), 1, "{args:?}");
        assert_eq!(opaque_colours(&images[0].rgba), [colour], "{args:?}");
    }
}
