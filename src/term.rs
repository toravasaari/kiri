//! What the output terminal can do, and asking it.

use std::ffi::OsStr;
use std::fs::File;
use std::os::fd::AsFd;

use crate::color::Color;
use crate::config::Environment;
use crate::kitty::graphics;

/// Terminal features the renderer may use. Built once at start-up and passed
/// to the renderer, so rendering itself never talks to the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// Width in cells.
    pub width: u16,
    /// Height of the terminal window in cells; `None` when it is not known
    /// or the terminal reports no rows.
    pub height: Option<u16>,
    /// Colours and text attributes through escape sequences.
    pub styling: bool,
    /// Foreground and background colours, with `styling`. Off when the user
    /// sets `NO_COLOR`; text attributes, links and sizes stay.
    pub color: bool,
    /// 24-bit colour; without it, RGB colours are approximated.
    pub truecolor: bool,
    /// OSC 8 hyperlinks.
    pub hyperlinks: bool,
    /// kitty's text sizing protocol (OSC 66).
    pub text_sizing: bool,
    /// kitty's graphics protocol: the terminal answered its query.
    pub graphics: bool,
    /// The most rows and the most columns one image may cover, placed with
    /// the graphics protocol's Unicode placeholders, which is how kiri shows
    /// every image; for kitty, the number of placeholder diacritics. 0 when
    /// the terminal has no placeholders: not all that answer the graphics
    /// query do.
    pub max_image_cells: u16,
    /// Underline styles other than single (SGR `4:x`), which kitty
    /// introduced; without them every underline is single. Only the probe
    /// sets it, apart from `--mode kitty`.
    pub styled_underlines: bool,
    /// Size of one cell in pixels, when the terminal reports it: in the
    /// window size, or else in an answer to the probe.
    pub cell_pixels: Option<(u16, u16)>,
    /// Whether the background is dark; `None` when unknown.
    pub dark_background: Option<bool>,
    /// The background colour and the 16 palette colours as RGB, when the
    /// terminal tells them, for colours mixed from them.
    pub background: Option<Color>,
    pub palette: [Option<Color>; 16],
}

impl Capabilities {
    /// No escape sequences at all, for pipes and files.
    pub fn plain(width: u16) -> Self {
        Self {
            width,
            height: None,
            styling: false,
            color: false,
            truecolor: false,
            hyperlinks: false,
            text_sizing: false,
            graphics: false,
            max_image_cells: 0,
            styled_underlines: false,
            cell_pixels: None,
            dark_background: None,
            background: None,
            palette: [None; 16],
        }
    }

    /// Whether images are drawn rather than shown as alt text.
    pub fn draws_images(&self) -> bool {
        self.styling && self.graphics && self.max_image_cells > 0
    }

    /// Colours and attributes that any modern terminal understands.
    pub fn ansi(width: u16) -> Self {
        Self {
            styling: true,
            color: true,
            ..Self::plain(width)
        }
    }

    /// Everything kitty supports.
    pub fn kitty(width: u16) -> Self {
        Self {
            styling: true,
            color: true,
            truecolor: true,
            hyperlinks: true,
            text_sizing: true,
            graphics: true,
            max_image_cells: graphics::MAX_CELLS,
            styled_underlines: true,
            ..Self::plain(width)
        }
    }
}

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
        probe::version(&answers).map(str::to_owned)
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
/// including the forced ones.
pub fn capabilities(mode: Mode, width: Option<u16>, env: &Environment) -> Capabilities {
    let stdout = std::io::stdout();
    let window = window_size(&stdout);
    let terminal = rustix::termios::isatty(&stdout);
    detect(mode, width, env, window, terminal, probe::run).caps
}

/// What kiri finds out in `mode` about the controlling terminal, whatever
/// stdout is, so a report written to a file still tells of the terminal.
/// Without a controlling terminal, it tells what [`capabilities`] sees.
pub fn examine(mode: Mode, width: Option<u16>, env: &Environment) -> Detection {
    match probe::controlling_terminal() {
        Some(tty) => examine_on(&tty, mode, width, env),
        None => {
            let stdout = std::io::stdout();
            let window = window_size(&stdout);
            let terminal = rustix::termios::isatty(&stdout);
            detect(mode, width, env, window, terminal, |_| None)
        }
    }
}

/// [`examine`] on the terminal `tty`.
fn examine_on(tty: &File, mode: Mode, width: Option<u16>, env: &Environment) -> Detection {
    let detection = detect(mode, width, env, window_size(tty), true, |ask_cell_size| {
        probe::ask(tty, ask_cell_size)
    });
    Detection {
        background: probe::runs_in_background(tty),
        ..detection
    }
}

/// The capabilities in `mode` for a window of `window`, which is a
/// terminal if `terminal`. In auto mode, `ask` sends the probe, with the
/// cell size question when its argument is true, and returns the answers.
/// Falls back to plain text when the output is not a terminal, and to ANSI
/// styling when the terminal does not answer.
fn detect(
    mode: Mode,
    width: Option<u16>,
    env: &Environment,
    window: Option<WindowSize>,
    terminal: bool,
    ask: impl FnOnce(bool) -> Option<Vec<u8>>,
) -> Detection {
    let width = width.or(window.map(|w| w.columns)).unwrap_or(80);
    let mut answers = None;
    let mut caps = match mode {
        Mode::Auto if !terminal => Capabilities::plain(width),
        Mode::Auto => {
            let caps = with_environment(Capabilities::ansi(width), env);
            let caps = with_window(caps, window);
            answers = ask(caps.cell_pixels.is_none());
            match &answers {
                Some(answers) => with_probe(caps, &probe::interpret(answers)),
                None => caps,
            }
        }
        Mode::Plain => Capabilities::plain(width),
        Mode::Ansi => Capabilities::ansi(width),
        // The window size is not a probe: it does not ask the terminal.
        Mode::Kitty => with_window(Capabilities::kitty(width), window),
    };
    if no_color(env) {
        caps.color = false;
    }
    Detection {
        caps,
        window,
        answers,
        background: false,
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
fn with_probe(mut caps: Capabilities, probe: &probe::Probe) -> Capabilities {
    caps.text_sizing = probe.text_sizing;
    caps.graphics = probe.graphics;
    caps.max_image_cells = if probe.image_placeholders {
        graphics::MAX_CELLS
    } else {
        0
    };
    caps.dark_background = probe.dark_background;
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

/// Whether stdout is a terminal.
pub fn stdout_is_terminal() -> bool {
    rustix::termios::isatty(std::io::stdout())
}

/// Whether stdin is a terminal.
pub fn stdin_is_terminal() -> bool {
    rustix::termios::isatty(std::io::stdin())
}

/// Whether the user asked for output without colour: `NO_COLOR` set and not
/// empty, as <https://no-color.org> defines it.
pub fn no_color(env: &Environment) -> bool {
    env.no_color
        .as_deref()
        .is_some_and(|value| !value.is_empty())
}

/// The size of a terminal window, from one query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    /// Columns and rows of cells.
    pub columns: u16,
    pub rows: u16,
    /// Width and height in pixels; 0 when the terminal does not tell.
    pub width: u16,
    pub height: u16,
}

impl WindowSize {
    /// Size of one cell in pixels, when the terminal reports its size in
    /// pixels.
    pub fn cell_pixels(&self) -> Option<(u16, u16)> {
        let cell_width = self.width / self.columns.max(1);
        let cell_height = self.height / self.rows.max(1);
        (cell_width > 0 && cell_height > 0).then_some((cell_width, cell_height))
    }
}

/// `caps` with the height and the cell size of `window`.
fn with_window(mut caps: Capabilities, window: Option<WindowSize>) -> Capabilities {
    caps.height = window.map(|w| w.rows).filter(|&rows| rows > 0);
    caps.cell_pixels = window.and_then(|w| w.cell_pixels());
    caps
}

/// The size of the terminal window `fd` refers to, if it is a terminal
/// that reports one.
pub fn window_size(fd: impl AsFd) -> Option<WindowSize> {
    let size = rustix::termios::tcgetwinsize(fd).ok()?;
    (size.ws_col > 0).then_some(WindowSize {
        columns: size.ws_col,
        rows: size.ws_row,
        width: size.ws_xpixel,
        height: size.ws_ypixel,
    })
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

    #[test]
    fn a_probed_kitty_draws_images_up_to_the_placeholder_limit() {
        let answers = "\x1b[5;1R\x1b[5;3R\x1b[5;5R\x1b_Gi=31;OK\x1b\\\x1b[?62;c";
        let caps = with_probe(
            Capabilities::ansi(80),
            &probe::interpret(answers.as_bytes()),
        );
        assert!(caps.draws_images());
        assert_eq!(caps.max_image_cells, graphics::MAX_CELLS);
        assert_eq!(caps, Capabilities::kitty(80));
    }

    #[test]
    fn the_probe_tells_the_cell_size_only_when_the_window_does_not() {
        let probe = probe::Probe {
            cell_pixels: Some((9, 19)),
            ..probe::Probe::default()
        };
        let caps = with_probe(Capabilities::ansi(80), &probe);
        assert_eq!(caps.cell_pixels, Some((9, 19)));
        let window = Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::ansi(80)
        };
        assert_eq!(with_probe(window, &probe).cell_pixels, Some((10, 20)));
        let caps = with_probe(Capabilities::ansi(80), &probe::Probe::default());
        assert_eq!(caps.cell_pixels, None);
    }

    #[test]
    fn a_probed_kitty_0_28_draws_images_without_text_sizing() {
        let answers = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b_Gi=31;OK\x1b\\\
                       \x1bP>|kitty(0.28.0)\x1b\\\x1b[?62;c";
        let caps = with_probe(
            Capabilities::ansi(80),
            &probe::interpret(answers.as_bytes()),
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
            &probe::interpret(answers.as_bytes()),
        );
        assert!(caps.graphics);
        assert!(!caps.draws_images());
        assert_eq!(caps.max_image_cells, 0);
    }

    #[test]
    fn draws_images_only_when_an_image_may_cover_a_cell() {
        // Images would be loaded and then not drawn.
        let caps = Capabilities {
            max_image_cells: 0,
            ..Capabilities::kitty(80)
        };
        assert!(!caps.draws_images());
        assert!(Capabilities::kitty(80).draws_images());
    }

    /// A pseudo-terminal: the terminal side, and kiri's side.
    pub(super) fn pty() -> (std::fs::File, std::fs::File) {
        use std::ffi::OsStr;
        use std::fs::File;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::OpenOptionsExt;

        use rustix::fs::OFlags;
        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};

        let terminal = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
        grantpt(&terminal).unwrap();
        unlockpt(&terminal).unwrap();
        let name = ptsname(&terminal, Vec::new()).unwrap();
        let tty = File::options()
            .read(true)
            .write(true)
            .custom_flags(OFlags::NOCTTY.bits() as i32)
            .open(OsStr::from_bytes(name.as_bytes()))
            .unwrap();
        (File::from(terminal), tty)
    }

    /// A window size for [`rustix::termios::tcsetwinsize`].
    pub(super) fn size(
        columns: u16,
        rows: u16,
        width: u16,
        height: u16,
    ) -> rustix::termios::Winsize {
        rustix::termios::Winsize {
            ws_col: columns,
            ws_row: rows,
            ws_xpixel: width,
            ws_ypixel: height,
        }
    }

    /// Runs `probe` on a pseudo-terminal with a window of `window`, whose
    /// other side answers like a kitty 0.47.1 with cells of 9 × 19 pixels,
    /// and tells the cell size only when asked. Returns the queries the
    /// terminal saw and what `probe` returned. The terminal waits for the
    /// queries for 2 s at most, so a probe that sends none fails its test
    /// instead of hanging it.
    pub(super) fn with_kitty<T>(
        window: rustix::termios::Winsize,
        probe: impl FnOnce(&File) -> T,
    ) -> (String, T) {
        use std::io::{Read, Write};
        use std::time::{Duration, Instant};

        use rustix::event::{PollFd, PollFlags, Timespec, poll};

        let (mut terminal, tty) = pty();
        rustix::termios::tcsetwinsize(&tty, window).unwrap();
        let kitty = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut seen = Vec::new();
            let mut buffer = [0; 1024];
            while !seen.ends_with(b"\x1b[c") {
                let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                    return (String::from_utf8(seen).unwrap(), terminal);
                };
                let timeout = Timespec::try_from(left).unwrap();
                let mut fds = [PollFd::new(&terminal, PollFlags::IN)];
                if poll(&mut fds, Some(&timeout)).unwrap() == 0 {
                    continue;
                }
                let read = terminal.read(&mut buffer).unwrap();
                seen.extend_from_slice(&buffer[..read]);
            }
            let seen = String::from_utf8(seen).unwrap();
            let cell_size = if seen.contains("\x1b[16t") {
                "\x1b[6;19;9t"
            } else {
                ""
            };
            let answers = format!(
                "\x1b[5;1R\x1b[5;3R\x1b[5;5R\x1b_Gi=31;OK\x1b\\\
                 \x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\\x1b]4;4;rgb:0000/0000/eeee\x1b\\\
                 \x1bP>|kitty(0.47.1)\x1b\\\
                 {cell_size}\x1b[?62;c"
            );
            terminal.write_all(answers.as_bytes()).unwrap();
            (seen, terminal)
        });
        let result = probe(&tty);
        let (seen, _terminal) = kitty.join().unwrap();
        (seen, result)
    }

    #[test]
    fn the_fake_kitty_gives_up_when_it_is_not_asked() {
        let (seen, ()) = with_kitty(size(80, 24, 0, 0), |_| ());
        assert_eq!(seen, "");
    }

    #[test]
    fn examining_a_window_without_pixels_asks_the_terminal_for_the_cell_size() {
        let (seen, found) = with_kitty(size(80, 24, 0, 0), |tty| {
            examine_on(tty, Mode::Auto, None, &Environment::default())
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
            examine_on(tty, Mode::Auto, Some(60), &Environment::default())
        });
        assert!(!seen.contains("\x1b[16t"), "{seen:?}");
        assert_eq!(found.cell_size(), CellSize::Window(10, 20));
        assert_eq!(found.caps.width, 60);
        // The colours the terminal tells go into the capabilities.
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
            Some(window),
            true,
            |_| None,
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
        let never = |_| -> Option<Vec<u8>> { panic!("the terminal was asked") };
        let found = detect(Mode::Kitty, None, &env, None, true, never);
        assert_eq!(found.caps, Capabilities::kitty(80));
        assert_eq!(found.cell_size(), CellSize::Unknown);
        let found = detect(Mode::Auto, Some(70), &env, None, false, never);
        assert_eq!(found.caps, Capabilities::plain(70));
    }

    #[test]
    fn reads_the_window_size_in_cells_and_pixels() {
        let (_terminal, tty) = pty();
        rustix::termios::tcsetwinsize(&tty, size(80, 24, 800, 480)).unwrap();
        assert_eq!(
            window_size(&tty),
            Some(WindowSize {
                columns: 80,
                rows: 24,
                width: 800,
                height: 480,
            })
        );
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
    fn a_process_group_other_than_the_foreground_one_is_in_the_background() {
        let pid = |raw| rustix::process::Pid::from_raw(raw).unwrap();
        assert!(probe::in_background(Some(pid(10)), pid(20)));
        assert!(!probe::in_background(Some(pid(20)), pid(20)));
        // A terminal that is not the controlling one has no foreground
        // process group to compare with.
        assert!(!probe::in_background(None, pid(20)));
    }

    #[test]
    fn a_pipe_has_no_window_size() {
        let (reader, _writer) = std::io::pipe().unwrap();
        assert_eq!(window_size(&reader), None);
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
                !capabilities(mode, Some(40), &without_colours).color,
                "{mode:?}"
            );
        }
        let env = Environment::default();
        assert!(capabilities(Mode::Ansi, Some(40), &env).color);
        assert!(capabilities(Mode::Kitty, Some(40), &env).color);
    }

    #[test]
    fn a_forced_mode_takes_the_width_it_is_given() {
        let env = Environment::default();
        assert_eq!(
            capabilities(Mode::Plain, Some(40), &env),
            Capabilities::plain(40)
        );
        assert_eq!(
            capabilities(Mode::Ansi, Some(40), &env),
            Capabilities::ansi(40)
        );
        assert_eq!(capabilities(Mode::Kitty, Some(40), &env).width, 40);
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

/// Escape-sequence queries and their answers.
mod probe {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::process::{Pid, getpgrp};
    use rustix::termios::{OptionalActions, Termios, tcgetattr, tcgetpgrp, tcsetattr};

    use crate::color::Color;
    use crate::kitty::query;

    /// Long enough for a terminal over SSH, short enough not to notice when
    /// a terminal ignores the queries entirely.
    const TIMEOUT: Duration = Duration::from_millis(500);

    /// Cursor position report.
    const CURSOR_POSITION: &str = "\x1b[6n";
    /// Background colour, OSC 11.
    const BACKGROUND: &str = "\x1b]11;?\x1b\\";
    /// Start of an answer to [`palette`]: `ESC ] 4 ; index ; rgb:…`.
    const PALETTE_ANSWER: &str = "\x1b]4;";
    /// The terminal's name and version, XTVERSION.
    const VERSION: &str = "\x1b[>q";
    /// Start of the answer to [`VERSION`]: `ESC P > | name(version) ST`.
    const VERSION_ANSWER: &str = "\x1bP>|";
    /// Primary device attributes.
    const DEVICE_ATTRIBUTES: &str = "\x1b[c";

    /// The 16 palette colours, OSC 4, one query each: not every terminal
    /// answers more than one colour a query.
    fn palette() -> String {
        (0..16)
            .map(|index| format!("{PALETTE_ANSWER}{index};?\x1b\\"))
            .collect()
    }

    /// Cursor position reports around kitty's two text sizing tests, then a
    /// graphics protocol query, the background and palette colours, the
    /// terminal's version, the cell size if `ask_cell_size`, and primary
    /// device attributes. Every terminal answers the last one, so its answer
    /// marks the end, also when a terminal ignores the others.
    fn queries(ask_cell_size: bool) -> String {
        let [width, scale] = query::text_sizing();
        [
            "\r",
            CURSOR_POSITION,
            &width,
            CURSOR_POSITION,
            &scale,
            CURSOR_POSITION,
            query::GRAPHICS,
            BACKGROUND,
            &palette(),
            VERSION,
            if ask_cell_size { query::CELL_SIZE } else { "" },
            DEVICE_ATTRIBUTES,
        ]
        .concat()
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    pub struct Probe {
        pub text_sizing: bool,
        pub graphics: bool,
        pub image_placeholders: bool,
        pub dark_background: Option<bool>,
        pub background: Option<Color>,
        pub palette: [Option<Color>; 16],
        /// Width and height of a cell in pixels, when asked and answered.
        pub cell_pixels: Option<(u16, u16)>,
    }

    /// Sends the queries on the controlling terminal and collects the
    /// answers, asking for the cell size if `ask_cell_size`. `None` if the
    /// terminal cannot be used or does not answer.
    pub fn run(ask_cell_size: bool) -> Option<Vec<u8>> {
        ask(&controlling_terminal()?, ask_cell_size)
    }

    /// The controlling terminal, open for reading and writing.
    pub fn controlling_terminal() -> Option<File> {
        File::options().read(true).write(true).open("/dev/tty").ok()
    }

    /// Whether a process in the process group `own` is in the background of
    /// a terminal whose foreground process group is `foreground`, if that
    /// is known.
    pub fn in_background(foreground: Option<Pid>, own: Pid) -> bool {
        foreground.is_some_and(|foreground| foreground != own)
    }

    /// Whether kiri runs in the background of `tty`, as in `kiri … &`.
    /// There raw mode would stop it with SIGTTOU until it is brought to the
    /// foreground.
    pub fn runs_in_background(tty: &File) -> bool {
        in_background(tcgetpgrp(tty).ok(), getpgrp())
    }

    /// [`run`] on `tty`.
    pub fn ask(tty: &File, ask_cell_size: bool) -> Option<Vec<u8>> {
        if runs_in_background(tty) {
            return None;
        }
        let raw = RawMode::enter(tty)?;
        let answers = exchange(tty, ask_cell_size);
        // Erase whatever the OSC 66 tests drew before restoring the mode.
        let _ = (&*tty).write_all(query::erase_text_sizing_tests().as_bytes());
        drop(raw);
        answers
    }

    /// Raw mode on a terminal until dropped, also when a panic unwinds.
    struct RawMode<'a> {
        tty: &'a File,
        original: Termios,
    }

    impl<'a> RawMode<'a> {
        fn enter(tty: &'a File) -> Option<Self> {
            let original = tcgetattr(tty).ok()?;
            let mut raw = original.clone();
            raw.make_raw();
            tcsetattr(tty, OptionalActions::Now, &raw).ok()?;
            Some(Self { tty, original })
        }
    }

    impl Drop for RawMode<'_> {
        fn drop(&mut self) {
            // Flush also discards input that arrived but was not read, such
            // as answers after the timeout, so they do not show up at the
            // shell prompt. Keys typed during the probe go with them. Answers
            // that arrive later still can show up; nothing here can wait for
            // a terminal that may never answer.
            let _ = tcsetattr(self.tty, OptionalActions::Flush, &self.original);
        }
    }

    fn exchange(mut tty: &File, ask_cell_size: bool) -> Option<Vec<u8>> {
        tty.write_all(queries(ask_cell_size).as_bytes()).ok()?;
        tty.flush().ok()?;
        let deadline = Instant::now() + TIMEOUT;
        let mut answers = Vec::new();
        let mut buffer = [0; 1024];
        while !has_device_attributes(&answers) {
            let left = deadline.checked_duration_since(Instant::now())?;
            let timeout = Timespec {
                tv_sec: left.as_secs() as i64,
                tv_nsec: i64::from(left.subsec_nanos()),
            };
            let mut fds = [PollFd::new(tty, PollFlags::IN)];
            if poll(&mut fds, Some(&timeout)).ok()? == 0 {
                return None;
            }
            let read = tty.read(&mut buffer).ok()?;
            answers.extend_from_slice(&buffer[..read]);
        }
        Some(answers)
    }

    /// The primary device attributes answer: `ESC [ ? … c`.
    fn has_device_attributes(answers: &[u8]) -> bool {
        answers
            .windows(3)
            .position(|w| w == b"\x1b[?")
            .is_some_and(|start| answers[start..].contains(&b'c'))
    }

    pub fn interpret(answers: &[u8]) -> Probe {
        let text = String::from_utf8_lossy(answers);
        // No query tells whether a terminal supports Unicode placeholders,
        // and some that answer the graphics query do not. A kitty from 0.28
        // on has them, which its version answer tells. Text sizing came to
        // kitty in 0.40, so kiri also takes a terminal with both protocols
        // to be a kitty that has placeholders. Other terminals with
        // placeholders get alt text for now.
        let text_sizing = query::text_sizing_answered(&cursor_columns(&text));
        let graphics = text.contains(query::GRAPHICS_ANSWER);
        let placeholders_by_version = version(&text).is_some_and(query::placeholders_by_version);
        Probe {
            text_sizing,
            graphics,
            image_placeholders: graphics && (text_sizing || placeholders_by_version),
            dark_background: background_is_dark(&text),
            background: background(&text),
            palette: palette_colors(&text),
            cell_pixels: query::cell_size(&text),
        }
    }

    /// The terminal's name and version from the answer to [`VERSION`],
    /// ended by ST or BEL.
    pub fn version(text: &str) -> Option<&str> {
        let start = text.find(VERSION_ANSWER)? + VERSION_ANSWER.len();
        let rest = &text[start..];
        let end = rest.find(['\x1b', '\x07'])?;
        Some(&rest[..end])
    }

    /// Columns from cursor position reports: `ESC [ row ; column R`.
    fn cursor_columns(text: &str) -> Vec<u32> {
        text.split("\x1b[")
            .filter_map(|part| {
                let report = &part[..part.find('R')?];
                let (row, column) = report.split_once(';')?;
                row.parse::<u32>().ok()?;
                column.parse().ok()
            })
            .collect()
    }

    /// The channels of `rgb:RRRR/GGGG/BBBB` at the start of `text`, with
    /// one to four hex digits each, as fractions of their maximum.
    fn channels(text: &str) -> Option<[f64; 3]> {
        let rest = text.strip_prefix("rgb:")?;
        let end = rest.find(|c: char| !(c.is_ascii_hexdigit() || c == '/'))?;
        let channels: Vec<f64> = rest[..end]
            .split('/')
            .map(|hex| {
                let max = 16f64.powi(hex.len() as i32) - 1.0;
                u32::from_str_radix(hex, 16)
                    .ok()
                    .map(|v| f64::from(v) / max)
            })
            .collect::<Option<_>>()?;
        channels.try_into().ok()
    }

    fn rgb([r, g, b]: [f64; 3]) -> Color {
        let byte = |channel: f64| (channel * 255.0).round() as u8;
        Color::Rgb(byte(r), byte(g), byte(b))
    }

    /// The channels of the answer to [`BACKGROUND`].
    fn background_channels(text: &str) -> Option<[f64; 3]> {
        let start = text.find("\x1b]11;")? + "\x1b]11;".len();
        channels(&text[start..])
    }

    fn background_is_dark(text: &str) -> Option<bool> {
        let [r, g, b] = background_channels(text)?;
        Some(0.2126 * r + 0.7152 * g + 0.0722 * b < 0.5)
    }

    fn background(text: &str) -> Option<Color> {
        background_channels(text).map(rgb)
    }

    /// The colours among the answers to [`palette`].
    fn palette_colors(text: &str) -> [Option<Color>; 16] {
        let mut palette = [None; 16];
        for (start, _) in text.match_indices(PALETTE_ANSWER) {
            let rest = &text[start + PALETTE_ANSWER.len()..];
            let Some((index, color)) = rest.split_once(';') else {
                continue;
            };
            if let Ok(index) = index.parse::<usize>()
                && let Some(entry) = palette.get_mut(index)
            {
                *entry = channels(color).map(rgb);
            }
        }
        palette
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const KITTY: &str = concat!(
            "\x1b[5;1R\x1b[5;3R\x1b[5;5R",
            "\x1b_Gi=31;OK\x1b\\",
            "\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\",
            "\x1b[?62;c",
        );

        #[test]
        fn sends_the_queries_in_order() {
            assert_eq!(
                queries(false),
                [
                    "\r\x1b[6n\x1b]66;w=2; \x1b\\\x1b[6n\x1b]66;s=2; \x1b\\\x1b[6n\
                     \x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b]11;?\x1b\\",
                    &(0..16)
                        .map(|index| format!("\x1b]4;{index};?\x1b\\"))
                        .collect::<String>(),
                    "\x1b[>q\x1b[c",
                ]
                .concat()
            );
        }

        #[test]
        fn reads_the_background_and_palette_colours() {
            // Four hex digits a channel ended by ST, two ended by BEL.
            let answers = concat!(
                "\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\",
                "\x1b]4;1;rgb:cccc/0000/0000\x1b\\",
                "\x1b]4;12;rgb:44/93/f8\x07",
                "\x1b]4;16;rgb:ff/ff/ff\x07",
                "\x1b[?62;c",
            );
            let probe = interpret(answers.as_bytes());
            assert_eq!(probe.background, Some(Color::Rgb(0x1e, 0x1e, 0x2e)));
            let mut palette = [None; 16];
            palette[1] = Some(Color::Rgb(0xcc, 0, 0));
            palette[12] = Some(Color::Rgb(0x44, 0x93, 0xf8));
            // Entries beyond the 16 kiri asks for are left out.
            assert_eq!(probe.palette, palette);
        }

        #[test]
        fn malformed_colour_answers_are_left_out() {
            let answers = concat!(
                "\x1b]4;99;rgb:ff/ff/ff\x1b\\",
                "\x1b]4;x;rgb:ff/ff/ff\x1b\\",
                "\x1b]4;2;#ffffff\x1b\\",
                "\x1b]4;3;rgb:ff/ff\x1b\\",
                "\x1b]4;5;rgb:zz/00/00\x1b\\",
                "\x1b]4;6;rgb:12/34",
            );
            let probe = interpret(answers.as_bytes());
            assert_eq!(probe.palette, [None; 16]);
            // An answer cut short has no colour.
            let cut = "\x1b]11;rgb:1e1e/1e";
            assert_eq!(interpret(cut.as_bytes()).background, None);
        }

        #[test]
        fn asks_for_the_cell_size_only_when_told() {
            let without = queries(false);
            let end = without.len() - DEVICE_ATTRIBUTES.len();
            assert_eq!(
                queries(true),
                [&without[..end], "\x1b[16t", DEVICE_ATTRIBUTES].concat()
            );
        }

        #[test]
        fn reads_the_cell_size_among_the_answers() {
            let answers = KITTY.replace("\x1b[?62;c", "\x1b[6;19;9t\x1b[?62;c");
            assert_eq!(interpret(answers.as_bytes()).cell_pixels, Some((9, 19)));
            assert_eq!(interpret(KITTY.as_bytes()).cell_pixels, None);
        }

        /// Runs the probe on a pseudo-terminal whose other side answers
        /// like a kitty with cells of 9 × 19 pixels. Returns the queries the
        /// terminal saw and what the probe read.
        fn ask_kitty(ask_cell_size: bool) -> (String, Option<Vec<u8>>) {
            use crate::term::tests::{size, with_kitty};
            with_kitty(size(80, 24, 0, 0), |tty| ask(tty, ask_cell_size))
        }

        #[test]
        fn a_terminal_tells_its_cell_size_when_asked() {
            let (seen, answers) = ask_kitty(true);
            assert!(seen.contains("\x1b[16t"), "{seen:?}");
            let probe = interpret(&answers.unwrap());
            assert_eq!(probe.cell_pixels, Some((9, 19)));
            assert!(probe.graphics);
        }

        #[test]
        fn the_cell_size_is_not_asked_unless_told() {
            let (seen, answers) = ask_kitty(false);
            assert!(!seen.contains("\x1b[16t"), "{seen:?}");
            assert_eq!(interpret(&answers.unwrap()).cell_pixels, None);
        }

        #[test]
        fn recognises_kitty() {
            let probe = interpret(KITTY.as_bytes());
            assert_eq!(
                probe,
                Probe {
                    text_sizing: true,
                    graphics: true,
                    image_placeholders: true,
                    dark_background: Some(true),
                    background: Some(Color::Rgb(0x1e, 0x1e, 0x2e)),
                    palette: [None; 16],
                    cell_pixels: None,
                }
            );
            assert!(has_device_attributes(KITTY.as_bytes()));
        }

        #[test]
        fn recognises_a_terminal_without_kitty_protocols() {
            let answers = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b]11;rgb:ffff/ffff/ffff\x07\x1b[?1;2c";
            assert_eq!(
                interpret(answers.as_bytes()),
                Probe {
                    text_sizing: false,
                    graphics: false,
                    image_placeholders: false,
                    dark_background: Some(false),
                    background: Some(Color::Rgb(0xff, 0xff, 0xff)),
                    palette: [None; 16],
                    cell_pixels: None,
                }
            );
        }

        #[test]
        fn graphics_alone_does_not_promise_placeholders() {
            // A terminal that answers the graphics query but has no text
            // sizing, as WezTerm and Konsole do: they show images but not
            // through Unicode placeholders.
            let answers = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b_Gi=31;OK\x1b\\\x1b[?62;c";
            let probe = interpret(answers.as_bytes());
            assert!(probe.graphics);
            assert!(!probe.image_placeholders);
        }

        /// Answers from a kitty that names itself `version` in its
        /// XTVERSION answer, with or without text sizing.
        fn kitty_answers(version: &str, text_sizing: bool) -> String {
            let columns = if text_sizing {
                "\x1b[5;1R\x1b[5;3R\x1b[5;5R"
            } else {
                "\x1b[5;1R\x1b[5;1R\x1b[5;1R"
            };
            format!(
                "{columns}\x1b_Gi=31;OK\x1b\\\x1b]11;rgb:0000/0000/0000\x1b\\\
                 \x1bP>|kitty({version})\x1b\\\x1b[?62;c"
            )
        }

        #[test]
        fn kitty_0_28_has_placeholders_without_text_sizing() {
            let probe = interpret(kitty_answers("0.28.0", false).as_bytes());
            assert!(!probe.text_sizing);
            assert!(probe.graphics);
            assert!(probe.image_placeholders);
        }

        #[test]
        fn kitty_0_27_has_no_placeholders() {
            let probe = interpret(kitty_answers("0.27.1", false).as_bytes());
            assert!(probe.graphics);
            assert!(!probe.image_placeholders);
        }

        #[test]
        fn kitty_0_47_1_has_placeholders_and_text_sizing() {
            let answers = kitty_answers("0.47.1", true);
            let probe = interpret(answers.as_bytes());
            assert!(probe.text_sizing);
            assert!(probe.image_placeholders);
            assert!(has_device_attributes(answers.as_bytes()));
        }

        #[test]
        fn a_kitty_version_without_graphics_has_no_placeholders() {
            let answers = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1bP>|kitty(0.30.0)\x1b\\\x1b[?62;c";
            assert!(!interpret(answers.as_bytes()).image_placeholders);
        }

        #[test]
        fn an_unknown_terminal_version_promises_no_placeholders() {
            let answers = "\x1b[5;1R\x1b[5;1R\x1b[5;1R\x1b_Gi=31;OK\x1b\\\
                           \x1bP>|WezTerm 20240203-110809-5046fc22\x1b\\\x1b[?62;c";
            let probe = interpret(answers.as_bytes());
            assert!(probe.graphics);
            assert!(!probe.image_placeholders);
        }

        #[test]
        fn reads_a_version_answer_ended_by_bel() {
            let answers = "\x1b_Gi=31;OK\x1b\\\x1bP>|kitty(0.28.0)\x07\x1b[?62;c";
            assert!(interpret(answers.as_bytes()).image_placeholders);
        }

        #[test]
        fn needs_both_width_and_scale_for_text_sizing() {
            let answers = "\x1b[5;1R\x1b[5;3R\x1b[5;3R\x1b[?62;c";
            assert!(!interpret(answers.as_bytes()).text_sizing);
        }

        #[test]
        fn raw_mode_ends_when_a_panic_unwinds() {
            use rustix::termios::LocalModes;

            let (_terminal, tty) = crate::term::tests::pty();
            let original = tcgetattr(&tty).unwrap();
            let result = std::panic::catch_unwind(|| {
                let _raw = RawMode::enter(&tty).unwrap();
                let modes = tcgetattr(&tty).unwrap().local_modes;
                assert!(!modes.contains(LocalModes::ICANON));
                panic!("the probe fails while the terminal is raw");
            });
            assert!(result.is_err());
            let restored = tcgetattr(&tty).unwrap();
            assert_eq!(restored.local_modes, original.local_modes);
            assert_eq!(restored.input_modes, original.input_modes);
        }

        #[test]
        fn ending_raw_mode_discards_unread_answers() {
            let (mut terminal, tty) = crate::term::tests::pty();
            {
                let _raw = RawMode::enter(&tty).unwrap();
                // A late answer that nothing reads.
                terminal.write_all(b"\x1b[?62;c\n").unwrap();
                let mut fds = [PollFd::new(&tty, PollFlags::IN)];
                let timeout = Timespec {
                    tv_sec: 1,
                    tv_nsec: 0,
                };
                assert_eq!(poll(&mut fds, Some(&timeout)).unwrap(), 1);
            }
            assert_eq!(rustix::io::ioctl_fionread(&tty).unwrap(), 0);
        }

        #[test]
        fn waits_for_device_attributes() {
            assert!(!has_device_attributes(b"\x1b[5;1R\x1b[5;3R"));
            assert!(!has_device_attributes(b"\x1b[?62;"));
        }
    }
}
