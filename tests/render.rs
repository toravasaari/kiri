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
fn kitty_headings_underlined() {
    let theme = example_theme("underline.toml");
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
fn text_indent_moves_everything_but_headings() {
    let theme = kiri::theme::parse("text_indent = 2\n").unwrap();
    insta::assert_snapshot!(render_with(
        "elements.md",
        &Capabilities::plain(WIDTH),
        &theme
    ));
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
