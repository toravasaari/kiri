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

use crate::session::animation::{Animator, Pacing};
use crate::session::document::Changed;
use crate::session::edition::Edition;
use crate::session::edition::Held;
use crate::session::links::{Kind, Links, Placed, on_layout};
use crate::session::reveal::Seen;
use crate::session::search::{self, Search};
use crate::session::settings::SearchCase;
use crate::session::settings::{Follow, FollowJump};
use crate::session::sources::Session;
use crate::session::state::State;
use crate::session::view::View;
use crate::typeset::caps::Capabilities;
use crate::typeset::compare::compare;
use crate::typeset::driver::{self, Content};
use crate::typeset::inputs::{FrontMatterStyle, Inputs};
use crate::typeset::laid::Laid;
use crate::typeset::locate::Hidden;
use crate::typeset::model::Document;
use crate::typeset::model::File;
use crate::typeset::parse::parse;
use crate::typeset::position::Pos;
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::page::Wide;
use crate::typeset::text::DocumentText;
use std::time::Instant;

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
                glyphs: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide,
                snap: false,
                zoom: 100,
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
        "| h1 | h2 | h3 |\n|---|---|---|\n| {cell} | {cell} | {cell} |\n| [a needle](https://example.com/a) | [b needle](https://example.com/b) | [c needle](https://example.com/c) |\n\n"
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
            "reading mode, a table in a band, laid out first",
            Engine::Reading(40, Wide::Scroll),
            format!("{}{}", wrapping_table(true), paragraphs(1600)),
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
    let in_table = |range: &Range<Pos>| range.start.path == [2];
    let at = want
        .ranges()
        .map(|(range, _)| range)
        .filter(in_table)
        .nth(1);
    let at = at.expect("a match in the table");
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

/// The stops of Tab that `laid` shows, and the edition of `document` they are
/// of.
fn stops_in(laid: &Laid, document: &Document) -> (Vec<Placed>, Edition) {
    let session = Session::new(["a.md"]);
    let edition = Edition::new(
        Content::Document(document),
        &session,
        session.base().id(),
        None,
    );
    let seen = Seen {
        map: laid.map(),
        lines: laid.lines(),
    };
    let all = on_layout(&edition, &Animator::new(Pacing::new(0)), seen, true);
    (all, edition)
}

/// The ranges Tab selects in turn from none, `presses` times, with the lines
/// `visible` in view, or Shift+Tab when `back`.
fn tabbed(
    all: &[Placed],
    edition: &Edition,
    visible: Range<usize>,
    (presses, back): (usize, bool),
) -> Vec<Range<Pos>> {
    let mut links = Links::default();
    let mut found = Vec::new();
    for _ in 0..presses {
        links.select_next(edition, all, visible.clone(), back);
        let at = links.selected(edition, all).expect("a stop is selected");
        found.push(at.stop.range.clone());
    }
    found
}

/// The urls of the stops, to tell them in a message.
fn urls(all: &[Placed]) -> Vec<String> {
    (all.iter())
        .map(|placed| match &placed.stop.kind {
            Kind::Link { url, .. } => url.clone(),
            Kind::Picture { name, .. } => name.clone(),
        })
        .collect()
}

#[test]
fn the_stops_of_tab_over_a_layout_with_a_part_not_laid_out_are_the_whole_layouts() {
    for (what, engine, source) in documents() {
        let document = parse(&source);
        let content = Content::Document(&document);
        let all = whole(&document, engine);
        let (mut part, inputs) = first(&document, engine);
        assert!(!part.is_whole(), "{what}: only the first piece is laid out");
        let (want, edition) = stops_in(&all, &document);
        assert!(want.len() >= 9, "{what}: {} stops", want.len());
        let (lazy, _) = stops_in(&part, &document);
        let at = |laid: usize| format!("{what}: {laid} lines laid out");
        // The same stops in the same order, with a line for those that are
        // laid out, and none for the others.
        assert_eq!(urls(&lazy), urls(&want), "{}", at(part.len()));
        let on_lines = lazy.iter().filter(|placed| placed.lines.is_some()).count();
        assert!(on_lines < lazy.len(), "{what}: {on_lines}");
        // Tab goes the same way from none, round, and Shift+Tab too.
        for back in [false, true] {
            // What is in view is laid out.
            let visible = 0..part.len().min(5);
            let by = (want.len() + 2, back);
            let found = tabbed(&lazy, &edition, visible.clone(), by);
            assert_eq!(
                found,
                tabbed(&want, &edition, visible, by),
                "{what}: back {back}"
            );
        }
        // The layout goes on: the same stops, and the lines of those it
        // reached are those of the whole layout.
        let mut steps = 0;
        while !part.is_whole() {
            part.extend(content, &inputs, part.len() + 1);
            steps += 1;
            let (lazy, _) = stops_in(&part, &document);
            let at = at(part.len());
            assert_eq!(urls(&lazy), urls(&want), "{at}, step {steps}");
            for (found, whole) in lazy.iter().zip(&want) {
                assert!(found.lines.is_none() || found.lines == whole.lines, "{at}");
            }
        }
        assert_eq!(stops_in(&part, &document).0, want, "{what}: whole");
    }
}

/// A state that follows a reload of `old` to `new` on auto, with the new
/// edition, and the layout `laid` of `new` that the view is made for: what
/// follow does with the change that `jump` chooses, from a view at the top.
fn followed((old, new): (&Document, &Document), laid: &Laid, jump: FollowJump) -> (State, View) {
    let mut state = State::new(SearchCase::Smart);
    state.follow.mode = Follow::Auto;
    state.jump = jump;
    let session = Session::new(["a.md"]);
    let edition = Edition::new(Content::Document(new), &session, session.base().id(), None);
    let doc = state.document_mut();
    doc.edition = Some(edition);
    doc.next.changes = Some(compare(old, new));
    let heights = vec![1; laid.len()];
    let view = View::new(heights.clone(), 5);
    let shown = (heights, (laid.lines(), laid.map()), 5);
    let view = state.view_for(Some(view), shown, (true, Instant::now()));
    (state, view)
}

#[test]
fn a_change_in_a_part_not_laid_out_is_kept_where_the_whole_layout_has_it() {
    // Two paragraphs change, one near the top and one far below, in the cell
    // layout and in reading mode, where the far one is on the second page.
    for (engine, count, far) in [
        (Engine::Text(80), 40, 33),
        (Engine::Reading(40, Wide::Wrap), 1600, 1590),
    ] {
        let source = paragraphs(count);
        let changed = source
            .replace("Paragraph 3 has", "Paragraph 3 had")
            .replace(
                &format!("Paragraph {far} has"),
                &format!("Paragraph {far} had"),
            );
        let (old, new) = (parse(&source), parse(&changed));
        let all = whole(&new, engine);
        for jump in [FollowJump::First, FollowJump::Last] {
            let (mut part, inputs) = first(&new, engine);
            assert!(part.len() < 20 || part.len() > 3000, "{} lines", part.len());
            let (want, _) = followed((&old, &new), &all, jump);
            let (mut lazy, view) = followed((&old, &new), &part, jump);
            let range = |state: &State| {
                let doc = state.document();
                let changed = doc.changed.as_ref();
                changed
                    .and_then(|changed| changed.range.get(doc.edition.as_ref()?))
                    .cloned()
            };
            let at = format!("{engine:?}, {jump:?}");
            let range_of_the_whole = range(&want).unwrap_or_else(|| panic!("{at}: no tint"));
            assert_eq!(
                range(&lazy),
                Some(range_of_the_whole.clone()),
                "{at}: the change"
            );
            // The layout goes on, and the change gets the lines it has in the
            // whole layout. Until then the view cannot go to it, so it stays.
            let mut laid = part.len();
            let seen = |laid: &Laid| laid.map().drawn_rows(laid.lines(), &range_of_the_whole);
            let unlaid = seen(&part) == Err(Hidden::NotLaidOut);
            assert_eq!(unlaid, jump == FollowJump::Last || count == 40, "{at}");
            if unlaid {
                assert_eq!(view.top(), 0, "{at}");
            }
            while !part.is_whole() {
                part.extend(Content::Document(&new), &inputs, laid + 1);
                laid = part.len();
                lazy.extended((part.map(), part.lines()));
                assert!(lazy.document().changed.is_some(), "{at}: {laid} lines");
            }
            assert_eq!(seen(&part), seen(&all), "{at}");
            assert!(lazy.document().tint(Instant::now()).is_some(), "{at}");
        }
    }
}

#[test]
fn a_removal_in_a_part_not_laid_out_leaves_the_view_where_it_is_and_tints_nothing() {
    // A removal is an empty range at the place of the next block, so it has
    // no lines to be laid out or not: the layout must still say it does not
    // know where it is.
    for (engine, count) in [
        (Engine::Text(80), 40),
        (Engine::Reading(40, Wide::Wrap), 1600),
        (Engine::Reading(40, Wide::Scroll), 1600),
    ] {
        let source = paragraphs(count);
        let gone = format!("Paragraph {} has", count - 7);
        let removed: String = source
            .split("\n\n")
            .filter(|paragraph| !paragraph.starts_with(&gone))
            .map(|paragraph| format!("{paragraph}\n\n"))
            .collect();
        let (old, new) = (parse(&source), parse(&removed));
        let all = whole(&new, engine);
        for jump in [FollowJump::First, FollowJump::Last] {
            let (mut part, inputs) = first(&new, engine);
            let at = format!("{engine:?}, {jump:?}");
            let (want, _) = followed((&old, &new), &all, jump);
            let (mut lazy, view) = followed((&old, &new), &part, jump);
            assert_eq!(view.top(), 0, "{at}: the view stays");
            assert!(
                lazy.document().tint(Instant::now()).is_none(),
                "{at}: no tint"
            );
            while !part.is_whole() {
                part.extend(Content::Document(&new), &inputs, part.len() + 1);
                lazy.extended((part.map(), part.lines()));
            }
            // Whole, the removal is where the whole layout has it: no tint.
            assert_eq!(
                lazy.document().changed.is_some(),
                want.document().changed.is_some(),
                "{at}"
            );
        }
    }
}

#[test]
fn a_state_goes_on_with_what_it_kept_when_the_layout_is_laid_out_further() {
    let source = format!("{}{}", paragraphs(6), wrapping_table(false));
    let document = parse(&source);
    let content = Content::Document(&document);
    let text = text_of(&document);
    let engine = Engine::Text(40);
    let all = whole(&document, engine);
    let (mut laid, inputs) = first(&document, engine);
    let session = Session::new(["a.md"]);
    let edition = Edition::new(content, &session, session.base().id(), None);
    // A change in the table, which has no lines yet, and the search.
    let range = Pos::block(0, &[6])..Pos::end(0, &[6]);
    let mut state = State::new(SearchCase::Smart);
    let doc = state.document_mut();
    doc.changed = Some(Changed {
        at: Instant::now(),
        range: Held::new(&edition, range.clone()),
    });
    doc.edition = Some(edition);
    let shown = (laid.map(), laid.lines());
    doc.search = Some(Search::new(shown, &text, "needle", SearchCase::Smart, 0));
    let searched = search::finds();
    while !laid.is_whole() {
        laid.extend(content, &inputs, laid.len() + 1);
        state.extended((laid.map(), laid.lines()));
        let changed = state.document().changed.as_ref();
        let kept =
            changed.and_then(|changed| changed.range.get(state.document().edition.as_ref()?));
        assert_eq!(kept, Some(&range), "{} lines", laid.len());
    }
    assert_eq!(
        state.document().search.as_ref(),
        Some(&search_in(&all, &text, "needle", 0))
    );
    assert_eq!(search::finds(), searched + 1, "one search, for the whole");
}

#[test]
fn a_change_that_turns_out_not_to_be_shown_is_gone_when_the_layout_reaches_it() {
    let files = [
        File {
            name: "a.md".to_owned(),
            document: parse("A paragraph.\n"),
        },
        File {
            name: "b.md".to_owned(),
            document: parse("---\ntitle: a needle\n---\n\nAnother paragraph.\n"),
        },
    ];
    let content = Content::Files(&files);
    let inputs = Inputs {
        front_matter: FrontMatterStyle::Hide,
        ..inputs(Engine::Text(40))
    };
    let mut laid = Laid::begin(content, 0, &inputs);
    laid.extend(content, &inputs, 1);
    let session = Session::new(["a.md", "b.md"]);
    let edition = Edition::new(content, &session, session.base().id(), None);
    let text = DocumentText::new(crate::session::edition::documents(content));
    // The front matter of the second file: no line will show it.
    let range = text.find("needle", false).remove(0);
    let mut state = State::new(SearchCase::Smart);
    let doc = state.document_mut();
    doc.changed = Some(Changed {
        at: Instant::now(),
        range: Held::new(&edition, range),
    });
    doc.edition = Some(edition);
    state.extended((laid.map(), laid.lines()));
    assert!(
        state.document().changed.is_some(),
        "not laid out is not hidden"
    );
    while !laid.is_whole() {
        laid.extend(content, &inputs, laid.len() + 1);
    }
    state.extended((laid.map(), laid.lines()));
    assert!(state.document().changed.is_none());
}

#[test]
fn the_matches_of_a_band_laid_out_are_in_the_order_of_its_lines_not_of_its_places() {
    let source = format!("{}{}", wrapping_table(true), paragraphs(1600));
    let document = parse(&source);
    let engine = Engine::Reading(40, Wide::Scroll);
    let text = text_of(&document);
    let (part, _) = first(&document, engine);
    let all = whole(&document, engine);
    assert!(
        !part.is_whole() && part.bands().len() == 1,
        "the table is in a band"
    );
    let in_table = |search: &Search| -> Vec<Range<Pos>> {
        let found = search.ranges().map(|(range, _)| range.clone());
        found.filter(|range| range.start.path == [0]).collect()
    };
    let (lazy, want) = (
        search_in(&part, &text, "needle", 0),
        search_in(&all, &text, "needle", 0),
    );
    let by_lines = in_table(&lazy);
    let mut by_places = by_lines.clone();
    by_places.sort_by(|a, b| a.start.cmp(&b.start));
    assert_eq!(by_lines.len(), 12);
    assert_ne!(
        by_lines, by_places,
        "the lines go against the places in the band"
    );
    assert_eq!(by_lines, in_table(&want));
    // The matches of the band are on lines and in a band.
    let matches = lazy.matches();
    let on_lines = matches.iter().take_while(|found| found.line.is_some());
    assert!(
        on_lines
            .clone()
            .all(|found| found.range.start.path != [0] || found.band.is_some())
    );
    assert!(on_lines.count() >= 12);
}

#[test]
fn a_current_match_that_turns_out_not_to_be_shown_becomes_the_next_by_place() {
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
    let hidden = text.find("needle", false).remove(1);
    let shown = (laid.map(), laid.lines());
    let mut search = search_in(&laid, &text, "needle", 0).again(
        shown,
        &text,
        (SearchCase::Smart, 0),
        Some(&hidden),
    );
    assert_eq!(current(&search), Some(hidden.clone()));
    assert_eq!(search.count(), Some((2, 3)));
    while !laid.is_whole() {
        laid.extend(content, &inputs, laid.len() + 1);
        search.extend((laid.map(), laid.lines()));
    }
    // The front matter is gone; the reader is at the next match by place,
    // the one in the second file's paragraph.
    assert_eq!(search.count(), Some((2, 2)));
    let next = current(&search).unwrap();
    assert!(next.start > hidden.start && next.start.file == 1);
}
