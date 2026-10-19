//! Pseudo-terminals for the tests.

use std::fs::File;

/// A pseudo-terminal: the terminal side, and kiri's side.
pub(crate) fn pty() -> (std::fs::File, std::fs::File) {
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
             \x1b]10;rgb:dddd/dddd/dddd\x1b\\\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\\
             \x1b]4;4;rgb:0000/0000/eeee\x1b\\\
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

mod tests {
    use super::*;

    #[test]
    fn the_fake_kitty_gives_up_when_it_is_not_asked() {
        let (seen, ()) = with_kitty(size(80, 24, 0, 0), |_| ());
        assert_eq!(seen, "");
    }
}
