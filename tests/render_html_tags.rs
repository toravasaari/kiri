//! The HTML tags kiri reads besides `<img>`: `<details>` with its
//! `<summary>`, `<kbd>`, `<mark>` and `<br>`.

use kiri::term::Capabilities;
use kiri::theme::Theme;

mod common;

use common::{WIDTH, inputs, render, render_for, visible};

#[test]
fn plain_html_tags() {
    insta::assert_snapshot!(render_for("html-tags.md", Capabilities::plain(WIDTH)));
}

#[test]
fn kitty_html_tags() {
    insta::assert_snapshot!(visible(&render_for(
        "html-tags.md",
        Capabilities::kitty(WIDTH)
    )));
}

#[test]
fn marked_text_takes_the_theme_colours() {
    let theme =
        kiri::theme::parse("[mark]\nforeground = \"#101010\"\nbackground = \"#fff8c5\"\n").unwrap();
    let out = render(
        "Some <mark>marked</mark> text\n",
        &inputs(&Capabilities::kitty(WIDTH), &theme),
    );
    assert!(
        out.contains("\x1b[0;38;2;16;16;16;48;2;255;248;197mmarked\x1b[0m"),
        "{out:?}"
    );
}

#[test]
fn without_colours_marked_text_is_in_reverse_video() {
    let caps = Capabilities {
        color: false,
        ..Capabilities::kitty(WIDTH)
    };
    let out = render(
        "Some <mark>marked</mark> text\n",
        &inputs(&caps, &Theme::default()),
    );
    assert!(out.contains("\x1b[0;7mmarked\x1b[0m"), "{out:?}");
}
