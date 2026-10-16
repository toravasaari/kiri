use std::path::Path;

use kiri::term::Capabilities;
use kiri::theme::Theme;

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
    kiri::render::render(&document, caps, theme, &mut out).expect("rendering to memory succeeds");
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
            let lines = kiri::render::layout::layout(&document, &caps, theme);
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
    kiri::render::render(&document, &Capabilities::ansi(30), &theme, &mut out).unwrap();
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
    kiri::render::render(&document, &Capabilities::ansi(40), &theme, &mut out).unwrap();
    // The indent stays on the top-level heading only; nested ones start
    // right after the quote bar or the bullet.
    insta::assert_snapshot!(visible(&String::from_utf8(out).unwrap()));
}

fn render_source(source: &str, caps: &Capabilities, theme: &Theme) -> String {
    let document = kiri::parse::parse(source);
    let mut out = Vec::new();
    kiri::render::render(&document, caps, theme, &mut out).unwrap();
    String::from_utf8(out).unwrap()
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
        kiri::render::render(&document, &caps, &Theme::default(), &mut out).unwrap();
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
