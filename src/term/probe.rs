//! Escape-sequence queries and their answers.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use rustix::process::{Pid, getpgrp};
use rustix::termios::tcgetpgrp;

use super::screen::RawMode;
use super::tmux;
use crate::present::answers::{PALETTE_ANSWER, device_attributes, sixel};
use crate::present::kitty::query;

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

/// The widest and highest sixel the terminal takes, XTSMGRAPHICS "read
/// maximum" of item 2. "Read" would follow the window's size.
pub(crate) const SIXEL_GEOMETRY: &str = "\x1b[?2;4;0S";
/// The number of sixel colour registers, XTSMGRAPHICS "read" of item 1.
pub(crate) const SIXEL_REGISTERS: &str = "\x1b[?1;1;0S";
/// Sets the foreground to an RGB colour a 256-colour terminal would not
/// keep.
pub(crate) const TRUECOLOR_SET: &str = "\x1b[38;2;1;2;3m";
/// Asks for the graphic rendition, DECRQSS: a terminal with 24-bit colour
/// answers with the colour it was set.
pub(crate) const TRUECOLOR_ASK: &str = "\x1bP$qm\x1b\\";
/// Back to the default rendition after [`TRUECOLOR_SET`].
const TRUECOLOR_RESET: &str = "\x1b[m";

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
/// colours, the terminal's version, whether a
/// colour set in 24 bits is kept, the cell size if `ask_cell_size`,
/// and primary device attributes. Every terminal answers the last one,
/// so its answer marks the end, also when a terminal ignores the
/// others. Behind tmux, tmux answers it at once, before kitty's answer to
/// the wrapped graphics query comes; the device attributes are then also
/// asked wrapped, from kitty, whose answer comes after its graphics
/// answer and marks the end. The cell size is asked wrapped there too:
/// tmux answers it from a pixel size it may only guess. The limits of
/// sixel are not asked here but in [`sixel_limits`], when the answer to
/// the device attributes lists sixel or sixel is forced: kitty prints a
/// parse error for each of them.
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
        TRUECOLOR_SET,
        TRUECOLOR_ASK,
        TRUECOLOR_RESET,
        cell_size,
        DEVICE_ATTRIBUTES,
        &end,
    ]
    .concat()
}

/// The second round: the limits of sixel, and primary device attributes
/// again as the end, which every terminal answers, also when it ignores
/// the limits. Behind tmux this one is not wrapped: tmux answers it.
fn sixel_limits() -> String {
    [SIXEL_GEOMETRY, SIXEL_REGISTERS, DEVICE_ATTRIBUTES].concat()
}

/// Sends the queries on the controlling terminal and collects the
/// answers, asking for the cell size if `ask_cell_size`, and for the
/// limits of sixel whatever the device attributes say if
/// `always_ask_limits`, as with `--mode sixel`. `None` if the
/// terminal cannot be used or does not answer.
pub fn run(
    ask_cell_size: bool,
    graphics: GraphicsQuery,
    always_ask_limits: bool,
) -> Option<Vec<u8>> {
    ask(
        &controlling_terminal()?,
        ask_cell_size,
        graphics,
        always_ask_limits,
    )
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
pub fn ask(
    tty: &File,
    ask_cell_size: bool,
    graphics: GraphicsQuery,
    always_ask_limits: bool,
) -> Option<Vec<u8>> {
    if runs_in_background(tty) {
        return None;
    }
    let raw = RawMode::enter(tty)?;
    let answers = exchange(&mut &*tty, ask_cell_size, graphics, always_ask_limits);
    // Erase whatever the OSC 66 tests drew before restoring the mode.
    let _ = (&*tty).write_all(query::erase_text_sizing_tests().as_bytes());
    drop(raw);
    answers
}

/// Reads into `answers` until it holds `wanted` device attributes
/// answers, `timeout` has passed or the terminal hangs up. `None` if
/// the terminal cannot be read.
fn read_until(
    tty: &mut (impl Read + AsFd),
    answers: &mut Vec<u8>,
    wanted: usize,
    timeout: Duration,
) -> Option<()> {
    let deadline = Instant::now() + timeout;
    let mut buffer = [0; 1024];
    while device_attributes(answers) < wanted {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return Some(());
        };
        if !super::readable(&*tty, left).ok()? {
            return Some(());
        }
        let read = tty.read(&mut buffer).ok()?;
        // No bytes from a readable terminal: it hung up, and poll would
        // report it readable again until the deadline. The answers heard
        // stay, as after a timeout; whether they are enough is for the
        // caller, who gives up without the first round's device
        // attributes.
        if read == 0 {
            return Some(());
        }
        answers.extend_from_slice(&buffer[..read]);
    }
    Some(())
}

/// Sends the second round and reads until `wanted` device attributes
/// are in `answers`.
fn ask_limits(
    tty: &mut (impl Read + Write + AsFd),
    answers: &mut Vec<u8>,
    wanted: usize,
) -> Option<()> {
    tty.write_all(sixel_limits().as_bytes()).ok()?;
    tty.flush().ok()?;
    read_until(tty, answers, wanted, TIMEOUT)
}

/// Two rounds of queries. The first is [`queries`]. If its device
/// attributes list sixel, or `always_ask_limits` says that sixel is forced
/// and the terminal may not list it, a second asks for the limits of sixel
/// ([`sixel_limits`]), which a terminal without sixel, kitty among them,
/// would only log as errors. A terminal that answers no device attributes
/// at all gets no second round, forced or not. Behind tmux the second round
/// is not wrapped, so tmux answers it, limits included, whether or not the
/// terminal outside has sixel; its device attributes make the one more that
/// is counted. Both rounds wait for [`TIMEOUT`] at most, the second from
/// its start: a terminal that answered the first is there, and answers the
/// device attributes that end the second, so the wait is long only for one
/// that lists sixel and then goes silent.
fn exchange(
    tty: &mut (impl Read + Write + AsFd),
    ask_cell_size: bool,
    graphics: GraphicsQuery,
    always_ask_limits: bool,
) -> Option<Vec<u8>> {
    tty.write_all(queries(ask_cell_size, graphics).as_bytes())
        .ok()?;
    tty.flush().ok()?;
    let mut answers = Vec::new();
    // Behind tmux, kitty's answers count only when they come, as
    // passthrough may be off; tmux's own answers are there anyway.
    let (wanted, enough) = match graphics {
        GraphicsQuery::Wrapped => (2, 1),
        GraphicsQuery::Plain | GraphicsQuery::Skip => (1, 1),
    };
    read_until(tty, &mut answers, wanted, TIMEOUT)?;
    let heard = device_attributes(&answers);
    if heard < enough {
        return None;
    }
    // `sixel` reads only the first answers, so the second round's
    // device attributes change nothing there.
    if always_ask_limits || sixel(&String::from_utf8_lossy(&answers)) {
        // An error here leaves the first round's answers as they are,
        // as a timeout does.
        let _ = ask_limits(tty, &mut answers, heard + 1);
    }
    Some(answers)
}

#[cfg(test)]
mod tests;
