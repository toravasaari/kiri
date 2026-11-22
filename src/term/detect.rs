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
use crate::typeset::facts::{self, Fact};

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
    /// Sixel images whether or not the terminal's device attributes list
    /// them. The terminal is still asked for its cell size and its
    /// sixel limits.
    Sixel,
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
    let forced = mode == Mode::Sixel;
    let ask = |ask_cell_size, graphics| probe::run(ask_cell_size, graphics, forced);
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
    let forced = mode == Mode::Sixel;
    let ask = |ask_cell_size, graphics| probe::ask(tty, ask_cell_size, graphics, forced);
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
        Mode::Auto | Mode::Sixel if !terminal => Capabilities::plain(width),
        Mode::Auto | Mode::Sixel => {
            let caps = with_environment(Capabilities::ansi(width), env);
            let caps = with_window(caps, window);
            // Inside tmux the window's pixel size may be tmux's guess, so
            // the terminal outside is asked for its cells as well.
            let wrapped = graphics == GraphicsQuery::Wrapped;
            answers = ask(caps.cell_pixels.is_none() || wrapped, graphics);
            let caps = match (&answers, &pane) {
                (Some(answers), None) => with_probe(caps, &answers::interpret(answers)),
                (Some(answers), Some(pane)) => {
                    through_tmux(caps, &answers::interpret(answers), pane)
                }
                (None, _) => caps,
            };
            match mode {
                Mode::Sixel => {
                    let probe = answers.as_deref().map(answers::interpret);
                    forced_sixel(caps, probe.as_ref())
                }
                _ => caps,
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
    // `width` is the layout's; the window keeps its own columns, which
    // differ from it only when the command line gives another width.
    caps.window_width = window.map_or(width, |window| window.columns);
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
    caps.over_ssh = over_ssh(env);
    Detection {
        caps,
        window,
        answers,
        background: false,
        tmux: pane,
    }
}

/// `caps` with sixel as the graphics, whatever the terminal answered: the
/// limits and the name are the probe's, if it was heard.
fn forced_sixel(mut caps: Capabilities, probe: Option<&answers::Probe>) -> Capabilities {
    let mut sixel = SixelGraphics::unknown(None);
    if let Some(probe) = probe {
        sixel.terminal = probe.terminal.clone();
        sixel.max_width = probe.sixel_max_width.unwrap_or(sixel.max_width);
        sixel.registers = probe.sixel_registers.unwrap_or(sixel.registers);
    }
    caps.graphics = Graphics::Sixel(sixel);
    caps.scroll = scrolls(probe.and_then(|p| p.terminal.as_deref()), &caps.graphics);
    caps
}

/// Whether the terminal that tells its name and version as `terminal`
/// scrolls its screen right for a shifted frame, as the table of facts
/// (`typeset::facts`) says of the terminals it was measured in. Any other
/// terminal gets every row written as before. Sixel is drawn on fewer of
/// them: the table has a fact of its own for that.
fn scrolls(terminal: Option<&str>, graphics: &Graphics) -> bool {
    let fact = match graphics {
        Graphics::Sixel(_) => Fact::ScrollsRightWithSixel,
        _ => Fact::ScrollsRight,
    };
    facts::holds(fact, terminal)
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
            max_width: (probe.sixel_max_width).unwrap_or(SixelGraphics::DEFAULT_MAX_WIDTH),
            registers: (probe.sixel_registers).unwrap_or(SixelGraphics::DEFAULT_REGISTERS),
        })
    } else if probe.kitty_graphics {
        kitty(0)
    } else {
        Graphics::None
    };
    caps.scroll = scrolls(probe.terminal.as_deref(), &caps.graphics);
    caps.dark_background = probe.dark_background;
    caps.foreground = probe.foreground;
    caps.background = probe.background;
    caps.palette = probe.palette;
    // The window size in pixels, when the kernel has it, comes first.
    caps.cell_pixels = caps.cell_pixels.or(probe.cell_pixels);
    // Only kitty-compatible terminals implement these protocols, and all
    // of them also do 24-bit colour, OSC 8 and underline styles.
    // A terminal that kept a colour it was set in 24 bits has 24-bit colour,
    // whatever its environment says; xterm sets no `COLORTERM`.
    caps.truecolor |= probe.truecolor;
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
    // A terminal that plays the frames itself, kitty, gets them once and
    // plays them behind tmux. One that kiri sends each frame to would get
    // every frame through the passthrough, more for tmux to drop: it shows
    // the first frame, as without an animation.
    if let Graphics::Kitty(kitty) = &mut caps.graphics
        && kitty.playback == Playback::Redraw
    {
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

/// Whether kiri runs over ssh: `SSH_CONNECTION` or `SSH_TTY` set and not
/// empty.
pub fn over_ssh(env: &Environment) -> bool {
    [&env.ssh_connection, &env.ssh_tty]
        .into_iter()
        .any(|value| value.as_deref().is_some_and(|value| !value.is_empty()))
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
mod tests;
