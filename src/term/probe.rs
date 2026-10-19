//! Escape-sequence queries and their answers.

use std::fs::File;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::process::{Pid, getpgrp};
use rustix::termios::tcgetpgrp;

use super::answers::{PALETTE_ANSWER, has_device_attributes};
use super::screen::RawMode;
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
    fn asks_for_the_cell_size_only_when_told() {
        let without = queries(false);
        let end = without.len() - DEVICE_ATTRIBUTES.len();
        assert_eq!(
            queries(true),
            [&without[..end], "\x1b[16t", DEVICE_ATTRIBUTES].concat()
        );
    }

    /// Runs the probe on a pseudo-terminal whose other side answers
    /// like a kitty with cells of 9 × 19 pixels. Returns the queries the
    /// terminal saw and what the probe read.
    fn ask_kitty(ask_cell_size: bool) -> (String, Option<Vec<u8>>) {
        use crate::term::testing::{size, with_kitty};
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
}
