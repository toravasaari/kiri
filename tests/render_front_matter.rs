//! Front matter: the metadata block at the start of a document.

use kiri::caps::Capabilities;
use kiri::render::{FrontMatterStyle, Inputs};

mod common;

use common::{Scratch, WIDTH, fixture, kiri, render, text, visible};

fn styled(caps: Capabilities, style: FrontMatterStyle) -> Inputs {
    Inputs {
        front_matter: style,
        ..Inputs::new(caps)
    }
}

fn render_with(name: &str, caps: Capabilities, style: FrontMatterStyle) -> String {
    render(&fixture(name), &styled(caps, style))
}

#[test]
fn plain_front_matter_table() {
    insta::assert_snapshot!(render_with(
        "front-matter.md",
        Capabilities::plain(WIDTH),
        FrontMatterStyle::Table
    ));
}

#[test]
fn kitty_front_matter_table() {
    insta::assert_snapshot!(visible(&render_with(
        "front-matter.md",
        Capabilities::kitty(WIDTH),
        FrontMatterStyle::Table
    )));
}

#[test]
fn front_matter_table_in_a_wide_window() {
    insta::assert_snapshot!(render_with(
        "front-matter.md",
        Capabilities::plain(100),
        FrontMatterStyle::Table
    ));
}

#[test]
fn front_matter_table_in_a_narrow_window() {
    insta::assert_snapshot!(render_with(
        "front-matter.md",
        Capabilities::plain(30),
        FrontMatterStyle::Table
    ));
}

#[test]
fn toml_front_matter_table() {
    insta::assert_snapshot!(render_with(
        "front-matter-toml.md",
        Capabilities::plain(WIDTH),
        FrontMatterStyle::Table
    ));
}

#[test]
fn json_front_matter_table() {
    insta::assert_snapshot!(render_with(
        "front-matter-json.md",
        Capabilities::plain(WIDTH),
        FrontMatterStyle::Table
    ));
}

#[test]
fn front_matter_in_another_language_is_a_code_block_in_the_table_style() {
    insta::assert_snapshot!(visible(&render_with(
        "front-matter-php.md",
        Capabilities::kitty(WIDTH),
        FrontMatterStyle::Table
    )));
}

#[test]
fn front_matter_that_does_not_parse_is_a_code_block_in_the_table_style() {
    insta::assert_snapshot!(render_with(
        "front-matter-invalid.md",
        Capabilities::plain(WIDTH),
        FrontMatterStyle::Table
    ));
}

#[test]
fn front_matter_that_is_not_a_map_is_a_code_block_in_the_table_style() {
    let inputs = styled(Capabilities::plain(WIDTH), FrontMatterStyle::Table);
    assert_eq!(
        render("---\n- a\n- b\n---\nBody\n", &inputs),
        render("```yaml\n- a\n- b\n```\n\nBody\n", &inputs),
    );
}

#[test]
fn empty_front_matter_shows_nothing_in_the_table_style() {
    let inputs = styled(Capabilities::kitty(WIDTH), FrontMatterStyle::Table);
    for front_matter in [
        "---\n---\n",
        "---\n\n---\n",
        "---\n# only a comment\n---\n",
        ";;;\n;;;\n",
    ] {
        assert_eq!(
            render(&format!("{front_matter}Body\n"), &inputs),
            render("Body\n", &inputs),
            "{front_matter}"
        );
    }
}

#[test]
fn the_table_style_is_the_default() {
    let caps = Capabilities::plain(WIDTH);
    assert_eq!(
        render(&fixture("front-matter.md"), &Inputs::new(caps.clone())),
        render_with("front-matter.md", caps, FrontMatterStyle::Table)
    );
}

#[test]
fn front_matter_shows_as_a_code_block_in_its_language() {
    for caps in [Capabilities::plain(WIDTH), Capabilities::kitty(WIDTH)] {
        let inputs = styled(caps, FrontMatterStyle::Code);
        for (front_matter, code_block) in [
            ("---\ntitle: A\n---\n", "```yaml\ntitle: A\n```\n"),
            ("+++\ntitle = 'A'\n+++\n", "```toml\ntitle = 'A'\n```\n"),
            (";;;\n{\"a\": 1}\n;;;\n", "```json\n{\"a\": 1}\n```\n"),
            ("---php\n$a = 1;\n---\n", "```php\n$a = 1;\n```\n"),
            ("---\n---\n", "```yaml\n```\n"),
        ] {
            assert_eq!(
                render(&format!("{front_matter}# Body\n"), &inputs),
                render(&format!("{code_block}\n# Body\n"), &inputs),
                "{front_matter}"
            );
        }
    }
}

#[test]
fn kitty_front_matter_code() {
    insta::assert_snapshot!(visible(&render_with(
        "front-matter.md",
        Capabilities::kitty(WIDTH),
        FrontMatterStyle::Code
    )));
}

#[test]
fn hidden_front_matter_shows_nothing() {
    for caps in [Capabilities::plain(WIDTH), Capabilities::kitty(WIDTH)] {
        let source = fixture("front-matter.md");
        let body = source.split_once("\n---\n").unwrap().1;
        assert_eq!(
            render(&source, &styled(caps.clone(), FrontMatterStyle::Hide)),
            render(body, &Inputs::new(caps))
        );
    }
}

#[test]
fn front_matter_that_does_not_parse_is_a_warning() {
    let dir = Scratch::new("front-matter-warning");
    let out = kiri(
        &dir,
        &fixture("front-matter-invalid.md"),
        &["--mode", "plain"],
    );
    assert!(out.status.success());
    assert_eq!(
        text(&out.stderr),
        "kiri: front matter: while parsing a flow sequence, \
         expected ',' or ']' at byte 57 line 3 column 1\n"
    );
    assert!(text(&out.stdout).contains("tags: [does, not, close"));
}
