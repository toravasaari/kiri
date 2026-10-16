use std::path::Path;

fn render_plain(fixture: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let source = std::fs::read_to_string(&path).expect("fixture is readable");
    let document = kiri::parse::parse(&source);
    let mut out = Vec::new();
    kiri::render::plain::render(&document, &mut out).expect("rendering to memory succeeds");
    String::from_utf8(out).expect("plain output is UTF-8")
}

#[test]
fn plain_basic() {
    insta::assert_snapshot!(render_plain("basic.md"));
}
