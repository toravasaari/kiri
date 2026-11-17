//! What search, Tab and a change do while a part of the layout is not laid
//! out: a match, a stop and a change that no line shows yet are kept without
//! a line, in the order of their place in the document, and get their line
//! when the layout reaches them, so that the answer is the whole layout's.
//!
//! Every test lays a document out in the first of its pieces only (the cell
//! layout: the first stretch of a code block or the first blocks; reading
//! mode: the first page), asks, goes on with `Laid::extend` and asks again,
//! and compares with the layout made whole.

use std::ops::Range;
use std::rc::Rc;

use crate::render::{FrontMatterStyle, Inputs};
use crate::session::search::{self, Search};
use crate::session::settings::SearchCase;
use crate::typeset::caps::Capabilities;
use crate::typeset::driver::{self, Content};
use crate::typeset::laid::Laid;
use crate::typeset::model::Document;
use crate::typeset::model::File;
use crate::typeset::parse::parse;
use crate::typeset::position::Pos;
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::page::Wide;
use crate::typeset::text::DocumentText;

/// A layout and a way to lay the document out.
#[derive(Debug, Clone, Copy)]
enum Engine {
    Text(u16),
    Reading(u16, Wide),
}

/// The inputs of a layout in `engine`. A layout in reading mode keeps what it
/// drew in them, so each layout gets its own.
fn inputs(engine: Engine) -> Inputs {
    match engine {
        Engine::Text(width) => Inputs::new(Capabilities::kitty(width)),
        Engine::Reading(width, wide) => Inputs {
            reading: Some(Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide,
                snap: false,
            }),
            ..Inputs::new(Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(width)
            })
        },
    }
}

/// `document` laid out in `engine` to its first piece only: at least one
/// line, and a stretch of a code block, a block or a page of reading mode
/// whole.
fn first(document: &Document, engine: Engine) -> (Laid, Inputs) {
    let inputs = inputs(engine);
    let mut laid = Laid::begin(Content::Document(document), 0, &inputs);
    laid.extend(Content::Document(document), &inputs, 1);
    (laid, inputs)
}

/// `document` laid out whole in `engine`.
fn whole(document: &Document, engine: Engine) -> Laid {
    driver::laid(Content::Document(document), &inputs(engine))
}

/// The text of `document` as the session's edition has it.
fn text_of(document: &Document) -> DocumentText {
    DocumentText::new(crate::session::edition::documents(Content::Document(
        document,
    )))
}

/// The paragraphs `Paragraph 0 has a needle`, `Paragraph 1 has a needle`, ...
/// with a link each, `count` of them.
fn paragraphs(count: usize) -> String {
    (0..count)
        .map(|n| format!("Paragraph {n} has a needle and [link {n}](https://example.com/{n}).\n\n"))
        .collect()
}

/// A search for `term` over what `laid` has laid out, from the line `top`.
fn search_in(laid: &Laid, text: &DocumentText, term: &str, top: usize) -> Search {
    let shown = (laid.map(), laid.lines());
    Search::new(shown, text, term, SearchCase::Smart, top)
}

/// A table whose cells wrap, so that the lines of a row hold a little of each
/// cell in turn: the matches of its first cell come before those of its
/// second in the document, and their lines come after them.
fn wrapping_table(wide: bool) -> String {
    let cell = match wide {
        // Words that cannot wrap in the area, so that reading mode scrolls the
        // table sideways, in a band, and still wraps what the cell can.
        true => format!("needle{} needle {} needle", "x".repeat(30), "y".repeat(40)),
        false => "a needle and another needle, and a third needle".to_owned(),
    };
    format!(
        "| h1 | h2 | h3 |\n|---|---|---|\n| {cell} | {cell} | {cell} |\n| a needle | b needle | c needle |\n\n"
    )
}

/// A code block of `lines` lines, `needle` on every `every`th.
fn code(lines: usize, every: usize) -> String {
    let body: String = (0..lines)
        .map(|n| match n % every == 0 {
            true => format!("let n{n} = \"needle\";\n"),
            false => format!("let n{n} = {n};\n"),
        })
        .collect();
    format!("```rust\n{body}```\n\n")
}

/// The documents and the layouts of the checks: each has a first piece that
/// holds some of the matches and a part after it that holds the rest, with
/// code in stretches in the cell layout, and a table whose matches' lines go
/// against their places, also in a band of reading mode.
fn documents() -> Vec<(&'static str, Engine, String)> {
    let near = paragraphs(6);
    vec![
        (
            "paragraphs, a table and a code block",
            Engine::Text(40),
            format!("{near}{}{}{near}", wrapping_table(false), code(1100, 100)),
        ),
        (
            "a code block in stretches first",
            Engine::Text(60),
            format!("{}{near}{}", code(1100, 100), wrapping_table(false)),
        ),
        (
            "reading mode, pages",
            Engine::Reading(40, Wide::Wrap),
            format!(
                "{}{}{}",
                paragraphs(1600),
                wrapping_table(false),
                code(60, 20)
            ),
        ),
        (
            "reading mode, a table in a band",
            Engine::Reading(40, Wide::Scroll),
            format!(
                "{}{}{}",
                paragraphs(1600),
                wrapping_table(true),
                code(60, 20)
            ),
        ),
    ]
}

/// The ranges of the matches of `want`, the search of a whole layout, in the
/// order a search of a layout that has laid out only the first `laid` of them
/// has: those on lines in the order of their lines, and the rest, which have
/// none, in the order of their places.
fn as_laid(want: &Search, laid: usize) -> Vec<Range<Pos>> {
    let mut found: Vec<Range<Pos>> = want.ranges().map(|(range, _)| range.clone()).collect();
    found[laid..].sort_by(|a, b| a.start.cmp(&b.start));
    found
}

/// How many matches of `search` have a line.
fn laid_matches(search: &Search) -> usize {
    search
        .matches()
        .iter()
        .take_while(|found| found.line.is_some())
        .count()
}

/// The match `search` is at.
fn current(search: &Search) -> Option<Range<Pos>> {
    search.current().map(|found| found.range.clone())
}

/// Search, `n` and `N` over a layout with a part not laid out are those of
/// the whole layout, and the matches get their lines as the layout goes on,
/// without the text being searched again.
fn check_search(what: &str, engine: Engine, source: &str) {
    let document = parse(source);
    let content = Content::Document(&document);
    let text = text_of(&document);
    let all = whole(&document, engine);
    let (mut part, inputs) = first(&document, engine);
    assert!(
        !part.is_whole() && part.len() < all.len(),
        "{what}: only the first piece is laid out"
    );
    let ranges_of = |search: &Search| -> Vec<Range<Pos>> {
        search.ranges().map(|(range, _)| range.clone()).collect()
    };
    let want = search_in(&all, &text, "needle", 0);
    let total = want.count().map_or(0, |(_, total)| total);
    assert!(total > 10, "{what}: {total} matches");
    // The count is the whole layout's, and so is the order of the matches
    // that have lines; the others are in the order of the document.
    let lazy = search_in(&part, &text, "needle", 0);
    let laid = laid_matches(&lazy);
    assert!(
        laid > 0 && laid < total,
        "{what}: {laid} of {total} on a line"
    );
    assert_eq!(lazy.count(), want.count(), "{what}: the count");
    assert_eq!(ranges_of(&lazy), as_laid(&want, laid), "{what}: the order");
    // From a line the first match on it or below is the same.
    let top = 3.min(part.len() - 1);
    let (lazy, want_at) = (
        search_in(&part, &text, "needle", top),
        search_in(&all, &text, "needle", top),
    );
    assert_eq!(
        lazy.count(),
        want_at.count(),
        "{what}: the match at line {top}"
    );
    // `n` goes the same way from the same place, over the matches that are
    // on lines: the view follows the match, and one with no line is not in
    // the view, so it is the last of them. `N` goes back round to the last.
    let mut lazy = search_in(&part, &text, "needle", 0);
    let mut seen = vec![current(&lazy)];
    let mut at = lazy.current().and_then(|found| found.line);
    while let Some(line) = at {
        assert!(!lazy.forward(line..line + 1), "{what}: round too early");
        seen.push(current(&lazy));
        at = lazy.current().and_then(|found| found.line);
    }
    let order = as_laid(&want, laid);
    let visited: Vec<_> = seen.into_iter().map(Option::unwrap).collect();
    assert_eq!(visited, order[..laid + 1], "{what}: `n` from the first");
    let mut lazy = search_in(&part, &text, "needle", 0);
    assert!(lazy.backward(0..1), "{what}: `N` from the first goes round");
    assert_eq!(
        current(&lazy),
        order.last().cloned(),
        "{what}: `N` from the first"
    );
    // The layout goes on: the matches are the same at each step, and the
    // lines come when the layout reaches them.
    let mut lazy = search_in(&part, &text, "needle", 0);
    let searched = search::finds();
    let mut steps = 0;
    while !part.is_whole() {
        part.extend(content, &inputs, part.len() + 1);
        lazy.extend((part.map(), part.lines()));
        steps += 1;
        let at = format!("{what}: after {steps} steps of {} lines", part.len());
        assert_eq!(lazy.count().map(|(_, total)| total), Some(total), "{at}");
        assert_eq!(
            ranges_of(&lazy),
            as_laid(&want, laid_matches(&lazy)),
            "{at}"
        );
        for (found, whole) in lazy.matches().iter().zip(want.matches()) {
            assert!(found.line.is_none() || found.line == whole.line, "{at}");
        }
    }
    assert_eq!(lazy, want, "{what}: the search when the layout is whole");
    assert_eq!(
        search::finds(),
        searched,
        "{what}: the text is not searched again"
    );
}

#[test]
fn a_search_over_a_layout_with_a_part_not_laid_out_is_the_whole_layouts() {
    for (what, engine, source) in documents() {
        check_search(what, engine, &source);
    }
}

#[test]
fn a_match_that_turns_out_not_to_be_shown_is_gone_when_the_layout_reaches_it() {
    let files = [
        File {
            name: "a.md".to_owned(),
            document: parse("A paragraph with a needle.\n"),
        },
        File {
            name: "b.md".to_owned(),
            document: parse("---\ntitle: a needle\n---\n\nAnother needle.\n"),
        },
    ];
    let content = Content::Files(&files);
    let text = DocumentText::new(crate::session::edition::documents(content));
    let inputs = Inputs {
        front_matter: FrontMatterStyle::Hide,
        ..inputs(Engine::Text(40))
    };
    let mut laid = Laid::begin(content, 0, &inputs);
    laid.extend(content, &inputs, 1);
    assert!(!laid.is_whole());
    let mut search = search_in(&laid, &text, "needle", 0);
    // Nothing says yet that the front matter of the second file is hidden.
    assert_eq!(search.count(), Some((1, 3)));
    let searched = search::finds();
    while !laid.is_whole() {
        laid.extend(content, &inputs, laid.len() + 1);
        search.extend((laid.map(), laid.lines()));
    }
    assert_eq!(search.count(), Some((1, 2)));
    let whole = driver::laid(content, &inputs);
    assert_eq!(search, search_in(&whole, &text, "needle", 0));
    assert_eq!(search::finds(), searched + 1, "one search, for the whole");
}

#[test]
fn the_match_the_reader_is_at_stays_when_the_layout_reaches_it_and_matches_move_around_it() {
    let source = format!(
        "{}{}{}",
        paragraphs(2),
        wrapping_table(false),
        paragraphs(2)
    );
    let document = parse(&source);
    let content = Content::Document(&document);
    let text = text_of(&document);
    let engine = Engine::Text(40);
    let (mut laid, inputs) = first(&document, engine);
    let all = whole(&document, engine);
    let want = search_in(&all, &text, "needle", 0);
    // The reader is at the second match of the table, which has no line yet.
    let in_table = |range: &&Range<Pos>| range.start.path == [2];
    let at = want
        .ranges()
        .map(|(range, _)| range)
        .filter(in_table)
        .nth(1);
    let at = at.expect("a match in the table").clone();
    let shown = (laid.map(), laid.lines());
    let mut lazy =
        search_in(&laid, &text, "needle", 0).again(shown, &text, (SearchCase::Smart, 0), Some(&at));
    assert_eq!(current(&lazy), Some(at.clone()));
    let before = lazy.count();
    while !laid.is_whole() {
        laid.extend(content, &inputs, laid.len() + 1);
        lazy.extend((laid.map(), laid.lines()));
        assert_eq!(current(&lazy), Some(at.clone()), "{} lines", laid.len());
    }
    // In the order of the lines the second of the table is the fourth: the
    // match is where it was, and what the count says moved with it.
    let (now, total) = lazy.count().unwrap();
    assert_eq!(Some(total), before.map(|(_, total)| total));
    assert_eq!(lazy.matches()[now - 1].range, at);
    assert_ne!(
        Some(now),
        before.map(|(at, _)| at),
        "the order inside the table changed"
    );
    let shown = (all.map(), all.lines());
    let again = want.again(shown, &text, (SearchCase::Smart, 0), Some(&at));
    assert_eq!(lazy, again);
}
