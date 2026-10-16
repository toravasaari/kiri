use std::path::Path;

use kiri::term::Capabilities;
use kiri::theme::Theme;

const WIDTH: u16 = 48;

fn render(fixture: &str, caps: &Capabilities) -> String {
    render_with(fixture, caps, &Theme::default())
}

fn render_with(fixture: &str, caps: &Capabilities, theme: &Theme) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let source = std::fs::read_to_string(&path).expect("fixture is readable");
    let document = kiri::parse::parse(&source);
    let mut out = Vec::new();
    kiri::render::render(&document, caps, theme, &mut out).expect("rendering to memory succeeds");
    String::from_utf8(out).expect("output is UTF-8")
}

fn example_theme(name: &str) -> Theme {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/themes")
        .join(name);
    kiri::theme::load(&path).expect("example theme loads")
}

/// Escape characters made visible, so snapshots can be read and diffed.
fn visible(output: &str) -> String {
    output.replace('\x1b', "␛")
}

#[test]
fn plain_basic() {
    insta::assert_snapshot!(render("basic.md", &Capabilities::plain(WIDTH)));
}

#[test]
fn plain_elements() {
    insta::assert_snapshot!(render("elements.md", &Capabilities::plain(WIDTH)));
}

#[test]
fn ansi_elements() {
    insta::assert_snapshot!(visible(&render("elements.md", &Capabilities::ansi(WIDTH))));
}

#[test]
fn kitty_elements() {
    insta::assert_snapshot!(visible(&render("elements.md", &Capabilities::kitty(WIDTH))));
}

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
fn kitty_headings_on_an_opaque_document() {
    let theme = example_theme("opaque.toml");
    insta::assert_snapshot!(visible(&render_with(
        "headings.md",
        &Capabilities::kitty(WIDTH),
        &theme
    )));
}

#[test]
fn text_margin_narrows_everything_but_headings() {
    let theme = kiri::theme::parse("text_margin = 2\n").unwrap();
    insta::assert_snapshot!(render_with(
        "elements.md",
        &Capabilities::plain(WIDTH),
        &theme
    ));
}

#[test]
fn rules_span_the_terminal_past_max_width() {
    let theme = kiri::theme::parse("max_width = 20\n[h1]\nrule = true\n").unwrap();
    let document = kiri::parse::parse("# Title\n\nText\n\n---\n");
    let mut out = Vec::new();
    kiri::render::render(&document, &Capabilities::ansi(30), &theme, &mut out).unwrap();
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
    kiri::render::render(&document, &Capabilities::ansi(40), &theme, &mut out).unwrap();
    // The indent stays on the top-level heading only; nested ones start
    // right after the quote bar or the bullet.
    insta::assert_snapshot!(visible(&String::from_utf8(out).unwrap()));
}

fn render_source(source: &str, caps: &Capabilities, theme: &Theme) -> String {
    let document = kiri::parse::parse(source);
    let mut out = Vec::new();
    kiri::render::render(&document, caps, theme, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn empty_blocks_get_no_separator() {
    let plain = Capabilities::plain(WIDTH);
    let theme = Theme::default();
    let source = "```\n```\n\nBefore\n\n```\n```\n\nAfter\n";
    assert_eq!(
        render_source(source, &plain, &theme),
        "  Before\n\n  After\n"
    );
    let quoted = "> Before\n>\n> ```\n> ```\n>\n> After\n";
    assert_eq!(
        render_source(quoted, &plain, &theme),
        "  > Before\n  >\n  > After\n"
    );
    // Without padding, a highlighted empty code block has no lines either.
    let ansi = render_source(source, &Capabilities::ansi(WIDTH), &theme.without_margins());
    assert_eq!(ansi.matches('\n').count(), 3, "{ansi:?}");
}

#[test]
fn tiny_widths_do_not_underflow() {
    // A wide character in a heading scaled to one character per line.
    let theme = kiri::theme::parse("[h1]\nscale = 7\nbackground = \"blue\"\n").unwrap();
    let out = render_source("# 日本\n", &Capabilities::kitty(10), &theme);
    assert!(out.contains('日'), "{out:?}");
    // Quotes nested until a code block's padding is wider than its room.
    let quotes = "> ".repeat(6);
    let source = format!("{quotes}```\n{quotes}code\n{quotes}```\n");
    let out = render_source(&source, &Capabilities::plain(10), &Theme::default());
    assert!(out.contains('c'), "{out:?}");
}

#[test]
fn no_margins_puts_text_and_code_at_the_edge() {
    let theme = Theme::default().without_margins();
    insta::assert_snapshot!(visible(&render_with(
        "elements.md",
        &Capabilities::kitty(WIDTH),
        &theme
    )));
}

#[test]
fn all_example_themes_load() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/themes");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            kiri::theme::load(&path).unwrap_or_else(|error| panic!("{error}"));
        }
    }
}

#[test]
fn escape_sequences_in_the_document_are_neutralised() {
    let document = kiri::parse::parse("text \x1b]0;evil\x07 here\n");
    let mut out = Vec::new();
    kiri::render::render(
        &document,
        &Capabilities::kitty(WIDTH),
        &Theme::default(),
        &mut out,
    )
    .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(!out.contains("\x1b]0;"), "{out:?}");
}
