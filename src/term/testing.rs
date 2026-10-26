//! Pseudo-terminals for the tests.

use std::fs::File;
use std::time::Duration;

pub(crate) use super::pty::pty;

/// A window size for [`rustix::termios::tcsetwinsize`].
pub(crate) fn size(columns: u16, rows: u16, width: u16, height: u16) -> rustix::termios::Winsize {
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
pub(crate) fn with_kitty<T>(
    window: rustix::termios::Winsize,
    probe: impl FnOnce(&File) -> T,
) -> (String, T) {
    with_terminal(window, "\x1b[c", probe, |seen| {
        let cell_size = if seen.contains("\x1b[16t") {
            "\x1b[6;19;9t"
        } else {
            ""
        };
        let answers = format!(
            "\x1b[5;1R\x1b[5;3R\x1b[5;5R\x1b_Gi=31;OK\x1b\\\
             \x1b]10;rgb:dddd/dddd/dddd\x1b\\\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\\
             \x1b]4;4;rgb:0000/0000/eeee\x1b\\\
             \x1bP>|kitty(0.47.1)\x1b\\\
             {cell_size}\x1b[?62;c"
        );
        vec![(Duration::ZERO, answers)]
    })
}

/// Runs `probe` on a pseudo-terminal like [`with_kitty`], whose other side
/// answers like tmux 3.8 in a kitty 0.47.1: tmux's own answers at once,
/// and kitty's to the queries wrapped in passthrough 50 ms later, if
/// `kitty_answers`, as they come back through tmux.
pub(crate) fn with_tmux<T>(kitty_answers: bool, probe: impl FnOnce(&File) -> T) -> (String, T) {
    let window = size(80, 24, 0, 0);
    with_terminal(window, "\x1bPtmux;\x1b\x1b[c\x1b\\", probe, move |_| {
        let tmux = "\x1b[1;1R\x1b[1;1R\x1b[1;1R\x1bP>|tmux 3.8\x1b\\\x1b[?1;2;4c";
        let mut answers = vec![(Duration::ZERO, tmux.to_owned())];
        if kitty_answers {
            let kitty = "\x1b_Gi=31;OK\x1b\\\x1b[?62;52;c";
            answers.push((Duration::from_millis(50), kitty.to_owned()));
        }
        answers
    })
}

/// Runs `probe` on a pseudo-terminal with a window of `window`, whose
/// other side reads until the queries end with `last`, then writes what
/// `answer` returns for them, each part after its wait. Returns the
/// queries the terminal saw and what `probe` returned. The terminal waits
/// for the queries for 2 s at most and then does not answer.
fn with_terminal<T>(
    window: rustix::termios::Winsize,
    last: &'static str,
    probe: impl FnOnce(&File) -> T,
    answer: impl FnOnce(&str) -> Vec<(Duration, String)> + Send + 'static,
) -> (String, T) {
    use std::io::{Read, Write};
    use std::time::Instant;

    use rustix::event::{PollFd, PollFlags, Timespec, poll};

    let (mut terminal, tty) = pty();
    rustix::termios::tcsetwinsize(&tty, window).unwrap();
    let kitty = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut seen = Vec::new();
        let mut buffer = [0; 1024];
        while !seen.ends_with(last.as_bytes()) {
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
        for (wait, part) in answer(&seen) {
            std::thread::sleep(wait);
            terminal.write_all(part.as_bytes()).unwrap();
        }
        (seen, terminal)
    });
    let result = probe(&tty);
    let (seen, _terminal) = kitty.join().unwrap();
    (seen, result)
}

mod tests {
    use super::*;

    #[test]
    fn the_fake_kitty_gives_up_when_it_is_not_asked() {
        let (seen, ()) = with_kitty(size(80, 24, 0, 0), |_| ());
        assert_eq!(seen, "");
    }
}
