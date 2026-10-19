//! What the output terminal can do, and asking it.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Stdout, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use signal_hook::SigId;
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};

use rustix::termios::{OptionalActions, Termios, tcgetattr, tcsetattr};

use crate::color::Color;
use crate::config::Environment;
use crate::kitty::graphics;

mod answers;
mod probe;
#[cfg(test)]
mod testing;
mod window;

pub use window::{WindowSize, window_size};

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
    /// The foreground and background colours and the 16 palette colours
    /// as RGB, when the terminal tells them, for colours mixed from them.
    pub foreground: Option<Color>,
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
            foreground: None,
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
                Some(answers) => with_probe(caps, &answers::interpret(answers)),
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

/// `caps` with the height and the cell size of `window`.
fn with_window(mut caps: Capabilities, window: Option<WindowSize>) -> Capabilities {
    caps.height = window.map(|w| w.rows).filter(|&rows| rows > 0);
    caps.cell_pixels = window.and_then(|w| w.cell_pixels());
    caps
}

/// Raw mode on a terminal until dropped, also when a panic unwinds: keys
/// arrive one by one without echo, and Ctrl+C is a key rather than a
/// signal.
pub(crate) struct RawMode {
    tty: File,
    original: Termios,
}

impl RawMode {
    pub(crate) fn enter(tty: &File) -> Option<Self> {
        let original = tcgetattr(tty).ok()?;
        let mut raw = original.clone();
        raw.make_raw();
        tcsetattr(tty, OptionalActions::Now, &raw).ok()?;
        let tty = tty.try_clone().ok()?;
        Some(Self { tty, original })
    }
}

impl RawMode {
    /// Ends raw mode.
    fn restore(&self) {
        // Flush also discards input that arrived but was not read, such as
        // probe answers after the timeout, so they do not show up at the
        // shell prompt. Keys typed meanwhile go with them. Answers that
        // arrive later still can show up; nothing here can wait for a
        // terminal that may never answer.
        let _ = tcsetattr(&self.tty, OptionalActions::Flush, &self.original);
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Leaving the pager's screen: the alternate screen and raw mode end once,
/// when the screen is dropped or, before that, when a panic hook runs.
struct Leave {
    raw: RawMode,
    done: AtomicBool,
}

impl Leave {
    /// Leaves the alternate screen on `out` and ends raw mode, unless that
    /// has been done.
    fn run(&self, out: &mut impl Write) {
        if self.done.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = out.write_all(LEAVE_SCREEN);
        let _ = out.flush();
        self.raw.restore();
    }

    fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }
}

/// The terminal while the pager shows a document: the alternate screen on
/// the output, and the controlling terminal in raw mode for keys. Both end
/// when it is dropped, also when a panic unwinds.
pub struct Screen<W: Write> {
    tty: File,
    out: W,
    /// What has been written and not yet flushed to `out`; dropped when a
    /// panic has left the screen, so no part of a frame lands on the main
    /// screen.
    buffer: Vec<u8>,
    leave: Arc<Leave>,
    /// Whether a panic hook of the screen's own is set.
    hooked: bool,
    /// The signal that asked kiri to end, 0 before one came.
    signal: Arc<AtomicUsize>,
    /// The handlers that set it.
    handlers: Vec<SigId>,
}

impl Screen<Stdout> {
    /// The pager's screen on standard output, with keys from the
    /// controlling terminal, so a piped document can be paged too. `None`
    /// without a controlling terminal or in the background, where raw mode
    /// would stop kiri until it is brought to the foreground.
    pub fn open() -> Option<Self> {
        let tty = probe::controlling_terminal()?;
        if probe::runs_in_background(&tty) {
            return None;
        }
        let mut screen = Self::on(tty, std::io::stdout())?;
        screen.catch_signals();
        screen.catch_panics();
        Some(screen)
    }
}

impl<W: Write> Screen<W> {
    /// The pager's screen on `out`, with keys from `tty`.
    fn on(tty: File, mut out: W) -> Option<Self> {
        let raw = RawMode::enter(&tty)?;
        out.write_all(ENTER_SCREEN).ok()?;
        out.flush().ok()?;
        Some(Self {
            tty,
            out,
            buffer: Vec::new(),
            leave: Arc::new(Leave {
                raw,
                done: AtomicBool::new(false),
            }),
            hooked: false,
            signal: Arc::new(AtomicUsize::new(0)),
            handlers: Vec::new(),
        })
    }

    /// Makes the signals that would end kiri end the screen instead:
    /// [`read`](Self::read) fails once one comes, and [`close`](Self::close)
    /// then ends kiri with it, after the terminal is restored. Ctrl+C sends
    /// no signal in raw mode; these come from elsewhere, such as `kill` or
    /// a terminal that closes.
    fn catch_signals(&mut self) {
        for signal in [SIGTERM, SIGHUP, SIGINT, SIGQUIT] {
            let flag = Arc::clone(&self.signal);
            if let Ok(id) = signal_hook::flag::register_usize(signal, flag, signal as usize) {
                self.handlers.push(id);
            }
        }
    }

    /// Makes a panic leave the screen before its message is written, so the
    /// message stays on the main screen.
    fn catch_panics(&mut self) {
        let leave = Arc::clone(&self.leave);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Standard output is the screen's output: see `open`.
            leave.run(&mut std::io::stdout());
            previous(info);
        }));
        self.hooked = true;
    }

    /// The signal that asked kiri to end, if one came.
    fn signal(&self) -> Option<i32> {
        match self.signal.load(Ordering::SeqCst) {
            0 => None,
            signal => Some(signal as i32),
        }
    }

    /// Restores the terminal, and then ends kiri with the signal that
    /// asked it to end, if one came.
    pub fn close(self) {
        // Read after the drop, so a signal that comes while the terminal
        // is restored still counts.
        let flag = Arc::clone(&self.signal);
        drop(self);
        match flag.load(Ordering::SeqCst) {
            0 => {}
            signal => {
                let _ = signal_hook::low_level::emulate_default_handler(signal as i32);
            }
        }
    }

    /// The size of the window now.
    pub fn size(&self) -> Option<WindowSize> {
        window_size(&self.tty)
    }

    /// The bytes of the keys pressed, waiting at most `timeout` for the
    /// first; empty when none came, and an error when the terminal has
    /// gone away or a signal asked kiri to end.
    pub fn read(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};

        let timeout = Timespec {
            tv_sec: timeout.as_secs() as i64,
            tv_nsec: i64::from(timeout.subsec_nanos()),
        };
        let mut fds = [PollFd::new(&self.tty, PollFlags::IN)];
        let ready = poll(&mut fds, Some(&timeout));
        if self.signal().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "ended by a signal",
            ));
        }
        match ready {
            Ok(0) | Err(rustix::io::Errno::INTR) => return Ok(Vec::new()),
            Ok(_) => {}
            Err(error) => return Err(error.into()),
        }
        let mut buffer = [0; 1024];
        match self.tty.read(&mut buffer)? {
            // Ready but empty: the terminal has gone away.
            0 => Err(io::ErrorKind::UnexpectedEof.into()),
            read => Ok(buffer[..read].to_vec()),
        }
    }
}

impl<W: Write> Write for Screen<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.write_all(&self.buffer)?;
        self.buffer.clear();
        self.out.flush()
    }
}

impl<W: Write> Drop for Screen<W> {
    fn drop(&mut self) {
        if !self.leave.is_done() {
            let _ = self.flush();
        }
        self.leave.run(&mut self.out);
        // Last, so a signal that comes until the terminal is restored is
        // caught rather than lost.
        for &id in &self.handlers {
            signal_hook::low_level::unregister(id);
        }
        // Back to the default hook, as kiri sets no other. The hook cannot
        // be taken back while a panic runs it.
        if self.hooked && !std::thread::panicking() {
            let _ = std::panic::take_hook();
        }
    }
}

/// The alternate screen, with the cursor hidden.
const ENTER_SCREEN: &[u8] = b"\x1b[?1049h\x1b[?25l";
/// Back to the main screen as it was, with the cursor shown.
const LEAVE_SCREEN: &[u8] = b"\x1b[?25h\x1b[?1049l";

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
    use super::testing::{pty, size, with_kitty};
    use super::*;

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
    fn draws_images_only_when_an_image_may_cover_a_cell() {
        // Images would be loaded and then not drawn.
        let caps = Capabilities {
            max_image_cells: 0,
            ..Capabilities::kitty(80)
        };
        assert!(!caps.draws_images());
        assert!(Capabilities::kitty(80).draws_images());
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
    fn raw_mode_ends_when_a_panic_unwinds() {
        use rustix::termios::LocalModes;

        let (_terminal, tty) = pty();
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
        use std::io::Write;

        use rustix::event::{PollFd, PollFlags, Timespec, poll};

        let (mut terminal, tty) = pty();
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

    /// A screen on a pseudo-terminal, writing to memory, and the
    /// terminal's side of the pseudo-terminal.
    fn screen() -> (Screen<Vec<u8>>, File, File) {
        let (terminal, tty) = pty();
        let probe = tty.try_clone().unwrap();
        let screen = Screen::on(tty, Vec::new()).expect("the screen opens");
        (screen, terminal, probe)
    }

    #[test]
    fn the_screen_is_the_alternate_screen_in_raw_mode() {
        use rustix::termios::LocalModes;

        let (screen, _terminal, tty) = screen();
        assert_eq!(screen.out, b"\x1b[?1049h\x1b[?25l");
        let modes = tcgetattr(&tty).unwrap().local_modes;
        assert!(!modes.contains(LocalModes::ICANON));
    }

    #[test]
    fn the_screen_ends_when_a_panic_unwinds() {
        use std::sync::{Arc, Mutex};

        /// Output that outlives the screen.
        #[derive(Clone)]
        struct Shared(Arc<Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let (_terminal, tty) = pty();
        let original = tcgetattr(&tty).unwrap();
        let out = Shared(Arc::new(Mutex::new(Vec::new())));
        let result = std::panic::catch_unwind({
            let tty = tty.try_clone().unwrap();
            let out = out.clone();
            move || {
                let _screen = Screen::on(tty, out).expect("the screen opens");
                panic!("the pager fails");
            }
        });
        assert!(result.is_err());
        let written = out.0.lock().unwrap().clone();
        assert!(written.ends_with(b"\x1b[?25h\x1b[?1049l"), "{written:?}");
        let restored = tcgetattr(&tty).unwrap();
        assert_eq!(restored.local_modes, original.local_modes);
    }

    #[test]
    fn the_screen_reads_the_keys_pressed() {
        use std::io::Write;

        let (mut screen, mut terminal, _tty) = screen();
        terminal.write_all(b"j\x1b[B").unwrap();
        let keys = screen.read(Duration::from_secs(5)).unwrap();
        assert_eq!(keys, b"j\x1b[B");
    }

    #[test]
    fn the_screen_reads_nothing_when_no_key_comes() {
        let (mut screen, _terminal, _tty) = screen();
        let keys = screen.read(Duration::from_millis(10)).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn the_screen_tells_the_window_size() {
        let (screen, _terminal, tty) = screen();
        rustix::termios::tcsetwinsize(&tty, size(100, 30, 1000, 600)).unwrap();
        let window = screen.size().expect("a pseudo-terminal has a size");
        assert_eq!((window.columns, window.rows), (100, 30));
    }

    #[test]
    fn the_screen_fails_when_the_terminal_goes_away() {
        let (mut screen, terminal, _tty) = screen();
        drop(terminal);
        assert!(screen.read(Duration::from_secs(5)).is_err());
    }

    /// Run on its own by [`a_panic_message_comes_after_the_screen`]: opens
    /// a screen on standard output, leaves part of a frame unwritten and
    /// panics.
    #[test]
    #[ignore = "run by a_panic_message_comes_after_the_screen"]
    fn panic_on_a_screen() {
        use std::io::Write;

        let (_terminal, tty) = pty();
        let mut screen = Screen::on(tty, std::io::stdout()).expect("the screen opens");
        screen.catch_panics();
        screen.write_all(b"[part of a frame]").unwrap();
        panic!("[the pager fails]");
    }

    #[test]
    fn a_panic_message_comes_after_the_screen() {
        use std::process::{Command, Stdio};

        let (mut reader, writer) = std::io::pipe().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "term::tests::panic_on_a_screen"])
            .args(["--ignored", "--nocapture", "--test-threads", "1"])
            .stdin(Stdio::null())
            .stdout(writer.try_clone().unwrap())
            .stderr(writer)
            .spawn()
            .unwrap();
        let mut output = Vec::new();
        let mut buffer = [0; 4096];
        // The child gets 10 s at most, so a hanging child fails the test
        // instead of hanging it.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            use rustix::event::{PollFd, PollFlags, Timespec, poll};

            let left = deadline.checked_duration_since(std::time::Instant::now());
            let timeout = left.map(|left| Timespec::try_from(left).unwrap());
            let mut fds = [PollFd::new(&reader, PollFlags::IN)];
            if left.is_none() || poll(&mut fds, timeout.as_ref()).unwrap() == 0 {
                child.kill().unwrap();
                panic!("the child did not finish: {output:?}");
            }
            // Reading ends once the child, which holds the writing end,
            // exits.
            match reader.read(&mut buffer).unwrap() {
                0 => break,
                read => output.extend_from_slice(&buffer[..read]),
            }
        }
        let _ = child.wait();
        let output = String::from_utf8_lossy(&output);
        let leave = output.find("\x1b[?1049l").expect("the screen is left");
        let message = output
            .find("[the pager fails]")
            .expect("the message is written");
        assert!(leave < message, "{output:?}");
        assert_eq!(output.matches("\x1b[?1049l").count(), 1, "{output:?}");
        assert!(!output.contains("[part of a frame]"), "{output:?}");
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
