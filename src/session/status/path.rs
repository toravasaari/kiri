//! The path of the views on the status line: the names of the views on the
//! deck, `start.md › other.md › third.md`, with the last one in bold.
//!
//! The path is cut to the room a name at a time, its names are split into
//! spans (the last in bold), and each name has its cells and its view on the
//! drawn line, which a click on the name goes back to.

use std::ops::Range;

use unicode_width::UnicodeWidthStr;

use crate::typeset::style::{Span, Style};
use crate::typeset::wrap;

/// A name of the path as drawn on the status line: the cells it takes,
/// counted from 1, and the view it is the name of, counted from the first
/// view on the deck. The `…` that stands for the names left out is one, of
/// the topmost view that is left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Named {
    pub(crate) cells: Range<usize>,
    pub(crate) view: usize,
}

/// What stands between two names.
pub(super) const SEPARATOR: &str = " › ";

/// What stands for the names at the start that are left out.
const OMITTED: &str = "…";

/// A piece of the path: a name, or the `…`, and the view it is of.
struct Piece {
    text: String,
    view: usize,
}

/// The pieces of `trail` and `last`, the names of the views from the first,
/// in at most `room` cells: the whole path, or else with `…` for the start
/// of it, a name at a time, or else the end of `last` alone.
fn pieces(trail: &[String], last: &str, room: usize) -> Vec<Piece> {
    let names: Vec<&str> = (trail.iter().map(String::as_str))
        .chain(std::iter::once(last))
        .collect();
    let width = |names: &[&str]| names.join(SEPARATOR).width();
    let named = |from: usize| {
        (names[from..].iter().enumerate())
            .map(|(at, name)| Piece {
                text: (*name).to_owned(),
                view: from + at,
            })
            .collect::<Vec<_>>()
    };
    if width(&names) <= room {
        return named(0);
    }
    for left in 1..=trail.len() {
        if OMITTED.width() + SEPARATOR.width() + width(&names[left..]) <= room {
            let dots = Piece {
                text: OMITTED.to_owned(),
                view: left - 1,
            };
            return std::iter::once(dots).chain(named(left)).collect();
        }
    }
    vec![Piece {
        text: wrap::tail(last, room),
        view: trail.len(),
    }]
}

/// `trail` and `last` as text of at most `room` cells.
pub(super) fn fit(trail: &[String], last: &str, room: usize) -> String {
    let texts: Vec<String> = pieces(trail, last, room)
        .into_iter()
        .map(|piece| piece.text)
        .collect();
    texts.join(SEPARATOR)
}

/// `spans`, a status line whose name, the second span, is the path of
/// `trail` and `last`, with the name split into its names and what is
/// between them: the names in `bar`, the last in `bold`. The names come
/// back with the cells they take. A line whose name is not that path is
/// as it was, with no names.
pub(super) fn split(
    mut spans: Vec<Span>,
    (trail, last): (&[String], &str),
    (bar, bold): (&Style, &Style),
) -> (Vec<Span>, Vec<Named>) {
    let Some(shown) = spans.get(1).and_then(|span| span.text.strip_prefix(' ')) else {
        return (spans, Vec::new());
    };
    let found = pieces(trail, last, shown.width());
    let joined: Vec<&str> = found.iter().map(|piece| piece.text.as_str()).collect();
    if shown.is_empty() || joined.join(SEPARATOR) != shown {
        return (spans, Vec::new());
    }
    // The badge, and the space before the name.
    let mut at = wrap::width(&spans[..1]) + 2;
    let mut names = Vec::new();
    let mut made = vec![Span::new(" ", bar.clone())];
    let count = found.len();
    for (n, piece) in found.into_iter().enumerate() {
        if n > 0 {
            made.push(Span::new(SEPARATOR, bar.clone()));
            at += SEPARATOR.width();
        }
        let width = piece.text.width();
        names.push(Named {
            cells: at..at + width,
            view: piece.view,
        });
        let style = if n + 1 == count { bold } else { bar };
        made.push(Span::new(&piece.text, style.clone()));
        at += width;
    }
    spans.splice(1..2, made);
    (spans, names)
}
