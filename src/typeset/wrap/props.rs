//! Property tests of `wrap`: whatever the text and the width, the lines fit,
//! keep every character that is not a space, and collapse spaces as the
//! style says. They are the safety net of a faster wrapper.

use proptest::prelude::*;

use super::*;
use crate::testing::props_config;

/// Pieces of text that reach the wrapper's cases: words, wide, combining and
/// joined characters, long words, spaces and newlines.
fn piece() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => "[a-z]{1,8}",
        2 => Just(" ".to_owned()),
        1 => Just("   ".to_owned()),
        1 => Just("\n".to_owned()),
        1 => Just("漢字かな".to_owned()),
        1 => Just("😀".to_owned()),
        1 => Just("e\u{301}\u{301}".to_owned()),
        1 => Just("👨\u{200d}👩\u{200d}👧".to_owned()),
        1 => Just("🇫🇮".to_owned()),
        1 => Just("a\u{200b}b".to_owned()),
        1 => Just("https://example.com/a/very/long/path?with=query&and=more#anchor".to_owned()),
        1 => "\\PC{1,6}",
    ]
}

fn style(spaces: Spaces) -> Style {
    Style {
        spaces,
        ..Style::default()
    }
}

fn any_spaces() -> impl Strategy<Value = Spaces> {
    prop_oneof![
        3 => Just(Spaces::Collapse),
        1 => Just(Spaces::Keep),
        1 => Just(Spaces::KeepTogether),
    ]
}

/// Spans of text, each in a style of its own.
fn spans() -> impl Strategy<Value = Vec<Span>> {
    proptest::collection::vec((piece(), any_spaces()), 0..14).prop_map(|parts| {
        (parts.into_iter())
            .map(|(text, spaces)| Span::new(&text, style(spaces)))
            .collect()
    })
}

/// Spans in the collapsing style only.
fn collapsing_spans() -> impl Strategy<Value = Vec<Span>> {
    proptest::collection::vec(piece(), 0..14).prop_map(|parts| {
        (parts.into_iter())
            .map(|text| Span::new(&text, style(Spaces::Collapse)))
            .collect()
    })
}

fn text(line: &[Span]) -> String {
    line.iter().map(|span| span.text.as_str()).collect()
}

fn letters(text: &str) -> Vec<char> {
    text.chars().filter(|c| !matches!(c, ' ' | '\n')).collect()
}

proptest! {
    #![proptest_config(props_config())]

    #[test]
    fn lines_fit_unless_one_grapheme_is_wider_than_the_width(
        spans in spans(),
        max in 1usize..40,
    ) {
        for line in wrap(&spans, max) {
            // A grapheme wider than the width stands alone, but a
            // grapheme without width, as a lone combining mark or a
            // zero-width space, may share its line.
            if width(&line) > max {
                let shown = text(&line);
                let wide = shown.graphemes(true).filter(|g| g.width() > 0).count();
                prop_assert_eq!(wide, 1, "line {:?} is wider than {}", shown, max);
            }
        }
    }

    #[test]
    fn the_lines_keep_every_character_that_is_not_a_space(
        spans in spans(),
        max in 1usize..40,
    ) {
        let input: String = spans.iter().map(|span| span.text.as_str()).collect();
        let lines: String = wrap(&spans, max).iter().map(|line| text(line)).collect();
        prop_assert_eq!(letters(&lines), letters(&input));
    }

    #[test]
    fn collapsing_spans_wrap_without_stray_spaces(
        spans in collapsing_spans(),
        max in 1usize..40,
    ) {
        for line in wrap(&spans, max) {
            let line = text(&line);
            prop_assert!(!line.starts_with(' '), "{line:?} starts with a space");
            prop_assert!(!line.ends_with(' '), "{line:?} ends with a space");
            prop_assert!(!line.contains("  "), "{line:?} has two spaces in a row");
        }
    }

    #[test]
    fn there_is_a_line_for_each_newline(spans in spans(), max in 1usize..40) {
        let newlines: usize = spans.iter().map(|span| span.text.matches('\n').count()).sum();
        prop_assert!(wrap(&spans, max).len() > newlines.saturating_sub(1));
    }
}
