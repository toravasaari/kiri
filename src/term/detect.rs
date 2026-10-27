//! What the output terminal can do: from the mode, the environment, the
//! window and the probe.

use std::ffi::OsStr;
use std::fs::File;

use super::probe::GraphicsQuery;
use super::tmux::{self, Pane, Run};
use super::window::{WindowSize, window_size};
use super::{answers, probe};
use crate::caps::Capabilities;
use crate::config::{Environment, Passthrough};
use crate::kitty::{graphics, query};

/// Which terminal features to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Ask the terminal; plain text in a pipe.
    Auto,
    /// Text only, no escape sequences.
    Plain,
    /// Colours and attributes, no kitty extensions.
    Ansi,
    /// All kitty features, without asking the terminal.
    Kitty,
}

/// What kiri learnt about a terminal and how, for `--doctor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    /// The capabilities kiri renders with.
    pub caps: Capabilities,
    /// The window size the kernel reports.
    pub window: Option<WindowSize>,
    /// The terminal's answers to the probe; `None` when it was not asked or
    /// did not answer.
    pub answers: Option<Vec<u8>>,
    /// Whether kiri runs in the background of its terminal, where the probe
    /// does not ask it.
    pub background: bool,
    /// What kiri learnt from tmux, when it wraps graphics for tmux.
    pub tmux: Option<Pane>,
}

/// How kiri learnt the size of a cell in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellSize {
    /// From the window size the kernel reports.
    Window(u16, u16),
    /// From the terminal's answer to the probe.
    Terminal(u16, u16),
    /// Not at all.
    Unknown,
}

impl Detection {
    /// The terminal's name and version as it answered them, such as
    /// `kitty(0.47.1)`.
    pub fn terminal(&self) -> Option<String> {
        let answers = String::from_utf8_lossy(self.answers.as_deref()?);
        answers::version(&answers).map(str::to_owned)
    }

    /// Where the cell size in `caps` comes from.
    pub fn cell_size(&self) -> CellSize {
        let window = self.window.and_then(|window| window.cell_pixels());
        match (self.caps.cell_pixels, window) {
            (Some((width, height)), Some(_)) => CellSize::Window(width, height),
            (Some((width, height)), None) => CellSize::Terminal(width, height),
            (None, _) => CellSize::Unknown,
        }
    }
}

/// The capabilities to render with in `mode`. `width` overrides the
/// terminal's own width. `NO_COLOR` turns colours off in every mode,
/// including the forced ones. `passthrough` says whether kitty's graphics
/// go through tmux.
pub fn capabilities(
    mode: Mode,
    width: Option<u16>,
    env: &Environment,
    passthrough: Passthrough,
) -> Capabilities {
    let (window, terminal) = stdout_window();
    let ask = probe::run;
    detect(
        mode,
        width,
        env,
        passthrough,
        window,
        terminal,
        ask,
        &tmux::run,
    )
    .caps
}

/// What kiri finds out in `mode` about the controlling terminal, whatever
/// stdout is, so a report written to a file still tells of the terminal.
/// Without a controlling terminal, it tells what [`capabilities`] sees.
pub fn examine(
    mode: Mode,
    width: Option<u16>,
    env: &Environment,
    passthrough: Passthrough,
) -> Detection {
    match probe::controlling_terminal() {
        Some(tty) => examine_on(&tty, mode, width, env, passthrough, &tmux::run),
        None => {
            let (window, terminal) = stdout_window();
            let ask = |_, _| None;
            detect(
                mode,
                width,
                env,
                passthrough,
                window,
                terminal,
                ask,
                &tmux::run,
            )
        }
    }
}

/// The window of stdout, and whether stdout is a terminal.
fn stdout_window() -> (Option<WindowSize>, bool) {
    let stdout = std::io::stdout();
    (window_size(&stdout), rustix::termios::isatty(&stdout))
}

/// [`examine`] on the terminal `tty`.
fn examine_on(
    tty: &File,
    mode: Mode,
    width: Option<u16>,
    env: &Environment,
    passthrough: Passthrough,
    tmux: Run,
) -> Detection {
    let ask = |ask_cell_size, graphics| probe::ask(tty, ask_cell_size, graphics);
    let window = window_size(tty);
    let detection = detect(mode, width, env, passthrough, window, true, ask, tmux);
    Detection {
        background: probe::runs_in_background(tty),
        ..detection
    }
}

/// The capabilities in `mode` for a window of `window`, which is a
/// terminal if `terminal`. In auto mode, `ask` sends the probe, with the
/// cell size question when its first argument is true and the graphics
/// query as its second says, and returns the answers. `tmux` runs tmux
/// commands. Falls back to plain text when the output is not a terminal,
/// and to ANSI styling when the terminal does not answer.
#[allow(clippy::too_many_arguments)]
fn detect(
    mode: Mode,
    width: Option<u16>,
    env: &Environment,
    passthrough: Passthrough,
    window: Option<WindowSize>,
    terminal: bool,
    ask: impl FnOnce(bool, GraphicsQuery) -> Option<Vec<u8>>,
    tmux: Run,
) -> Detection {
    let width = width.or(window.map(|w| w.columns)).unwrap_or(80);
    let in_tmux = tmux::in_tmux(env);
    let kitty_features = terminal && matches!(mode, Mode::Auto | Mode::Kitty);
    // tmux is asked before the probe, which needs passthrough on.
    let pane = match passthrough {
        Passthrough::Detect if kitty_features && in_tmux => Some(tmux::prepare(env, tmux)),
        Passthrough::Tmux if kitty_features => Some(Pane {
            client: None,
            passthrough: None,
            turned_on: false,
            focus_events: None,
        }),
        _ => None,
    };
    let graphics = match &pane {
        Some(_) => GraphicsQuery::Wrapped,
        None if in_tmux => GraphicsQuery::Skip,
        None => GraphicsQuery::Plain,
    };
    let mut answers = None;
    let mut caps = match mode {
        Mode::Auto if !terminal => Capabilities::plain(width),
        Mode::Auto => {
            let caps = with_environment(Capabilities::ansi(width), env);
            let caps = with_window(caps, window);
            answers = ask(caps.cell_pixels.is_none(), graphics);
            match (&answers, &pane) {
                (Some(answers), None) => with_probe(caps, &answers::interpret(answers)),
                (Some(answers), Some(pane)) => {
                    through_tmux(caps, &answers::interpret(answers), pane)
                }
                (None, _) => caps,
            }
        }
        Mode::Plain => Capabilities::plain(width),
        Mode::Ansi => Capabilities::ansi(width),
        // The window size is not a probe: it does not ask the terminal.
        Mode::Kitty => {
            let caps = with_window(Capabilities::kitty(width), window);
            // tmux needs features of its own for links and underline
            // styles.
            match pane {
                Some(_) => Capabilities {
                    hyperlinks: false,
                    styled_underlines: false,
                    ..caps
                },
                None => caps,
            }
        }
    };
    if pane.is_some() {
        // tmux cannot pass text sizing.
        caps.text_sizing = false;
        caps.tmux_passthrough = true;
        caps.image_run = Some(tmux::run_id());
    }
    if no_color(env) {
        caps.color = false;
    }
    Detection {
        caps,
        window,
        answers,
        background: false,
        tmux: pane,
    }
}

/// `caps` with what the environment tells without asking the terminal:
/// 24-bit colour and hyperlinks.
fn with_environment(mut caps: Capabilities, env: &Environment) -> Capabilities {
    let colorterm = env.colorterm.as_deref();
    caps.truecolor = colorterm.is_some_and(|v| v == "truecolor" || v == "24bit");
    caps.hyperlinks = hyperlinks_by_environment(env);
    caps
}

/// `caps` with what the terminal's answers to the probe tell.
fn with_probe(mut caps: Capabilities, probe: &answers::Probe) -> Capabilities {
    caps.text_sizing = probe.text_sizing;
    caps.graphics = probe.graphics;
    caps.max_image_cells = if probe.image_placeholders {
        graphics::MAX_CELLS
    } else {
        0
    };
    caps.dark_background = probe.dark_background;
    caps.foreground = probe.foreground;
    caps.background = probe.background;
    caps.palette = probe.palette;
    // The window size in pixels, when the kernel has it, comes first.
    caps.cell_pixels = caps.cell_pixels.or(probe.cell_pixels);
    // Only kitty-compatible terminals implement these protocols, and all
    // of them also do 24-bit colour, OSC 8 and underline styles.
    if probe.text_sizing || probe.graphics {
        caps.truecolor = true;
        caps.hyperlinks = true;
        caps.styled_underlines = true;
    }
    caps
}

/// `caps` with what the answers to a probe sent through tmux tell: the
/// graphics answer comes from the terminal outside tmux, the rest from
/// tmux itself, which `pane` tells about.
fn through_tmux(caps: Capabilities, probe: &answers::Probe, pane: &Pane) -> Capabilities {
    let hyperlinks = caps.hyperlinks;
    // Only tmux's name says which terminal is outside; without it, the
    // terminal's answer to the graphics query decides.
    let client = pane.client.as_deref();
    let placeholders = probe.graphics && client.is_none_or(query::placeholders_by_version);
    let probe = answers::Probe {
        text_sizing: false,
        image_placeholders: placeholders,
        ..probe.clone()
    };
    Capabilities {
        hyperlinks,
        styled_underlines: false,
        ..with_probe(caps, &probe)
    }
}

/// Whether the user asked for output without colour: `NO_COLOR` set and not
/// empty, as <https://no-color.org> defines it.
pub fn no_color(env: &Environment) -> bool {
    env.no_color
        .as_deref()
        .is_some_and(|value| !value.is_empty())
}

/// `caps` with the height and the cell size of `window`.
fn with_window(mut caps: Capabilities, window: Option<WindowSize>) -> Capabilities {
    caps.height = window.map(|w| w.rows).filter(|&rows| rows > 0);
    caps.cell_pixels = window.and_then(|w| w.cell_pixels());
    caps
}

/// Terminals known to support OSC 8 that cannot be recognised by a probe.
fn hyperlinks_by_environment(env: &Environment) -> bool {
    let program = env.term_program.as_deref().and_then(OsStr::to_str);
    matches!(
        program,
        Some("WezTerm" | "iTerm.app" | "vscode" | "ghostty" | "Hyper")
    ) || (env.vte_version.as_deref())
        .and_then(OsStr::to_str)
        .and_then(|v| v.parse::<u32>().ok())
        .is_some_and(|v| v >= 5000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::term::testing::{size, with_kitty};

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
    fn inside_tmux_the_graphics_query_is_wrapped() {
        let answers = (BEHIND_TMUX, "kitty(0.47.1)|on");
        let (calls, sent, found) =
            through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
        assert_eq!(sent, Some(GraphicsQuery::Wrapped));
        assert_eq!(calls.len(), 1, "{calls:?}");
        let caps = &found.caps;
        assert!(caps.draws_images());
        assert_eq!(caps.max_image_cells, graphics::MAX_CELLS);
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
        };
        assert_eq!(found.tmux, Some(pane));
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
        let (_, _, found) =
            through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
        assert!(found.caps.graphics);
        assert_eq!(found.caps.max_image_cells, 0);
        assert!(!found.caps.draws_images());
    }

    #[test]
    fn behind_tmux_without_an_answer_from_kitty_no_images_are_drawn() {
        // Passthrough stayed off: kitty never saw the graphics query.
        let answers = (
            "\x1b[1;1R\x1bP>|tmux 3.8\x1b\\\x1b[?1;2;4c",
            "kitty(0.47.1)|off",
        );
        let (_, _, found) =
            through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
        assert!(!found.caps.graphics);
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
        let (calls, sent, found) =
            through_tmux(Mode::Auto, &env, Passthrough::Detect, true, answers);
        assert_eq!(sent, Some(GraphicsQuery::Plain));
        assert_eq!(calls, Vec::<String>::new());
        assert_eq!(found.caps, Capabilities::kitty(80));
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
        let (calls, _, found) =
            through_tmux(Mode::Kitty, &in_tmux(), Passthrough::None, true, answers);
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
        assert_eq!(caps.max_image_cells, graphics::MAX_CELLS);
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
        assert_eq!(caps.max_image_cells, graphics::MAX_CELLS);
    }

    #[test]
    fn a_terminal_without_placeholders_draws_no_images() {
        let answers = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b_Gi=31;OK\x1b\\\x1b[?62;c";
        let caps = with_probe(
            Capabilities::ansi(80),
            &answers::interpret(answers.as_bytes()),
        );
        assert!(caps.graphics);
        assert!(!caps.draws_images());
        assert_eq!(caps.max_image_cells, 0);
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
}
