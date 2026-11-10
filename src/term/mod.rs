//! The terminal itself: finding out what it can do, its window and the
//! pager's screen. All of kiri's terminal I/O happens here.

use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::time::Duration;

use rustix::event::{
    FdSetElement, PollFd, PollFlags, Timespec, fd_set_insert, fd_set_num_elements, poll, select,
};
use rustix::termios::{OptionalActions, QueueSelector, Termios, tcflush, tcsetattr};

use crate::typeset::caps::Capabilities;

mod check;
mod detect;
mod interrupt;
mod keys;
mod printable;
mod probe;
pub mod progress;
#[cfg(test)]
mod pty;
mod screen;
#[cfg(test)]
mod testing;
pub mod tmux;
mod window;

pub use check::lost_images;
pub use detect::{CellSize, Detection, Mode, capabilities, examine, no_color};
pub use interrupt::{Interrupt, end_by};
pub use keys::WaitKeys;
#[cfg(test)]
pub(crate) use keys::skips;
pub use printable::printable;
pub use screen::Screen;
pub use window::{WindowSize, window_size};

/// `out` for rendering with `caps`, which wraps kitty's graphics for tmux
/// when they say so.
pub fn wrapped<W: Write>(out: W, caps: &Capabilities) -> tmux::Wrapping<W> {
    tmux::Wrapping::new(out, caps.tmux_passthrough)
}

/// Whether stdout is a terminal.
pub fn stdout_is_terminal() -> bool {
    rustix::termios::isatty(std::io::stdout())
}

/// Whether stderr is a terminal.
pub fn stderr_is_terminal() -> bool {
    rustix::termios::isatty(std::io::stderr())
}

/// Whether stdin is a terminal.
pub fn stdin_is_terminal() -> bool {
    rustix::termios::isatty(std::io::stdin())
}

/// Sets the terminal `tty` back to `original`, and discards what was typed
/// or answered on it and not read. `OptionalActions::Flush` would also
/// wait until the terminal has read all that kiri wrote: on macOS a
/// pseudo-terminal whose other end nobody reads, as in the tests, waits
/// for ever, and so does a terminal stopped with Ctrl-S.
fn set_back(tty: impl AsFd, original: &Termios) {
    let _ = tcflush(&tty, QueueSelector::IFlush);
    let _ = tcsetattr(&tty, OptionalActions::Now, original);
}

/// Whether `fd` has something to read, waiting at most `timeout` for it.
/// On macOS, poll does not work on devices such as the terminal: it says
/// at once that there is something to read, and the read that follows
/// waits for a key. There select waits instead.
fn readable(fd: impl AsFd, timeout: Duration) -> rustix::io::Result<bool> {
    if cfg!(target_os = "macos") {
        readable_by_select(fd.as_fd(), timeout)
    } else {
        readable_by_poll(fd.as_fd(), timeout)
    }
}

/// [`readable`] with poll.
fn readable_by_poll(fd: BorrowedFd<'_>, timeout: Duration) -> rustix::io::Result<bool> {
    let mut fds = [PollFd::new(&fd, PollFlags::IN)];
    Ok(poll(&mut fds, Some(&timespec(timeout)))? > 0)
}

/// [`readable`] with select.
fn readable_by_select(fd: BorrowedFd<'_>, timeout: Duration) -> rustix::io::Result<bool> {
    let raw = fd.as_raw_fd();
    let mut set = vec![FdSetElement::default(); fd_set_num_elements(1, raw + 1)];
    fd_set_insert(&mut set, raw);
    // SAFETY: the only descriptor in the set is `fd`, which stays open
    // while it is borrowed.
    let ready = unsafe {
        select(
            raw + 1,
            Some(&mut set),
            None,
            None,
            Some(&timespec(timeout)),
        )
    }?;
    Ok(ready > 0)
}

/// `duration` as a [`Timespec`].
fn timespec(duration: Duration) -> Timespec {
    Timespec {
        tv_sec: duration.as_secs() as i64,
        tv_nsec: i64::from(duration.subsec_nanos()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::fd::AsFd;
    use std::time::{Duration, Instant};

    use super::{readable_by_poll, readable_by_select};

    /// Both ways to wait: they wait the whole time when nothing was typed,
    /// and answer at once when something was.
    #[test]
    fn poll_and_select_wait_for_what_was_typed() {
        for readable in [readable_by_poll, readable_by_select] {
            let (mut terminal, tty) = super::pty::pty();
            let timeout = Duration::from_millis(50);
            let start = Instant::now();
            assert!(!readable(tty.as_fd(), timeout).unwrap());
            assert!(start.elapsed() >= timeout);

            terminal.write_all(b"q\n").unwrap();
            assert!(readable(tty.as_fd(), Duration::from_secs(5)).unwrap());
        }
    }
}
