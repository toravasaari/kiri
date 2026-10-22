//! Front matter: the metadata block at the start of a document.

use kiri::caps::Capabilities;
use kiri::render::{FrontMatterStyle, Inputs};

mod common;

use common::{WIDTH, fixture, render, visible};

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
fn values_past_the_third_level_look_like_yaml_code() {
    let source = "---\na:\n  b:\n    c:\n      d: 1\n      e: [x]\n---\n";
    let inputs = styled(Capabilities::kitty(WIDTH), FrontMatterStyle::Table);
    let out = visible(&render(source, &inputs));
    // The key `d` in the code theme's key colour on its background, as in
    // a fenced ```yaml block, which has the same colours.
    let code = visible(&render("```yaml\nd: 1\n```\n", &inputs));
    let key = "␛[0;38;2;205;130;137;48;2;43;48;59md";
    assert!(code.contains(key), "{code}");
    assert!(out.contains(key), "{out}");
    // The background fills the cell to its border.
    let line = out.lines().find(|line| line.contains("me␛")).unwrap();
    assert!(
        line.contains(":␛[0;48;2;43;48;59m   ␛[0m ␛[0;90m│"),
        "{line}"
    );
    // Without colour it is plain text, indented as written.
    let plain = styled(Capabilities::plain(WIDTH), FrontMatterStyle::Table);
    let out = render(source, &plain);
    assert!(out.contains("│ d: 1  │"), "{out}");
    assert!(out.contains("│ e:    │"), "{out}");
    assert!(out.contains("│   - x │"), "{out}");
}
