//! The step is driven without a terminal: messages in, effects out.

use std::path::Path;
use std::time::Duration;

use super::*;
use crate::render::Inputs;
use crate::session::document::Changed;
use crate::session::edition::{Edition, Held};
use crate::session::files::Files;
use crate::session::follow::{Opening, Return};
use crate::session::frame::{Layout, Shown, Status};
use crate::session::history::{Ahead, Back};
use crate::session::settings::{self, SearchCase};
use crate::session::sources::{Session, SourceFile, ViewId, Watched};
use crate::session::state::{Change, Look};
use crate::session::status;
use crate::session::view::{Anchor, View};
use crate::typeset::block::Blocks;
use crate::typeset::caps::Capabilities;
use crate::typeset::color::Rgb;
use crate::typeset::compare::Comparison;
use crate::typeset::driver::{Content, Stream, content};
use crate::typeset::layout::layers::Marks;
use crate::typeset::locate::Map;
use crate::typeset::model::Document;
use crate::typeset::reading::Fonts;
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::page::Wide;
use crate::typeset::style::{Line, Span, Style};
use crate::typeset::theme::Theme;
use std::rc::Rc;

/// What a run is like when a test does not say.
fn run(now: Instant) -> Run {
    Run {
        can_reload: true,
        tmux: false,
        sixel: false,
        started: now,
        draws_images: true,
    }
}

/// Runs `test` on a pager that shows a document of `count` lines in a
/// window of `rows` rows above the status line, at `now`, in `state`.
fn with_pager(
    count: usize,
    rows: usize,
    state: &mut State,
    run: Run,
    now: Instant,
    test: impl FnOnce(&mut Pager),
) {
    let lines: Vec<Line> = (0..count)
        .map(|n| Line::new(vec![Span::new(&format!("Line {n}"), Style::default())]))
        .collect();
    with_lines(&lines, rows, (state, run, now), test);
}

/// Runs `test` on a pager that shows `lines`, of
/// one row each, in a window of `rows` rows above the status line. They are
/// the lines of no document, so a search finds nothing in them.
fn with_lines(
    lines: &[Line],
    rows: usize,
    state: (&mut State, Run, Instant),
    test: impl FnOnce(&mut Pager),
) {
    let map = Map::new(lines, &Blocks::default());
    let shown = Shown::new(Content::Files(&[]), true);
    with_shown((lines, &map), shown, rows, state, test);
}

/// `with_lines` for the lines of `document` laid out, which a search looks in.
fn with_document(
    document: &Document,
    rows: usize,
    state: (&mut State, Run, Instant),
    test: impl FnOnce(&mut Pager),
) {
    let Stream { lines, blocks } = content(
        Content::Document(document),
        &Inputs::new(Capabilities::kitty(60)),
    );
    let map = Map::new(&lines, &blocks);
    let shown = Shown::new(Content::Document(document), true);
    with_shown((&lines, &map), shown, rows, state, test);
}

fn with_shown(
    (lines, map): (&[Line], &Map),
    shown: Shown,
    rows: usize,
    (state, run, now): (&mut State, Run, Instant),
    test: impl FnOnce(&mut Pager),
) {
    let theme = Theme::default();
    let caps = Capabilities::kitty(60);
    let marks = Marks::new(&theme, &caps);
    let rgb = Rgb::new(&theme, &caps);
    let status = Status {
        name: "notes.md",
        loading: None,
        files: Files::new(&[], map, lines.len()),
        path: Vec::new(),
        contents: state.bar_columns(60),
        rows: Default::default(),
        styles: status::Styles::new(&theme, &caps),
        width: 60,
        bar: true,
        panel: Style::default(),
        muted: Style::default(),
        scrollbar: Style::default(),
        red: Style::default(),
        reading: false,
    };
    let mut view = View::new(vec![1; lines.len()], rows);
    let mut layout = Layout {
        lines,
        shown,
        marks: &marks,
        rgb: rgb.as_ref(),
        columns: 60,
        status: &status,
        view: &mut view,
        map,
        panel: None,
    };
    let mut pager = Pager::new(state, &mut layout, run, None, now);
    test(&mut pager);
}

/// The kinds of `effects`, to compare in a test.
fn kinds(effects: &[Effect]) -> Vec<&'static str> {
    effects
        .iter()
        .map(|effect| match effect {
            Effect::Read => "read",
            Effect::PollWatches => "poll watches",
            Effect::PollLoads => "poll loads",
            Effect::PollWindow => "poll window",
            Effect::Draw(..) => "draw",
            Effect::Check => "check",
            Effect::Answered(_) => "answered",
            Effect::Resend(_) => "resend",
            Effect::Lose => "lose",
            Effect::LoseSome(_) => "lose some",
            Effect::Copy(_) => "copy",
            Effect::Animate(_) => "animate",
            Effect::Quit => "quit",
            Effect::SetCells(_) => "set cells",
            Effect::AskCells => "ask cells",
            Effect::SwapReading => "swap reading",
            Effect::FlipWide => "flip wide",
            Effect::SetSnap(_) => "set snap",
            Effect::SetZoom(_) => "set zoom",
            Effect::SetBackground(_) => "set background",
            Effect::Reload => "reload",
            Effect::Widths => "widths",
            Effect::Relayout { reuse: false } => "relayout",
            Effect::Relayout { reuse: true } => "relayout, same lines",
            Effect::Turn(Turn::Open(_)) => "turn: open",
            Effect::Turn(Turn::Back(_)) => "turn: back",
        })
        .collect()
}

/// `text` typed at `now`, stepped.
fn typed(pager: &mut Pager, text: &str, now: Instant) -> Vec<Effect> {
    step(pager, Msg::Input(text.as_bytes().to_vec(), now))
}

#[test]
fn the_pager_waits_a_poll_when_idle_and_a_frame_while_something_fades() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    let source: String = (0..30).map(|n| format!("Line {n}\n\n")).collect();
    let document = crate::typeset::parse::parse(&source);
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(pager.wait(false), POLL);
        // The bands of an image that loads move with each frame.
        assert_eq!(pager.wait(true), FRAME);
        // The match of a search fades after the jump to it.
        typed(pager, "/Line 12\r", now);
        assert_eq!(pager.wait(false), FRAME);
        // And not any more once it has faded.
        let later = now + Duration::from_secs(10);
        step(pager, Msg::Input(Vec::new(), later));
        assert_eq!(pager.wait(false), POLL);
    });
}

#[test]
fn the_first_turn_draws_and_waits() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        let effects = step(pager, Msg::Shown(now));
        assert_eq!(kinds(&effects), ["draw", "read"]);
    });
}

#[test]
fn a_key_that_scrolls_draws_a_new_frame_and_waits() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        // Each stage of the turn asks for what it needs next.
        let later = now + Duration::from_millis(100);
        assert_eq!(kinds(&typed(pager, "j", later)), ["poll loads"]);
        let effects = step(
            pager,
            Msg::Loads {
                came: false,
                built: Vec::new(),
            },
        );
        assert_eq!(kinds(&effects), ["poll window"]);
        let effects = step(
            pager,
            Msg::Window {
                resized: None,
                bands: false,
            },
        );
        assert_eq!(kinds(&effects), ["draw", "read"]);
    });
}

#[test]
fn nothing_new_to_show_draws_nothing() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(3, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(kinds(&typed(pager, "", now)), ["poll loads"]);
        step(
            pager,
            Msg::Loads {
                came: false,
                built: Vec::new(),
            },
        );
        let window = Msg::Window {
            resized: None,
            bands: false,
        };
        assert_eq!(kinds(&step(pager, window)), ["read"]);
    });
}

#[test]
fn q_quits_and_drops_the_keys_after_it() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(kinds(&typed(pager, "qj", now)), ["quit"]);
    });
}

#[test]
fn r_says_it_is_reading_and_then_reads_the_document_again() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        let effects = typed(pager, "r", now);
        // The long tables of what is read are fitted to all their rows.
        assert_eq!(kinds(&effects), ["draw", "widths", "reload"]);
    });
    assert_eq!(state.told.as_ref().unwrap().message, "reloading…");
}

#[test]
fn r_without_a_source_says_there_is_nothing_to_reload() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    let run = Run {
        can_reload: false,
        ..run(now)
    };
    with_pager(30, 5, &mut state, run, now, |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(kinds(&typed(pager, "r", now)), ["poll loads"]);
    });
    assert_eq!(state.told.as_ref().unwrap().message, "nothing to reload");
}

#[test]
fn a_reload_that_failed_tells_why_and_lays_out_again() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        let effects = step(pager, Msg::Reloaded(Reply::Failed("gone".into()), now));
        assert_eq!(kinds(&effects), ["relayout"]);
    });
    assert_eq!(state.told.as_ref().unwrap().message, "gone");
}

#[test]
fn a_reload_that_worked_keeps_what_it_found_for_the_next_layout() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        let warning = crate::typeset::diagnostic::Diagnostic {
            line: Some(2),
            ..crate::typeset::diagnostic::Diagnostic::new(
                crate::typeset::diagnostic::Code::FrontMatter,
                "odd",
            )
        };
        let reply = Reply::Read {
            warnings: vec![warning],
            changes: Some(Comparison::default()),
            hashes: Vec::new(),
        };
        assert_eq!(kinds(&step(pager, Msg::Reloaded(reply, now))), ["relayout"]);
    });
    assert_eq!(state.document().warnings.len(), 1);
    assert!(state.document().next.changes.is_some());
    assert!(state.document().next.hashes.is_some());
    assert_eq!(state.told.as_ref().unwrap().message, "reloaded");
}

#[test]
fn a_following_pager_looks_at_the_watches_and_reads_again_when_it_settled() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    state.follow.mode = settings::Follow::On;
    let what = Watched::Source(Session::new(["a.md"]).sources()[0].id());
    let look = || {
        let change = Change::Changed(Look {
            modified: None,
            length: 7,
        });
        vec![Looked { what, change }]
    };
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(kinds(&typed(pager, "", now)), ["poll watches"]);
        // Seen once, it is not read yet, in case it is half written.
        let effects = step(pager, Msg::Changed(look()));
        assert_eq!(kinds(&effects), ["poll loads"]);
        step(
            pager,
            Msg::Loads {
                came: false,
                built: Vec::new(),
            },
        );
        step(
            pager,
            Msg::Window {
                resized: None,
                bands: false,
            },
        );
        let later = now + Duration::from_millis(150);
        assert_eq!(kinds(&typed(pager, "", later)), ["poll watches"]);
        let effects = step(pager, Msg::Changed(look()));
        assert_eq!(kinds(&effects), ["reload"]);
    });
    assert!(state.document_mut().polls.seen.is_none());
}

#[test]
fn a_turn_without_a_look_leaves_what_the_debounce_has_seen() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    state.follow.mode = settings::Follow::On;
    let what = Watched::Source(Session::new(["a.md"]).sources()[0].id());
    let look = Look {
        modified: None,
        length: 7,
    };
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(kinds(&typed(pager, "", now)), ["poll watches"]);
        let change = Change::Changed(look);
        step(pager, Msg::Changed(vec![Looked { what, change }]));
        // Nothing was due on the next turn: it goes on to the loads, and
        // the change is still the one seen, not forgotten.
        let later = now + Duration::from_millis(5);
        assert_eq!(kinds(&typed(pager, "", later)), ["poll watches"]);
        assert_eq!(
            kinds(&step(pager, Msg::Changed(Vec::new()))),
            ["poll loads"]
        );
    });
    assert_eq!(state.document_mut().polls.seen, Some((look, now)));
}

#[test]
fn a_window_of_another_size_asks_for_the_cells_when_it_tells_none() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        let resized = |tells_cells| Msg::Window {
            resized: Some(Resized {
                known: true,
                tells_cells,
            }),
            bands: false,
        };
        let effects = step(pager, resized(false));
        assert_eq!(kinds(&effects), ["ask cells", "relayout"]);
        // The window's cells are trusted when it tells them, and outside
        // tmux.
        let effects = step(pager, resized(true));
        assert_eq!(kinds(&effects), ["relayout"]);
    });
}

#[test]
fn a_cell_answer_that_differs_lays_out_again_for_the_new_cells() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        let effects = typed(pager, "\x1b[6;20;10t", now);
        assert_eq!(kinds(&effects), ["set cells", "relayout"]);
        let Effect::SetCells(cells) = &effects[0] else {
            panic!("the cells are set");
        };
        assert_eq!(*cells, (10, 20));
    });
}

#[test]
fn loads_that_came_lay_out_again_and_keep_the_top_line() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        let effects = step(
            pager,
            Msg::Loads {
                came: true,
                built: Vec::new(),
            },
        );
        assert_eq!(kinds(&effects), ["relayout"]);
    });
    assert!(state.document().next.keep_top);
}

#[test]
fn m_swaps_reading_mode_and_lays_out_again() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        let effects = typed(pager, "M", now);
        assert_eq!(kinds(&effects), ["swap reading", "relayout"]);
    });
}

#[test]
fn m_on_a_terminal_without_images_only_says_why() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    let run = Run {
        draws_images: false,
        ..run(now)
    };
    with_pager(30, 5, &mut state, run, now, |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(kinds(&typed(pager, "M", now)), ["relayout"]);
    });
    assert_eq!(
        state.told.as_ref().unwrap().message,
        crate::session::state::NEEDS_IMAGES
    );
}

#[test]
fn w_flips_wide_blocks_and_lays_out_again() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(kinds(&typed(pager, "W", now)), ["flip wide", "relayout"]);
    });
}

#[test]
fn keys_after_a_colon_in_a_lone_document_act_as_they_would_alone() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(kinds(&typed(pager, ":\x03", now)), ["quit"]);
    });
}

#[test]
fn colon_s_turns_headings_on_rows_says_so_and_lays_out_again_with_one_file() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(kinds(&typed(pager, ":s", now)), ["set snap", "relayout"]);
        let told = |pager: &Pager| pager.state.told.as_ref().map(|told| told.message.clone());
        assert_eq!(told(pager).as_deref(), Some("headings on rows: on"));
        typed(pager, ":s", now);
        assert_eq!(told(pager).as_deref(), Some("headings on rows: off"));
    });
}

/// What the status line of `pager` tells, if it tells something.
fn told_of(pager: &Pager) -> Option<String> {
    pager.state.told.as_ref().map(|told| told.message.clone())
}

#[test]
fn z_and_capital_z_step_the_zoom_say_so_and_lay_out_again() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    assert_eq!(state.zoom, 100);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        // Text mode takes the zoom too, for reading mode to come back to.
        assert_eq!(kinds(&typed(pager, "z", now)), ["set zoom", "relayout"]);
        assert_eq!(
            told_of(pager).as_deref(),
            Some("zoom: 110 % (reading mode)")
        );
        assert_eq!(pager.state.zoom, 110);
        typed(pager, "Z", now);
        typed(pager, "Z", now);
        assert_eq!(told_of(pager).as_deref(), Some("zoom: 90 % (reading mode)"));
        assert!(
            pager
                .state
                .told
                .as_ref()
                .is_some_and(|told| told.fades.is_some())
        );
    });
}

#[test]
fn the_zoom_steps_from_a_setting_between_two_steps_and_stays_at_the_ends() {
    let now = Instant::now();
    for (from, key, to) in [
        (120, "z", 125),
        (120, "Z", 110),
        (200, "z", 200),
        (50, "Z", 50),
    ] {
        let mut state = State::new(SearchCase::Smart);
        state.zoom = from;
        with_pager(30, 5, &mut state, run(now), now, |pager| {
            step(pager, Msg::Shown(now));
            let effects = typed(pager, key, now);
            let set = effects.iter().find_map(|effect| match effect {
                Effect::SetZoom(zoom) => Some(*zoom),
                _ => None,
            });
            assert_eq!(set, Some(to), "{key} from {from}");
        });
        assert_eq!(state.zoom, to, "{key} from {from}");
    }
    // The note at an end says the zoom again.
    let mut state = State::new(SearchCase::Smart);
    state.zoom = 200;
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        typed(pager, "z", now);
        assert_eq!(
            told_of(pager).as_deref(),
            Some("zoom: 200 % (reading mode)")
        );
    });
}

#[test]
fn without_images_z_says_that_reading_mode_needs_them_and_changes_nothing() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    let run = Run {
        draws_images: false,
        ..run(now)
    };
    with_pager(30, 5, &mut state, run, now, |pager| {
        step(pager, Msg::Shown(now));
        let kinds = kinds(&typed(pager, "z", now));
        assert!(!kinds.contains(&"set zoom") && !kinds.contains(&"relayout"));
        assert_eq!(
            told_of(pager).as_deref(),
            Some(crate::session::state::NEEDS_IMAGES)
        );
    });
    assert_eq!(state.zoom, 100);
}

#[test]
fn ctrl_l_loses_the_images_and_draws_the_frame_again() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(kinds(&typed(pager, "\x0c", now)), ["lose", "poll loads"]);
        step(
            pager,
            Msg::Loads {
                came: false,
                built: Vec::new(),
            },
        );
        let window = Msg::Window {
            resized: None,
            bands: false,
        };
        // Drawn again though nothing changed.
        assert_eq!(kinds(&step(pager, window)), ["draw", "read"]);
    });
}

#[test]
fn the_effects_that_end_a_layout_are_the_last_of_their_list() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        for keys in ["q", "r", "M", "W", ":s"] {
            let effects = typed(pager, keys, now);
            let ends: Vec<bool> = effects.iter().map(Effect::ends).collect();
            let first = ends.iter().position(|&ends| ends).unwrap();
            assert!(ends[first..].iter().all(|&ends| ends), "{keys}");
            assert!(effects.last().unwrap().ends(), "{keys}");
        }
    });
}

#[test]
fn a_resize_keeps_the_tint_which_is_of_the_change_and_not_of_its_lines() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    let document = crate::typeset::parse::parse("a\n\nb\n");
    let session = crate::session::sources::Session::new(["a.md"]);
    let edition = crate::session::edition::Edition::new(
        Content::Document(&document),
        &session,
        session.base().id(),
        None,
    );
    let range =
        crate::typeset::position::Pos::block(0, &[0])..crate::typeset::position::Pos::end(0, &[0]);
    state.document_mut().changed = Some(Changed {
        at: now,
        range: crate::session::edition::Held::new(&edition, range),
    });
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        let resized = Resized {
            known: true,
            tells_cells: true,
        };
        let msg = Msg::Window {
            resized: Some(resized),
            bands: false,
        };
        assert_eq!(kinds(&step(pager, msg)), ["relayout"]);
    });
    assert!(state.document().changed.is_some());
}

/// Two code blocks too wide for the area, laid out to scroll, as reading
/// mode lays them out, with the blocks of their lines.
fn two_bands() -> (Stream, Document) {
    let long = "a".repeat(200);
    let document =
        crate::typeset::parse::parse(&format!("```\n{long}\n```\n\nText.\n\n```\n{long}\n```\n"));
    let inputs = Inputs {
        reading: Some(Reading {
            pictures: Default::default(),
            fonts: Rc::new(Fonts::fixture()),
            wide: Wide::Scroll,
            snap: false,
            zoom: 100,
        }),
        ..Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(80)
        })
    };
    (content(Content::Document(&document), &inputs), document)
}

#[test]
fn a_selected_band_that_scrolls_off_the_screen_gives_its_selection_to_the_next() {
    let now = Instant::now();
    let (Stream { lines, blocks }, document) = two_bands();
    let map = Map::new(&lines, &blocks);
    let first = map.bands()[0].lines.clone();
    let mut state = State::new(SearchCase::Smart);
    let session = Session::new(["a.md"]);
    state.document_mut().edition = Some(Edition::new(
        Content::Document(&document),
        &session,
        session.base().id(),
        None,
    ));
    let rows = first.len();
    let shown = Shown::new(Content::Document(&document), true);
    with_shown(
        (&lines, &map),
        shown,
        rows,
        (&mut state, run(now), now),
        |pager| {
            // The first band is on the screen and selected.
            let visible = pager.layout.view.visible();
            assert!(pager.state.document_mut().refresh_bands(&map, visible));
            step(pager, Msg::Shown(now));
            let effects = typed(pager, &"j".repeat(lines.len()), now);
            assert_eq!(kinds(&effects).last(), Some(&"relayout, same lines"));
        },
    );
}

#[test]
fn focus_in_draws_the_screen_anew_for_sixel_alone() {
    let now = Instant::now();
    let focus_in = |sixel: bool| {
        let mut kinds_of = Vec::new();
        let mut state = State::new(SearchCase::Smart);
        let run = Run { sixel, ..run(now) };
        with_pager(5, 10, &mut state, run, now, |pager| {
            kinds_of = kinds(&typed(pager, "\x1b[I", now));
        });
        kinds_of
    };
    // xterm keeps only some of the sixels it drew when it redraws its own
    // window, and a focus-in may follow: the whole screen is drawn again.
    assert!(focus_in(true).contains(&"lose"), "{:?}", focus_in(true));
    assert!(!focus_in(false).contains(&"lose"), "{:?}", focus_in(false));
}

/// A state over a base, with `count` views opened over it as a reader
/// following links does, whose top shows `document`, and the ids of the
/// views, and one more that is open in the session and not on the deck.
fn state_with_views(document: &Document, count: usize) -> (State, Vec<ViewId>, ViewId) {
    let mut session = Session::new(["a.md"]);
    let mut open = |at: usize| {
        let file = SourceFile {
            path: format!("{at}.md").into(),
            input: crate::typeset::input::Input::Markdown,
            look: None,
        };
        session.open(format!("{at}.md"), file)
    };
    let views: Vec<ViewId> = (1..=count).map(&mut open).collect();
    let gone = open(count + 1);
    let mut state = State::new(SearchCase::Smart);
    let now = Instant::now();
    let content = Content::Document(document);
    for &view in &views {
        state.push_view(view, Vec::new(), now);
        let edition = Edition::new(content, &session, view, None);
        state.document_mut().edition = Some(edition);
    }
    let base = Edition::new(content, &session, ViewId::BASE, None);
    state.base.edition = Some(base);
    (state, views, gone)
}

/// A place in `document`, held for the version `state`'s base shows.
fn place_in_base(state: &State, block: usize) -> Held<Anchor> {
    let anchor = Anchor::At {
        pos: crate::typeset::position::Pos::block(0, &[block]),
        lines: 0,
    };
    Held::new(state.base.edition.as_ref().unwrap(), anchor)
}

/// A state over a base showing `document`, which has its edition.
fn state_over(document: &Document) -> State {
    let session = Session::new(["a.md"]);
    let mut state = State::new(SearchCase::Smart);
    state.base.edition = Some(Edition::new(
        Content::Document(document),
        &session,
        ViewId::BASE,
        None,
    ));
    state
}

const BACKSPACE: &str = "\x7f";

#[test]
fn enter_on_a_markdown_link_asks_to_open_it_with_the_place_to_come_back_to() {
    let now = Instant::now();
    let document = crate::typeset::parse::parse("[go](docs/b.md#part) here\n\nmore\n");
    let mut state = state_over(&document);
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        typed(pager, "\t", now);
        let effects = typed(pager, "\r", now);
        assert_eq!(kinds(&effects), ["draw", "turn: open"]);
        let Some(Effect::Turn(Turn::Open(opening))) = effects.last() else {
            panic!("an opening")
        };
        assert_eq!(opening.path, Path::new("docs/b.md"));
        assert_eq!(opening.id.as_deref(), Some("part"));
        assert_eq!(opening.back.view, ViewId::BASE);
    });
    assert_eq!(state.told.as_ref().unwrap().message, "opening docs/b.md…");
}

fn opening(state: &State, path: &str, id: Option<&str>) -> Opening {
    Opening {
        path: path.into(),
        id: id.map(Into::into),
        at: None,
        forward: false,
        back: Back {
            view: ViewId::BASE,
            file: 0,
            place: place_in_base(state, 0),
        },
    }
}

#[test]
fn a_failed_open_tells_why_and_adds_no_place() {
    let document = crate::typeset::parse::parse("text\n");
    let mut state = state_over(&document);
    let opening = opening(&state, "gone.md", None);
    state.opened(opening, Err("No such file".to_owned()));
    let told = state.told.as_ref().unwrap();
    assert_eq!(told.message, "cannot open gone.md: No such file");
    assert!(state.history.pop().is_none(), "no place was added");
    assert!(state.deck.document().is_none(), "no view was pushed");
}

#[test]
fn a_failed_forward_open_empties_the_forward_list_and_a_failed_link_does_not() {
    let document = crate::typeset::parse::parse("text\n");
    let mut state = state_over(&document);
    let ahead = |name: &str| Ahead {
        path: name.into(),
        place: None,
    };
    state.ahead = vec![ahead("c.md"), ahead("b.md")];
    let mut opening = opening(&state, "gone.md", None);
    state.opened(
        Opening {
            path: "x.md".into(),
            id: None,
            at: None,
            forward: false,
            back: opening.back.clone(),
        },
        Err("No such file".to_owned()),
    );
    assert_eq!(state.ahead.len(), 2, "a link that fails leaves the list");
    opening.forward = true;
    state.opened(opening, Err("No such file".to_owned()));
    assert!(state.ahead.is_empty(), "{:?}", state.ahead);
}

#[test]
fn an_opened_view_keeps_the_place_it_came_from_and_the_anchor_it_goes_to() {
    let document = crate::typeset::parse::parse("text\n");
    let (mut state, views, _) = state_with_views(&document, 1);
    state.told = Some(Told::new("opening b.md…"));
    let opening = opening(&state, "b.md", Some("part"));
    let back = opening.back.clone();
    state.opened(opening, Ok(views[0]));
    assert!(state.told.is_none(), "the busy message goes");
    assert_eq!(state.history.pop(), Some(back));
    assert_eq!(state.document().arrive.as_deref(), Some("part"));
}

/// A document with three headings, the second named `second`.
fn headed() -> Document {
    let rows: String = (1..=8).map(|n| format!("Row {n}\n\n")).collect();
    crate::typeset::parse::parse(&format!("# First\n\n{rows}# Second\n\nUnder.\n\n{rows}"))
}

fn top_text(pager: &Pager) -> String {
    let line = &pager.layout.lines[pager.layout.view.top()];
    crate::typeset::style::text(&line.spans)
}

#[test]
fn an_opened_view_arrives_at_the_anchor_of_the_link() {
    let now = Instant::now();
    let document = headed();
    let mut state = state_over(&document);
    state.document_mut().arrive = Some("second".to_owned());
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        assert_eq!(pager.layout.view.top(), 0);
        step(pager, Msg::Shown(now));
        assert!(top_text(pager).contains("Second"), "{}", top_text(pager));
    });
    assert!(state.document().arrive.is_none(), "taken once");
    assert!(state.told.is_none());
}

#[test]
fn an_anchor_missing_in_the_opened_file_is_told_and_the_view_stays_at_the_top() {
    let now = Instant::now();
    let document = headed();
    let mut state = state_over(&document);
    state.document_mut().arrive = Some("third".to_owned());
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(pager.layout.view.top(), 0);
    });
    assert_eq!(
        state.told.as_ref().unwrap().message,
        "no #third in notes.md"
    );
}

#[test]
fn backspace_into_another_view_asks_to_turn_back_with_its_place() {
    let now = Instant::now();
    let document = crate::typeset::parse::parse("text\n");
    let (mut state, _, _) = state_with_views(&document, 1);
    let place = place_in_base(&state, 0);
    state.history.push(Back {
        view: ViewId::BASE,
        file: 0,
        place: place.clone(),
    });
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        let effects = typed(pager, BACKSPACE, now);
        assert_eq!(kinds(&effects), ["turn: back"]);
        let Some(Effect::Turn(Turn::Back(Return {
            view,
            place: held,
            left,
        }))) = effects.last()
        else {
            panic!("a return")
        };
        assert_eq!(*view, ViewId::BASE);
        assert_eq!(held.as_ref(), Some(&place));
        // The view on top is left where it is now.
        assert_eq!(left.len(), 1);
        assert!(left[0].place.is_some());
    });
}

#[test]
fn backspace_with_the_history_run_out_goes_back_to_the_view_below() {
    let now = Instant::now();
    let document = crate::typeset::parse::parse("text\n");
    let (mut state, views, _) = state_with_views(&document, 2);
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        let at = |effects: &[Effect]| match effects.last() {
            Some(Effect::Turn(Turn::Back(back))) => Some((back.view, back.place.is_none())),
            _ => None,
        };
        let effects = typed(pager, BACKSPACE, now);
        assert_eq!(at(&effects), Some((views[0], true)));
        // As the loop does, the view on top is dropped; the next one goes
        // to the base.
        pager.state.back_to(views[0]);
        let effects = typed(pager, BACKSPACE, now);
        assert_eq!(at(&effects), Some((ViewId::BASE, true)));
    });
}

#[test]
fn backspace_on_the_base_with_the_history_run_out_has_nothing_to_go_back_to() {
    let now = Instant::now();
    let document = crate::typeset::parse::parse("text\n");
    let mut state = state_over(&document);
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        assert!(!typed(pager, BACKSPACE, now).iter().any(Effect::ends));
    });
    assert_eq!(
        state.told.as_ref().unwrap().message,
        "nothing to go back to"
    );
}

#[test]
fn a_place_of_a_view_no_longer_on_the_deck_is_passed_over() {
    let now = Instant::now();
    let document = crate::typeset::parse::parse("text\n");
    let (mut state, _, gone) = state_with_views(&document, 1);
    let place = place_in_base(&state, 0);
    for view in [ViewId::BASE, gone] {
        let place = place.clone();
        state.history.push(Back {
            view,
            file: 0,
            place,
        });
    }
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        let effects = typed(pager, BACKSPACE, now);
        let Some(Effect::Turn(Turn::Back(back))) = effects.last() else {
            panic!("a return: {:?}", kinds(&effects))
        };
        assert_eq!(back.view, ViewId::BASE);
    });
}

#[test]
fn escape_leaves_an_opened_view_open() {
    let now = Instant::now();
    let document = crate::typeset::parse::parse("text\n");
    let (mut state, views, _) = state_with_views(&document, 1);
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        typed(pager, "\x1b", now);
    });
    assert_eq!(state.document().view, views[0]);
}

/// A window of 60 columns with a sidebar of 20 at the left and the
/// document beside it, over `rows` rows.
fn with_a_sidebar(rows: usize) -> crate::session::areas::Areas {
    use crate::present::scene::Area;
    use crate::session::areas::{Areas, Region, Role};
    let area = |role, column, columns| Region {
        role,
        area: Area {
            column,
            row: 1,
            columns,
            rows,
        },
    };
    Areas::new(vec![
        area(Role::Contents, 1, 20),
        area(Role::Document, 21, 40),
    ])
}

#[test]
fn a_click_in_an_offset_document_area_lands_on_the_link_at_its_own_column() {
    use crate::session::commands::Action;
    let now = Instant::now();
    let document = crate::typeset::parse::parse("A [first](https://one.example/) link.\n");
    let mut state = state_over(&document);
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        let areas = with_a_sidebar(5);
        let mut click = |column| {
            let action = Action::Click { column, row: 1 };
            pager
                .state
                .pointed_in(&areas, action, now, pager.layout)
                .expect("a click is answered")
        };
        // The link is on columns 5 to 9 of the document's own area, so on
        // 25 to 29 of the window. Column 5 of the window is the sidebar's.
        assert_eq!(kinds(&click(25)), ["copy"]);
        assert!(click(5).is_empty(), "the sidebar's cell is not the link");
        assert!(click(15).is_empty());
    });
}

#[test]
fn a_key_goes_to_the_document_while_another_area_is_the_key_area() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    state.key_area = crate::session::areas::Role::Contents;
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        let top = pager.layout.view.top();
        typed(pager, "j", now);
        assert_eq!(pager.layout.view.top(), top + 1, "j scrolled the document");
    });
}

#[test]
fn the_wheel_with_shift_over_a_docked_area_is_dropped_and_over_the_document_falls_back() {
    use crate::session::commands::Action;
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        let areas = with_a_sidebar(5);
        let mut wheel = |column| {
            let action = Action::Wheel {
                right: true,
                shift: true,
                column,
                row: 1,
            };
            pager.state.pointed_in(&areas, action, now, pager.layout)
        };
        assert!(
            wheel(5).is_ok_and(|effects| effects.is_empty()),
            "the sidebar takes the wheel"
        );
        assert!(matches!(wheel(25), Err(Action::Move(_))));
    });
}

// The contents bar: visibility and focus.

use crate::session::areas::Role;
use crate::session::sidebar::{Sidebar, Visibility};

const HEADED: &str = "# One\n\ntext\n\n## Two\n\nmore\n";

/// A state over `HEADED` with the bar `visibility` and the focus in `area`.
fn with_bar(visibility: Visibility, area: Role) -> (State, Document) {
    let document = crate::typeset::parse::parse(HEADED);
    let mut state = state_over(&document);
    state.sidebar = Sidebar::new(visibility);
    state.key_area = area;
    (state, document)
}

/// What `keys` do in a window of 60 columns to a state over `HEADED`
/// that starts as `with_bar` says: the state after them and the kinds of
/// the effects of the last key.
fn pressed(visibility: Visibility, area: Role, keys: &str) -> (State, Vec<&'static str>) {
    let now = Instant::now();
    let (mut state, document) = with_bar(visibility, area);
    let mut last = Vec::new();
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        let mut effects = typed(pager, keys, now);
        // A lone Escape waits for the rest of a sequence until a read
        // brings nothing more.
        if keys == "\x1b" {
            effects = step(pager, Msg::Input(Vec::new(), now));
        }
        last = kinds(&effects);
    });
    (state, last)
}

#[test]
fn capital_t_opens_a_hidden_bar_and_it_stays_open() {
    // A window of 60 columns is too narrow for `auto`.
    let (state, effects) = pressed(Visibility::Auto, Role::Document, "T");
    assert_eq!(state.sidebar.visibility, Visibility::Open);
    assert_eq!(state.key_area, Role::Document);
    assert_eq!(effects, ["relayout"]);
}

#[test]
fn capital_t_closes_a_shown_bar_and_gives_the_focus_back() {
    for from in [Visibility::Open, Visibility::Auto] {
        let columns = 60;
        let (mut state, document) = with_bar(from, Role::Contents);
        // `auto` shows the bar from 110 columns, so only `open` shows it here.
        let shown = state.bar_columns(columns) > 0;
        let now = Instant::now();
        let mut effects = Vec::new();
        with_document(&document, 5, (&mut state, run(now), now), |pager| {
            step(pager, Msg::Shown(now));
            effects = kinds(&typed(pager, "T", now));
        });
        match shown {
            true => {
                assert_eq!(state.sidebar.visibility, Visibility::Closed);
                assert_eq!(state.key_area, Role::Document);
                assert_eq!(effects, ["relayout"]);
            }
            false => assert_eq!(state.sidebar.visibility, Visibility::Open),
        }
    }
}

#[test]
fn small_t_opens_a_hidden_bar_and_takes_the_focus() {
    let (state, effects) = pressed(Visibility::Auto, Role::Document, "t");
    assert_eq!(state.sidebar.visibility, Visibility::Open);
    assert_eq!(state.key_area, Role::Contents);
    assert_eq!(effects, ["relayout"]);
}

#[test]
fn small_t_on_a_closed_bar_opens_it() {
    let (state, effects) = pressed(Visibility::Closed, Role::Document, "t");
    assert_eq!(state.sidebar.visibility, Visibility::Open);
    assert_eq!(state.key_area, Role::Contents);
    assert_eq!(effects, ["relayout"]);
}

#[test]
fn small_t_on_a_shown_bar_takes_the_focus_and_does_not_lay_out_again() {
    let (state, effects) = pressed(Visibility::Open, Role::Document, "t");
    assert_eq!(state.sidebar.visibility, Visibility::Open);
    assert_eq!(state.key_area, Role::Contents);
    assert!(!effects.contains(&"relayout"));
}

#[test]
fn zero_and_escape_give_the_focus_back_and_the_bar_stays_open() {
    for keys in ["0", "\x1b"] {
        let (state, effects) = pressed(Visibility::Open, Role::Contents, keys);
        assert_eq!(state.key_area, Role::Document, "{keys:?}");
        assert_eq!(state.sidebar.visibility, Visibility::Open, "{keys:?}");
        assert!(!effects.contains(&"relayout"), "{keys:?}");
    }
}

#[test]
fn escape_closes_a_card_before_it_gives_the_focus_back() {
    let now = Instant::now();
    let (mut state, document) = with_bar(Visibility::Open, Role::Contents);
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        typed(pager, "/", now);
        assert!(pager.state.deck.top().is_some(), "the prompt is open");
        typed(pager, "\x1b", now);
        step(pager, Msg::Input(Vec::new(), now));
        assert!(pager.state.deck.top().is_none());
        assert_eq!(
            pager.state.key_area,
            Role::Contents,
            "the prompt went first"
        );
        typed(pager, "\x1b", now);
        step(pager, Msg::Input(Vec::new(), now));
        assert_eq!(pager.state.key_area, Role::Document);
    });
}

#[test]
fn a_document_without_headings_has_no_bar_to_open_and_the_status_line_says_so() {
    let now = Instant::now();
    let document = crate::typeset::parse::parse("just text\n");
    for keys in ["T", "t"] {
        let mut state = state_over(&document);
        with_document(&document, 5, (&mut state, run(now), now), |pager| {
            step(pager, Msg::Shown(now));
            let effects = typed(pager, keys, now);
            assert!(!effects.iter().any(|e| matches!(e, Effect::Relayout { .. })));
        });
        assert_eq!(state.key_area, Role::Document, "{keys}");
        assert_eq!(
            state.told.as_ref().map(|told| told.message.as_str()),
            Some("no headings to list"),
            "{keys}"
        );
    }
}

#[test]
fn a_key_goes_to_the_deck_first_then_to_the_bar_in_focus_then_to_the_document() {
    let now = Instant::now();
    let source = "# One\n\n## Two\n\n## Three\n\ntext\n";
    let document = crate::typeset::parse::parse(source);
    let mut state = state_over(&document);
    state.sidebar = Sidebar::new(Visibility::Open);
    state.key_area = Role::Contents;
    state.sidebar.selected = Some(0);
    with_document(&document, 3, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        let top = pager.layout.view.top();
        // The bar's: `j` selects the next entry, and the document stays.
        typed(pager, "j", now);
        assert_eq!(pager.state.sidebar.selected, Some(1));
        assert_eq!(pager.layout.view.top(), top);
        // A card on the deck first: the prompt takes `j` as text.
        typed(pager, "/", now);
        typed(pager, "j", now);
        assert_eq!(pager.state.sidebar.selected, Some(1));
        assert!(pager.state.deck.top().is_some());
        typed(pager, "\r", now);
        // The document's: a key the bar does not have.
        typed(pager, " ", now);
        assert_eq!(pager.state.sidebar.selected, Some(1));
        assert_ne!(pager.layout.view.top(), top);
    });
}

#[test]
fn a_bar_that_is_not_shown_leaves_the_keys_to_the_document_and_takes_the_focus_back() {
    let now = Instant::now();
    let document = crate::typeset::parse::parse("# One\n\ntext\n\nmore\n\nmore\n\nmore\n");
    let mut state = state_over(&document);
    state.key_area = Role::Contents;
    with_document(&document, 3, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        let top = pager.layout.view.top();
        typed(pager, "j", now);
        assert_eq!(pager.layout.view.top(), top + 1);
        assert_eq!(pager.state.key_area, Role::Document);
    });
}

/// `# Aaa`, 40 paragraphs, `## Bbb` and a line.
fn long_ending() -> Document {
    let body: String = (1..=40).map(|n| format!("Paragraph {n}.\n\n")).collect();
    crate::typeset::parse::parse(&format!("# Aaa\n\n{body}## Bbb\n\nOne line.\n"))
}

#[test]
fn t_at_the_end_of_a_document_selects_the_entry_the_mark_shows() {
    let now = Instant::now();
    let document = long_ending();
    let mut state = state_over(&document);
    state.sidebar = Sidebar::new(Visibility::Open);
    with_document(&document, 12, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        typed(pager, "G", now);
        typed(pager, "t", now);
        let contents = &pager.state.document().edition.as_ref().unwrap().contents;
        let mark = pager
            .state
            .current_entry(contents, pager.layout.map, pager.layout.view);
        assert_eq!(mark, Some(1), "the mark is on Bbb at the end");
        assert_eq!(pager.state.sidebar.selected, mark, "t selects the mark");
    });
}

#[test]
fn a_view_pushed_over_a_focused_bar_takes_the_focus() {
    let document = crate::typeset::parse::parse(HEADED);
    let (mut state, views, _) = state_with_views(&document, 1);
    state.back_to(ViewId::BASE);
    state.sidebar = Sidebar::new(Visibility::Open);
    state.key_area = Role::Contents;
    state.sidebar.selected = Some(1);
    state.push_view(views[0], Vec::new(), Instant::now());
    assert_eq!(
        state.key_area,
        Role::Document,
        "the view's keys are its own"
    );
    assert_eq!(
        state.selected_entry(),
        None,
        "no selection in the view's bar"
    );
}

#[test]
fn t_in_a_view_that_covers_the_document_does_not_take_the_focus() {
    let now = Instant::now();
    let document = crate::typeset::parse::parse(HEADED);
    let (mut state, _, _) = state_with_views(&document, 1);
    state.sidebar = Sidebar::new(Visibility::Open);
    with_document(&document, 5, (&mut state, run(now), now), |pager| {
        step(pager, Msg::Shown(now));
        typed(pager, "t", now);
        assert_eq!(pager.state.key_area, Role::Document);
    });
}
