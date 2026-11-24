use super::*;
use crate::typeset::model::AlertKind;
use crate::typeset::theme::{
    ContentsTheme, FileHeaderTheme, FormulaTheme, MarkTheme, SearchTheme, StatusLineTheme,
};

#[test]
fn the_documented_defaults_are_the_defaults() {
    let docs = include_str!("../../../../docs/theme.md");
    let defaults = docs
        .find("## Defaults")
        .expect("docs/theme.md has a Defaults section");
    let start = defaults
        + docs[defaults..]
            .find("```toml\n")
            .expect("the Defaults section has a TOML block")
        + 8;
    let end = start + docs[start..].find("```").expect("the TOML block ends");
    assert_eq!(parse(&docs[start..end]).unwrap(), Theme::default());
}

#[test]
fn the_status_line_takes_colours_none_and_auto() {
    let theme = parse(
        "[status_line]\nbackground = \"#101010\"\nforeground = \"none\"\n\
             badge_background = \"red\"\nbadge_foreground = \"auto\"\n",
    )
    .unwrap();
    assert_eq!(
        theme.status_line,
        StatusLineTheme {
            background: BarColor::Color(Some(Color::Rgb(16, 16, 16))),
            foreground: BarColor::Color(None),
            badge_background: BarColor::Color(Some(Color::Palette(1))),
            badge_foreground: BarColor::Auto,
        }
    );
    assert!(parse("[status_line]\nbar = \"red\"\n").is_err());
}

#[test]
fn the_contents_bar_takes_colours_none_and_auto() {
    let theme = parse(
        "[contents]\nbackground = \"#101010\"\ntitle = \"red\"\ntext = \"none\"\n\
             glyph = \"auto\"\ncurrent = \"bright-blue\"\ncurrent_background = \"234\"\n\
             selected = \"#ffffff\"\nborder = \"none\"\n",
    )
    .unwrap();
    let rgb = |level| ContentsColor::Color(Some(Color::Rgb(level, level, level)));
    assert_eq!(
        theme.contents,
        ContentsTheme {
            background: ContentsColor::Color(Some(Color::Rgb(16, 16, 16))),
            title: ContentsColor::Color(Some(Color::Palette(1))),
            text: ContentsColor::Color(None),
            glyph: ContentsColor::Auto,
            current: ContentsColor::Color(Some(Color::Palette(12))),
            current_background: ContentsColor::Color(Some(Color::Palette(234))),
            selected: rgb(255),
            border: ContentsColor::Color(None),
        }
    );
    // Left out, a key is auto.
    let theme = parse("[contents]\nborder = \"red\"\n").unwrap();
    assert_eq!(theme.contents.title, ContentsColor::Auto);
    assert_eq!(parse("").unwrap().contents, ContentsTheme::default());
    assert!(parse("[contents]\nwidth = 30\n").is_err());
}

#[test]
fn the_file_header_takes_a_band_colour_or_auto_and_a_bar_colour() {
    let theme = parse("[file_header]\nbackground = \"#303030\"\nbar = \"none\"\n").unwrap();
    assert_eq!(
        theme.file_header,
        FileHeaderTheme {
            background: BarColor::Color(Some(Color::Rgb(0x30, 0x30, 0x30))),
            bar: None,
        }
    );
    let theme = parse("[file_header]\nbackground = \"auto\"\nbar = \"red\"\n").unwrap();
    assert_eq!(theme.file_header.background, BarColor::Auto);
    assert_eq!(theme.file_header.bar, Some(Color::Palette(1)));
    assert!(parse("[file_header]\nbar = \"auto\"\n").is_err());
    assert!(parse("[file_header]\nname = \"bold\"\n").is_err());
}

#[test]
fn a_formula_takes_a_text_and_a_background_colour() {
    assert_eq!(Theme::default().formula, FormulaTheme::default());
    let theme = parse("[formula]\ntext = \"#102030\"\nbackground = \"blue\"\n").unwrap();
    assert_eq!(
        theme.formula,
        FormulaTheme {
            text: Some(Color::Rgb(0x10, 0x20, 0x30)),
            background: Some(Color::Palette(4)),
        }
    );
    let theme = parse("[formula]\ntext = \"none\"\n").unwrap();
    assert_eq!(theme.formula, FormulaTheme::default());
    let error = parse("[formula]\nbackground = \"pink\"\n").unwrap_err();
    assert!(error.contains("formula.background"), "{error}");
    assert!(parse("[formula]\nborder = \"red\"\n").is_err());
}

#[test]
fn search_matches_take_colours_none_and_auto() {
    let theme = parse(
        "[search]\nmatch_foreground = \"#101010\"\nmatch_background = \"none\"\n\
             current_foreground = \"auto\"\ncurrent_background = \"yellow\"\n",
    )
    .unwrap();
    assert_eq!(
        theme.search,
        SearchTheme {
            match_foreground: SearchColor::Color(Some(Color::Rgb(16, 16, 16))),
            match_background: SearchColor::Color(None),
            current_foreground: SearchColor::Auto,
            current_background: SearchColor::Color(Some(Color::Palette(3))),
        }
    );
    assert!(parse("[search]\nbackground = \"red\"\n").is_err());
    assert!(parse("[search]\nmatch_background = \"pink\"\n").is_err());
}

#[test]
fn search_has_no_flash_colours() {
    assert!(parse("[search]\nflash_foreground = \"black\"\n").is_err());
    assert!(parse("[search]\nflash_background = \"yellow\"\n").is_err());
}

#[test]
fn marked_text_is_black_on_yellow_unless_the_theme_says_otherwise() {
    let black_on_yellow = MarkTheme {
        foreground: Some(Color::Palette(0)),
        background: Some(Color::Palette(3)),
    };
    assert_eq!(Theme::default().mark, black_on_yellow);
    assert_eq!(
        parse("[mark]\nbackground = \"#fff8c5\"\n").unwrap().mark,
        MarkTheme {
            background: Some(Color::Rgb(0xff, 0xf8, 0xc5)),
            ..black_on_yellow
        }
    );
    assert_eq!(
        parse("[mark]\nforeground = \"none\"\nbackground = \"none\"\n")
            .unwrap()
            .mark,
        MarkTheme::default()
    );
    assert!(parse("[mark]\ncolor = \"red\"\n").is_err());
    assert!(parse("[mark]\nbackground = \"auto\"\n").is_err());
    assert_eq!(Theme::default().without_colors().mark, MarkTheme::default());
}

#[test]
fn an_empty_file_gives_the_defaults() {
    assert_eq!(parse("").unwrap(), Theme::default());
}

#[test]
fn overrides_only_the_given_keys() {
    let theme =
        parse("max_width = 0\nlink = \"#ff8000\"\n[h2]\nscale = 1\nrule = false\n").unwrap();
    assert_eq!(theme.max_width, 0);
    assert_eq!(theme.link, Some(Color::Rgb(255, 128, 0)));
    assert_eq!(theme.heading(2).size, TextSize::NORMAL);
    assert!(!theme.heading(2).rule);
    assert_eq!(theme.heading(2).color, Theme::default().heading(2).color);
    assert_eq!(theme.heading(1), Theme::default().heading(1));
}

#[test]
fn inline_code_can_stay_on_one_line() {
    assert!(Theme::default().inline_code_wrap);
    assert!(
        !parse("inline_code_wrap = false\n")
            .unwrap()
            .inline_code_wrap
    );
}

#[test]
fn the_table_style_is_a_frame_unless_the_theme_says_grid() {
    use crate::typeset::theme::TableStyle;
    assert_eq!(Theme::default().table_style, TableStyle::Frame);
    assert_eq!(parse("").unwrap().table_style, TableStyle::Frame);
    assert_eq!(
        parse("table_style = \"frame\"\n").unwrap().table_style,
        TableStyle::Frame
    );
    assert_eq!(
        parse("table_style = \"grid\"\n").unwrap().table_style,
        TableStyle::Grid
    );
    assert_eq!(
        parse("table_style = \"round\"\n").unwrap_err(),
        "unknown table_style \"round\"; use frame or grid"
    );
}

#[test]
fn the_language_label_can_be_hidden_or_coloured() {
    let theme = Theme::default();
    assert!(theme.show_language_label);
    assert_eq!(theme.language_label, LabelColor::CodeBlock);
    assert!(
        !parse("show_language_label = false\n")
            .unwrap()
            .show_language_label
    );
    let parsed = |value: &str| parse(&format!("language_label = \"{value}\"\n"));
    assert_eq!(
        parsed("code-block").unwrap().language_label,
        LabelColor::CodeBlock
    );
    assert_eq!(
        parsed("red").unwrap().language_label,
        LabelColor::Color(Some(Color::Palette(1)))
    );
    assert_eq!(
        parsed("none").unwrap().language_label,
        LabelColor::Color(None)
    );
    assert!(parsed("purple").is_err());
}

#[test]
fn alerts_take_github_colours_unless_the_theme_says_otherwise() {
    let alerts = Theme::default().alerts;
    assert!(alerts.tint);
    assert_eq!(
        alerts.color(AlertKind::Note, true),
        Some(Color::Rgb(0x44, 0x93, 0xf8))
    );
    assert_eq!(
        alerts.color(AlertKind::Caution, false),
        Some(Color::Rgb(0xd1, 0x24, 0x2f))
    );
    let alerts = parse(
        "[alerts]\ncolors = \"terminal\"\nbackground = \"none\"\n\
             tip = \"#123456\"\nwarning = \"none\"\nnote = \"auto\"\n",
    )
    .unwrap()
    .alerts;
    assert!(!alerts.tint);
    assert_eq!(alerts.color(AlertKind::Note, true), Some(Color::Palette(4)));
    assert_eq!(
        alerts.color(AlertKind::Caution, true),
        Some(Color::Palette(1))
    );
    assert_eq!(
        alerts.color(AlertKind::Tip, true),
        Some(Color::Rgb(0x12, 0x34, 0x56))
    );
    assert_eq!(alerts.color(AlertKind::Warning, true), None);
    for mistake in [
        "colors = \"pink\"",
        "background = \"blur\"",
        "note = \"nope\"",
    ] {
        assert!(
            parse(&format!("[alerts]\n{mistake}\n")).is_err(),
            "{mistake}"
        );
    }
    // Without colours, alerts have neither colours nor a tint.
    let plain = Theme::default().without_colors().alerts;
    assert!(!plain.tint);
    assert_eq!(plain.color(AlertKind::Important, true), None);
}

#[test]
fn headings_table_applies_to_all_levels_before_each_level() {
    let theme = parse("[headings]\nindent = 1\nrule = false\n[h2]\nindent = 2\n").unwrap();
    let indents: Vec<u8> = theme.headings.iter().map(|h| h.indent).collect();
    assert_eq!(indents, vec![1, 2, 1, 1, 1, 1]);
    assert!(theme.headings.iter().all(|h| !h.rule));
}

#[test]
fn parses_colour_names_and_numbers() {
    assert_eq!(parse_color("cyan"), Some(Some(Color::Palette(6))));
    assert_eq!(parse_color("bright-black"), Some(Some(Color::Palette(8))));
    assert_eq!(parse_color("208"), Some(Some(Color::Palette(208))));
    assert_eq!(parse_color("none"), Some(None));
    assert_eq!(
        parse_color("#1a2B3c"),
        Some(Some(Color::Rgb(0x1a, 0x2b, 0x3c)))
    );
    assert!(parse_color("#12345").is_none());
    assert!(parse_color("#+12345").is_none());
    assert!(parse_color("+12").is_none());
    assert!(parse_color("purple").is_none());
}

#[test]
fn accepts_every_bundled_code_theme_and_lists_them_on_a_mistake() {
    let names = [
        "InspiredGitHub",
        "Solarized (dark)",
        "Solarized (light)",
        "base16-eighties.dark",
        "base16-mocha.dark",
        "base16-ocean.dark",
        "base16-ocean.light",
    ];
    for name in names {
        let theme = parse(&format!("code_theme_light = \"{name}\"\n")).unwrap();
        assert_eq!(theme.code_theme_light, name);
    }
    assert_eq!(
        parse("code_theme_dark = \"nope\"\n").unwrap_err(),
        format!(
            "unknown code theme \"nope\" for code_theme_dark; available: {}",
            names.join(", ")
        )
    );
}

#[test]
fn a_heading_size_may_be_a_decimal() {
    let theme = parse("[h1]\nscale = 2.5\n[h2]\nscale = 3\n[h3]\nscale = 1.3\n").unwrap();
    let fraction = |size: TextSize| {
        let fraction = size.fraction().unwrap();
        (size.scale(), fraction.numerator(), fraction.denominator())
    };
    assert_eq!(fraction(theme.heading(1).size), (3, 5, 6));
    assert_eq!(theme.heading(1).size.align(), VerticalAlign::Bottom);
    assert_eq!(theme.heading(1).written_scale, None);
    assert_eq!(theme.heading(2).size, TextSize::new(3).unwrap());
    // 1.3 is drawn at 2 · 7/11, about 1.27, and the theme remembers
    // what was written.
    assert_eq!(fraction(theme.heading(3).size), (2, 7, 11));
    assert_eq!(theme.heading(3).written_scale.as_deref(), Some("1.3"));
    // A later table that gives an exact size forgets the rounding.
    let theme = parse("[headings]\nscale = 1.3\n[h3]\nscale = 2\n").unwrap();
    assert_eq!(theme.heading(1).written_scale.as_deref(), Some("1.3"));
    assert_eq!(theme.heading(3).written_scale, None);
}

#[test]
fn rejects_mistakes_with_a_reason() {
    assert_eq!(
        parse("[h1]\nscale = 9\n").unwrap_err(),
        "h1.scale must be between 0.5 and 7, not 9"
    );
    assert_eq!(
        parse("[h2]\nscale = 0.4\n").unwrap_err(),
        "h2.scale must be between 0.5 and 7, not 0.4"
    );
    assert!(parse("[h1]\nscale = \"big\"\n").is_err());
    assert!(
        parse("code_theme_dark = \"nope\"\n")
            .unwrap_err()
            .contains("InspiredGitHub")
    );
    assert!(parse("colour = \"blue\"\n").is_err());
}

#[test]
fn inline_code_takes_the_code_block_background_or_a_colour() {
    let parsed = |value: &str| {
        parse(&format!("inline_code_background = \"{value}\"\n"))
            .unwrap()
            .inline_code_background
    };
    assert_eq!(parsed("code-block"), CodeBackground::CodeBlock);
    assert_eq!(
        parsed("blue"),
        CodeBackground::Color(Some(Color::Palette(4)))
    );
    assert_eq!(parsed("none"), CodeBackground::Color(None));
}

#[test]
fn an_underline_or_code_theme_that_is_wrong_names_its_key() {
    let error = |file: &str| parse(&format!("{file}\n")).unwrap_err();
    let underline = "; use none, single, double, curly, dotted or dashed";
    assert_eq!(
        error("[h2]\nunderline = \"wavy\""),
        format!("unknown underline \"wavy\" for h2.underline{underline}")
    );
    assert_eq!(
        error("[headings]\nunderline = \"wavy\""),
        format!("unknown underline \"wavy\" for headings.underline{underline}")
    );
    for key in ["code_theme_dark", "code_theme_light"] {
        let error = error(&format!("{key} = \"nope\""));
        let start = format!("unknown code theme \"nope\" for {key}; available: ");
        assert!(error.starts_with(&start), "{error}");
    }
}

#[test]
fn a_colour_that_is_wrong_names_its_key() {
    for (file, key) in [
        ("background = \"pink\"", "background"),
        ("muted = \"pink\"", "muted"),
        ("[headings]\ncolor = \"pink\"", "headings.color"),
        ("[h2]\nbackground = \"pink\"", "h2.background"),
        ("[mark]\nforeground = \"pink\"", "mark.foreground"),
        (
            "inline_code_background = \"pink\"",
            "inline_code_background",
        ),
        ("language_label = \"pink\"", "language_label"),
        ("[alerts]\nnote = \"pink\"", "alerts.note"),
        (
            "[status_line]\nbackground = \"pink\"",
            "status_line.background",
        ),
        (
            "[search]\nmatch_background = \"pink\"",
            "search.match_background",
        ),
        ("[contents]\nborder = \"pink\"", "contents.border"),
        (
            "[contents]\ncurrent_background = \"pink\"",
            "contents.current_background",
        ),
    ] {
        let unknown = format!(
            "unknown colour \"pink\" for {key}; use a name such as blue or \
                 bright-blue, 0-255, #rrggbb or none"
        );
        assert_eq!(parse(&format!("{file}\n")).unwrap_err(), unknown, "{file}");
    }
}
