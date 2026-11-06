//! What the output terminal can do: from the mode, the environment, the
//! window and the probe.

use std::ffi::OsStr;
use std::fs::File;

use super::probe;
use super::probe::GraphicsQuery;
use super::tmux::{self, Pane, Run};
use super::window::{WindowSize, window_size};
use crate::config::{Environment, Passthrough};
use crate::present::answers;
use crate::present::kitty::{graphics, query};
use crate::typeset::caps::{Capabilities, Graphics, KittyGraphics, Playback, SixelGraphics};

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

    /// The image protocols the terminal answered to, by name, in the order
    /// "kitty", "sixel"; `None` when it was not asked or did not answer.
    pub fn protocols(&self) -> Option<Vec<&'static str>> {
        let probe = answers::interpret(self.answers.as_deref()?);
        let mut found = Vec::new();
        if probe.kitty_graphics {
            found.push("kitty");
        }
        if probe.sixel {
            found.push("sixel");
        }
        Some(found)
    }

    /// Where the cell size in `caps` comes from.
    pub fn cell_size(&self) -> CellSize {
        let window = self.window.and_then(|window| window.cell_pixels());
        match (self.caps.cell_pixels, window) {
            (Some((width, height)), Some(window)) if window == (width, height) => {
                CellSize::Window(width, height)
            }
            (Some((width, height)), _) => CellSize::Terminal(width, height),
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
            ..Pane::default()
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
            // Inside tmux the window's pixel size may be tmux's guess, so
            // the terminal outside is asked for its cells as well.
            let wrapped = graphics == GraphicsQuery::Wrapped;
            answers = ask(caps.cell_pixels.is_none() || wrapped, graphics);
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
            // styles; links come below when tmux is asked.
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
    if let Some(pane) = &pane {
        // tmux cannot pass text sizing.
        caps.text_sizing = false;
        caps.tmux_passthrough = true;
        caps.image_run = Some(tmux::run_id());
        // Once asked, tmux alone says whether links pass, whether or not
        // the terminal answered: a variable inherited from outside tmux
        // does not tell.
        if passthrough == Passthrough::Detect {
            caps.hyperlinks = passes_links(pane);
        }
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
    let kitty = |max_image_cells| {
        Graphics::Kitty(KittyGraphics {
            max_image_cells,
            playback: query::playback_by_version(probe.terminal.as_deref()),
            terminal: probe.terminal.clone(),
        })
    };
    caps.graphics = if probe.image_placeholders {
        kitty(graphics::MAX_CELLS)
    } else if probe.sixel {
        Graphics::Sixel(SixelGraphics {
            terminal: probe.terminal.clone(),
        })
    } else if probe.kitty_graphics {
        kitty(0)
    } else {
        Graphics::None
    };
    caps.dark_background = probe.dark_background;
    caps.foreground = probe.foreground;
    caps.background = probe.background;
    caps.palette = probe.palette;
    // The window size in pixels, when the kernel has it, comes first.
    caps.cell_pixels = caps.cell_pixels.or(probe.cell_pixels);
    // Only kitty-compatible terminals implement these protocols, and all
    // of them also do 24-bit colour, OSC 8 and underline styles.
    if probe.text_sizing || probe.kitty_graphics {
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
    // Links come from the environment here; `detect` puts tmux's word in
    // their place when it asked tmux.
    let hyperlinks = caps.hyperlinks;
    // Only tmux's name says which terminal is outside; without it, the
    // terminal's answer to the graphics query decides.
    let client = pane.client.as_deref();
    let placeholders = probe.kitty_graphics && client.is_none_or(query::placeholders_by_version);
    let probe = answers::Probe {
        text_sizing: false,
        image_placeholders: placeholders,
        terminal: client.map(str::to_owned),
        ..probe.clone()
    };
    // The terminal outside knows its cells. tmux gives a pane 16 x 32
    // pixels a cell when its own terminal does not tell the pixel size,
    // as in a container.
    let caps = Capabilities {
        cell_pixels: probe.cell_pixels.or(caps.cell_pixels),
        ..caps
    };
    let mut caps = Capabilities {
        hyperlinks,
        styled_underlines: false,
        ..with_probe(caps, &probe)
    };
    // Every frame through the passthrough is more for tmux to drop: the
    // first frame, as without an animation.
    if let Graphics::Kitty(kitty) = &mut caps.graphics {
        kitty.playback = Playback::None;
    }
    caps
}

/// Whether tmux said it passes OSC 8 links on to the terminal outside.
fn passes_links(pane: &Pane) -> bool {
    pane.hyperlinks == Some(true)
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
    caps.height = window.and_then(|w| w.height_in_rows());
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
    use crate::term::testing::{ghostty_1_1_3, size, with_kitty};
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
        let (_, _, found) =
            through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
        assert_eq!(found.caps.graphics.name(), "kitty");
        assert_eq!(found.caps.max_image_cells(), 0);
        assert!(!found.caps.draws_images());
    }

    #[test]
    fn ghostty_behind_tmux_draws_images() {
        // tmux names Ghostty 1.1.3 so, and Ghostty answers the wrapped
        // graphics query as kitty does.
        let answers = (BEHIND_TMUX, "ghostty 1.1.3-1.fc43|on");
        let (_, _, found) =
            through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
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
        let (_, _, found) =
            through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
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
        let (calls, sent, found) =
            through_tmux(Mode::Auto, &env, Passthrough::Detect, true, answers);
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
    fn behind_tmux_an_animation_shows_its_first_frame() {
        for client in ["kitty(0.47.1)", "ghostty 1.1.3-1.fc43"] {
            let answers = (BEHIND_TMUX, &*format!("{client}|on"));
            let (_, _, found) =
                through_tmux(Mode::Auto, &in_tmux(), Passthrough::Detect, true, answers);
            assert!(found.caps.draws_images(), "{client}");
            assert_eq!(found.caps.playback(), Playback::None, "{client}");
        }
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
                let (_, _, found) =
                    through_tmux(mode, &in_tmux(), Passthrough::Detect, true, answers);
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
        let (calls, _, found) =
            through_tmux(Mode::Auto, &in_tmux(), Passthrough::Tmux, true, answers);
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
    fn sixel_alone_is_sixel_and_draws_nothing_yet() {
        let caps = caps_from("\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b[?62;4;22c");
        assert_eq!(caps.graphics.name(), "sixel");
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
}
