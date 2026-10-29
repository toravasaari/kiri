//! Escape-sequence queries and their answers.

use std::fs::File;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use rustix::process::{Pid, getpgrp};
use rustix::termios::tcgetpgrp;

use super::answers::{PALETTE_ANSWER, device_attributes};
use super::screen::RawMode;
use super::tmux;
use crate::kitty::query;

/// Long enough for a terminal over SSH, short enough not to notice when
/// a terminal ignores the queries entirely.
pub(super) const TIMEOUT: Duration = Duration::from_millis(500);

/// Cursor position report.
const CURSOR_POSITION: &str = "\x1b[6n";
/// Foreground colour, OSC 10.
const FOREGROUND: &str = "\x1b]10;?\x1b\\";
/// Background colour, OSC 11.
const BACKGROUND: &str = "\x1b]11;?\x1b\\";
/// The terminal's name and version, XTVERSION.
const VERSION: &str = "\x1b[>q";
/// Primary device attributes.
const DEVICE_ATTRIBUTES: &str = "\x1b[c";

/// The 16 palette colours, OSC 4, one query each: not every terminal
/// answers more than one colour a query.
fn palette() -> String {
    (0..16)
        .map(|index| format!("{PALETTE_ANSWER}{index};?\x1b\\"))
        .collect()
}

/// How the probe sends the graphics protocol query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsQuery {
    /// As it is.
    Plain,
    /// Wrapped in tmux's passthrough, so that it reaches the terminal
    /// outside tmux.
    Wrapped,
    /// Not at all: tmux would take it for a pane title.
    Skip,
}

/// Cursor position reports around kitty's two text sizing tests, then a
/// graphics protocol query, the foreground, background and palette
/// colours, the terminal's version, the cell size if `ask_cell_size`,
/// and primary device attributes. Every terminal answers the last one,
/// so its answer marks the end, also when a terminal ignores the
/// others. Behind tmux, tmux answers it at once, before kitty's answer to
/// the wrapped graphics query comes; the device attributes are then also
/// asked wrapped, from kitty, whose answer comes after its graphics
/// answer and marks the end. The cell size is asked wrapped there too:
/// tmux answers it from a pixel size it may only guess.
fn queries(ask_cell_size: bool, graphics: GraphicsQuery) -> String {
    let [width, scale] = query::text_sizing();
    let cell_size = if ask_cell_size { query::CELL_SIZE } else { "" };
    let (graphics, cell_size, end) = match graphics {
        GraphicsQuery::Plain => (query::GRAPHICS.to_owned(), cell_size, String::new()),
        GraphicsQuery::Wrapped => (
            tmux::passthrough(query::GRAPHICS),
            "",
            [cell_size, DEVICE_ATTRIBUTES]
                .into_iter()
                .filter(|query| !query.is_empty())
                .map(tmux::passthrough)
                .collect(),
        ),
        GraphicsQuery::Skip => (String::new(), cell_size, String::new()),
    };
    [
        "\r",
        CURSOR_POSITION,
        &width,
        CURSOR_POSITION,
        &scale,
        CURSOR_POSITION,
        &graphics,
        FOREGROUND,
        BACKGROUND,
        &palette(),
        VERSION,
        cell_size,
        DEVICE_ATTRIBUTES,
        &end,
    ]
    .concat()
}

/// Sends the queries on the controlling terminal and collects the
/// answers, asking for the cell size if `ask_cell_size`. `None` if the
/// terminal cannot be used or does not answer.
pub fn run(ask_cell_size: bool, graphics: GraphicsQuery) -> Option<Vec<u8>> {
    ask(&controlling_terminal()?, ask_cell_size, graphics)
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
pub fn ask(tty: &File, ask_cell_size: bool, graphics: GraphicsQuery) -> Option<Vec<u8>> {
    if runs_in_background(tty) {
        return None;
    }
    let raw = RawMode::enter(tty)?;
    let answers = exchange(tty, ask_cell_size, graphics);
    // Erase whatever the OSC 66 tests drew before restoring the mode.
    let _ = (&*tty).write_all(query::erase_text_sizing_tests().as_bytes());
    drop(raw);
    answers
}

fn exchange(mut tty: &File, ask_cell_size: bool, graphics: GraphicsQuery) -> Option<Vec<u8>> {
    tty.write_all(queries(ask_cell_size, graphics).as_bytes())
        .ok()?;
    tty.flush().ok()?;
    let deadline = Instant::now() + TIMEOUT;
    let mut answers = Vec::new();
    let mut buffer = [0; 1024];
    // Behind tmux, kitty's answers count only when they come, as
    // passthrough may be off; tmux's own answers are there anyway.
    let (wanted, enough) = match graphics {
        GraphicsQuery::Wrapped => (2, 1),
        GraphicsQuery::Plain | GraphicsQuery::Skip => (1, 1),
    };
    let ended = |answers: Vec<u8>| (device_attributes(&answers) >= enough).then_some(answers);
    while device_attributes(&answers) < wanted {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return ended(answers);
        };
        if !super::readable(tty, left).ok()? {
            return ended(answers);
        }
        let read = tty.read(&mut buffer).ok()?;
        answers.extend_from_slice(&buffer[..read]);
    }
    Some(answers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::answers::interpret;

    #[test]
    fn a_process_group_other_than_the_foreground_one_is_in_the_background() {
        let pid = |raw| rustix::process::Pid::from_raw(raw).unwrap();
        assert!(in_background(Some(pid(10)), pid(20)));
        assert!(!in_background(Some(pid(20)), pid(20)));
        // A terminal that is not the controlling one has no foreground
        // process group to compare with.
        assert!(!in_background(None, pid(20)));
    }

    #[test]
    fn sends_the_queries_in_order() {
        assert_eq!(
            queries(false, GraphicsQuery::Plain),
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
    fn sends_the_graphics_query_wrapped_for_tmux() {
        let plain = queries(false, GraphicsQuery::Plain);
        let wrapped = "\x1bPtmux;\x1b\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\x1b\\\x1b\\";
        // tmux answers the device attributes itself, at once; kitty's
        // answer to the wrapped ones comes after its graphics answer.
        let expected = plain.replace(query::GRAPHICS, wrapped) + "\x1bPtmux;\x1b\x1b[c\x1b\\";
        assert_eq!(queries(false, GraphicsQuery::Wrapped), expected);
    }

    #[test]
    fn behind_tmux_the_cell_size_is_asked_from_the_terminal_outside() {
        // tmux answers it itself, from a pixel size it may not know.
        let plain = queries(false, GraphicsQuery::Plain);
        let wrapped = "\x1bPtmux;\x1b\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\x1b\\\x1b\\";
        let expected = plain.replace(query::GRAPHICS, wrapped)
            + "\x1bPtmux;\x1b\x1b[16t\x1b\\\x1bPtmux;\x1b\x1b[c\x1b\\";
        assert_eq!(queries(true, GraphicsQuery::Wrapped), expected);
    }

    #[test]
    fn skips_the_graphics_query_when_told() {
        let plain = queries(true, GraphicsQuery::Plain);
        let expected = plain.replace(query::GRAPHICS, "");
        assert_eq!(queries(true, GraphicsQuery::Skip), expected);
    }

    #[test]
    fn asks_for_the_cell_size_only_when_told() {
        let without = queries(false, GraphicsQuery::Plain);
        let end = without.len() - DEVICE_ATTRIBUTES.len();
        assert_eq!(
            queries(true, GraphicsQuery::Plain),
            [&without[..end], "\x1b[16t", DEVICE_ATTRIBUTES].concat()
        );
    }

    /// Runs the probe on a pseudo-terminal whose other side answers
    /// like a kitty with cells of 9 × 19 pixels. Returns the queries the
    /// terminal saw and what the probe read.
    fn ask_kitty(ask_cell_size: bool) -> (String, Option<Vec<u8>>) {
        use crate::term::testing::{size, with_kitty};
        with_kitty(size(80, 24, 0, 0), |tty| {
            ask(tty, ask_cell_size, GraphicsQuery::Plain)
        })
    }

    #[test]
    fn behind_tmux_the_probe_waits_for_kittys_answers() {
        use crate::term::testing::with_tmux;
        let (seen, answers) = with_tmux(true, |tty| ask(tty, false, GraphicsQuery::Wrapped));
        assert!(seen.contains("\x1bPtmux;\x1b\x1b_G"), "{seen:?}");
        let probe = interpret(&answers.unwrap());
        assert!(probe.graphics);
    }

    #[test]
    fn behind_tmux_tmuxs_own_answers_count_when_kitty_does_not_answer() {
        use crate::term::testing::with_tmux;
        let (_, answers) = with_tmux(false, |tty| ask(tty, false, GraphicsQuery::Wrapped));
        let answers = String::from_utf8(answers.unwrap()).unwrap();
        assert!(answers.contains("\x1bP>|tmux 3.8"), "{answers:?}");
        assert!(!interpret(answers.as_bytes()).graphics);
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
}
