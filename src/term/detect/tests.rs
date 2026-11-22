use super::*;
use crate::term::testing::{XTERM_407_LIMITS, ghostty_1_1_3, size, with_kitty};
use crate::typeset::color::Color;

/// A tmux runner for tests where tmux must not run.
const NO_TMUX: Run = &|args| panic!("tmux was run: {args:?}");

/// The answers of a kitty 0.47.1 behind tmux 3.8, to a probe that
/// sends the graphics query wrapped: cursor reports and the version
/// come from tmux, the graphics answer from kitty.
const BEHIND_TMUX: &str = "\x1b[1;1R\x1b[1;1R\x1b[1;1R\x1b_Gi=31;OK\x1b\\\
                               \x1bP>|tmux 3.8\x1b\\\x1b[?1;2;4c";

/// The answers of a kitty 0.47.1 on its own.
const KITTY: &str = "\x1b[5;1R\x1b[5;3R\x1b[5;5R\x1b_Gi=31;OK\x1b\\\
                         \x1bP>|kitty(0.47.1)\x1b\\\x1b[?62;c";

/// An environment inside tmux, in the pane `%3`.
fn in_tmux() -> Environment {
    Environment {
        tmux: Some("/tmp/tmux-1000/default,1,0".into()),
        tmux_pane: Some("%3".into()),
        ..Environment::default()
    }
}

/// What tmux was asked, which graphics query the probe was told to
/// send, if it was sent, and what detection found, in `mode` on a
/// terminal if `terminal`, where the probe gets `answers` and tmux
/// answers `client|allow-passthrough` with `pane`, and succeeds with
/// every other command.
fn through_tmux(
    mode: Mode,
    env: &Environment,
    passthrough: Passthrough,
    terminal: bool,
    (answers, pane): (&str, &str),
) -> (Vec<String>, Option<GraphicsQuery>, Detection) {
    let calls = std::cell::RefCell::new(Vec::new());
    let run = |args: &[&str]| {
        calls.borrow_mut().push(args.join(" "));
        let display = args[0] == "display-message";
        Some(if display { pane } else { "" }.to_owned())
    };
    let mut sent = None;
    let ask = |_, graphics| {
        sent = Some(graphics);
        Some(answers.as_bytes().to_vec())
    };
    let found = detect(mode, None, env, passthrough, None, terminal, ask, &run);
    (calls.into_inner(), sent, found)
}

#[test]
fn sixel_behind_tmux_is_named_but_draws_nothing() {
    // Both tmux's answer and the terminal outside's list sixel, and the
    // cells are known from the window of the terminal outside.
    let answers =
        "\x1b[1;1R\x1b[1;1R\x1b[1;1R\x1bP>|tmux 3.8\x1b\\\x1b[?1;2;4c\x1b[?62;4c\x1b[6;20;10t";
    let (_, _, found) = through_tmux(
        Mode::Auto,
        &in_tmux(),
        Passthrough::Detect,
        true,
        (answers, "foot|on"),
    );
    let caps = &found.caps;
    assert_eq!(caps.graphics.name(), "sixel");
    assert!(caps.cell_pixels.is_some());
    assert!(caps.tmux_passthrough);
    assert!(!caps.draws_images());
    assert!(!caps.is_sixel());
}

#[test]
fn inside_tmux_the_graphics_query_is_wrapped() {
    let answers = (BEHIND_TMUX, "kitty(0.47.1)|on");
    let (calls, sent, found) =
        through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
    assert_eq!(sent, Some(GraphicsQuery::Wrapped));
    assert_eq!(calls.len(), 1, "{calls:?}");
    let caps = &found.caps;
    assert!(caps.draws_images());
    assert_eq!(caps.max_image_cells(), graphics::MAX_CELLS);
    assert!(caps.tmux_passthrough);
    assert!(caps.truecolor);
    // tmux cannot pass text sizing, and links and underline styles
    // need tmux features of their own.
    assert!(!caps.text_sizing);
    assert!(!caps.hyperlinks);
    assert!(!caps.styled_underlines);
    let pane = Pane {
        client: Some("kitty(0.47.1)".into()),
        passthrough: Some("on".into()),
        turned_on: false,
        focus_events: None,
        ..Pane::default()
    };
    assert_eq!(found.tmux, Some(pane));
}

#[test]
fn inside_tmux_the_cell_size_comes_from_the_terminal_outside() {
    // tmux gives a pane 16 x 32 pixels a cell when it does not know
    // the terminal's pixel size, as in a container whose terminal does
    // not pass it; kitty's cells are 10 x 21.
    let window = WindowSize {
        columns: 112,
        rows: 65,
        width: 112 * 16,
        height: 65 * 32,
    };
    let answers = BEHIND_TMUX.replace("\x1bP>|", "\x1b[6;21;10t\x1bP>|");
    let run = |args: &[&str]| {
        let display = args[0] == "display-message";
        Some(if display { "kitty(0.47.1)|on" } else { "" }.to_owned())
    };
    let mut asked = None;
    let ask = |cell_size, _| {
        asked = Some(cell_size);
        Some(answers.as_bytes().to_vec())
    };
    let found = detect(
        Mode::Auto,
        None,
        &in_tmux(),
        Passthrough::Detect,
        Some(window),
        true,
        ask,
        &run,
    );
    assert_eq!(asked, Some(true));
    assert_eq!(found.caps.cell_pixels, Some((10, 21)));
    assert_eq!(found.cell_size(), CellSize::Terminal(10, 21));
}

#[test]
fn inside_tmux_the_window_s_cell_size_stays_when_kitty_does_not_tell() {
    let window = WindowSize {
        columns: 112,
        rows: 65,
        width: 112 * 16,
        height: 65 * 32,
    };
    let run = |args: &[&str]| {
        let display = args[0] == "display-message";
        Some(if display { "kitty(0.47.1)|on" } else { "" }.to_owned())
    };
    let ask = |_, _| Some(BEHIND_TMUX.as_bytes().to_vec());
    let found = detect(
        Mode::Auto,
        None,
        &in_tmux(),
        Passthrough::Detect,
        Some(window),
        true,
        ask,
        &run,
    );
    assert_eq!(found.caps.cell_pixels, Some((16, 32)));
    assert_eq!(found.cell_size(), CellSize::Window(16, 32));
}

#[test]
fn each_run_through_tmux_has_image_ids_of_its_own() {
    let answers = (BEHIND_TMUX, "kitty(0.47.1)|on");
    let run = || {
        let (_, _, found) =
            through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
        found.caps.image_run
    };
    let (first, second) = (run(), run());
    assert!(first.is_some());
    assert_ne!(first, second);
    let (_, _, found) = through_tmux(Mode::Kitty, &in_tmux(), Passthrough::Tmux, true, answers);
    assert!(found.caps.image_run.is_some());
    // Without tmux, ids stay the same on every run.
    let (_, _, found) = through_tmux(
        Mode::Auto,
        &Environment::default(),
        Passthrough::Detect,
        true,
        (KITTY, ""),
    );
    assert_eq!(found.caps.image_run, None);
}

#[test]
fn inside_tmux_passthrough_off_is_turned_on() {
    let answers = (BEHIND_TMUX, "kitty(0.47.1)|off");
    let (calls, _, found) =
        through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
    let set = "set-option -p -t %3 allow-passthrough on";
    assert_eq!(calls.last().map(String::as_str), Some(set));
    assert!(found.tmux.unwrap().turned_on);
}

#[test]
fn an_old_kitty_behind_tmux_gets_no_placeholders() {
    let answers = (BEHIND_TMUX, "kitty(0.27.0)|on");
    let (_, _, found) = through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
    assert_eq!(found.caps.graphics.name(), "kitty");
    assert_eq!(found.caps.max_image_cells(), 0);
    assert!(!found.caps.draws_images());
}

#[test]
fn ghostty_behind_tmux_draws_images() {
    // tmux names Ghostty 1.1.3 so, and Ghostty answers the wrapped
    // graphics query as kitty does.
    let answers = (BEHIND_TMUX, "ghostty 1.1.3-1.fc43|on");
    let (_, _, found) = through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
    assert!(found.caps.draws_images());
    assert_eq!(found.caps.max_image_cells(), graphics::MAX_CELLS);
    assert!(!found.caps.text_sizing);
}

#[test]
fn behind_tmux_without_an_answer_from_kitty_no_images_are_drawn() {
    // Passthrough stayed off: kitty never saw the graphics query.
    let answers = (
        "\x1b[1;1R\x1bP>|tmux 3.8\x1b\\\x1b[?1;2;4c",
        "kitty(0.47.1)|off",
    );
    let (_, _, found) = through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
    assert_eq!(found.caps.graphics, Graphics::None);
    assert!(!found.caps.draws_images());
    assert!(found.caps.tmux_passthrough);
}

#[test]
fn tmux_with_passthrough_none_sends_no_graphics_query() {
    let answers = (BEHIND_TMUX, "kitty(0.47.1)|on");
    let (calls, sent, found) =
        through_tmux(Mode::Auto, &in_tmux(), Passthrough::None, true, answers);
    assert_eq!(sent, Some(GraphicsQuery::Skip));
    assert_eq!(calls, Vec::<String>::new());
    assert!(!found.caps.tmux_passthrough);
    assert_eq!(found.tmux, None);
}

#[test]
fn outside_tmux_detect_changes_nothing() {
    let env = Environment::default();
    let answers = (KITTY, "");
    let (calls, sent, found) = through_tmux(Mode::Auto, &env, Passthrough::Detect, true, answers);
    assert_eq!(sent, Some(GraphicsQuery::Plain));
    assert_eq!(calls, Vec::<String>::new());
    // What it is the same as, apart from the version kitty tells.
    let mut kitty = Capabilities::kitty(80);
    if let Graphics::Kitty(g) = &mut kitty.graphics {
        g.terminal = Some("kitty(0.47.1)".into());
    }
    assert_eq!(found.caps, kitty);
    assert_eq!(found.tmux, None);
    // An empty TMUX is no tmux either.
    let empty = Environment {
        tmux: Some("".into()),
        ..in_tmux()
    };
    let (calls, sent, _) = through_tmux(Mode::Auto, &empty, Passthrough::Detect, true, answers);
    assert_eq!(sent, Some(GraphicsQuery::Plain));
    assert_eq!(calls, Vec::<String>::new());
}

#[test]
fn passthrough_tmux_wraps_without_asking_tmux() {
    let env = Environment::default();
    let answers = (BEHIND_TMUX, "kitty(0.47.1)|off");
    let (calls, sent, found) = through_tmux(Mode::Auto, &env, Passthrough::Tmux, true, answers);
    assert_eq!(sent, Some(GraphicsQuery::Wrapped));
    assert_eq!(calls, Vec::<String>::new());
    assert!(found.caps.tmux_passthrough);
    // Without tmux's word on the terminal, kitty's answer decides.
    assert!(found.caps.draws_images());
    let pane = Pane {
        client: None,
        passthrough: None,
        turned_on: false,
        focus_events: None,
        ..Pane::default()
    };
    assert_eq!(found.tmux, Some(pane));
}

#[test]
fn a_pipe_inside_tmux_runs_no_tmux_command() {
    for passthrough in [Passthrough::Detect, Passthrough::Tmux] {
        let answers = (BEHIND_TMUX, "kitty(0.47.1)|off");
        let (calls, sent, found) =
            through_tmux(Mode::Auto, &in_tmux(), passthrough, false, answers);
        assert_eq!(sent, None);
        assert_eq!(calls, Vec::<String>::new());
        assert_eq!(found.caps, Capabilities::plain(80));
        assert_eq!(found.tmux, None);
    }
}

#[test]
fn plain_and_ansi_modes_inside_tmux_run_no_tmux_command() {
    for mode in [Mode::Plain, Mode::Ansi] {
        let answers = (BEHIND_TMUX, "kitty(0.47.1)|off");
        let (calls, sent, found) =
            through_tmux(mode, &in_tmux(), Passthrough::Detect, true, answers);
        assert_eq!(sent, None);
        assert_eq!(calls, Vec::<String>::new());
        assert!(!found.caps.tmux_passthrough);
    }
}

#[test]
fn kitty_mode_inside_tmux_wraps_without_text_sizing() {
    let answers = (BEHIND_TMUX, "kitty(0.47.1)|off");
    let (calls, sent, found) =
        through_tmux(Mode::Kitty, &in_tmux(), Passthrough::Detect, true, answers);
    assert_eq!(sent, None);
    assert_eq!(calls.len(), 2, "{calls:?}");
    let expected = Capabilities {
        text_sizing: false,
        hyperlinks: false,
        styled_underlines: false,
        tmux_passthrough: true,
        image_run: found.caps.image_run,
        ..Capabilities::kitty(80)
    };
    assert_eq!(found.caps, expected);
    assert!(found.tmux.unwrap().turned_on);
    // With none, kitty mode stays as it is.
    let (calls, _, found) = through_tmux(Mode::Kitty, &in_tmux(), Passthrough::None, true, answers);
    assert_eq!(calls, Vec::<String>::new());
    assert_eq!(found.caps, Capabilities::kitty(80));
}

#[test]
fn a_probed_kitty_draws_images_up_to_the_placeholder_limit() {
    let answers = "\x1b[5;1R\x1b[5;3R\x1b[5;5R\x1b_Gi=31;OK\x1b\\\x1b[?62;c";
    let caps = with_probe(
        Capabilities::ansi(80),
        &answers::interpret(answers.as_bytes()),
    );
    assert!(caps.draws_images());
    assert_eq!(caps.max_image_cells(), graphics::MAX_CELLS);
    assert_eq!(caps, Capabilities::kitty(80));
}

#[test]
fn the_probe_tells_the_cell_size_only_when_the_window_does_not() {
    let probe = answers::Probe {
        cell_pixels: Some((9, 19)),
        ..answers::Probe::default()
    };
    let caps = with_probe(Capabilities::ansi(80), &probe);
    assert_eq!(caps.cell_pixels, Some((9, 19)));
    let window = Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::ansi(80)
    };
    assert_eq!(with_probe(window, &probe).cell_pixels, Some((10, 20)));
    let caps = with_probe(Capabilities::ansi(80), &answers::Probe::default());
    assert_eq!(caps.cell_pixels, None);
}

#[test]
fn a_probed_kitty_0_28_draws_images_without_text_sizing() {
    let answers = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b_Gi=31;OK\x1b\\\
                       \x1bP>|kitty(0.28.0)\x1b\\\x1b[?62;c";
    let caps = with_probe(
        Capabilities::ansi(80),
        &answers::interpret(answers.as_bytes()),
    );
    assert!(caps.draws_images());
    assert!(!caps.text_sizing);
    assert_eq!(caps.max_image_cells(), graphics::MAX_CELLS);
}

#[test]
fn kitty_plays_animations_itself_and_ghostty_has_them_redrawn() {
    let kitty = "\x1b[5;1R\x1b[5;3R\x1b[5;5R\x1b_Gi=31;OK\x1b\\\
                     \x1bP>|kitty(0.47.1)\x1b\\\x1b[?62;c";
    let caps = with_probe(
        Capabilities::ansi(80),
        &answers::interpret(kitty.as_bytes()),
    );
    assert_eq!(caps.playback(), Playback::Terminal);
    assert!(caps.plays_animations());
    let ghostty = ghostty_1_1_3(true);
    let caps = with_probe(
        Capabilities::ansi(80),
        &answers::interpret(ghostty.as_bytes()),
    );
    assert_eq!(caps.playback(), Playback::Redraw);
    assert_eq!(Capabilities::ansi(80).playback(), Playback::None);
}

#[test]
fn kitty_behind_tmux_plays_animations_itself() {
    let answers = (BEHIND_TMUX, "kitty(0.47.1)|on");
    let (_, _, found) = through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
    assert!(found.caps.draws_images());
    assert_eq!(found.caps.playback(), Playback::Terminal);
}

#[test]
fn ghostty_behind_tmux_shows_the_first_frame_of_an_animation() {
    let answers = (BEHIND_TMUX, "ghostty 1.1.3-1.fc43|on");
    let (_, _, found) = through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
    assert!(found.caps.draws_images());
    assert_eq!(found.caps.playback(), Playback::None);
}

#[test]
fn a_probed_ghostty_draws_images_without_text_sizing() {
    // Ghostty 1.1.3's answers, the palette left out: it ignores the
    // text sizing tests and answers the graphics query.
    let answers = ghostty_1_1_3(true);
    let caps = with_probe(
        Capabilities::ansi(80),
        &answers::interpret(answers.as_bytes()),
    );
    assert!(caps.draws_images());
    assert!(!caps.text_sizing);
    assert_eq!(caps.max_image_cells(), graphics::MAX_CELLS);
    assert_eq!(caps.cell_pixels, Some((9, 20)));
    assert_eq!(caps.dark_background, Some(true));
}

#[test]
fn a_terminal_without_placeholders_draws_no_images() {
    let answers = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b_Gi=31;OK\x1b\\\x1b[?62;c";
    let caps = with_probe(
        Capabilities::ansi(80),
        &answers::interpret(answers.as_bytes()),
    );
    assert_eq!(caps.graphics.name(), "kitty");
    assert!(!caps.draws_images());
    assert_eq!(caps.max_image_cells(), 0);
}

#[test]
fn examining_a_window_without_pixels_asks_the_terminal_for_the_cell_size() {
    let (seen, found) = with_kitty(size(80, 24, 0, 0), |tty| {
        let env = Environment::default();
        examine_on(tty, Mode::Auto, None, &env, Passthrough::Detect, NO_TMUX)
    });
    assert!(seen.contains("\x1b[16t"), "{seen:?}");
    assert_eq!(found.cell_size(), CellSize::Terminal(9, 19));
    assert_eq!(found.caps.cell_pixels, Some((9, 19)));
    assert_eq!(found.terminal().as_deref(), Some("kitty(0.47.1)"));
    assert_eq!(
        found.window,
        Some(WindowSize {
            columns: 80,
            rows: 24,
            width: 0,
            height: 0,
        })
    );
    assert!(found.caps.draws_images());
    assert_eq!(found.caps.width, 80);
    let answers = String::from_utf8(found.answers.unwrap()).unwrap();
    assert!(answers.ends_with("\x1b[6;19;9t\x1b[?62;c"), "{answers:?}");
}

#[test]
fn examining_a_window_with_pixels_takes_the_cell_size_from_it() {
    let (seen, found) = with_kitty(size(80, 24, 800, 480), |tty| {
        let env = Environment::default();
        examine_on(
            tty,
            Mode::Auto,
            Some(60),
            &env,
            Passthrough::Detect,
            NO_TMUX,
        )
    });
    assert!(!seen.contains("\x1b[16t"), "{seen:?}");
    assert_eq!(found.cell_size(), CellSize::Window(10, 20));
    assert_eq!(found.caps.width, 60);
    // The colours the terminal tells go into the capabilities.
    assert_eq!(found.caps.foreground, Some(Color::Rgb(0xdd, 0xdd, 0xdd)));
    assert_eq!(found.caps.background, Some(Color::Rgb(0x1e, 0x1e, 0x2e)));
    assert_eq!(found.caps.palette[4], Some(Color::Rgb(0, 0, 0xee)));
}

#[test]
fn a_terminal_that_does_not_answer_leaves_the_cell_size_unknown() {
    let window = WindowSize {
        columns: 80,
        rows: 24,
        width: 0,
        height: 0,
    };
    let found = detect(
        Mode::Auto,
        None,
        &Environment::default(),
        Passthrough::Detect,
        Some(window),
        true,
        |_, _| None,
        NO_TMUX,
    );
    assert_eq!(found.cell_size(), CellSize::Unknown);
    assert_eq!(found.terminal(), None);
    assert_eq!(found.answers, None);
    assert_eq!(
        found.caps,
        with_window(Capabilities::ansi(80), Some(window))
    );
}

#[test]
fn only_the_auto_mode_asks_a_terminal() {
    let env = Environment::default();
    let never = |_, _| -> Option<Vec<u8>> { panic!("the terminal was asked") };
    let detect = |mode, width, terminal| {
        detect(
            mode,
            width,
            &env,
            Passthrough::Detect,
            None,
            terminal,
            never,
            NO_TMUX,
        )
    };
    let found = detect(Mode::Kitty, None, true);
    assert_eq!(found.caps, Capabilities::kitty(80));
    assert_eq!(found.cell_size(), CellSize::Unknown);
    let found = detect(Mode::Auto, Some(70), false);
    assert_eq!(found.caps, Capabilities::plain(70));
}

#[test]
fn takes_the_height_and_the_cell_size_from_the_window() {
    let window = WindowSize {
        columns: 80,
        rows: 24,
        width: 800,
        height: 480,
    };
    let caps = with_window(Capabilities::kitty(60), Some(window));
    assert_eq!(caps.height, Some(24));
    assert_eq!(caps.cell_pixels, Some((10, 20)));
    // The width given on the command line stays.
    assert_eq!(caps.width, 60);
    let without_pixels = WindowSize {
        width: 0,
        height: 0,
        ..window
    };
    let caps = with_window(Capabilities::kitty(60), Some(without_pixels));
    assert_eq!(caps.height, Some(24));
    assert_eq!(caps.cell_pixels, None);
    assert_eq!(
        with_window(Capabilities::kitty(60), None),
        Capabilities::kitty(60)
    );
}

#[test]
fn a_window_of_no_rows_gives_its_width_and_no_height() {
    let window = WindowSize {
        columns: 120,
        rows: 0,
        width: 0,
        height: 0,
    };
    let found = detect(
        Mode::Ansi,
        None,
        &Environment::default(),
        Passthrough::Detect,
        Some(window),
        true,
        |_, _| None,
        NO_TMUX,
    );
    assert_eq!(found.caps.width, 120);
    assert_eq!(found.caps.height, None);
}

#[test]
fn a_width_given_apart_from_the_window_leaves_the_window_its_columns() {
    let window = WindowSize {
        columns: 120,
        rows: 40,
        width: 0,
        height: 0,
    };
    let env = Environment::default();
    let found = |mode, width, window| {
        let found = detect(
            mode,
            width,
            &env,
            Passthrough::Detect,
            window,
            true,
            |_, _| None,
            NO_TMUX,
        );
        (found.caps.width, found.caps.window_width)
    };
    for mode in [Mode::Auto, Mode::Kitty, Mode::Ansi, Mode::Plain] {
        assert_eq!(found(mode, Some(40), Some(window)), (40, 120), "{mode:?}");
    }
    assert_eq!(found(Mode::Kitty, None, Some(window)), (120, 120));
    assert_eq!(found(Mode::Kitty, Some(40), None), (40, 40));
}

fn env_with(set: impl Fn(&mut Environment)) -> Environment {
    let mut env = Environment::default();
    set(&mut env);
    env
}

#[test]
fn no_color_needs_a_value() {
    let colours_off =
        |value: Option<&str>| no_color(&env_with(|env| env.no_color = value.map(Into::into)));
    assert!(!colours_off(None));
    assert!(!colours_off(Some("")));
    assert!(colours_off(Some("1")));
    assert!(colours_off(Some("0")));
}

#[test]
fn the_image_compression_level_is_a_whole_number_from_0_to_9() {
    let level = |value: Option<&str>| {
        image_compression(&env_with(|env| {
            env.kiri_image_compression = value.map(Into::into);
        }))
    };
    assert_eq!(level(None), None);
    assert_eq!(level(Some("")), None);
    assert_eq!(level(Some("0")), Some(0));
    assert_eq!(level(Some("2")), Some(2));
    assert_eq!(level(Some("9")), Some(9));
    // Anything else is left out, and the medium decides.
    assert_eq!(level(Some("10")), None);
    assert_eq!(level(Some("-1")), None);
    assert_eq!(level(Some("fast")), None);
    assert_eq!(level(Some("2.5")), None);
}

#[test]
fn ssh_is_told_by_either_variable_having_a_value() {
    let over_ssh = |connection: Option<&str>, tty: Option<&str>| {
        over_ssh(&env_with(|env| {
            env.ssh_connection = connection.map(Into::into);
            env.ssh_tty = tty.map(Into::into);
        }))
    };
    assert!(!over_ssh(None, None));
    assert!(!over_ssh(Some(""), Some("")));
    assert!(over_ssh(Some("10.0.0.2 5000 10.0.0.1 22"), None));
    assert!(over_ssh(None, Some("/dev/pts/3")));
    assert!(over_ssh(Some("10.0.0.2 5000 10.0.0.1 22"), Some("")));
}

#[test]
fn capabilities_tell_whether_kiri_runs_over_ssh_in_every_mode() {
    let ssh = env_with(|env| env.ssh_connection = Some("10.0.0.2 5000 10.0.0.1 22".into()));
    for mode in [Mode::Plain, Mode::Ansi, Mode::Kitty] {
        let caps = |env| capabilities(mode, Some(40), env, Passthrough::None);
        assert!(caps(&ssh).over_ssh, "{mode:?}");
        assert!(!caps(&Environment::default()).over_ssh, "{mode:?}");
    }
}

#[test]
fn no_color_turns_colours_off_in_the_forced_modes() {
    let without_colours = env_with(|env| env.no_color = Some("1".into()));
    for mode in [Mode::Plain, Mode::Ansi, Mode::Kitty] {
        assert!(
            !capabilities(mode, Some(40), &without_colours, Passthrough::None).color,
            "{mode:?}"
        );
    }
    let env = Environment::default();
    assert!(capabilities(Mode::Ansi, Some(40), &env, Passthrough::None).color);
    assert!(capabilities(Mode::Kitty, Some(40), &env, Passthrough::None).color);
}

#[test]
fn a_forced_mode_takes_the_width_it_is_given() {
    let env = Environment::default();
    assert_eq!(
        capabilities(Mode::Plain, Some(40), &env, Passthrough::None),
        Capabilities::plain(40)
    );
    assert_eq!(
        capabilities(Mode::Ansi, Some(40), &env, Passthrough::None),
        Capabilities::ansi(40)
    );
    assert_eq!(
        capabilities(Mode::Kitty, Some(40), &env, Passthrough::None).width,
        40
    );
}

#[test]
fn colorterm_tells_of_24_bit_colour() {
    for (value, truecolor) in [
        (None, false),
        (Some("truecolor"), true),
        (Some("24bit"), true),
        (Some("yes"), false),
    ] {
        let env = env_with(|env| env.colorterm = value.map(Into::into));
        let caps = with_environment(Capabilities::ansi(80), &env);
        assert_eq!(caps.truecolor, truecolor, "{value:?}");
    }
}

#[test]
fn known_terminals_get_hyperlinks() {
    let hyperlinks = |set: &dyn Fn(&mut Environment)| {
        with_environment(Capabilities::ansi(80), &env_with(set)).hyperlinks
    };
    assert!(!hyperlinks(&|_| {}));
    assert!(hyperlinks(&|env| env.term_program = Some("WezTerm".into())));
    assert!(!hyperlinks(
        &|env| env.term_program = Some("Apple_Terminal".into())
    ));
    assert!(hyperlinks(&|env| env.vte_version = Some("5000".into())));
    assert!(!hyperlinks(&|env| env.vte_version = Some("4999".into())));
    assert!(!hyperlinks(&|env| env.vte_version = Some("new".into())));
}

/// tmux's answer for kitty 0.47.1 behind it, with the client's terminal
/// `features`.
fn kitty_with(features: &str) -> String {
    format!("kitty(0.47.1)|on|1|{features}|xterm-kitty")
}

#[test]
fn inside_tmux_links_are_drawn_when_tmux_passes_them() {
    for mode in [Mode::Auto, Mode::Kitty] {
        let pane = kitty_with("bpaste,clipboard,hyperlinks,focus,title");
        let answers = (BEHIND_TMUX, pane.as_str());
        let (_, _, found) = through_tmux(mode, &in_tmux(), Passthrough::Detect, true, answers);
        assert!(found.caps.hyperlinks, "{mode:?}");
        // Underline styles still need a feature of their own.
        assert!(!found.caps.styled_underlines, "{mode:?}");
        assert_eq!(found.tmux.unwrap().hyperlinks, Some(true));
    }
}

#[test]
fn inside_tmux_links_are_not_drawn_when_tmux_does_not_pass_them() {
    for mode in [Mode::Auto, Mode::Kitty] {
        let pane = kitty_with("bpaste,clipboard,focus,title");
        let answers = (BEHIND_TMUX, pane.as_str());
        let (_, _, found) = through_tmux(mode, &in_tmux(), Passthrough::Detect, true, answers);
        assert!(!found.caps.hyperlinks, "{mode:?}");
        assert_eq!(found.tmux.unwrap().hyperlinks, Some(false));
    }
}

#[test]
fn inside_tmux_links_are_not_drawn_when_tmux_does_not_tell() {
    for mode in [Mode::Auto, Mode::Kitty] {
        for pane in ["", "kitty(0.47.1)|on|1"] {
            let answers = (BEHIND_TMUX, pane);
            let (_, _, found) = through_tmux(mode, &in_tmux(), Passthrough::Detect, true, answers);
            assert!(!found.caps.hyperlinks, "{mode:?} {pane:?}");
        }
    }
}

#[test]
fn inside_tmux_only_tmux_tells_of_links() {
    // A variable inherited from outside tmux says nothing about whether
    // tmux passes links.
    let env = Environment {
        vte_version: Some("7600".into()),
        term_program: Some("WezTerm".into()),
        ..in_tmux()
    };
    for mode in [Mode::Auto, Mode::Kitty] {
        for (pane, links) in [
            (kitty_with("bpaste,focus"), false),
            (kitty_with("bpaste,hyperlinks,focus"), true),
            // tmux did not answer.
            (String::new(), false),
        ] {
            let answers = (BEHIND_TMUX, pane.as_str());
            let (_, _, found) = through_tmux(mode, &env, Passthrough::Detect, true, answers);
            assert_eq!(found.caps.hyperlinks, links, "{mode:?} {pane:?}");
        }
    }
    // Also when the probe gets no answer.
    for (features, links) in [("bpaste,focus", false), ("hyperlinks", true)] {
        let pane = kitty_with(features);
        let run = |args: &[&str]| {
            let display = args[0] == "display-message";
            Some(if display { pane.as_str() } else { "" }.to_owned())
        };
        let no_answer = |_, _| None;
        let found = detect(
            Mode::Auto,
            None,
            &env,
            Passthrough::Detect,
            None,
            true,
            no_answer,
            &run,
        );
        assert_eq!(found.caps.hyperlinks, links, "no answer, {features:?}");
    }
}

#[test]
fn with_passthrough_tmux_links_follow_the_environment_in_auto_mode_only() {
    // tmux is not asked, so its features do not count.
    let pane = kitty_with("hyperlinks");
    let answers = (BEHIND_TMUX, pane.as_str());
    let (calls, _, found) = through_tmux(Mode::Auto, &in_tmux(), Passthrough::Tmux, true, answers);
    assert_eq!(calls, Vec::<String>::new());
    assert!(!found.caps.hyperlinks);
    let env = Environment {
        vte_version: Some("7600".into()),
        ..in_tmux()
    };
    let (_, _, found) = through_tmux(Mode::Auto, &env, Passthrough::Tmux, true, answers);
    assert!(found.caps.hyperlinks);
    let (_, _, found) = through_tmux(Mode::Kitty, &env, Passthrough::Tmux, true, answers);
    assert!(!found.caps.hyperlinks);
}

#[test]
fn inside_tmux_links_follow_tmux_also_when_the_probe_gets_no_answer() {
    for (features, links) in [("bpaste,hyperlinks,focus", true), ("bpaste,focus", false)] {
        let pane = kitty_with(features);
        let run = |args: &[&str]| {
            let display = args[0] == "display-message";
            Some(if display { pane.as_str() } else { "" }.to_owned())
        };
        let no_answer = |_, _| None;
        let found = detect(
            Mode::Auto,
            None,
            &in_tmux(),
            Passthrough::Detect,
            None,
            true,
            no_answer,
            &run,
        );
        assert_eq!(found.caps.hyperlinks, links, "{features:?}");
    }
}

/// Capabilities from the answers `answers`, as detection builds them
/// outside tmux.
fn caps_from(answers: &str) -> Capabilities {
    with_probe(
        Capabilities::ansi(80),
        &answers::interpret(answers.as_bytes()),
    )
}

#[test]
fn kitty_with_placeholders_wins_over_sixel() {
    let answers = crate::term::testing::kitty_0_47_1(false).replace("?62;c", "?62;4;c");
    let caps = caps_from(&answers);
    assert_eq!(caps.graphics.name(), "kitty");
    assert_eq!(caps.max_image_cells(), graphics::MAX_CELLS);
}

#[test]
fn graphics_without_placeholders_is_sixel_where_the_terminal_has_it() {
    let graphics = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b_Gi=31;OK\x1b\\";
    let caps = caps_from(&format!("{graphics}\x1b[?65;4;6;18;22c"));
    assert_eq!(caps.graphics.name(), "sixel");
    assert!(!caps.draws_images());
    let caps = caps_from(&format!("{graphics}\x1b[?62;c"));
    assert_eq!(caps.graphics.name(), "kitty");
    assert_eq!(caps.max_image_cells(), 0);
}

#[test]
fn sixel_alone_is_sixel_and_is_drawn_when_a_cell_size_is_known() {
    let caps = caps_from("\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b[?62;4;22c");
    assert_eq!(caps.graphics.name(), "sixel");
    // `caps_from` has no window and the answers tell no cell size.
    assert!(!caps.draws_images());
    assert_eq!(caps.playback(), Playback::None);
    assert!(!caps.truecolor);
}

#[test]
fn the_protocols_found_are_listed_from_the_answers() {
    use crate::term::testing::{KITTY_BEHIND_TMUX, TMUX_3_8, kitty_0_47_1};
    let found = |answers: Option<String>| {
        Detection {
            caps: Capabilities::plain(80),
            window: None,
            answers: answers.map(String::into_bytes),
            background: false,
            tmux: None,
        }
        .protocols()
    };
    assert_eq!(found(Some(kitty_0_47_1(false))), Some(vec!["kitty"]));
    let behind = format!("{TMUX_3_8}{KITTY_BEHIND_TMUX}");
    assert_eq!(found(Some(behind)), Some(vec!["kitty"]));
    let sixel = format!("{TMUX_3_8}\x1b[?65;4;22c");
    assert_eq!(found(Some(sixel)), Some(vec!["sixel"]));
    assert_eq!(found(None), None);
}

/// A window of xterm's, 100 × 30 cells of 12 × 23 pixels and its border.
const XTERM_WINDOW: WindowSize = WindowSize {
    columns: 100,
    rows: 30,
    width: 1204,
    height: 694,
};

/// Detection in `mode` of a terminal in a window of `window`, which
/// gave `answers`, outside tmux.
fn detected(mode: Mode, answers: &str, window: WindowSize) -> Capabilities {
    let ask = |_, _| Some(answers.as_bytes().to_vec());
    let env = Environment::default();
    let (passthrough, window) = (Passthrough::Detect, Some(window));
    detect(mode, None, &env, passthrough, window, true, ask, NO_TMUX).caps
}

#[test]
fn ghostty_foot_and_xterm_scroll_the_screen_and_other_terminals_do_not() {
    let ghostty = crate::term::testing::ghostty_1_1_3(true);
    let foot = "\x1b[1;1R\x1b[1;1R\x1b[1;1R\x1bP>|foot(1.16.2)\x1b\\\x1b[?62;4c";
    let foot_text = foot.replace("?62;4c", "?62;c");
    let wezterm = "\x1b[1;1R\x1b[1;1R\x1b[1;1R\x1bP>|WezTerm 20240203\x1b\\\x1b[?65;c";
    let cases = [
        // kitty can show the next row of a page's picture in the status
        // row after a scroll, which no test of kiri's output catches.
        ("kitty, until its ghost is understood", KITTY, false),
        ("ghostty", ghostty.as_str(), true),
        ("foot as text", foot_text.as_str(), true),
        // foot keeps every sixel it is sent in its grid, with no count
        // limit, so xterm's 16 graphics do not apply.
        ("foot drawing sixel", foot, true),
        ("wezterm", wezterm, false),
        (
            "a terminal that does not tell its name",
            "\x1b[?62;c",
            false,
        ),
    ];
    for (name, answers, scrolls) in cases {
        let caps = detected(Mode::Auto, answers, XTERM_WINDOW);
        assert_eq!(caps.scroll, scrolls, "{name}");
    }
}

#[test]
fn nothing_scrolls_where_the_terminal_was_not_asked() {
    let ask = |_, _| None;
    let env = Environment::default();
    let (passthrough, window) = (Passthrough::Detect, Some(XTERM_WINDOW));
    for mode in [Mode::Plain, Mode::Ansi, Mode::Kitty, Mode::Auto] {
        let caps = detect(mode, None, &env, passthrough, window, true, ask, NO_TMUX).caps;
        assert!(!caps.scroll, "{mode:?}");
    }
}

#[test]
fn inside_tmux_the_terminal_outside_decides_whether_the_screen_scrolls() {
    for (client, scrolls) in [
        ("ghostty 1.1.3|on", true),
        ("kitty(0.47.1)|on", false),
        ("foot(1.16.2)|on", true),
        ("WezTerm 20240203|on", false),
        ("|on", false),
    ] {
        let answers = (BEHIND_TMUX, client);
        let (_, _, found) =
            through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
        assert_eq!(found.caps.scroll, scrolls, "{client}");
    }
}

#[test]
fn xterm_scrolls_as_text_but_not_while_it_draws_sixel() {
    // xterm keeps only 16 sixel graphics when it draws its window
    // again, which kiri is not told of: rows a scroll kept would stay
    // blank. Without sixel the screen is text and scrolls.
    let sixel = detected(Mode::Auto, crate::term::testing::XTERM_407, XTERM_WINDOW);
    assert_eq!(sixel.graphics.name(), "sixel");
    assert!(!sixel.scroll);
    let forced = detected(Mode::Sixel, crate::term::testing::XTERM_407, XTERM_WINDOW);
    assert!(!forced.scroll);
    // foot keeps its sixel, so forcing the mode it has anyway costs no
    // scroll.
    let foot = "\x1b[1;1R\x1b[1;1R\x1b[1;1R\x1bP>|foot(1.16.2)\x1b\\\x1b[?62;4c";
    for mode in [Mode::Auto, Mode::Sixel] {
        let caps = detected(mode, foot, XTERM_WINDOW);
        assert_eq!(caps.graphics.name(), "sixel");
        assert!(caps.scroll, "foot, {mode:?}");
    }
    let text = crate::term::testing::XTERM_407.replace("?63;4;22c", "?63;22c");
    let caps = detected(Mode::Auto, &text, XTERM_WINDOW);
    assert_eq!(caps.graphics.name(), "none");
    assert!(caps.scroll);
}

#[test]
fn xterm_has_24_bit_colour_though_its_environment_says_nothing() {
    let caps = detected(Mode::Auto, crate::term::testing::XTERM_407, XTERM_WINDOW);
    assert!(caps.truecolor);
    // The answer is what tells, not a variable: without it, as in a
    // terminal that rounds colours, there is no 24-bit colour.
    let rounded = crate::term::testing::XTERM_407.replace("38:2::1:2:3", "38:5:16");
    assert!(!detected(Mode::Auto, &rounded, XTERM_WINDOW).truecolor);
}

#[test]
fn detection_reads_the_limits_of_sixel_and_draws_it() {
    let caps = detected(Mode::Auto, crate::term::testing::XTERM_407, XTERM_WINDOW);
    let Graphics::Sixel(sixel) = &caps.graphics else {
        panic!("{:?}", caps.graphics);
    };
    assert_eq!(sixel.terminal.as_deref(), Some("XTerm(407)"));
    assert_eq!((sixel.max_width, sixel.registers), (1000, 256));
    assert!(caps.draws_images());
    assert!(caps.is_sixel());
    assert_eq!(caps.max_image_cells(), u16::MAX);
    assert_eq!(caps.cell_pixels, Some((12, 23)));
}

#[test]
fn mode_sixel_draws_sixel_whether_or_not_the_terminal_lists_it() {
    let answers = "\x1b[1;1R\x1b[1;1R\x1b[1;1R\x1b[?62;c";
    let caps = detected(Mode::Sixel, answers, XTERM_WINDOW);
    assert_eq!(caps.graphics.name(), "sixel");
    assert!(caps.draws_images());
    assert_eq!(caps.max_image_cells(), u16::MAX);
    // The limits are the defaults for an unanswered question.
    let Graphics::Sixel(sixel) = &caps.graphics else {
        unreachable!()
    };
    assert_eq!((sixel.max_width, sixel.registers), (1000, 256));
}

#[test]
fn mode_sixel_takes_the_limits_a_terminal_tells_that_does_not_list_sixel() {
    use crate::term::testing::with_unlisted_sixel_terminal;
    let limits = XTERM_407_LIMITS.replace("1000;1000", "640;480");
    let (seen, found) = with_unlisted_sixel_terminal(&limits, |tty| {
        let env = Environment::default();
        examine_on(tty, Mode::Sixel, None, &env, Passthrough::Detect, NO_TMUX)
    });
    assert!(seen.contains(probe::SIXEL_GEOMETRY), "{seen:?}");
    assert!(seen.contains(probe::SIXEL_REGISTERS), "{seen:?}");
    let Graphics::Sixel(sixel) = &found.caps.graphics else {
        panic!("{:?}", found.caps.graphics);
    };
    assert_eq!((sixel.max_width, sixel.registers), (640, 256));
}

#[test]
fn mode_sixel_on_a_terminal_without_sixel_asks_and_takes_the_defaults() {
    use crate::term::testing::with_kitty_and_a_second_round;
    let (seen, found) = with_kitty_and_a_second_round(|tty| {
        let env = Environment::default();
        examine_on(tty, Mode::Sixel, None, &env, Passthrough::Detect, NO_TMUX)
    });
    assert!(seen.contains(probe::SIXEL_GEOMETRY), "{seen:?}");
    assert!(seen.contains(probe::SIXEL_REGISTERS), "{seen:?}");
    let Graphics::Sixel(sixel) = &found.caps.graphics else {
        panic!("{:?}", found.caps.graphics);
    };
    assert_eq!((sixel.max_width, sixel.registers), (1000, 256));
}

#[test]
fn mode_auto_on_a_terminal_without_sixel_does_not_ask_for_the_limits() {
    use crate::term::testing::with_kitty_and_a_second_round;
    let (seen, _) = with_kitty_and_a_second_round(|tty| {
        let env = Environment::default();
        examine_on(tty, Mode::Auto, None, &env, Passthrough::Detect, NO_TMUX)
    });
    assert!(!seen.contains(probe::SIXEL_GEOMETRY), "{seen:?}");
    assert!(!seen.contains(probe::SIXEL_REGISTERS), "{seen:?}");
}

#[test]
fn mode_sixel_needs_the_cell_size_to_draw() {
    // No window, and the terminal does not tell its cells.
    let ask = |_, _| Some(b"\x1b[?62;4c".to_vec());
    let env = Environment::default();
    let caps = detect(
        Mode::Sixel,
        None,
        &env,
        Passthrough::Detect,
        None,
        true,
        ask,
        NO_TMUX,
    )
    .caps;
    assert_eq!(caps.graphics.name(), "sixel");
    assert!(!caps.draws_images());
}

#[test]
fn mode_sixel_in_a_pipe_is_plain_text() {
    let ask = |_, _| panic!("a pipe is not asked");
    let env = Environment::default();
    let caps = detect(
        Mode::Sixel,
        None,
        &env,
        Passthrough::Detect,
        None,
        false,
        ask,
        NO_TMUX,
    )
    .caps;
    assert_eq!(caps, Capabilities::plain(80));
}
