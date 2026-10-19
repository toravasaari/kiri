//! The paper themes: the document as a page of fixed greys.

use super::{CodeBackground, SearchColor, SearchTheme, StatusColor, StatusLineTheme, Theme};
use crate::color::Color;

/// The built-in themes that draw the document as a page of fixed colours,
/// whatever the terminal's palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paper {
    Light,
    Dark,
}

/// The greys of a paper theme, from the page to the text.
struct PaperColors {
    background: Option<Color>,
    text: Option<Color>,
    /// Alt text, raw HTML and link targets.
    muted: Option<Color>,
    /// Headings and links.
    strong: Option<Color>,
    /// The smallest heading.
    quiet: Option<Color>,
    marker: Option<Color>,
    border: Option<Color>,
    /// Quote bars and rules.
    line: Option<Color>,
    code_background: Option<Color>,
    code_theme: &'static str,
    /// The pager's status line and the text on it.
    status: Option<Color>,
    status_text: Option<Color>,
    /// Search matches in the pager and the text on them.
    found: Option<Color>,
    found_text: Option<Color>,
}

impl Theme {
    /// A light or dark page in greys, with colour only in code blocks.
    pub fn paper(paper: Paper) -> Self {
        let grey = |level: u8| Some(Color::Rgb(level, level, level));
        let colors = match paper {
            Paper::Light => PaperColors {
                background: grey(0xff),
                text: grey(0x1f),
                muted: grey(0x6e),
                strong: grey(0x00),
                quiet: grey(0x55),
                marker: grey(0x70),
                border: grey(0xb4),
                line: grey(0xc8),
                code_background: grey(0xeb),
                code_theme: "base16-ocean.light",
                status: grey(0xe0),
                status_text: grey(0x1f),
                found: grey(0xcc),
                found_text: grey(0x00),
            },
            Paper::Dark => PaperColors {
                background: grey(0x1e),
                text: grey(0xd4),
                muted: grey(0x8e),
                strong: grey(0xff),
                quiet: grey(0xa0),
                marker: grey(0x9a),
                border: grey(0x5a),
                line: grey(0x50),
                code_background: grey(0x35),
                code_theme: "base16-eighties.dark",
                status: grey(0x3a),
                status_text: grey(0xe0),
                found: grey(0x60),
                found_text: grey(0xff),
            },
        };
        let mut theme = Self {
            background: colors.background,
            foreground: colors.text,
            muted: colors.muted,
            link: colors.strong,
            inline_code: colors.text,
            inline_code_background: CodeBackground::Color(colors.code_background),
            quote_bar: colors.line,
            rule: colors.line,
            list_marker: colors.marker,
            table_border: colors.border,
            // The page is the same whatever the terminal's background.
            code_theme_dark: colors.code_theme.into(),
            code_theme_light: colors.code_theme.into(),
            // The badge is the page's text and background swapped.
            status_line: StatusLineTheme {
                background: StatusColor::Color(colors.status),
                foreground: StatusColor::Color(colors.status_text),
                badge_background: StatusColor::Color(colors.text),
                badge_foreground: StatusColor::Color(colors.background),
            },
            // The current match is in the badge's colours.
            search: SearchTheme {
                match_foreground: SearchColor::Color(colors.found_text),
                match_background: SearchColor::Color(colors.found),
                current_foreground: SearchColor::Color(colors.background),
                current_background: SearchColor::Color(colors.text),
            },
            ..Self::default()
        };
        for heading in &mut theme.headings {
            heading.color = colors.strong;
        }
        theme.headings[5].color = colors.quiet;
        theme
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::parse;

    #[test]
    fn paper_themes_mark_matches_in_greys_and_no_colour_in_reverse_video() {
        let grey = |level| SearchColor::Color(Some(Color::Rgb(level, level, level)));
        let light = Theme::paper(Paper::Light).search;
        assert_eq!(
            [light.match_foreground, light.match_background],
            [grey(0x00), grey(0xcc)]
        );
        assert_eq!(
            [light.current_foreground, light.current_background],
            [grey(0xff), grey(0x1f)]
        );
        let dark = Theme::paper(Paper::Dark).search;
        assert_eq!(
            [dark.match_foreground, dark.match_background],
            [grey(0xff), grey(0x60)]
        );
        assert_eq!(
            [dark.current_foreground, dark.current_background],
            [grey(0x1e), grey(0xd4)]
        );
        let plain = Theme::paper(Paper::Dark).without_colors().search;
        assert_eq!(plain, SearchTheme::default());
    }

    /// The TOML block after `heading` in docs/theme.md.
    fn documented(heading: &str) -> &'static str {
        let docs = include_str!("../../docs/theme.md");
        let section = docs
            .find(heading)
            .unwrap_or_else(|| panic!("docs/theme.md has {heading}"));
        let start = section
            + docs[section..]
                .find("```toml\n")
                .expect("the section has a TOML block")
            + 8;
        let end = start + docs[start..].find("```").expect("the TOML block ends");
        &docs[start..end]
    }

    #[test]
    fn the_documented_paper_themes_are_the_paper_themes() {
        assert_eq!(
            parse(documented("### Light paper")).unwrap(),
            Theme::paper(Paper::Light)
        );
        assert_eq!(
            parse(documented("### Dark paper")).unwrap(),
            Theme::paper(Paper::Dark)
        );
    }

    /// Every colour of `theme` outside code blocks.
    fn colors(theme: &Theme) -> Vec<Option<Color>> {
        let mut colors = vec![
            theme.background,
            theme.foreground,
            theme.muted,
            theme.link,
            theme.inline_code,
            theme.quote_bar,
            theme.rule,
            theme.list_marker,
            theme.table_border,
        ];
        match theme.inline_code_background {
            CodeBackground::Color(color) => colors.push(color),
            CodeBackground::CodeBlock => panic!("inline code takes the code block's colour"),
        }
        for heading in &theme.headings {
            colors.push(heading.color);
            colors.push(heading.background);
        }
        colors
    }

    fn lightness(color: Option<Color>) -> u8 {
        match color {
            Some(Color::Rgb(r, g, b)) if r == g && g == b => r,
            _ => panic!("{color:?} is not a fixed grey"),
        }
    }

    #[test]
    fn paper_themes_are_fixed_greys_outside_code_blocks() {
        for paper in [Paper::Light, Paper::Dark] {
            let theme = Theme::paper(paper);
            // Anything else may be none: the page's colours show then.
            for (index, color) in colors(&theme).into_iter().enumerate() {
                if color.is_some() || index < 2 {
                    lightness(color);
                }
            }
        }
    }

    #[test]
    fn light_paper_is_dark_text_on_a_light_page_and_dark_paper_the_reverse() {
        let light = Theme::paper(Paper::Light);
        assert!(lightness(light.background) > 0xf0);
        assert!(lightness(light.foreground) < 0x40);
        let dark = Theme::paper(Paper::Dark);
        assert!(lightness(dark.background) < 0x30);
        assert!(lightness(dark.foreground) > 0xc0);
    }

    /// The WCAG contrast ratio of two greys.
    fn contrast(a: Option<Color>, b: Option<Color>) -> f64 {
        let luminance = |color| {
            let c = f64::from(lightness(color)) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn paper_themes_mute_text_in_a_readable_grey() {
        for paper in [Paper::Light, Paper::Dark] {
            let theme = Theme::paper(paper);
            let ratio = contrast(theme.muted, theme.background);
            assert!(ratio >= 4.5, "{paper:?}: {ratio}");
            // Muted, but not as strong as the text.
            assert!(ratio < contrast(theme.foreground, theme.background));
        }
    }

    #[test]
    fn paper_themes_highlight_code_to_match_the_page() {
        let light = Theme::paper(Paper::Light);
        assert_eq!(light.code_theme_light, "base16-ocean.light");
        assert_eq!(light.code_theme_dark, light.code_theme_light);
        let dark = Theme::paper(Paper::Dark);
        assert_eq!(dark.code_theme_dark, "base16-eighties.dark");
        assert_eq!(dark.code_theme_light, dark.code_theme_dark);
    }
}
