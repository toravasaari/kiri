//! Paragraphs, lists, quotes, code and the space between blocks.

use kiri::caps::Capabilities;
use kiri::render::Inputs;
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

/// The lines of the code block in `source`, rendered with `theme` for
/// `caps`, without escape sequences.
fn code_lines(source: &str, caps: Capabilities, theme: &Theme) -> Vec<String> {
    let out = strip_own_sequences(&render(source, &inputs(&caps, theme)));
    out.lines().map(str::to_owned).collect()
}

#[test]
fn code_blocks_name_their_language_in_the_top_right_corner() {
    let source = "```rust\nfn main() {}\n```\n";
    for caps in [Capabilities::ansi(WIDTH), Capabilities::kitty(WIDTH)] {
        let lines = code_lines(source, caps, &Theme::default());
        // One cell of padding after the name, as before the code.
        assert!(lines[0].ends_with(" rust "), "{lines:#?}");
        assert!(lines[0].trim_start().starts_with("rust"), "{lines:#?}");
        assert!(lines[1].starts_with("   fn main"), "{lines:#?}");
        // The label takes the padding line; it adds none of its own.
        assert_eq!(lines.len(), 3, "{lines:#?}");
    }
}

#[test]
fn the_language_label_is_left_out_without_padding_colour_or_room() {
    let kitty = Capabilities::kitty(WIDTH);
    let theme = Theme::default();
    let hidden = kiri::theme::parse("show_language_label = false\n").unwrap();
    let no_color = Capabilities {
        color: false,
        ..kitty.clone()
    };
    let cases = [
        (
            "```rust\ncode\n```\n",
            kitty.clone(),
            theme.clone().without_margins(),
        ),
        (
            "```rust\ncode\n```\n",
            Capabilities::plain(WIDTH),
            theme.clone(),
        ),
        ("```rust\ncode\n```\n", no_color, theme.clone()),
        ("```rust\ncode\n```\n", kitty.clone(), hidden),
        // The name and a cell on each side do not fit in the 44-cell box.
        (
            "```rust-and-a-name-too-long-for-the-box-to-hold\ncode\n```\n",
            kitty.clone(),
            theme.clone(),
        ),
    ];
    for (source, caps, theme) in cases {
        let lines = code_lines(source, caps, &theme);
        assert!(
            lines.iter().all(|line| !line.contains("rust")),
            "{lines:#?}"
        );
    }
    // Without a language, the padding line stays empty.
    let lines = code_lines("```\ncode\n```\n", kitty, &theme);
    assert_eq!(lines[0].trim(), "", "{lines:#?}");
}

#[test]
fn the_language_label_fills_the_padding_line_whatever_the_name() {
    let kitty = Capabilities::kitty(WIDTH);
    let theme = Theme::default();
    let block = |language: &str| format!("```{language}\ncode\n```\n");
    // Each of these takes two cells.
    let wide = code_lines(&block("日本語"), kitty.clone(), &theme);
    assert!(wide[0].ends_with(" 日本語 "), "{wide:#?}");
    assert_eq!(
        wide[0].chars().count() + 3,
        wide[1].chars().count(),
        "{wide:#?}"
    );
    // A control character is replaced, as in the rest of the document;
    // code_lines panics on any escape that gets through.
    let control = code_lines(&block("ru\x1bst"), kitty.clone(), &theme);
    assert!(control[0].ends_with(" ru\u{fffd}st "), "{control:#?}");
    assert_eq!(control[0].len() - 2, control[1].len(), "{control:#?}");
    // The 44-cell box holds a 42-cell name with a cell on each side, and
    // no more.
    let fits = code_lines(&block(&"a".repeat(42)), kitty.clone(), &theme);
    assert_eq!(fits[0], format!("   {} ", "a".repeat(42)), "{fits:#?}");
    let too_long = code_lines(&block(&"a".repeat(43)), kitty, &theme);
    assert_eq!(too_long[0].trim(), "", "{too_long:#?}");
}

#[test]
fn the_language_label_takes_its_colour_from_the_code_theme_or_the_theme() {
    let source = "```rust\nfn main() {}\n```\n";
    let kitty = Capabilities::kitty(WIDTH);
    let top = |theme: &Theme| {
        render(source, &inputs(&kitty, theme))
            .lines()
            .next()
            .unwrap()
            .to_owned()
    };
    // Halfway between base16-ocean.dark's text, #c0c5ce, and its
    // background, #2b303b.
    let default = top(&Theme::default());
    assert!(default.contains("38;2;118;123;133;"), "{default:?}");
    let red = top(&kiri::theme::parse("language_label = \"red\"\n").unwrap());
    assert!(red.contains("\x1b[0;31;"), "{red:?}");
    let ansi = Capabilities::ansi(WIDTH);
    let red = kiri::theme::parse("language_label = \"red\"\n").unwrap();
    let out = render(source, &inputs(&ansi, &red));
    let top = out.lines().next().unwrap();
    assert!(top.contains("\x1b[0;31;"), "{top:?}");
}

#[test]
fn kitty_code_blocks() {
    insta::assert_snapshot!(visible(&render_for(
        "code-blocks.md",
        Capabilities::kitty(WIDTH)
    )));
}
