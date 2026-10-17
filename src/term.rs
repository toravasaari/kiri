//! What the output terminal can do, and asking it.

/// Terminal features the renderer may use. Built once at start-up and passed
/// to the renderer, so rendering itself never talks to the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// Width in cells.
    pub width: u16,
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
    /// Images placed with the graphics protocol's Unicode placeholders,
    /// which is how kiri shows every image. Terminals that answer the
    /// graphics query do not all support them.
    pub image_placeholders: bool,
    /// Underline styles other than single (SGR `4:x`), which kitty
    /// introduced; without them every underline is single. Only the probe
    /// sets it, apart from `--mode kitty`.
    pub styled_underlines: bool,
    /// Size of one cell in pixels, when the terminal reports it.
    pub cell_pixels: Option<(u16, u16)>,
    /// Whether the background is dark; `None` when unknown.
    pub dark_background: Option<bool>,
}

impl Capabilities {
    /// No escape sequences at all, for pipes and files.
    pub fn plain(width: u16) -> Self {
        Self {
            width,
            styling: false,
            color: false,
            truecolor: false,
            hyperlinks: false,
            text_sizing: false,
            graphics: false,
            image_placeholders: false,
            styled_underlines: false,
            cell_pixels: None,
            dark_background: None,
        }
    }

    /// Whether images are drawn rather than shown as alt text.
    pub fn draws_images(&self) -> bool {
        self.styling && self.graphics && self.image_placeholders
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
            image_placeholders: true,
            styled_underlines: true,
            ..Self::plain(width)
        }
    }
}

/// Asks the terminal on stdout what it supports. Falls back to plain text
/// when stdout is not a terminal, and to ANSI styling when it does not
/// answer. `width` overrides the terminal's own width.
pub fn detect(width: Option<u16>) -> Capabilities {
    let stdout = std::io::stdout();
    let width = width.or_else(terminal_width).unwrap_or(80);
    if !rustix::termios::isatty(&stdout) {
        return Capabilities::plain(width);
    }
    let mut caps = Capabilities::ansi(width);
    caps.cell_pixels = cell_pixels();
    caps.truecolor = std::env::var("COLORTERM").is_ok_and(|v| v == "truecolor" || v == "24bit");
    caps.hyperlinks = hyperlinks_by_environment();

    if let Some(answers) = probe::run() {
        let probe = probe::interpret(&answers);
        caps.text_sizing = probe.text_sizing;
        caps.graphics = probe.graphics;
        caps.image_placeholders = probe.image_placeholders;
        caps.dark_background = probe.dark_background;
        // Only kitty-compatible terminals implement these protocols, and all
        // of them also do 24-bit colour, OSC 8 and underline styles.
        if probe.text_sizing || probe.graphics {
            caps.truecolor = true;
            caps.hyperlinks = true;
            caps.styled_underlines = true;
        }
    }
    caps
}

/// Whether the user asked for output without colour: `NO_COLOR` set and not
/// empty, as <https://no-color.org> defines it.
pub fn no_color() -> bool {
    no_color_in(std::env::var_os("NO_COLOR").as_deref())
}

fn no_color_in(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

/// Width of the terminal on stdout in cells, if stdout is a terminal.
pub fn terminal_width() -> Option<u16> {
    let size = rustix::termios::tcgetwinsize(std::io::stdout()).ok()?;
    (size.ws_col > 0).then_some(size.ws_col)
}

/// Size of one cell in pixels of the terminal on stdout, if stdout is a
/// terminal that reports its size in pixels.
pub fn cell_pixels() -> Option<(u16, u16)> {
    let size = rustix::termios::tcgetwinsize(std::io::stdout()).ok()?;
    let (w, h) = (
        size.ws_xpixel / size.ws_col.max(1),
        size.ws_ypixel / size.ws_row.max(1),
    );
    (w > 0 && h > 0).then_some((w, h))
}

/// Terminals known to support OSC 8 that cannot be recognised by a probe.
fn hyperlinks_by_environment() -> bool {
    let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    matches!(
        program.as_str(),
        "WezTerm" | "iTerm.app" | "vscode" | "ghostty" | "Hyper"
    ) || std::env::var("VTE_VERSION")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .is_some_and(|v| v >= 5000)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn no_color_needs_a_value() {
        assert!(!no_color_in(None));
        assert!(!no_color_in(Some(OsStr::new(""))));
        assert!(no_color_in(Some(OsStr::new("1"))));
        assert!(no_color_in(Some(OsStr::new("0"))));
    }
}

/// Escape-sequence queries and their answers.
mod probe {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::termios::{OptionalActions, Termios, tcgetattr, tcsetattr};

    use crate::kitty::query;

    /// Long enough for a terminal over SSH, short enough not to notice when
    /// a terminal ignores the queries entirely.
    const TIMEOUT: Duration = Duration::from_millis(500);

    /// Cursor position report.
    const CURSOR_POSITION: &str = "\x1b[6n";
    /// Background colour, OSC 11.
    const BACKGROUND: &str = "\x1b]11;?\x1b\\";
    /// Primary device attributes.
    const DEVICE_ATTRIBUTES: &str = "\x1b[c";

    /// Cursor position reports around kitty's two text sizing tests, then a
    /// graphics protocol query, the background colour, and primary device
    /// attributes. Every terminal answers the last one, so its answer marks
    /// the end.
    fn queries() -> String {
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
    }

    /// Sends the queries on the controlling terminal and collects the
    /// answers. `None` if the terminal cannot be used or does not answer.
    pub fn run() -> Option<Vec<u8>> {
        let tty = File::options()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .ok()?;
        let raw = RawMode::enter(&tty)?;
        let answers = exchange(&tty);
        // Erase whatever the OSC 66 tests drew before restoring the mode.
        let _ = (&tty).write_all(b"\r\x1b[2K");
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

    fn exchange(mut tty: &File) -> Option<Vec<u8>> {
        tty.write_all(queries().as_bytes()).ok()?;
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
        // and some that answer the graphics query do not. Text sizing came
        // to kitty in 0.40, long after placeholders in 0.28, so kiri takes a
        // terminal with both protocols to be a kitty that has placeholders.
        // Other terminals with placeholders get alt text for now.
        let text_sizing = query::text_sizing_answered(&cursor_columns(&text));
        let graphics = text.contains(query::GRAPHICS_ANSWER);
        Probe {
            text_sizing,
            graphics,
            image_placeholders: graphics && text_sizing,
            dark_background: background_is_dark(&text),
        }
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

    /// From `ESC ] 11 ; rgb:RRRR/GGGG/BBBB`, with one to four hex digits
    /// per channel.
    fn background_is_dark(text: &str) -> Option<bool> {
        let start = text.find("\x1b]11;rgb:")? + "\x1b]11;rgb:".len();
        let rest = &text[start..];
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
        let [r, g, b] = channels[..] else {
            return None;
        };
        Some(0.2126 * r + 0.7152 * g + 0.0722 * b < 0.5)
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
                queries(),
                "\r\x1b[6n\x1b]66;w=2; \x1b\\\x1b[6n\x1b]66;s=2; \x1b\\\x1b[6n\
                 \x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b]11;?\x1b\\\x1b[c"
            );
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

        #[test]
        fn needs_both_width_and_scale_for_text_sizing() {
            let answers = "\x1b[5;1R\x1b[5;3R\x1b[5;3R\x1b[?62;c";
            assert!(!interpret(answers.as_bytes()).text_sizing);
        }

        /// A pseudo-terminal: the terminal side, and kiri's side.
        fn pty() -> (File, File) {
            use std::ffi::OsStr;
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

        #[test]
        fn waits_for_device_attributes() {
            assert!(!has_device_attributes(b"\x1b[5;1R\x1b[5;3R"));
            assert!(!has_device_attributes(b"\x1b[?62;"));
        }
    }
}
