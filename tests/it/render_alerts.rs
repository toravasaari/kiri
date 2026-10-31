//! GitHub alert blocks: `> [!NOTE]` and the other kinds.

use kiri::render::Inputs;
use kiri::typeset::caps::Capabilities;
use kiri::typeset::color::Color;
use kiri::typeset::theme::{Paper, Theme};

use crate::common::{
    WIDTH, fixed_inputs, fixed_theme, inputs, render, render_fixture, strip_own_sequences, visible,
};

const NOTE: &str = "> [!NOTE]\n> Read this.\n";

/// kitty on a black background whose palette blue is #0000ee.
fn kitty() -> Capabilities {
    let mut palette = [None; 16];
    palette[4] = Some(Color::Rgb(0, 0, 0xee));
    Capabilities {
        background: Some(Color::Rgb(0, 0, 0)),
        dark_background: Some(true),
        palette,
        ..Capabilities::kitty(WIDTH)
    }
}

fn text(source: &str, caps: Capabilities, theme: &Theme) -> Vec<String> {
    let out = strip_own_sequences(&render(source, &inputs(&caps, theme)));
    out.lines().map(str::to_owned).collect()
}

#[test]
fn plain_output_shows_the_title_in_a_quote() {
    let lines = text(NOTE, Capabilities::plain(WIDTH), &fixed_theme());
    assert_eq!(lines, ["  > Note", "  > Read this."]);
}

#[test]
fn kitty_shows_the_kinds_icon_before_the_title() {
    let lines = text(NOTE, kitty(), &fixed_theme());
    // An Octicon from the font kitty bundles; it covers the first space.
    assert!(lines[0].starts_with("  ▎ \u{f449}  Note"), "{lines:#?}");
    assert!(lines[1].starts_with("  ▎ Read this."), "{lines:#?}");
    // The tint fills the block to its right edge.
    assert_eq!(
        lines[0].chars().count(),
        lines[1].chars().count(),
        "{lines:#?}"
    );
    assert_eq!(
        lines[1].chars().count(),
        usize::from(WIDTH) - 2,
        "{lines:#?}"
    );
}

#[test]
fn other_terminals_get_no_icon() {
    let caps = Capabilities {
        background: Some(Color::Rgb(0, 0, 0)),
        ..Capabilities::ansi(WIDTH)
    };
    let lines = text(NOTE, caps, &fixed_theme());
    assert!(lines[0].starts_with("  ▎ Note"), "{lines:#?}");
}

#[test]
fn a_title_of_its_own_replaces_the_kinds_name() {
    let lines = text(
        "> [!WARNING] Mind the gap\n> Text.\n",
        kitty(),
        &fixed_theme(),
    );
    assert!(
        lines[0].starts_with("  ▎ \u{f421}  Mind the gap"),
        "{lines:#?}"
    );
}

#[test]
fn the_tint_mixes_the_kinds_colour_into_the_background() {
    let out = render(NOTE, &inputs(&kitty(), &fixed_theme()));
    // 15 % of GitHub's dark note blue, #4493f8, over black.
    assert!(out.contains("48;2;10;22;37"), "{out:?}");
    // The bar and the title are in the blue itself.
    assert!(out.contains("38;2;68;147;248"), "{out:?}");
}

#[test]
fn inline_code_in_an_alert_takes_a_stronger_tint() {
    let out = render(
        "> [!NOTE]\n> Run `make`.\n",
        &inputs(&kitty(), &fixed_theme()),
    );
    // 30 % of #4493f8 over black.
    assert!(out.contains("48;2;20;44;74m\u{a0}make"), "{out:?}");
    // A theme that gives inline code no background keeps it so.
    let theme = kiri::typeset::theme::parse("inline_code_background = \"none\"\n").unwrap();
    let out = render("> [!NOTE]\n> Run `make`.\n", &inputs(&kitty(), &theme));
    assert!(!out.contains("48;2;20;44;74"), "{out:?}");
}

#[test]
fn the_terminal_palette_gives_the_colours_when_the_theme_says_so() {
    let theme = kiri::typeset::theme::parse("[alerts]\ncolors = \"terminal\"\n").unwrap();
    let out = render(NOTE, &inputs(&kitty(), &theme));
    assert!(out.contains("\x1b[0;34"), "{out:?}");
    // 15 % of the palette's #0000ee over black.
    assert!(out.contains("48;2;0;0;36"), "{out:?}");
    // A palette the terminal did not tell leaves the tint out.
    let unknown = Capabilities {
        palette: [None; 16],
        ..kitty()
    };
    let out = render(NOTE, &inputs(&unknown, &theme));
    assert!(!out.contains("48;2"), "{out:?}");
}

#[test]
fn no_tint_without_a_known_background_or_when_the_theme_turns_it_off() {
    let unknown = Capabilities {
        background: None,
        ..kitty()
    };
    let out = render(NOTE, &inputs(&unknown, &fixed_theme()));
    assert!(!out.contains("48;2"), "{out:?}");
    let off = kiri::typeset::theme::parse("[alerts]\nbackground = \"none\"\n").unwrap();
    let out = render(NOTE, &inputs(&kitty(), &off));
    assert!(!out.contains("48;2"), "{out:?}");
    assert!(out.contains("38;2;68;147;248"), "the colour stays: {out:?}");
}

#[test]
fn paper_tints_its_own_page() {
    // The light page is white, whatever the terminal's background.
    let caps = Capabilities {
        background: None,
        dark_background: Some(false),
        ..kitty()
    };
    let out = render(NOTE, &inputs(&caps, &Theme::paper(Paper::Light)));
    // GitHub's pale blue over the white page.
    assert!(out.contains("48;2;221;244;255"), "{out:?}");
}

/// kitty on a white background.
fn light_kitty() -> Capabilities {
    Capabilities {
        background: Some(Color::Rgb(0xff, 0xff, 0xff)),
        dark_background: Some(false),
        ..kitty()
    }
}

#[test]
fn a_tinted_alert_ends_with_a_blank_row_of_its_tint() {
    let lines = text(NOTE, kitty(), &fixed_theme());
    assert_eq!(lines.len(), 3, "{lines:#?}");
    assert_eq!(lines[2].trim_end(), "  ▎", "{lines:#?}");
    assert_eq!(
        lines[2].chars().count(),
        usize::from(WIDTH) - 2,
        "{lines:#?}"
    );
    // Without a tint there is nothing to pad.
    let off = kiri::typeset::theme::parse("[alerts]\nbackground = \"none\"\n").unwrap();
    let lines = text(NOTE, kitty(), &off);
    assert_eq!(lines.len(), 2, "{lines:#?}");
}

#[test]
fn a_light_page_tints_alerts_with_githubs_pale_backgrounds() {
    // A plain mix of a dark colour with white is dusty: the warning would
    // be beige. GitHub's own pale backgrounds are used instead.
    for (source, tint) in [
        ("> [!NOTE]\n> x\n", "48;2;221;244;255"),
        ("> [!TIP]\n> x\n", "48;2;218;251;225"),
        ("> [!IMPORTANT]\n> x\n", "48;2;251;239;255"),
        ("> [!WARNING]\n> x\n", "48;2;255;248;197"),
        ("> [!CAUTION]\n> x\n", "48;2;255;235;233"),
    ] {
        let out = render(source, &inputs(&light_kitty(), &fixed_theme()));
        assert!(out.contains(tint), "{source:?}: {out:?}");
    }
    // Inline code in an alert takes the same tint twice.
    let out = render(
        "> [!WARNING]\n> Run `make`.\n",
        &inputs(&light_kitty(), &fixed_theme()),
    );
    assert!(out.contains("48;2;255;241;152m\u{a0}make"), "{out:?}");
}

#[test]
fn a_warning_on_a_light_page_has_a_gold_bar_and_title() {
    let out = render(
        "> [!WARNING]\n> x\n",
        &inputs(&light_kitty(), &fixed_theme()),
    );
    // GitHub's #9a6700 reads as brown on the pale yellow.
    assert!(!out.contains("38;2;154;103;0"), "{out:?}");
    assert!(out.contains("38;2;191;135;0"), "the bar: {out:?}");
    assert!(out.contains("38;2;172;122;0"), "the title: {out:?}");
}

#[test]
fn a_warning_without_a_tint_keeps_githubs_colour_on_a_light_page() {
    // The gold is there for the pale yellow; on the plain page it would
    // only lower the contrast.
    let off = kiri::typeset::theme::parse("[alerts]\nbackground = \"none\"\n").unwrap();
    let out = render("> [!WARNING]\n> x\n", &inputs(&light_kitty(), &off));
    assert!(out.contains("38;2;154;103;0"), "{out:?}");
    assert!(!out.contains("38;2;191;135;0"), "{out:?}");
}

#[test]
fn a_colour_the_theme_sets_is_mixed_with_a_light_page() {
    let theme = kiri::typeset::theme::parse("[alerts]\nwarning = \"#000000\"\n").unwrap();
    let out = render("> [!WARNING]\n> x\n", &inputs(&light_kitty(), &theme));
    // 15 % of black over white.
    assert!(out.contains("48;2;217;217;217"), "{out:?}");
}

#[test]
fn kitty_alerts() {
    insta::assert_snapshot!(visible(&render_fixture(
        "alerts.md",
        &fixed_inputs(kitty())
    )));
}

#[test]
fn a_long_title_wraps_under_itself_inside_the_block() {
    let source = "> [!NOTE] A title far too long to fit on one line of the block\n> x\n";
    let lines = text(source, kitty(), &fixed_theme());
    assert!(lines[0].starts_with("  ▎ \u{f449}  A title"), "{lines:#?}");
    // Continued under the title's text, past the icon.
    assert!(lines[1].starts_with("  ▎    "), "{lines:#?}");
    assert!(
        lines[1]
            .trim_start_matches(['▎', ' '])
            .starts_with(char::is_alphabetic),
        "{lines:#?}"
    );
    // The title's two rows, the text and the blank row under it.
    assert_eq!(lines.len(), 4, "{lines:#?}");
    for line in &lines {
        assert_eq!(line.chars().count(), usize::from(WIDTH) - 2, "{lines:#?}");
    }
}

/// The cells each line of `source` covers, with `inputs`: the margin, the
/// text, an image row and the fill after it.
fn widths(source: &str, inputs: &Inputs) -> Vec<usize> {
    use kiri::typeset::wrap::width;
    let lines = kiri::typeset::layout::layout(&kiri::typeset::parse::parse(source), inputs);
    lines
        .iter()
        .filter(|line| !line.is_blank())
        .map(|line| {
            let image = line
                .image
                .as_ref()
                .map_or(0, |row| row.placement.size.columns);
            width(&line.lead) + width(&line.spans) + usize::from(image) + width(&line.trail)
        })
        .collect()
}

#[test]
fn blocks_inside_an_alert_keep_the_bar_and_fill_it_to_the_edge() {
    let source = "> [!TIP]\n> Text 日本語 with wide characters.\n>\n\
                  > - a list\n>   - nested\n>\n> ```rust\n> fn main() {}\n> ```\n>\n\
                  > | a | b |\n> |---|---|\n> | 1 | 2 |\n>\n> > a quote\n>\n> ![alt](image.png)\n";
    let inputs = Inputs {
        images: crate::common::one_image("image.png", crate::common::quadrants(40, 40)),
        ..fixed_inputs(kitty())
    };
    let widths = widths(source, &inputs);
    assert!(
        widths.iter().all(|&w| w == usize::from(WIDTH) - 2),
        "{widths:?}"
    );
    let laid_out = kiri::typeset::layout::layout(&kiri::typeset::parse::parse(source), &inputs);
    assert!(laid_out.iter().any(|line| line.image.is_some()), "no image");
    let lines = text(source, kitty(), &fixed_theme());
    assert!(
        lines.iter().all(|line| line.starts_with("  ▎ ")),
        "{lines:#?}"
    );
    // A blank line between the blocks keeps the bar and the tint.
    assert!(
        lines.iter().any(|line| line.trim_end() == "  ▎"),
        "{lines:#?}"
    );
    // The code block keeps its own background.
    let out = render(source, &inputs);
    assert!(out.contains("48;2;43;48;59"), "{out:?}");
}

#[test]
fn an_alert_inside_a_list_or_a_quote_fills_its_own_width() {
    for source in [
        "- item\n\n  > [!NOTE]\n  > Inside a list item.\n",
        "> outer\n>\n> > [!NOTE]\n> > Inside a quote.\n",
    ] {
        let lines = text(source, kitty(), &fixed_theme());
        let alert: Vec<&String> = lines.iter().filter(|line| line.contains('▎')).collect();
        assert_eq!(alert.len(), 3, "{lines:#?}");
        assert!(alert[0].contains("▎ \u{f449}  Note"), "{lines:#?}");
        // The tint ends where the list item or the quote ends.
        assert!(
            alert
                .iter()
                .all(|line| line.chars().count() == usize::from(WIDTH) - 2),
            "{lines:#?}"
        );
    }
}

#[test]
fn a_narrow_alert_stays_within_its_width() {
    let caps = Capabilities {
        width: 12,
        ..kitty()
    };
    let widths = widths(
        "> [!IMPORTANT]\n> Several words that wrap.\n",
        &fixed_inputs(caps),
    );
    assert!(widths.iter().all(|&w| w <= 12), "{widths:?}");
}

#[test]
fn no_color_leaves_the_bar_and_the_title_without_colours() {
    let caps = Capabilities {
        color: false,
        ..kitty()
    };
    let out = render(NOTE, &inputs(&caps, &fixed_theme()));
    assert!(crate::common::colour_codes(&out).is_empty(), "{out:?}");
    let lines = text(NOTE, caps, &fixed_theme());
    assert!(lines[0].starts_with("  ▎ \u{f449}  Note"), "{lines:#?}");
    assert!(lines[1].starts_with("  ▎ Read this."), "{lines:#?}");
}

/// The background in force where the first row of an image in `out`
/// starts, as RGB, from the SGR sequences before its first placeholder;
/// `None` for the terminal's own.
fn background_at_image(out: &str) -> Option<[u8; 3]> {
    let image = out
        .find(kiri::present::kitty::graphics::PLACEHOLDER)
        .expect("an image row");
    let mut background = None;
    for sequence in out[..image].split("\x1b[").skip(1) {
        let Some(end) = sequence.find('m') else {
            continue;
        };
        let params: Vec<&str> = sequence[..end].split(';').collect();
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                "" | "0" | "49" => background = None,
                "48" if params.get(i + 1) == Some(&"2") => {
                    let channel = |at: usize| params[i + at].parse().unwrap();
                    background = Some([channel(2), channel(3), channel(4)]);
                    i += 4;
                }
                "38" | "58" if params.get(i + 1) == Some(&"2") => i += 4,
                _ => {}
            }
            i += 1;
        }
    }
    background
}

#[test]
fn an_image_in_an_alert_lies_on_the_alert_s_tint() {
    // A transparent image shows what is behind its cells: the tint, as
    // on the rest of the alert's lines, not the page.
    let inputs = Inputs {
        images: crate::common::one_image("a.png", crate::common::quadrants(40, 40)),
        ..fixed_inputs(kitty())
    };
    let out = render("> [!NOTE]\n> ![a](a.png)\n", &inputs);
    assert_eq!(
        background_at_image(&out),
        Some([10, 22, 37]),
        "{}",
        visible(&out)
    );
    // Outside an alert, on the page.
    let out = render("![a](a.png)\n", &inputs);
    assert_eq!(background_at_image(&out), None, "{}", visible(&out));
}

#[test]
fn a_long_title_wraps_inside_the_block_without_styling_too() {
    let source = "> [!TIP] A title of its own\n> Text.\n";
    let lines = text(source, Capabilities::plain(16), &fixed_theme());
    assert!(
        lines.iter().all(|line| line.chars().count() <= 16),
        "{lines:?}"
    );
    assert_eq!(lines, ["  > A title of", "  > its own", "  > Text."]);
}
