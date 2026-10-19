//! The HTML tags kiri reads besides `<img>`: `<details>` with its
//! `<summary>`, `<kbd>`, `<mark>` and `<br>`.

use kiri::term::Capabilities;

mod common;

use common::{WIDTH, render_for, visible};

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
