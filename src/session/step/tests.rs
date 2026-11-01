//! The step is driven without a terminal: messages in, effects out.

use std::time::Duration;

use super::*;
use crate::render::Inputs;
use crate::session::fade::Rgb;
use crate::session::files::Files;
use crate::session::frame::{Layout, Status};
use crate::session::search::Marks;
use crate::session::settings::{self, SearchCase};
use crate::session::state::Changed;
use crate::session::state::Look;
use crate::session::status;
use crate::session::view::View;
use crate::typeset::block::{BlockInfo, Blocks};
use crate::typeset::caps::Capabilities;
use crate::typeset::compare::Comparison;
use crate::typeset::layout::{Content, Stream, content};
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
    let places = vec![None; count];
    with_lines(&lines, &places, rows, (state, run, now), test);
}

/// Runs `test` on a pager that shows `lines`, from the blocks `places`, of
/// one row each, in a window of `rows` rows above the status line.
fn with_lines(
    lines: &[Line],
    places: &[Option<BlockInfo>],
    rows: usize,
    (state, run, now): (&mut State, Run, Instant),
    test: impl FnOnce(&mut Pager),
) {
    let theme = Theme::default();
    let caps = Capabilities::kitty(60);
    let blocks = Blocks::default();
    let marks = Marks::new(&theme, &caps);
    let rgb = Rgb::new(&theme, &caps);
    let status = Status {
        name: "notes.md",
        loading: None,
        files: Files::new(&[], lines, &blocks),
        styles: status::Styles::new(&theme, &caps),
        width: 60,
        bar: true,
        panel: Style::default(),
        muted: Style::default(),
        red: Style::default(),
        reading: false,
    };
    let mut view = View::new(vec![1; lines.len()], rows).with_blocks(places.to_vec());
    let mut layout = Layout {
        lines,
        places,
        marks: &marks,
        rgb: rgb.as_ref(),
        columns: 60,
        status: &status,
        view: &mut view,
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
            Effect::PollChange => "poll change",
            Effect::PollLoads => "poll loads",
            Effect::PollWindow => "poll window",
            Effect::Draw(..) => "draw",
            Effect::Check => "check",
            Effect::Resend(_) => "resend",
            Effect::Lose => "lose",
            Effect::LoseSome(_) => "lose some",
            Effect::Copy(_) => "copy",
            Effect::Quit => "quit",
            Effect::SetCells(_) => "set cells",
            Effect::AskCells => "ask cells",
            Effect::SwapReading => "swap reading",
            Effect::FlipWide => "flip wide",
            Effect::SetBackground(_) => "set background",
            Effect::Reload => "reload",
            Effect::Relayout { reuse: false } => "relayout",
            Effect::Relayout { reuse: true } => "relayout, same lines",
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
    with_pager(30, 5, &mut state, run(now), now, |pager| {
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
        let effects = step(pager, Msg::Loads { came: false });
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
        step(pager, Msg::Loads { came: false });
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
        assert_eq!(kinds(&effects), ["draw", "reload"]);
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
        };
        assert_eq!(kinds(&step(pager, Msg::Reloaded(reply, now))), ["relayout"]);
    });
    assert_eq!(state.warnings.len(), 1);
    assert!(state.next.changes.is_some());
    assert_eq!(state.told.as_ref().unwrap().message, "reloaded");
}

#[test]
fn a_following_pager_polls_the_document_and_reads_it_again_when_it_settled() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    state.follow.mode = settings::Follow::On;
    let look = Change::Changed(Look {
        modified: None,
        length: 7,
    });
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(kinds(&typed(pager, "", now)), ["poll change"]);
        // Seen once, it is not read yet, in case it is half written.
        let effects = step(pager, Msg::Changed(look.clone()));
        assert_eq!(kinds(&effects), ["poll loads"]);
        step(pager, Msg::Loads { came: false });
        step(
            pager,
            Msg::Window {
                resized: None,
                bands: false,
            },
        );
        let later = now + Duration::from_millis(150);
        assert_eq!(kinds(&typed(pager, "", later)), ["poll change"]);
        let effects = step(pager, Msg::Changed(look));
        assert_eq!(kinds(&effects), ["reload"]);
    });
    assert!(state.follow.seen.is_none());
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
        let effects = step(pager, Msg::Loads { came: true });
        assert_eq!(kinds(&effects), ["relayout"]);
    });
    assert!(state.next.keep_top);
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
fn ctrl_l_loses_the_images_and_draws_the_frame_again() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    with_pager(30, 5, &mut state, run(now), now, |pager| {
        step(pager, Msg::Shown(now));
        assert_eq!(kinds(&typed(pager, "\x0c", now)), ["lose", "poll loads"]);
        step(pager, Msg::Loads { came: false });
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
        for keys in ["q", "r", "M", "W"] {
            let effects = typed(pager, keys, now);
            let ends: Vec<bool> = effects.iter().map(Effect::ends).collect();
            let first = ends.iter().position(|&ends| ends).unwrap();
            assert!(ends[first..].iter().all(|&ends| ends), "{keys}");
            assert!(effects.last().unwrap().ends(), "{keys}");
        }
    });
}

#[test]
fn a_resize_takes_the_tint_off_the_lines_of_the_old_layout() {
    let now = Instant::now();
    let mut state = State::new(SearchCase::Smart);
    state.changed = Some(Changed {
        at: now,
        lines: 2..4,
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
    assert!(state.changed.is_none());
}

/// Two code blocks too wide for the area, laid out to scroll, as reading
/// mode lays them out, with the blocks of their lines.
fn two_bands() -> (Vec<Line>, Vec<Option<BlockInfo>>) {
    let long = "a".repeat(200);
    let document =
        crate::typeset::parse::parse(&format!("```\n{long}\n```\n\nText.\n\n```\n{long}\n```\n"));
    let inputs = Inputs {
        reading: Some(Reading {
            pictures: Default::default(),
            fonts: Rc::new(Fonts::fixture()),
            wide: Wide::Scroll,
        }),
        ..Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(80)
        })
    };
    let Stream { lines, blocks } = content(Content::Document(&document), &inputs);
    let places = (lines.iter())
        .map(|line| line.block.map(|id| blocks.get(id).clone()))
        .collect();
    (lines, places)
}

#[test]
fn a_selected_band_that_scrolls_off_the_screen_gives_its_selection_to_the_next() {
    let now = Instant::now();
    let (lines, places) = two_bands();
    let first = crate::typeset::reading::engine::bands(&lines)[0]
        .lines
        .clone();
    let mut state = State::new(SearchCase::Smart);
    let rows = first.len();
    with_lines(
        &lines,
        &places,
        rows,
        (&mut state, run(now), now),
        |pager| {
            // The first band is on the screen and selected.
            let visible = pager.layout.view.visible();
            assert!(pager.state.bands.refresh(&lines, &places, visible));
            step(pager, Msg::Shown(now));
            let effects = typed(pager, &"j".repeat(lines.len()), now);
            assert_eq!(kinds(&effects).last(), Some(&"relayout, same lines"));
        },
    );
}
