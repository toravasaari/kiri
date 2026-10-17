//! Headings: sizes, rules, backgrounds and indentation.

use kiri::images::Images;
use kiri::term::Capabilities;
use kiri::theme::Theme;

mod common;

use common::{WIDTH, example_theme, render_source, render_with, strip_own_sequences, visible};

#[test]
fn kitty_headings_with_rules() {
    let theme = example_theme("rules.toml");
    insta::assert_snapshot!(visible(&render_with(
        "headings.md",
        &Capabilities::kitty(WIDTH),
        &theme
    )));
}

#[test]
fn kitty_headings_on_a_background() {
    let theme = example_theme("background.toml");
    insta::assert_snapshot!(visible(&render_with(
        "headings.md",
        &Capabilities::kitty(WIDTH),
        &theme
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
            let caps = Capabilities::kitty(terminal_width);
            let lines = kiri::render::layout::layout(&document, &Images::new(), &caps, theme);
            for line in lines.iter().filter(|line| line.scale > 1) {
                let cells = width(&line.lead)
                    + width(&line.spans) * usize::from(line.scale)
                    + width(&line.trail);
                assert_eq!(cells, usize::from(terminal_width), "{line:?}");
            }
        }
    }
}

#[test]
fn kitty_headings_on_an_opaque_document() {
    let theme = example_theme("opaque.toml");
    insta::assert_snapshot!(visible(&render_with(
        "headings.md",
        &Capabilities::kitty(WIDTH),
        &theme
    )));
}

#[test]
fn rules_span_the_terminal_past_max_width() {
    let theme = kiri::theme::parse("max_width = 20\n[h1]\nrule = true\n").unwrap();
    let document = kiri::parse::parse("# Title\n\nText\n\n---\n");
    let mut out = Vec::new();
    kiri::render::render(
        &document,
        &Images::new(),
        &Capabilities::ansi(30),
        &theme,
        &mut out,
    )
    .unwrap();
    let rules: Vec<usize> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| line.matches('─').count())
        .filter(|&count| count > 0)
        .collect();
    // Both the heading rule and the thematic break fill the terminal.
    assert_eq!(rules, vec![30, 30]);
}

#[test]
fn nested_headings_are_not_indented() {
    let theme = example_theme("indent.toml");
    let document = kiri::parse::parse("# Top\n\n> # Quoted\n\n- # Listed\n");
    let mut out = Vec::new();
    kiri::render::render(
        &document,
        &Images::new(),
        &Capabilities::ansi(40),
        &theme,
        &mut out,
    )
    .unwrap();
    // The indent stays on the top-level heading only; nested ones start
    // right after the quote bar or the bullet.
    insta::assert_snapshot!(visible(&String::from_utf8(out).unwrap()));
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
        let out = render_source(source, &caps, &theme);
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
    let out = render_source(source, &Capabilities::kitty(WIDTH), &Theme::default());
    assert_eq!(strip_own_sequences(&out), "  Before\n\n  After\n");
}

#[test]
fn an_empty_heading_keeps_its_rule() {
    let theme = kiri::theme::parse("[h1]\nrule = true\n").unwrap();
    let out = render_source("Before\n\n#\n\nAfter\n", &Capabilities::kitty(20), &theme);
    let rule = "─".repeat(20);
    assert_eq!(
        strip_own_sequences(&out),
        format!("  Before\n\n{rule}\n\n  After\n")
    );
}
