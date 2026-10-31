//! The warning line: one line just above the status line that tells the
//! first of the document's warnings, right-aligned.

use unicode_width::UnicodeWidthStr;

use crate::typeset::style::{self, Span, Style};
use crate::typeset::wrap;

use crate::typeset::diagnostic::Diagnostic;

/// The spans of the warning line for `warnings` in a window `columns`
/// wide, with the first warning in `red` and how many more there are in
/// `dim`; empty when there are none.
pub fn spans(warnings: &[Diagnostic], columns: usize, red: &Style, dim: &Style) -> Vec<Span> {
    let Some(first) = warnings.first() else {
        return Vec::new();
    };
    let place = match first.line {
        Some(line) => format!(" [:{line}]"),
        None => String::new(),
    };
    let message = format!("\u{26a0}{place} {}", first.pager_message());
    let more = warnings.len() - 1;
    let tail = match more {
        0 => String::new(),
        1 => "  and 1 more warning\u{2026}".to_owned(),
        more => format!("  and {more} more warnings\u{2026}"),
    };
    // A cell of room at each end, as the status line has at its end.
    let room = columns.saturating_sub(2);
    let message = head(&message, room.saturating_sub(tail.width()));
    let mut spans = vec![Span::new(&message, red.clone())];
    if !tail.is_empty() {
        let italic = Style {
            italic: true,
            ..dim.clone()
        };
        spans.push(Span::new(&tail, italic));
    }
    wrap::cut(spans, room)
}

/// The start of `text` in at most `max` cells, before `\u{2026}` when its
/// end is left out.
fn head(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_owned();
    }
    let kept = wrap::cut(
        vec![Span::new(text, Style::default())],
        max.saturating_sub(1),
    );
    format!("{}\u{2026}", style::text(&kept).trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::diagnostic::Code;
    use crate::typeset::style::text;

    fn warning(line: Option<usize>, message: &str) -> Diagnostic {
        Diagnostic {
            line,
            ..Diagnostic::new(Code::Image, message)
        }
    }

    fn styles() -> (Style, Style) {
        let red = Style::fg(Some(crate::typeset::color::Color::Palette(1)));
        let dim = Style {
            italic: true,
            ..Style::fg(Some(crate::typeset::color::Color::Palette(8)))
        };
        (red, dim)
    }

    fn shown(warnings: &[Diagnostic], columns: usize) -> Vec<Span> {
        let (red, dim) = styles();
        spans(warnings, columns, &red, &dim)
    }

    #[test]
    fn the_line_names_the_file_line_when_it_is_known() {
        let spans = shown(&[warning(Some(3), "front matter: bad")], 60);
        assert_eq!(text(&spans), "\u{26a0} [:3] front matter: bad");
        let spans = shown(&[warning(None, "front matter: bad")], 60);
        assert_eq!(text(&spans), "\u{26a0} front matter: bad");
    }

    #[test]
    fn nothing_is_shown_without_warnings() {
        assert!(shown(&[], 60).is_empty());
    }

    #[test]
    fn more_warnings_are_counted_in_the_dim_style_after_the_first() {
        let more = [
            warning(Some(3), "a"),
            warning(None, "b"),
            warning(None, "c"),
        ];
        let spans = shown(&more, 60);
        assert_eq!(text(&spans), "\u{26a0} [:3] a  and 2 more warnings\u{2026}");
        let (red, dim) = styles();
        assert_eq!(spans[0].style, red);
        assert_eq!(spans.last().unwrap().style, dim);
        let two = shown(&more[..2], 60);
        assert_eq!(text(&two), "\u{26a0} [:3] a  and 1 more warning\u{2026}");
    }

    #[test]
    fn a_long_line_is_cut_with_an_ellipsis_before_the_tail() {
        let long = "word ".repeat(30);
        let two = [warning(Some(3), &long), warning(None, "b")];
        let spans = shown(&two, 40);
        let line = text(&spans);
        assert!(line.starts_with("\u{26a0} [:3] word word"), "{line}");
        assert!(
            line.ends_with("\u{2026}  and 1 more warning\u{2026}"),
            "{line}"
        );
        assert!(crate::typeset::wrap::width(&spans) <= 38, "{line}");
        let one = shown(&two[..1], 40);
        assert!(text(&one).ends_with("word\u{2026}"), "{}", text(&one));
        assert!(crate::typeset::wrap::width(&one) <= 38);
    }
}
