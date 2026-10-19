//! The HTML tags kiri reads besides `<img>`: `<details>` with its
//! `<summary>`, `<kbd>`, `<mark>` and `<br>`.

use kiri::caps::Capabilities;
use kiri::theme::Theme;

mod common;

use common::{Scratch, WIDTH, colour_codes, command, inputs, render, render_for, shown, visible};

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
    // The whole way from NO_COLOR, with a theme that colours marks.
    let dir = Scratch::new("mark-without-colours");
    let theme = dir.join("theme.toml");
    std::fs::write(
        &theme,
        "[mark]\nforeground = \"#101010\"\nbackground = \"#fff8c5\"\n",
    )
    .unwrap();
    let document = dir.join("marked.md");
    std::fs::write(&document, "Some <mark>marked</mark> text\n").unwrap();
    let out = command(&dir)
        .env("NO_COLOR", "1")
        .args(["--mode", "kitty", "--theme"])
        .arg(&theme)
        .arg(&document)
        .output()
        .unwrap();
    let out = shown(&out);
    assert!(out.contains("\x1b[0;7mmarked\x1b[0m"), "{out:?}");
    assert!(colour_codes(&out).is_empty(), "{out:?}");
}

#[test]
fn details_are_open_under_a_bold_summary() {
    let source = "<details>\n<summary>Sum <mark>up</mark></summary>\n\nText.\n\n</details>\n";
    let plain = render(
        source,
        &inputs(&Capabilities::plain(WIDTH), &Theme::default()),
    );
    assert_eq!(plain, "  ▼ Sum up\n\n  Text.\n");
    let kitty = render(
        source,
        &inputs(&Capabilities::kitty(WIDTH), &Theme::default()),
    );
    assert!(
        kitty.starts_with("  \x1b[0;1m▼ Sum \x1b[0;1;30;43mup\x1b[0m\n\n  Text.\n"),
        "{kitty:?}"
    );
}

#[test]
fn a_summary_with_line_breaks_stays_on_one_line() {
    let source = "<details>\n<summary>Two<br>lines, <mark>then <br> more</mark></summary>\n\n\
        Text.\n\n</details>\n";
    let plain = render(
        source,
        &inputs(&Capabilities::plain(WIDTH), &Theme::default()),
    );
    assert_eq!(plain, "  ▼ Two lines, then more\n\n  Text.\n");
}

#[test]
fn a_heading_in_details_is_drawn_as_at_the_top_level() {
    let source = "<details>\n<summary>S</summary>\n\n## Inside\n\n</details>\n\n## Outside\n";
    let caps = Capabilities::kitty(WIDTH);
    let out = render(source, &inputs(&caps, &Theme::default()));
    let inside = out.replace("Outside", "Inside");
    assert!(
        inside.matches("\x1b[0;1;4;34m\x1b]66;s=2;Inside").count() == 2,
        "{out:?}"
    );
}
