//! The HTML tags kiri reads besides `<img>`: `<details>` with its
//! `<summary>`, `<kbd>`, `<mark>`, `<sub>`, `<sup>` and `<br>`.

use kiri::caps::Capabilities;
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
        inside.matches("\x1b[0;1;34m\x1b]66;s=2;Inside").count() == 2,
        "{out:?}"
    );
}

/// `source` rendered as plain text, and in kitty without text sizing.
fn without_text_sizing(source: &str) -> [String; 2] {
    let kitty = Capabilities {
        text_sizing: false,
        ..Capabilities::kitty(WIDTH)
    };
    [Capabilities::plain(WIDTH), kitty]
        .map(|caps| render(source, &inputs(&caps, &Theme::default())))
}

#[test]
fn without_text_sizing_sub_and_sup_take_script_characters() {
    let source = "H<sub>2</sub>O, x<sup>2</sup>, 2<sup>nd</sup>, E<sub>max</sub>, \
        10<sup>-(n+1)</sup>, a<sub>i+j</sub>, x<sup>i<sub>2</sub></sup>\n";
    for out in without_text_sizing(source) {
        assert_eq!(out, "  H₂O, x², 2ⁿᵈ, Eₘₐₓ, 10⁻⁽ⁿ⁺¹⁾, aᵢ₊ⱼ, xⁱ²\n");
    }
}

#[test]
fn a_sub_or_sup_without_script_characters_is_plain_text() {
    // No superscript q, no subscript b, and code or a link is not plain
    // text.
    let source = "x<sup>q</sup>, y<sub>b</sub>, z<sup>`2`</sup>, w<sup>*2*</sup>, \
        v<sup>[1](#a)</sup>\n";
    let [plain, _] = without_text_sizing(source);
    assert_eq!(plain, "  xq, yb, z`2`, w2, v1 <#a>\n");
    let [_, kitty] = without_text_sizing("x<sup>q</sup>, y<sub>b</sub>, w<sup>*2*</sup>\n");
    assert_eq!(kitty, "  xq, yb, w\x1b[0;3m2\x1b[0m\n");
}

#[test]
fn with_text_sizing_sub_and_sup_are_two_thirds_of_the_size_around_them() {
    let kitty = |source: &str| {
        render(
            source,
            &inputs(&Capabilities::kitty(WIDTH), &Theme::default()),
        )
    };
    // Three cells of text in two, a sup at the top and a sub at the bottom
    // of the row, whatever their characters.
    let out = kitty("x<sup>2</sup> E<sub>max</sub> e<sup>iπ</sup>\n");
    for sized in [
        "x\x1b]66;s=1:n=2:d=3:w=1;2\x1b\\",
        "E\x1b]66;s=1:n=2:d=3:v=1:w=2;max\x1b\\",
        "e\x1b]66;s=1:n=2:d=3:w=2;iπ\x1b\\",
    ] {
        assert!(out.contains(sized), "{sized:?} in {out:?}");
    }
    // A script inside a script keeps its size.
    let out = kitty("x<sup>a<sub>i</sub></sup>\n");
    assert!(out.contains("x\x1b]66;s=1:n=2:d=3:w=2;ai\x1b\\"), "{out:?}");
}

#[test]
fn a_sub_or_sup_in_a_heading_shrinks_in_the_heading_s_rows() {
    let kitty = |source: &str| {
        render(
            source,
            &inputs(&Capabilities::kitty(WIDTH), &Theme::default()),
        )
    };
    // An h1 is 3 · 5/6, so its sup is 3 · 5/9.
    let out = kitty("# x<sup>2</sup>\n");
    assert!(out.contains("\x1b]66;s=3:n=5:d=9:w=1;2\x1b\\"), "{out:?}");
    // An h4 is 2 · 3/4, so its sub is 2 · 1/2.
    let out = kitty("#### H<sub>2</sub>O\n");
    assert!(
        out.contains("\x1b]66;s=2:n=1:d=2:v=1:w=1;2\x1b\\"),
        "{out:?}"
    );
    // A heading in a quote is at normal size, and so is the text around
    // its sup.
    let out = kitty("> # x<sup>2</sup>\n");
    assert!(out.contains("x\x1b]66;s=1:n=2:d=3:w=1;2\x1b\\"), "{out:?}");
}
