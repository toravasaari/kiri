//! The path of the views on the status line: the names of the views on the
//! deck, `start.md › other.md › third.md`, with the last one in bold.

use unicode_width::UnicodeWidthStr;

use crate::typeset::style::{Span, Style};
use crate::typeset::wrap;

/// What stands between two names.
pub(super) const SEPARATOR: &str = " › ";

/// What stands for the names at the start that left out.
const OMITTED: &str = "…";

/// `trail` and `last`, the names of the views from the first, as text of at
/// most `room` cells: the whole path, or else with `…` for the start of it,
/// a name at a time, or else the end of `last` alone.
pub(super) fn fit(trail: &[String], last: &str, room: usize) -> String {
    let names: Vec<&str> = trail
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(last))
        .collect();
    let joined = |names: &[&str]| names.join(SEPARATOR);
    let whole = joined(&names);
    if whole.width() <= room {
        return whole;
    }
    for left in 1..=trail.len() {
        let cut = format!("{OMITTED}{SEPARATOR}{}", joined(&names[left..]));
        if cut.width() <= room {
            return cut;
        }
    }
    wrap::tail(last, room)
}

/// `spans`, a status line, with its name split into the names of the path
/// and what is between them: the names in `bar`, the last in `bold`.
pub(super) fn split(mut spans: Vec<Span>, bar: &Style, bold: &Style) -> Vec<Span> {
    let Some(name) = spans.get(1).and_then(|span| span.text.strip_prefix(' ')) else {
        return spans;
    };
    if name.trim().is_empty() {
        return spans;
    }
    let names: Vec<&str> = name.split(SEPARATOR).collect();
    let mut pieces = vec![Span::new(" ", bar.clone())];
    for (at, name) in names.iter().enumerate() {
        let last = at + 1 == names.len();
        if at > 0 {
            pieces.push(Span::new(SEPARATOR, bar.clone()));
        }
        pieces.push(Span::new(name, if last { bold } else { bar }.clone()));
    }
    spans.splice(1..2, pieces);
    spans
}

/// The view that the cell at `column`, counted from 1, of the status line
/// `spans` is the name of, counted from the first of the `views` of the
/// path; `None` for a cell that is no name, and for the `…` that stands for
/// the names that were left out.
pub(crate) fn view_at(spans: &[Span], views: usize, column: usize) -> Option<usize> {
    // The names are every other span after the badge and the space: the
    // last is the one in bold, and the gap after the path is all spaces.
    let mut at = wrap::width(spans.get(..2)?) + 1;
    let mut names = Vec::new();
    for (n, span) in spans.iter().enumerate().skip(2) {
        if span.text.trim().is_empty() {
            break;
        }
        let width = span.text.width();
        if n % 2 == 0 {
            names.push((at..at + width, span.text.as_str()));
        }
        at += width;
        if span.style.bold {
            break;
        }
    }
    let cut = names.first().is_some_and(|(_, text)| *text == OMITTED);
    let first = views.checked_sub(names.len() - usize::from(cut))?;
    let found = names
        .iter()
        .position(|(cells, _)| cells.contains(&column))?;
    let view = found.checked_sub(usize::from(cut))?;
    Some(first + view)
}
