use std::path::Path;

use kiri::term::Capabilities;
use kiri::theme::Theme;

const WIDTH: u16 = 48;

fn render(fixture: &str, caps: &Capabilities) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let source = std::fs::read_to_string(&path).expect("fixture is readable");
    let document = kiri::parse::parse(&source);
    let mut out = Vec::new();
    kiri::render::render(&document, caps, &Theme::default(), &mut out)
        .expect("rendering to memory succeeds");
    String::from_utf8(out).expect("output is UTF-8")
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
