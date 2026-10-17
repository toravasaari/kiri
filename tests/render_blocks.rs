//! Paragraphs, lists, quotes, code and the space between blocks.

use kiri::render::Inputs;
use kiri::term::Capabilities;
use kiri::theme::Theme;

mod common;

use common::{
    WIDTH, example_theme, inputs, render, render_fixture, render_for, strip_own_sequences, visible,
};

#[test]
fn plain_basic() {
    insta::assert_snapshot!(render_for("basic.md", Capabilities::plain(WIDTH)));
}

#[test]
fn plain_elements() {
    insta::assert_snapshot!(render_for("elements.md", Capabilities::plain(WIDTH)));
}

#[test]
fn ansi_elements() {
    insta::assert_snapshot!(visible(&render_for(
        "elements.md",
        Capabilities::ansi(WIDTH)
    )));
}

#[test]
fn kitty_elements() {
    insta::assert_snapshot!(visible(&render_for(
        "elements.md",
        Capabilities::kitty(WIDTH)
    )));
}

#[test]
fn plain_nested() {
    insta::assert_snapshot!(render_for("nested.md", Capabilities::plain(WIDTH)));
}

#[test]
fn kitty_nested() {
    insta::assert_snapshot!(visible(&render_for(
        "nested.md",
        Capabilities::kitty(WIDTH)
    )));
}

#[test]
fn plain_nested_narrow() {
    insta::assert_snapshot!(render_for("nested.md", Capabilities::plain(12)));
}

#[test]
fn plain_wide_characters() {
    insta::assert_snapshot!(render_for("wide.md", Capabilities::plain(WIDTH)));
}

#[test]
fn kitty_wide_characters() {
    insta::assert_snapshot!(visible(&render_for("wide.md", Capabilities::kitty(WIDTH))));
}

#[test]
fn plain_wide_characters_narrow() {
    insta::assert_snapshot!(render_for("wide.md", Capabilities::plain(12)));
}

#[test]
fn text_margin_narrows_everything_but_headings() {
    let theme = kiri::theme::parse("text_margin = 2\n").unwrap();
    insta::assert_snapshot!(render_fixture(
        "elements.md",
        &inputs(&Capabilities::plain(WIDTH), &theme)
    ));
}

#[test]
fn empty_blocks_get_no_separator() {
    let plain = Capabilities::plain(WIDTH);
    let theme = Theme::default();
    let source = "```\n```\n\nBefore\n\n```\n```\n\nAfter\n";
    assert_eq!(
        render(source, &inputs(&plain, &theme)),
        "  Before\n\n  After\n"
    );
    let quoted = "> Before\n>\n> ```\n> ```\n>\n> After\n";
    assert_eq!(
        render(quoted, &inputs(&plain, &theme)),
        "  > Before\n  >\n  > After\n"
    );
    // Without padding, a highlighted empty code block has no lines either.
    let ansi = render(
        source,
        &inputs(&Capabilities::ansi(WIDTH), &theme.without_margins()),
    );
    assert_eq!(ansi.matches('\n').count(), 3, "{ansi:?}");
}

#[test]
fn blank_lines_in_quotes_end_at_the_bar() {
    let source = "> a\n>\n> > b\n> >\n> > c\n\n- item\n\n  > d\n  >\n  > e\n";
    for caps in [Capabilities::ansi(WIDTH), Capabilities::kitty(WIDTH)] {
        let text = strip_own_sequences(&render(source, &Inputs::new(caps)));
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.iter().all(|line| !line.ends_with(' ')), "{lines:#?}");
        assert!(lines.contains(&"  │"), "{lines:#?}");
        assert!(lines.contains(&"  │ │"), "{lines:#?}");
        assert!(lines.contains(&"    │"), "{lines:#?}");
    }
    // On an opaque document, the background still reaches the edge.
    let out = render(
        "> a\n>\n> b\n",
        &inputs(&Capabilities::kitty(WIDTH), &example_theme("opaque.toml")),
    );
    let blank = out.lines().nth(1).unwrap();
    assert!(
        blank.ends_with("│\x1b[0;48;2;28;31;38m\x1b[K\x1b[0m"),
        "{blank:?}"
    );
}

#[test]
fn empty_list_items_end_at_the_marker() {
    let source = "- a\n-\n- b\n\n1. a\n2.\n";
    for caps in [Capabilities::ansi(WIDTH), Capabilities::kitty(WIDTH)] {
        let text = strip_own_sequences(&render(source, &Inputs::new(caps)));
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.contains(&"  •"), "{lines:#?}");
        assert!(lines.contains(&"  2."), "{lines:#?}");
    }
}

#[test]
fn tiny_widths_do_not_underflow() {
    // A wide character in a heading scaled to one character per line.
    let theme = kiri::theme::parse("[h1]\nscale = 7\nbackground = \"blue\"\n").unwrap();
    let out = render("# 日本\n", &inputs(&Capabilities::kitty(10), &theme));
    assert!(out.contains('日'), "{out:?}");
    // Quotes nested until a code block's padding is wider than its room.
    let quotes = "> ".repeat(6);
    let source = format!("{quotes}```\n{quotes}code\n{quotes}```\n");
    let out = render(&source, &Inputs::new(Capabilities::plain(10)));
    assert!(out.contains('c'), "{out:?}");
}

#[test]
fn inline_code_keeps_its_spaces() {
    let source = "Run `git  commit   -m` now.\n";
    let theme = Theme::default();
    assert_eq!(
        render(source, &inputs(&Capabilities::plain(WIDTH), &theme)),
        "  Run `git  commit   -m` now.\n"
    );
    let kitty = render(source, &inputs(&Capabilities::kitty(WIDTH), &theme));
    assert!(kitty.contains("\u{a0}git  commit   -m\u{a0}"), "{kitty:?}");
    // Code still wraps at its spaces, and the next line does not start with
    // the ones it broke at.
    assert_eq!(
        render(source, &inputs(&Capabilities::plain(20), &theme)),
        "  Run `git  commit\n  -m` now.\n"
    );
}

#[test]
fn inline_code_wraps_at_its_spaces_unless_the_theme_keeps_it_together() {
    let plain = Capabilities::plain(16);
    let source = "Type `git commit` here.\n";
    assert_eq!(
        render(source, &Inputs::new(plain.clone())),
        "  Type `git\n  commit`\n  here.\n"
    );
    let together = kiri::theme::parse("inline_code_wrap = false\n").unwrap();
    assert_eq!(
        render(source, &inputs(&plain, &together)),
        "  Type\n  `git commit`\n  here.\n"
    );
    // Code longer than a line still breaks at its spaces.
    let source = "Type `git commit --amend` here.\n";
    assert_eq!(
        render(source, &inputs(&plain, &together)),
        "  Type `git\n  commit\n  --amend`\n  here.\n"
    );
}

#[test]
fn no_margins_puts_text_and_code_at_the_edge() {
    let theme = Theme::default().without_margins();
    insta::assert_snapshot!(visible(&render_fixture(
        "elements.md",
        &inputs(&Capabilities::kitty(WIDTH), &theme)
    )));
}
