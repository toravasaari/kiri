//! GitHub alert blocks: `> [!NOTE]` and the other kinds.

use kiri::color::Color;
use kiri::render::Inputs;
use kiri::term::Capabilities;
use kiri::theme::{Paper, Theme};

mod common;

use common::{WIDTH, inputs, render, render_fixture, strip_own_sequences, visible};

const NOTE: &str = "> [!NOTE]\n> Read this.\n";

/// kitty on a black background whose palette blue is #0000ee.
fn kitty() -> Capabilities {
    let mut palette = [None; 16];
    palette[4] = Some(Color::Rgb(0, 0, 0xee));
    Capabilities {
        background: Some(Color::Rgb(0, 0, 0)),
        dark_background: Some(true),
        palette,
        ..Capabilities::kitty(WIDTH)
    }
}

fn text(source: &str, caps: Capabilities, theme: &Theme) -> Vec<String> {
    let out = strip_own_sequences(&render(source, &inputs(&caps, theme)));
    out.lines().map(str::to_owned).collect()
}

#[test]
fn plain_output_shows_the_title_in_a_quote() {
    let lines = text(NOTE, Capabilities::plain(WIDTH), &Theme::default());
    assert_eq!(lines, ["  > Note", "  > Read this."]);
}

#[test]
fn kitty_shows_the_kinds_icon_before_the_title() {
    let lines = text(NOTE, kitty(), &Theme::default());
    // An Octicon from the font kitty bundles; it covers the first space.
    assert!(lines[0].starts_with("  ▎ \u{f449}  Note"), "{lines:#?}");
    assert!(lines[1].starts_with("  ▎ Read this."), "{lines:#?}");
    // The tint fills the block to its right edge.
    assert_eq!(
        lines[0].chars().count(),
        lines[1].chars().count(),
        "{lines:#?}"
    );
    assert_eq!(
        lines[1].chars().count(),
        usize::from(WIDTH) - 2,
        "{lines:#?}"
    );
}

#[test]
fn other_terminals_get_no_icon() {
    let caps = Capabilities {
        background: Some(Color::Rgb(0, 0, 0)),
        ..Capabilities::ansi(WIDTH)
    };
    let lines = text(NOTE, caps, &Theme::default());
    assert!(lines[0].starts_with("  ▎ Note"), "{lines:#?}");
}

#[test]
fn a_title_of_its_own_replaces_the_kinds_name() {
    let lines = text(
        "> [!WARNING] Mind the gap\n> Text.\n",
        kitty(),
        &Theme::default(),
    );
    assert!(
        lines[0].starts_with("  ▎ \u{f421}  Mind the gap"),
        "{lines:#?}"
    );
}

#[test]
fn the_tint_mixes_the_kinds_colour_into_the_background() {
    let out = render(NOTE, &inputs(&kitty(), &Theme::default()));
    // 15 % of GitHub's dark note blue, #4493f8, over black.
    assert!(out.contains("48;2;10;22;37"), "{out:?}");
    // The bar and the title are in the blue itself.
    assert!(out.contains("38;2;68;147;248"), "{out:?}");
}

#[test]
fn inline_code_in_an_alert_takes_a_stronger_tint() {
    let out = render(
        "> [!NOTE]\n> Run `make`.\n",
        &inputs(&kitty(), &Theme::default()),
    );
    // 30 % of #4493f8 over black.
    assert!(out.contains("48;2;20;44;74m\u{a0}make"), "{out:?}");
}

#[test]
fn the_terminal_palette_gives_the_colours_when_the_theme_says_so() {
    let theme = kiri::theme::parse("[alerts]\ncolors = \"terminal\"\n").unwrap();
    let out = render(NOTE, &inputs(&kitty(), &theme));
    assert!(out.contains("\x1b[0;34"), "{out:?}");
    // 15 % of the palette's #0000ee over black.
    assert!(out.contains("48;2;0;0;36"), "{out:?}");
    // A palette the terminal did not tell leaves the tint out.
    let unknown = Capabilities {
        palette: [None; 16],
        ..kitty()
    };
    let out = render(NOTE, &inputs(&unknown, &theme));
    assert!(!out.contains("48;2"), "{out:?}");
}

#[test]
fn no_tint_without_a_known_background_or_when_the_theme_turns_it_off() {
    let unknown = Capabilities {
        background: None,
        ..kitty()
    };
    let out = render(NOTE, &inputs(&unknown, &Theme::default()));
    assert!(!out.contains("48;2"), "{out:?}");
    let off = kiri::theme::parse("[alerts]\nbackground = \"none\"\n").unwrap();
    let out = render(NOTE, &inputs(&kitty(), &off));
    assert!(!out.contains("48;2"), "{out:?}");
    assert!(out.contains("38;2;68;147;248"), "the colour stays: {out:?}");
}

#[test]
fn paper_mixes_the_tint_into_its_own_page() {
    // The light page is white, whatever the terminal's background.
    let caps = Capabilities {
        background: None,
        dark_background: Some(false),
        ..kitty()
    };
    let out = render(NOTE, &inputs(&caps, &Theme::paper(Paper::Light)));
    // 15 % of GitHub's light note blue, #0969da, over white.
    assert!(out.contains("48;2;218;233;249"), "{out:?}");
}

#[test]
fn kitty_alerts() {
    insta::assert_snapshot!(visible(&render_fixture("alerts.md", &Inputs::new(kitty()))));
}
