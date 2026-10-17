use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine;
use image::{Rgba, RgbaImage};
use kiri::images::{DEFAULT_MAX_FILE_SIZE, Fonts, ImageError, Images, Source};
use kiri::kitty::graphics::{DIACRITICS, PLACEHOLDER};
use kiri::model::Document;
use kiri::term::Capabilities;
use kiri::theme::{Paper, Theme};

const WIDTH: u16 = 48;

fn render(fixture: &str, caps: &Capabilities) -> String {
    render_with(fixture, caps, &Theme::default())
}

fn render_with(fixture: &str, caps: &Capabilities, theme: &Theme) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let source = std::fs::read_to_string(&path).expect("fixture is readable");
    let document = kiri::parse::parse(&source);
    let mut out = Vec::new();
    kiri::render::render(&document, &Images::new(), caps, theme, &mut out)
        .expect("rendering to memory succeeds");
    String::from_utf8(out).expect("output is UTF-8")
}

fn example_theme(name: &str) -> Theme {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/themes")
        .join(name);
    kiri::theme::load(&path).expect("example theme loads")
}

/// Escape characters and no-break spaces made visible, so snapshots can be
/// read and diffed.
fn visible(output: &str) -> String {
    output.replace('\x1b', "␛").replace('\u{a0}', "⍽")
}

#[test]
fn plain_basic() {
    insta::assert_snapshot!(render("basic.md", &Capabilities::plain(WIDTH)));
}

#[test]
fn plain_elements() {
    insta::assert_snapshot!(render("elements.md", &Capabilities::plain(WIDTH)));
}

#[test]
fn ansi_elements() {
    insta::assert_snapshot!(visible(&render("elements.md", &Capabilities::ansi(WIDTH))));
}

#[test]
fn kitty_elements() {
    insta::assert_snapshot!(visible(&render("elements.md", &Capabilities::kitty(WIDTH))));
}

#[test]
fn plain_nested() {
    insta::assert_snapshot!(render("nested.md", &Capabilities::plain(WIDTH)));
}

#[test]
fn kitty_nested() {
    insta::assert_snapshot!(visible(&render("nested.md", &Capabilities::kitty(WIDTH))));
}

#[test]
fn plain_nested_narrow() {
    insta::assert_snapshot!(render("nested.md", &Capabilities::plain(12)));
}

#[test]
fn plain_wide_characters() {
    insta::assert_snapshot!(render("wide.md", &Capabilities::plain(WIDTH)));
}

#[test]
fn kitty_wide_characters() {
    insta::assert_snapshot!(visible(&render("wide.md", &Capabilities::kitty(WIDTH))));
}

#[test]
fn plain_wide_characters_narrow() {
    insta::assert_snapshot!(render("wide.md", &Capabilities::plain(12)));
}

#[test]
fn kitty_elements_on_light_paper() {
    let theme = Theme::paper(Paper::Light);
    insta::assert_snapshot!(visible(&render_with(
        "elements.md",
        &Capabilities::kitty(WIDTH),
        &theme
    )));
}

#[test]
fn kitty_elements_on_dark_paper() {
    let theme = Theme::paper(Paper::Dark);
    insta::assert_snapshot!(visible(&render_with(
        "elements.md",
        &Capabilities::kitty(WIDTH),
        &theme
    )));
}

#[test]
fn kitty_headings_with_rules() {
    let theme = example_theme("rules.toml");
    insta::assert_snapshot!(visible(&render_with(
        "headings.md",
        &Capabilities::kitty(WIDTH),
        &theme
    )));
}

#[test]
fn kitty_headings_on_a_background() {
    let theme = example_theme("background.toml");
    insta::assert_snapshot!(visible(&render_with(
        "headings.md",
        &Capabilities::kitty(WIDTH),
        &theme
    )));
}

#[test]
fn heading_background_reaches_the_right_edge() {
    use kiri::render::wrap::width;
    let document = kiri::parse::parse("# One\n\n## Two\n");
    // Indented by one cell, and at the edge on an opaque document.
    let unindented = "background = \"#1c1f26\"\n\
        [h1]\nbackground = \"#2b303b\"\n[h2]\nbackground = \"#232830\"\n";
    let themes = [
        example_theme("background.toml"),
        kiri::theme::parse(unindented).unwrap(),
    ];
    // Scales 3 and 2 leave a remainder at most of these widths.
    for theme in &themes {
        for terminal_width in [46, 47, 48, 49] {
            let caps = Capabilities::kitty(terminal_width);
            let lines = kiri::render::layout::layout(&document, &Images::new(), &caps, theme);
            for line in lines.iter().filter(|line| line.scale > 1) {
                let cells = width(&line.lead)
                    + width(&line.spans) * usize::from(line.scale)
                    + width(&line.trail);
                assert_eq!(cells, usize::from(terminal_width), "{line:?}");
            }
        }
    }
}

#[test]
fn kitty_headings_on_an_opaque_document() {
    let theme = example_theme("opaque.toml");
    insta::assert_snapshot!(visible(&render_with(
        "headings.md",
        &Capabilities::kitty(WIDTH),
        &theme
    )));
}

#[test]
fn text_margin_narrows_everything_but_headings() {
    let theme = kiri::theme::parse("text_margin = 2\n").unwrap();
    insta::assert_snapshot!(render_with(
        "elements.md",
        &Capabilities::plain(WIDTH),
        &theme
    ));
}

#[test]
fn rules_span_the_terminal_past_max_width() {
    let theme = kiri::theme::parse("max_width = 20\n[h1]\nrule = true\n").unwrap();
    let document = kiri::parse::parse("# Title\n\nText\n\n---\n");
    let mut out = Vec::new();
    kiri::render::render(
        &document,
        &Images::new(),
        &Capabilities::ansi(30),
        &theme,
        &mut out,
    )
    .unwrap();
    let rules: Vec<usize> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| line.matches('─').count())
        .filter(|&count| count > 0)
        .collect();
    // Both the heading rule and the thematic break fill the terminal.
    assert_eq!(rules, vec![30, 30]);
}

#[test]
fn nested_headings_are_not_indented() {
    let theme = example_theme("indent.toml");
    let document = kiri::parse::parse("# Top\n\n> # Quoted\n\n- # Listed\n");
    let mut out = Vec::new();
    kiri::render::render(
        &document,
        &Images::new(),
        &Capabilities::ansi(40),
        &theme,
        &mut out,
    )
    .unwrap();
    // The indent stays on the top-level heading only; nested ones start
    // right after the quote bar or the bullet.
    insta::assert_snapshot!(visible(&String::from_utf8(out).unwrap()));
}

fn render_source(source: &str, caps: &Capabilities, theme: &Theme) -> String {
    let document = kiri::parse::parse(source);
    let mut out = Vec::new();
    kiri::render::render(&document, &Images::new(), caps, theme, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn text_without_a_colour_of_its_own_takes_the_foreground() {
    let theme =
        kiri::theme::parse("foreground = \"#123456\"\nlink = \"none\"\nrule = \"#ff0000\"\n")
            .unwrap();
    let out = render_source(
        "Plain *text* and [a link](https://example.com)\n\n---\n\n```rust\nfn main() {}\n```\n",
        &Capabilities::kitty(WIDTH),
        &theme,
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
    let out = render_source("Plain text\n", &caps, &theme);
    assert!(colour_codes(&out).is_empty(), "{out:?}");
}

#[test]
fn empty_blocks_get_no_separator() {
    let plain = Capabilities::plain(WIDTH);
    let theme = Theme::default();
    let source = "```\n```\n\nBefore\n\n```\n```\n\nAfter\n";
    assert_eq!(
        render_source(source, &plain, &theme),
        "  Before\n\n  After\n"
    );
    let quoted = "> Before\n>\n> ```\n> ```\n>\n> After\n";
    assert_eq!(
        render_source(quoted, &plain, &theme),
        "  > Before\n  >\n  > After\n"
    );
    // Without padding, a highlighted empty code block has no lines either.
    let ansi = render_source(source, &Capabilities::ansi(WIDTH), &theme.without_margins());
    assert_eq!(ansi.matches('\n').count(), 3, "{ansi:?}");
}

#[test]
fn empty_headings_disappear() {
    let theme = Theme::default();
    let source = "Before\n\n#\n\n##\n\nAfter\n\n> #\n";
    for caps in [
        Capabilities::plain(WIDTH),
        Capabilities::ansi(WIDTH),
        Capabilities::kitty(WIDTH),
    ] {
        let out = render_source(source, &caps, &theme);
        assert_eq!(
            strip_own_sequences(&out),
            "  Before\n\n  After\n",
            "{caps:?}"
        );
        assert!(!out.contains("\x1b]66;"), "{out:?}");
    }
}

#[test]
fn a_heading_that_renders_nothing_disappears() {
    // An empty link shows nothing when links are clickable.
    let source = "Before\n\n# [](https://example.com)\n\nAfter\n";
    let out = render_source(source, &Capabilities::kitty(WIDTH), &Theme::default());
    assert_eq!(strip_own_sequences(&out), "  Before\n\n  After\n");
}

#[test]
fn an_empty_heading_keeps_its_rule() {
    let theme = kiri::theme::parse("[h1]\nrule = true\n").unwrap();
    let out = render_source("Before\n\n#\n\nAfter\n", &Capabilities::kitty(20), &theme);
    let rule = "─".repeat(20);
    assert_eq!(
        strip_own_sequences(&out),
        format!("  Before\n\n{rule}\n\n  After\n")
    );
}

#[test]
fn blank_lines_in_quotes_end_at_the_bar() {
    let source = "> a\n>\n> > b\n> >\n> > c\n\n- item\n\n  > d\n  >\n  > e\n";
    for caps in [Capabilities::ansi(WIDTH), Capabilities::kitty(WIDTH)] {
        let text = strip_own_sequences(&render_source(source, &caps, &Theme::default()));
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.iter().all(|line| !line.ends_with(' ')), "{lines:#?}");
        assert!(lines.contains(&"  │"), "{lines:#?}");
        assert!(lines.contains(&"  │ │"), "{lines:#?}");
        assert!(lines.contains(&"    │"), "{lines:#?}");
    }
    // On an opaque document, the background still reaches the edge.
    let out = render_source(
        "> a\n>\n> b\n",
        &Capabilities::kitty(WIDTH),
        &example_theme("opaque.toml"),
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
        let text = strip_own_sequences(&render_source(source, &caps, &Theme::default()));
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.contains(&"  •"), "{lines:#?}");
        assert!(lines.contains(&"  2."), "{lines:#?}");
    }
}

#[test]
fn tiny_widths_do_not_underflow() {
    // A wide character in a heading scaled to one character per line.
    let theme = kiri::theme::parse("[h1]\nscale = 7\nbackground = \"blue\"\n").unwrap();
    let out = render_source("# 日本\n", &Capabilities::kitty(10), &theme);
    assert!(out.contains('日'), "{out:?}");
    // Quotes nested until a code block's padding is wider than its room.
    let quotes = "> ".repeat(6);
    let source = format!("{quotes}```\n{quotes}code\n{quotes}```\n");
    let out = render_source(&source, &Capabilities::plain(10), &Theme::default());
    assert!(out.contains('c'), "{out:?}");
}

#[test]
fn inline_code_keeps_its_spaces() {
    let source = "Run `git  commit   -m` now.\n";
    let theme = Theme::default();
    assert_eq!(
        render_source(source, &Capabilities::plain(WIDTH), &theme),
        "  Run `git  commit   -m` now.\n"
    );
    let kitty = render_source(source, &Capabilities::kitty(WIDTH), &theme);
    assert!(kitty.contains("\u{a0}git  commit   -m\u{a0}"), "{kitty:?}");
    // Code still wraps at its spaces, and the next line does not start with
    // the ones it broke at.
    assert_eq!(
        render_source(source, &Capabilities::plain(20), &theme),
        "  Run `git  commit\n  -m` now.\n"
    );
}

#[test]
fn inline_code_wraps_at_its_spaces_unless_the_theme_keeps_it_together() {
    let plain = Capabilities::plain(16);
    let source = "Type `git commit` here.\n";
    assert_eq!(
        render_source(source, &plain, &Theme::default()),
        "  Type `git\n  commit`\n  here.\n"
    );
    let together = kiri::theme::parse("inline_code_wrap = false\n").unwrap();
    assert_eq!(
        render_source(source, &plain, &together),
        "  Type\n  `git commit`\n  here.\n"
    );
    // Code longer than a line still breaks at its spaces.
    let source = "Type `git commit --amend` here.\n";
    assert_eq!(
        render_source(source, &plain, &together),
        "  Type `git\n  commit\n  --amend`\n  here.\n"
    );
}

#[test]
fn inline_code_kept_together_in_a_narrow_table_cell() {
    let source = "| A | B |\n|---|---|\n| see (`a b`). | x |\n";
    let together = kiri::theme::parse("inline_code_wrap = false\n").unwrap();
    // The first column is nine cells wide.
    let plain = Capabilities::plain(21);
    let out = render_source(source, &plain, &together);
    assert!(out.contains("│ see       │"), "{out}");
    assert!(out.contains("│ (`a b`).  │"), "{out}");
    let out = render_source(source, &plain, &Theme::default());
    assert!(out.contains("│ see (`a   │"), "{out}");
}

#[test]
fn a_table_stacks_only_when_spaced_inline_code_cannot_wrap_in_it() {
    let source = "| Notes | Command |\n|---|---|\n\
                  | a fairly long note about the step | `git commit -m message` |\n";
    // By default the code wraps at its spaces like the text beside it.
    let out = render_source(source, &Capabilities::plain(28), &Theme::default());
    assert!(out.contains("│ a fairly  │ `git     │"), "{out}");
    assert!(out.contains("│ long note │ commit   │"), "{out}");
    // Kept together, the code stays whole while the notes keep room,
    let together = kiri::theme::parse("inline_code_wrap = false\n").unwrap();
    let out = render_source(source, &Capabilities::plain(50), &together);
    assert!(
        out.contains("│ a fairly long    │ `git commit -m message` │"),
        "{out}"
    );
    // then wraps at its spaces like by default before the table stacks,
    let narrow = Capabilities::plain(30);
    let out = render_source(source, &narrow, &together);
    assert!(out.contains("│ a fairly   │ `git      │"), "{out}");
    assert!(out.contains("│ long note  │ commit -m │"), "{out}");
    assert_eq!(out, render_source(source, &narrow, &Theme::default()));
    // and stacks only when not even its words fit.
    let out = render_source(source, &Capabilities::plain(24), &together);
    assert!(out.starts_with("  Notes: a fairly long\n"), "{out}");
}

#[test]
fn no_margins_puts_text_and_code_at_the_edge() {
    let theme = Theme::default().without_margins();
    insta::assert_snapshot!(visible(&render_with(
        "elements.md",
        &Capabilities::kitty(WIDTH),
        &theme
    )));
}

/// SGR colour codes in the output: 30-37, 38, 40-47, 48, 58, 90-97 and
/// 100-107. Default-colour codes (39, 49, 59) are not counted.
fn colour_codes(output: &str) -> Vec<String> {
    let mut codes = Vec::new();
    for sequence in output.split("\x1b[").skip(1) {
        let Some(end) = sequence.find(|c: char| !c.is_ascii_digit() && c != ';' && c != ':') else {
            continue;
        };
        if !sequence[end..].starts_with('m') {
            continue;
        }
        for code in sequence[..end].split(';') {
            let number: u32 = code.split(':').next().unwrap().parse().unwrap_or(0);
            if matches!(number, 30..=38 | 40..=48 | 58 | 90..=97 | 100..=107) {
                codes.push(sequence[..end].to_owned());
                break;
            }
        }
    }
    codes
}

#[test]
fn kitty_elements_without_colour() {
    let caps = Capabilities {
        color: false,
        ..Capabilities::kitty(WIDTH)
    };
    insta::assert_snapshot!(visible(&render("elements.md", &caps)));
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
    let out = render_source(MUTED_SOURCE, &caps, &theme);
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
        let out = render_source(MUTED_SOURCE, caps, &theme);
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
    insta::assert_snapshot!(visible(&render_with(
        "images.md",
        &Capabilities::kitty(WIDTH),
        &Theme::paper(Paper::Light)
    )));
}

#[test]
fn image_alt_text_on_dark_paper() {
    insta::assert_snapshot!(visible(&render_with(
        "images.md",
        &Capabilities::kitty(WIDTH),
        &Theme::paper(Paper::Dark)
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
        let out = render_with(fixture, &caps, &theme);
        assert_eq!(colour_codes(&out), Vec::<String>::new(), "{fixture}");
        assert!(out.contains("\x1b]66;s="), "{fixture}: heading sizes stay");
        assert!(out.contains("\x1b[0;1"), "{fixture}: bold stays");
    }
    let out = render_with("elements.md", &caps, &Theme::default());
    assert!(out.contains("\x1b]8;;https://example.com"), "links stay");
    assert!(out.contains(";4:4"), "underline styles stay");
    assert!(out.contains("\x1b[0;9m"), "strikethrough stays");
    // Backticks mark inline code instead of its colours.
    assert!(out.contains("`inline code`"), "{out:?}");
    let ansi = Capabilities {
        color: false,
        ..Capabilities::ansi(WIDTH)
    };
    assert!(colour_codes(&render("elements.md", &ansi)).is_empty());
    // Layout on its own gives no colours either.
    let lines = kiri::render::layout::layout(
        &kiri::parse::parse("# Heading\n\nSome `code`\n"),
        &Images::new(),
        &caps,
        &example_theme("background.toml"),
    );
    let mut spans =
        (lines.iter()).flat_map(|line| line.lead.iter().chain(&line.spans).chain(&line.trail));
    assert!(
        spans.all(|span| span.style.fg.is_none() && span.style.bg.is_none()),
        "{lines:?}"
    );
    // The colour check itself finds colours when they are on.
    let coloured = render_with(
        "elements.md",
        &Capabilities::kitty(WIDTH),
        &Theme::default(),
    );
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

#[test]
fn escape_sequences_in_the_document_are_neutralised() {
    let document = kiri::parse::parse("text \x1b]0;evil\x07 here\n");
    let mut out = Vec::new();
    kiri::render::render(
        &document,
        &Images::new(),
        &Capabilities::kitty(WIDTH),
        &Theme::default(),
        &mut out,
    )
    .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(!out.contains("\x1b]0;"), "{out:?}");
}

/// Removes the sequences kiri writes itself: CSI (SGR, erase, cursor
/// forward), OSC 8 hyperlinks and OSC 66 scaled text. Panics on any other
/// escape, and on a control character inside an OSC payload.
fn strip_own_sequences(output: &str) -> String {
    let mut out = String::new();
    let mut rest = output;
    while let Some(start) = rest.find('\x1b') {
        out.push_str(&rest[..start]);
        let sequence = &rest[start..];
        let end = if let Some(csi) = sequence.strip_prefix("\x1b[") {
            let params = csi
                .find(|c: char| !c.is_ascii_digit() && c != ';' && c != ':')
                .unwrap_or_else(|| panic!("unterminated CSI: {sequence:?}"));
            assert!(
                matches!(csi.as_bytes()[params], b'm' | b'K' | b'C'),
                "unexpected CSI: {sequence:?}"
            );
            2 + params + 1
        } else if sequence.starts_with("\x1b]8;;") || sequence.starts_with("\x1b]66;") {
            let payload = sequence
                .find("\x1b\\")
                .unwrap_or_else(|| panic!("unterminated OSC: {sequence:?}"));
            assert!(
                !sequence[1..payload].chars().any(char::is_control),
                "control character in an OSC: {sequence:?}"
            );
            payload + 2
        } else {
            panic!("escape from the document: {sequence:?}");
        };
        rest = &sequence[end..];
    }
    out.push_str(rest);
    out
}

#[test]
fn control_characters_in_the_document_do_not_reach_the_terminal() {
    // ESC and the C1 string terminator U+009C in a heading, a link target,
    // image alt text, a table cell, a code fence's info string and a code
    // block.
    let source = "# Head \x1b]0;evil\x07 \u{9c}\n\n\
        [link](<https://example.com/\x1b]0;evil\x07\u{9c}>) and \
        ![alt \x1bc \u{9c}](image.png)\n\n\
        | Cell |\n|---|\n| \x1b]0;evil\x07 \u{9c} |\n\n\
        ```rust\x1b]0;evil\x07\u{9c}\ncode \x1b[2J \u{9c}\n```\n";
    let document = kiri::parse::parse(source);
    // The control characters survive parsing, so the renderer sees them.
    assert_eq!(format!("{document:?}").matches("\\u{1b}").count(), 6);
    assert_eq!(format!("{document:?}").matches("\\u{9c}").count(), 6);
    for caps in [
        Capabilities::plain(WIDTH),
        Capabilities::ansi(WIDTH),
        Capabilities::kitty(WIDTH),
    ] {
        let mut out = Vec::new();
        kiri::render::render(
            &document,
            &Images::new(),
            &caps,
            &Theme::default(),
            &mut out,
        )
        .unwrap();
        let out = String::from_utf8(out).unwrap();
        if caps.hyperlinks {
            // The link target is still written, without the controls.
            assert!(
                out.contains("\x1b]8;;https://example.com/]0;evil\x1b\\"),
                "{out:?}"
            );
        }
        let text = strip_own_sequences(&out);
        assert!(
            !text.chars().any(|c| c.is_control() && c != '\n'),
            "{caps:?}: {out:?}"
        );
    }
}

#[test]
fn the_visual_check_documents_fit_every_mode() {
    use kiri::render::wrap::width;
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let themes = [
        Theme::default(),
        example_theme("background.toml"),
        example_theme("rules.toml"),
        example_theme("opaque.toml"),
        kiri::theme::parse("inline_code_wrap = false\n").unwrap(),
        Theme::paper(Paper::Light),
        Theme::paper(Paper::Dark),
    ];
    // Each document says which of its images kiri warns about.
    for (name, warnings) in [("visual-check.md", 1), ("visual-check-images.md", 5)] {
        let source = std::fs::read_to_string(examples.join(name)).unwrap();
        let document = kiri::parse::parse(&source);
        // Layout needs only the sizes of the images, so it gets the real
        // ones; rendering them at every width would only take longer. Text
        // in SVGs does not change their size, so no fonts are loaded.
        let images =
            kiri::images::load_with(&document, &examples, DEFAULT_MAX_FILE_SIZE, &Fonts::none());
        assert_eq!(images.warnings().len(), warnings, "{name}: {images:?}");
        for terminal_width in [16, 24, 40, 80, 120] {
            for caps in [
                Capabilities::plain(terminal_width),
                Capabilities::ansi(terminal_width),
                Capabilities::kitty(terminal_width),
            ] {
                for theme in &themes {
                    let mut out = Vec::new();
                    kiri::render::render(&document, &Images::new(), &caps, theme, &mut out)
                        .unwrap();
                    assert!(!out.is_empty());
                    for line in kiri::render::layout::layout(&document, &images, &caps, theme) {
                        let image = (line.image.as_ref())
                            .map_or(0, |image| usize::from(image.placement.size.columns));
                        let cells = width(&line.lead)
                            + width(&line.spans) * usize::from(line.scale)
                            + image
                            + width(&line.trail);
                        assert!(
                            cells <= usize::from(terminal_width),
                            "{name}, {caps:?}: {cells} cells in {line:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn plain_tables() {
    insta::assert_snapshot!(render("tables.md", &Capabilities::plain(WIDTH)));
}

#[test]
fn kitty_tables() {
    insta::assert_snapshot!(visible(&render("tables.md", &Capabilities::kitty(WIDTH))));
}

#[test]
fn stacked_tables_nest_in_quotes_and_list_items() {
    let table = "| City | Notes |\n|---|---|\n| Helsinki | Capital |\n| Oulu | North |\n";
    let quoted: String = table.lines().map(|line| format!("> {line}\n")).collect();
    let listed = table
        .replacen("| City", "- | City", 1)
        .replace("\n|", "\n  |");
    let source = format!("{quoted}\n{listed}");
    // Eighteen cells inside the quote and the item: too few for the
    // borders, enough for a header and a value on one line.
    assert_eq!(
        render_source(&source, &Capabilities::plain(24), &Theme::default()),
        "  > City: Helsinki\n  > Notes: Capital\n  > ──────────────────\n  \
         > City: Oulu\n  > Notes: North\n\n  \
         - City: Helsinki\n    Notes: Capital\n    ──────────────────\n    \
         City: Oulu\n    Notes: North\n"
    );
}

// Images

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Opaque quadrants: red, green, blue and yellow from the top left.
fn quadrants(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba(match (x < width / 2, y < height / 2) {
            (true, true) => [255, 0, 0, 255],
            (false, true) => [0, 255, 0, 255],
            (true, false) => [0, 0, 255, 255],
            (false, false) => [255, 255, 0, 255],
        })
    })
}

fn one_image(url: &str, image: RgbaImage) -> Images {
    let mut images = Images::new();
    images.insert(url, Ok(Source::Raster(image)));
    images
}

/// The images of `document` as kiri loads them from the fixtures, with each
/// one that loads replaced by quadrants of the same size. Snapshots then
/// show real sizes and errors, but not the decoders' work, and the image
/// data compresses to a few bytes. Without fonts, as the sizes do not
/// depend on them.
fn fixture_images(document: &Document) -> Images {
    let loaded =
        kiri::images::load_with(document, &fixtures(), DEFAULT_MAX_FILE_SIZE, &Fonts::none());
    let mut images = Images::new();
    for (url, image) in loaded.iter() {
        let image = match image {
            Ok(source) => {
                let (width, height) = source.size();
                Ok(Source::Raster(quadrants(width, height)))
            }
            Err(error) => Err(error.clone()),
        };
        images.insert(url, image);
    }
    images
}

fn render_document(document: &Document, images: &Images, caps: &Capabilities) -> String {
    let mut out = Vec::new();
    kiri::render::render(document, images, caps, &Theme::default(), &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn render_images(fixture: &str, caps: &Capabilities) -> String {
    let source = std::fs::read_to_string(fixtures().join(fixture)).unwrap();
    let document = kiri::parse::parse(&source);
    render_document(&document, &fixture_images(&document), caps)
}

fn render_with_images(source: &str, images: &Images, caps: &Capabilities) -> String {
    render_document(&kiri::parse::parse(source), images, caps)
}

/// The keys and base64 data of every graphics command in `output`.
fn graphics_commands(output: &str) -> Vec<(String, String)> {
    let mut commands = Vec::new();
    let mut rest = output;
    while let Some(start) = rest.find("\x1b_G") {
        let body = &rest[start + 3..];
        let end = body.find("\x1b\\").expect("terminated graphics command");
        let (keys, data) = body[..end].split_once(';').expect("keys and data");
        commands.push((keys.to_owned(), data.to_owned()));
        rest = &body[end + 2..];
    }
    commands
}

/// `visible`, with kitty's graphics made readable: the base64 data of each
/// graphics command becomes its length, and each run of placeholders shows
/// the image row it draws and how many cells it covers. Panics on
/// placeholders that do not count their columns up from the left.
fn readable(output: &str) -> String {
    let mut commands = String::new();
    let mut rest = output;
    while let Some(start) = rest.find("\x1b_G") {
        commands.push_str(&rest[..start]);
        let body = &rest[start + 3..];
        let end = body.find("\x1b\\").expect("terminated graphics command");
        let (keys, data) = body[..end].split_once(';').expect("keys and data");
        commands.push_str(&format!("\x1b_G{keys};<{} bytes>\x1b\\", data.len()));
        rest = &body[end + 2..];
    }
    commands.push_str(rest);

    let position = |c: Option<char>| {
        let c = c.expect("placeholder with its diacritics");
        DIACRITICS
            .iter()
            .position(|&d| d == c)
            .expect("a diacritic")
    };
    let mut out = String::new();
    let mut chars = commands.chars().peekable();
    while let Some(c) = chars.next() {
        if c != PLACEHOLDER {
            out.push(c);
            continue;
        }
        let row = position(chars.next());
        assert_eq!(position(chars.next()), 0, "a row starts at column 0");
        let mut cells = 1;
        while chars.next_if_eq(&PLACEHOLDER).is_some() {
            assert_eq!(position(chars.next()), row);
            assert_eq!(position(chars.next()), cells);
            cells += 1;
        }
        out.push_str(&format!("⟦row {row}: {cells} cells⟧"));
    }
    visible(&out)
}

/// The placeholder rows in `output`, as `readable` shows them.
fn drawn(output: &str) -> Vec<String> {
    readable(output)
        .split('⟦')
        .skip(1)
        .map(|rest| format!("⟦{}", &rest[..rest.find('⟧').unwrap() + '⟧'.len_utf8()]))
        .collect()
}

/// One image sent to the terminal: the keys of its first command, and its
/// pixels decoded from all of its commands.
struct Sent {
    keys: HashMap<String, String>,
    rgba: Vec<u8>,
}

fn sent(output: &str) -> Vec<Sent> {
    let mut images = Vec::new();
    let mut keys = HashMap::new();
    let mut data = String::new();
    for (command_keys, chunk) in graphics_commands(output) {
        let command: HashMap<String, String> = (command_keys.split(','))
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap();
                (key.to_owned(), value.to_owned())
            })
            .collect();
        if command.contains_key("a") {
            keys = command.clone();
        }
        data.push_str(&chunk);
        if command["m"] == "0" {
            let compressed = base64::engine::general_purpose::STANDARD
                .decode(&data)
                .unwrap();
            let mut rgba = Vec::new();
            flate2::read::ZlibDecoder::new(&compressed[..])
                .read_to_end(&mut rgba)
                .unwrap();
            images.push(Sent {
                keys: std::mem::take(&mut keys),
                rgba,
            });
            data.clear();
        }
    }
    images
}

#[test]
fn kitty_images() {
    insta::assert_snapshot!(readable(&render_images(
        "images.md",
        &Capabilities::kitty(WIDTH)
    )));
}

#[test]
fn ansi_images() {
    insta::assert_snapshot!(visible(&render_images(
        "images.md",
        &Capabilities::ansi(WIDTH)
    )));
}

#[test]
fn plain_images() {
    insta::assert_snapshot!(render_images("images.md", &Capabilities::plain(WIDTH)));
}

#[test]
fn an_image_alone_in_a_paragraph_is_drawn() {
    let images = one_image("p.png", quadrants(40, 30));
    let out = render_with_images("![alone](p.png)\n", &images, &Capabilities::kitty(WIDTH));
    assert_eq!(sent(&out).len(), 1);
    assert_eq!(drawn(&out), ["⟦row 0: 4 cells⟧", "⟦row 1: 4 cells⟧"]);
    assert!(!out.contains("[image: alone]"), "{out:?}");
}

#[test]
fn a_linked_image_is_drawn_but_cannot_be_clicked() {
    let images = one_image("p.png", quadrants(40, 30));
    let source = "[![linked](p.png)](https://example.com)\n";
    let out = render_with_images(source, &images, &Capabilities::kitty(WIDTH));
    assert_eq!(drawn(&out), ["⟦row 0: 4 cells⟧", "⟦row 1: 4 cells⟧"]);
    assert!(!out.contains("\x1b]8;"), "{out:?}");
}

#[test]
fn images_in_running_text_tables_and_headings_stay_alt_text() {
    let images = one_image("p.png", quadrants(40, 30));
    let source = "Text ![inline](p.png) here.\n\n\
        [![with words](p.png) and words](https://example.com)\n\n\
        | ![cell](p.png) |\n|---|\n| x |\n\n\
        # ![heading](p.png)\n";
    let out = render_with_images(source, &images, &Capabilities::kitty(WIDTH));
    assert!(sent(&out).is_empty(), "{out:?}");
    assert!(drawn(&out).is_empty(), "{out:?}");
    for alt in ["inline", "with words", "cell", "heading"] {
        assert!(out.contains(&format!("[image: {alt}]")), "{alt}: {out:?}");
    }
}

#[test]
fn images_stay_alt_text_without_graphics_placeholders_or_pixels() {
    let source = "![alone](p.png)\n";
    let loaded = one_image("p.png", quadrants(40, 30));
    let mut missing = Images::new();
    missing.insert("p.png", Err(ImageError::Missing("p.png".into())));
    let cases = [
        (Capabilities::plain(WIDTH), &loaded),
        (Capabilities::ansi(WIDTH), &loaded),
        (
            Capabilities {
                max_image_cells: 0,
                ..Capabilities::kitty(WIDTH)
            },
            &loaded,
        ),
        (Capabilities::kitty(WIDTH), &Images::new()),
        (Capabilities::kitty(WIDTH), &missing),
    ];
    for (caps, images) in cases {
        let out = render_with_images(source, images, &caps);
        assert!(out.contains("[image: alone]"), "{caps:?}: {out:?}");
        assert!(
            !out.contains('\u{10EEEE}') && !out.contains("\x1b_G"),
            "{out:?}"
        );
    }
}

/// `source` with `images`, rendered with `theme`.
fn render_images_with_theme(
    source: &str,
    images: &Images,
    caps: &Capabilities,
    theme: &Theme,
) -> String {
    let mut out = Vec::new();
    kiri::render::render(&kiri::parse::parse(source), images, caps, theme, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn images_sit_on_the_document_background() {
    let images = one_image("p.png", quadrants(40, 30));
    let out = render_images_with_theme(
        "Text\n\n![p](p.png)\n",
        &images,
        &Capabilities::kitty(WIDTH),
        &example_theme("opaque.toml"),
    );
    assert_eq!(drawn(&out), ["⟦row 0: 4 cells⟧", "⟦row 1: 4 cells⟧"]);
    // The background of opaque.toml, #1c1f26, behind the placeholders and
    // the rest of each row, where the image is transparent or ends.
    let background = "\x1b[0;48;2;28;31;38m";
    let rows: Vec<&str> = (out.lines())
        .filter(|line| line.contains(PLACEHOLDER))
        .collect();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let painted = row.find(background).expect("background on the row");
        assert!(painted < row.find(PLACEHOLDER).unwrap(), "{row:?}");
        assert!(row.ends_with("\x1b[39m\x1b[K\x1b[0m"), "{row:?}");
    }
}

#[test]
fn text_after_an_image_takes_the_paper_foreground_again() {
    let images = one_image("p.png", quadrants(40, 30));
    let out = render_images_with_theme(
        "![p](p.png)\n\nText after the image\n",
        &images,
        &Capabilities::kitty(WIDTH),
        &Theme::paper(Paper::Light),
    );
    assert_eq!(drawn(&out), ["⟦row 0: 4 cells⟧", "⟦row 1: 4 cells⟧"]);
    // The placeholders carry the image id in their foreground; the text
    // after them is in the paper's grey #1f1f1f again.
    let line = (out.lines())
        .find(|line| line.contains("Text after"))
        .expect("the text line");
    let before = &line[..line.find("Text after").unwrap()];
    let last = before.rfind("\x1b[").map_or("", |start| &before[start..]);
    assert!(last.contains("38;2;31;31;31"), "{line:?}");
}

#[test]
fn no_color_draws_images_without_other_colours() {
    let images = one_image("p.png", quadrants(40, 30));
    let caps = Capabilities {
        color: false,
        ..Capabilities::kitty(WIDTH)
    };
    for theme in [Theme::default(), example_theme("opaque.toml")] {
        let out = render_images_with_theme("Text\n\n![p](p.png)\n", &images, &caps, &theme);
        let sent = sent(&out);
        assert_eq!(sent.len(), 1);
        assert_eq!(drawn(&out), ["⟦row 0: 4 cells⟧", "⟦row 1: 4 cells⟧"]);
        // The only colour is the image id that kitty reads from the
        // placeholders' foreground.
        let id: u32 = sent[0].keys["i"].parse().unwrap();
        let [_, red, green, blue] = id.to_be_bytes();
        let id_colour = format!("38;2;{red};{green};{blue}");
        let codes = colour_codes(&out);
        assert_eq!(codes, [id_colour.clone(), id_colour], "{out:?}");
    }
}

#[test]
fn images_indent_with_margins_lists_and_quotes() {
    use kiri::render::wrap::width;
    let images = one_image("p.png", quadrants(40, 30));
    let source = "![top](p.png)\n\n- ![listed](p.png)\n\n> ![quoted](p.png)\n";
    let lines = kiri::render::layout::layout(
        &kiri::parse::parse(source),
        &images,
        &Capabilities::kitty(WIDTH),
        &Theme::default(),
    );
    let rows: Vec<(String, u16, u16)> = (lines.iter())
        .filter_map(|line| {
            let image = line.image.as_ref()?;
            let before: String = (line.lead.iter().chain(&line.spans))
                .map(|span| span.text.as_str())
                .collect();
            assert_eq!(width(&line.trail), 0);
            Some((before, image.row, image.placement.size.columns))
        })
        .collect();
    let expected = [
        ("  ", 0, 4),
        ("  ", 1, 4),
        ("  • ", 0, 4),
        ("    ", 1, 4),
        ("  │ ", 0, 4),
        ("  │ ", 1, 4),
    ]
    .map(|(before, row, columns)| (before.to_owned(), row, columns));
    assert_eq!(rows, expected);
}

#[test]
fn images_never_reach_past_the_width() {
    use kiri::render::wrap::width;
    let images = one_image("p.png", quadrants(2000, 100));
    let source = "![top](p.png)\n\n- ![listed](p.png)\n\n> > ![quoted](p.png)\n";
    let document = kiri::parse::parse(source);
    for terminal_width in [12, 16, 24, 40, 80] {
        let caps = Capabilities::kitty(terminal_width);
        let lines = kiri::render::layout::layout(&document, &images, &caps, &Theme::default());
        let image_rows: Vec<_> = lines.iter().filter(|line| line.image.is_some()).collect();
        let first_rows = image_rows
            .iter()
            .filter(|line| line.image.as_ref().unwrap().row == 0);
        assert_eq!(first_rows.count(), 3, "{lines:?}");
        for line in image_rows {
            let columns = line.image.as_ref().unwrap().placement.size.columns;
            let cells = width(&line.lead) + width(&line.spans) + usize::from(columns);
            assert!(cells <= usize::from(terminal_width), "{line:?}");
        }
    }
}

#[test]
fn images_taller_than_the_cell_limit_are_clamped() {
    // 2000 CSS pixels high, a hundred rows at their natural size.
    let tall = one_image("p.png", quadrants(400, 2000));
    let document = kiri::parse::parse("![tall](p.png)\n");
    let image_sizes = |caps: &Capabilities, images: &Images| -> Vec<(u16, u16)> {
        let lines = kiri::render::layout::layout(&document, images, caps, &Theme::default());
        (lines.iter())
            .filter_map(|line| line.image.as_ref())
            .filter(|image| image.row == 0)
            .map(|image| (image.placement.size.columns, image.placement.size.rows))
            .collect()
    };
    let limited = Capabilities {
        max_image_cells: 10,
        ..Capabilities::kitty(WIDTH)
    };
    // Ten rows of 20 pixels, and the width shrinks with them: 40 pixels.
    assert_eq!(image_sizes(&limited, &tall), [(4, 10)]);
    assert_eq!(image_sizes(&Capabilities::kitty(WIDTH), &tall), [(40, 100)]);
    // kitty's own limit, one row for each placeholder diacritic.
    let taller = one_image("p.png", quadrants(10, 8000));
    let max = DIACRITICS.len() as u16;
    assert_eq!(
        image_sizes(&Capabilities::kitty(WIDTH), &taller),
        [(1, max)]
    );
    let none = Capabilities {
        max_image_cells: 0,
        ..Capabilities::kitty(WIDTH)
    };
    assert_eq!(image_sizes(&none, &tall), []);
}

#[test]
fn an_image_is_sent_once_for_each_size_it_is_shown_at() {
    // As wide as the text at the top level, so it shrinks in the quote.
    let images = one_image("p.png", quadrants(440, 40));
    let source = "![a](p.png)\n\n![again](p.png)\n\n> ![quoted](p.png)\n";
    let out = render_with_images(source, &images, &Capabilities::kitty(WIDTH));
    let sent = sent(&out);
    let sizes: Vec<(&str, &str)> = (sent.iter())
        .map(|image| (image.keys["c"].as_str(), image.keys["r"].as_str()))
        .collect();
    assert_eq!(sizes, [("44", "2"), ("42", "2")]);
    assert_ne!(sent[0].keys["i"], sent[1].keys["i"]);
    assert_eq!(drawn(&out).len(), 6);
}

#[test]
fn sent_pixels_are_the_image_at_its_shown_size() {
    let images = one_image("p.png", quadrants(50, 30));
    // Cells 40 pixels high double the CSS pixels: 100 × 60 pixels in ten
    // columns and two rows, the last 20 pixels of which stay transparent.
    let caps = Capabilities {
        cell_pixels: Some((10, 40)),
        ..Capabilities::kitty(WIDTH)
    };
    let out = render_with_images("![p](p.png)\n", &images, &caps);
    let sent = sent(&out);
    assert_eq!(sent.len(), 1);
    let keys = &sent[0].keys;
    for (key, value) in [
        ("a", "T"),
        ("f", "32"),
        ("o", "z"),
        ("s", "100"),
        ("v", "80"),
        ("c", "10"),
        ("r", "2"),
    ] {
        assert_eq!(keys[key], value, "{key}");
    }
    let image = RgbaImage::from_raw(100, 80, sent[0].rgba.clone()).expect("100 × 80 RGBA");
    assert_eq!(image.get_pixel(25, 15).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(75, 15).0, [0, 255, 0, 255]);
    assert_eq!(image.get_pixel(25, 45).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(75, 45).0, [255, 255, 0, 255]);
    assert_eq!(image.get_pixel(50, 70).0[3], 0);
    assert_eq!(image.get_pixel(99, 79).0[3], 0);
    // The placeholders carry the id as their colour.
    let id: u32 = keys["i"].parse().unwrap();
    let [_, red, green, blue] = id.to_be_bytes();
    let colour = format!("\x1b[38;2;{red};{green};{blue}m{PLACEHOLDER}");
    assert_eq!(out.matches(&colour).count(), 2, "{out:?}");
}

#[test]
fn an_svg_is_drawn_at_its_shown_size_not_scaled() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="30">
        <rect width="25" height="30" fill="#ff0000"/>
        <rect x="25" width="25" height="30" fill="#0000ff"/></svg>"##;
    let source = kiri::images::decode(svg, DEFAULT_MAX_FILE_SIZE, &Fonts::none()).unwrap();
    let mut images = Images::new();
    images.insert("p.svg", Ok(source));
    // Cells 40 pixels high double the CSS pixels: 100 × 60 pixels. Drawn at
    // that size, the edge between the halves stays sharp; a scaled raster
    // would blur it.
    let caps = Capabilities {
        cell_pixels: Some((10, 40)),
        ..Capabilities::kitty(WIDTH)
    };
    let out = render_with_images("![p](p.svg)\n", &images, &caps);
    let sent = sent(&out);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].keys["s"].as_str(), sent[0].keys["v"].as_str()),
        ("100", "80")
    );
    let image = RgbaImage::from_raw(100, 80, sent[0].rgba.clone()).expect("100 × 80 RGBA");
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(49, 30).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(50, 30).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(99, 59).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(50, 60).0[3], 0);
    assert_eq!(drawn(&out), ["⟦row 0: 10 cells⟧", "⟦row 1: 10 cells⟧"]);
}
