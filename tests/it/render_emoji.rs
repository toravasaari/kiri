//! Emoji shortcodes, shown as the emoji GitHub would show.

use kiri::typeset::caps::Capabilities;

use crate::common::{WIDTH, render_for, visible};

#[test]
fn plain_emoji() {
    insta::assert_snapshot!(render_for("emoji.md", Capabilities::plain(WIDTH)));
}

#[test]
fn kitty_emoji() {
    insta::assert_snapshot!(visible(&render_for("emoji.md", Capabilities::kitty(WIDTH))));
}
