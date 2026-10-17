//! Themes and colours: papers, muted text, NO_COLOR and the example themes.

use std::path::Path;

use kiri::render::Inputs;
use kiri::term::Capabilities;
use kiri::theme::{Paper, Theme};

mod common;

use common::{
    WIDTH, colour_codes, example_theme, inputs, render, render_fixture, render_for, visible,
};

#[test]
fn kitty_elements_on_light_paper() {
    let theme = Theme::paper(Paper::Light);
    insta::assert_snapshot!(visible(&render_fixture(
        "elements.md",
        &inputs(&Capabilities::kitty(WIDTH), &theme)
    )));
}

#[test]
fn kitty_elements_on_dark_paper() {
    let theme = Theme::paper(Paper::Dark);
    insta::assert_snapshot!(visible(&render_fixture(
        "elements.md",
        &inputs(&Capabilities::kitty(WIDTH), &theme)
    )));
}

#[test]
fn text_without_a_colour_of_its_own_takes_the_foreground() {
    let theme =
        kiri::theme::parse("foreground = \"#123456\"\nlink = \"none\"\nrule = \"#ff0000\"\n")
            .unwrap();
    let out = render(
        "Plain *text* and [a link](https://example.com)\n\n---\n\n```rust\nfn main() {}\n```\n",
        &inputs(&Capabilities::kitty(WIDTH), &theme),
    );
    let foreground = "38;2;18;52;86";
    // The margin before the text takes it too, which does no harm.
    assert!(
        out.contains(&format!("\x1b[0;{foreground}m  Plain ")),
        "{out:?}"
    );
    assert!(
        out.contains(&format!("\x1b[0;3;{foreground}mtext")),
        "{out:?}"
    );
    assert!(
        out.contains(&format!("\x1b[0;4;{foreground}ma link")),
        "{out:?}"
    );
    // Colours of their own stay: the rule's and the code's.
    assert!(out.contains("\x1b[0;38;2;255;0;0m─"), "{out:?}");
    let code = (out.lines())
        .find(|line| line.contains("main"))
        .expect("the code line");
    // Only the padding around the code, spaces, takes the foreground.
    for sequence in code.split("\x1b[").skip(1) {
        let (codes, text) = sequence.split_once('m').unwrap();
        if !text.trim().is_empty() {
            assert!(!codes.contains(foreground), "{code:?}");
        }
    }
    // Without colours there is no foreground either.
    let caps = Capabilities {
        color: false,
        ..Capabilities::kitty(WIDTH)
    };
    let out = render("Plain text\n", &inputs(&caps, &theme));
    assert!(colour_codes(&out).is_empty(), "{out:?}");
}

#[test]
fn kitty_elements_without_colour() {
    let caps = Capabilities {
        color: false,
        ..Capabilities::kitty(WIDTH)
    };
    insta::assert_snapshot!(visible(&render_for("elements.md", caps)));
}

/// Alt text, inline and block HTML, and a link target with no clickable link.
const MUTED_SOURCE: &str = "![alt](m.png) <b>x</b> [link](https://e.com)\n\n<div>block</div>\n";

#[test]
fn muted_text_takes_the_theme_colour_instead_of_dim() {
    let caps = Capabilities {
        hyperlinks: false,
        ..Capabilities::kitty(WIDTH)
    };
    let theme = kiri::theme::parse("muted = \"#808080\"\n").unwrap();
    let out = render(MUTED_SOURCE, &inputs(&caps, &theme));
    for muted in [
        "\x1b[0;3;38;2;128;128;128m[image: alt]",
        "\x1b[0;38;2;128;128;128m<b>",
        "\x1b[0;38;2;128;128;128m <https://e.com>",
        "\x1b[0;38;2;128;128;128m<div>block</div>",
    ] {
        assert!(out.contains(muted), "{muted:?} in {out:?}");
    }
    assert!(!out.contains("\x1b[0;2"), "nothing is dimmed: {out:?}");
}

#[test]
fn muted_text_is_dimmed_without_a_muted_colour_or_colours() {
    let caps = Capabilities {
        hyperlinks: false,
        ..Capabilities::kitty(WIDTH)
    };
    let no_color = Capabilities {
        color: false,
        ..caps.clone()
    };
    for (caps, theme) in [
        (&caps, Theme::default()),
        (&no_color, Theme::paper(Paper::Light)),
    ] {
        let out = render(MUTED_SOURCE, &inputs(caps, &theme));
        for dimmed in [
            "\x1b[0;2;3m[image: alt]",
            "\x1b[0;2m<b>",
            "\x1b[0;2m <https://e.com>",
            "\x1b[0;2m<div>block</div>",
        ] {
            assert!(out.contains(dimmed), "{dimmed:?} in {out:?}");
        }
    }
}

#[test]
fn image_alt_text_on_light_paper() {
    insta::assert_snapshot!(visible(&render_fixture(
        "images.md",
        &inputs(&Capabilities::kitty(WIDTH), &Theme::paper(Paper::Light))
    )));
}

#[test]
fn image_alt_text_on_dark_paper() {
    insta::assert_snapshot!(visible(&render_fixture(
        "images.md",
        &inputs(&Capabilities::kitty(WIDTH), &Theme::paper(Paper::Dark))
    )));
}

#[test]
fn no_color_keeps_everything_but_colours() {
    let caps = Capabilities {
        color: false,
        ..Capabilities::kitty(WIDTH)
    };
    let themes = [
        ("elements.md", Theme::default()),
        ("headings.md", example_theme("background.toml")),
        ("headings.md", example_theme("opaque.toml")),
    ];
    for (fixture, theme) in themes {
        let out = render_fixture(fixture, &inputs(&caps, &theme));
        assert_eq!(colour_codes(&out), Vec::<String>::new(), "{fixture}");
        assert!(out.contains("\x1b]66;s="), "{fixture}: heading sizes stay");
        assert!(out.contains("\x1b[0;1"), "{fixture}: bold stays");
    }
    let out = render_for("elements.md", caps.clone());
    assert!(out.contains("\x1b]8;;https://example.com"), "links stay");
    assert!(out.contains(";4:4"), "underline styles stay");
    assert!(out.contains("\x1b[0;9m"), "strikethrough stays");
    // Backticks mark inline code instead of its colours.
    assert!(out.contains("`inline code`"), "{out:?}");
    let ansi = Capabilities {
        color: false,
        ..Capabilities::ansi(WIDTH)
    };
    assert!(colour_codes(&render_for("elements.md", ansi)).is_empty());
    // Layout on its own gives no colours either.
    let inputs = Inputs {
        theme: example_theme("background.toml"),
        ..Inputs::new(caps)
    };
    let lines =
        kiri::render::layout::layout(&kiri::parse::parse("# Heading\n\nSome `code`\n"), &inputs);
    let mut spans =
        (lines.iter()).flat_map(|line| line.lead.iter().chain(&line.spans).chain(&line.trail));
    assert!(
        spans.all(|span| span.style.fg.is_none() && span.style.bg.is_none()),
        "{lines:?}"
    );
    // The colour check itself finds colours when they are on.
    let coloured = render_for("elements.md", Capabilities::kitty(WIDTH));
    assert!(!colour_codes(&coloured).is_empty());
}

#[test]
fn all_example_themes_load() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/themes");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            kiri::theme::load(&path).unwrap_or_else(|error| panic!("{error}"));
        }
    }
}
