//! Escape-sequence queries and their answers.

use std::fs::File;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::process::{Pid, getpgrp};
use rustix::termios::tcgetpgrp;

use super::RawMode;
use crate::color::Color;
use crate::kitty::query;

/// Long enough for a terminal over SSH, short enough not to notice when
/// a terminal ignores the queries entirely.
const TIMEOUT: Duration = Duration::from_millis(500);

/// Cursor position report.
const CURSOR_POSITION: &str = "\x1b[6n";
/// Foreground colour, OSC 10.
const FOREGROUND: &str = "\x1b]10;?\x1b\\";
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
/// graphics protocol query, the foreground, background and palette
/// colours, the terminal's version, the cell size if `ask_cell_size`,
/// and primary device attributes. Every terminal answers the last one,
/// so its answer marks the end, also when a terminal ignores the
/// others.
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
        FOREGROUND,
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
    pub foreground: Option<Color>,
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
        foreground: foreground(&text),
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

/// The colour in the answer to [`FOREGROUND`].
fn foreground(text: &str) -> Option<Color> {
    let start = text.find("\x1b]10;")? + "\x1b]10;".len();
    channels(&text[start..]).map(rgb)
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
                 \x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b]10;?\x1b\\\x1b]11;?\x1b\\",
                &(0..16)
                    .map(|index| format!("\x1b]4;{index};?\x1b\\"))
                    .collect::<String>(),
                "\x1b[>q\x1b[c",
            ]
            .concat()
        );
    }

    #[test]
    fn reads_the_foreground_background_and_palette_colours() {
        // Four hex digits a channel ended by ST, two ended by BEL.
        let answers = concat!(
            "\x1b]10;rgb:d4/d4/d4\x07",
            "\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\",
            "\x1b]4;1;rgb:cccc/0000/0000\x1b\\",
            "\x1b]4;12;rgb:44/93/f8\x07",
            "\x1b]4;16;rgb:ff/ff/ff\x07",
            "\x1b[?62;c",
        );
        let probe = interpret(answers.as_bytes());
        assert_eq!(probe.foreground, Some(Color::Rgb(0xd4, 0xd4, 0xd4)));
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
                foreground: None,
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
                foreground: None,
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
    fn waits_for_device_attributes() {
        assert!(!has_device_attributes(b"\x1b[5;1R\x1b[5;3R"));
        assert!(!has_device_attributes(b"\x1b[?62;"));
    }
}
