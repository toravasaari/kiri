//! The terminal itself: finding out what it can do, its window and the
//! pager's screen. All of kiri's terminal I/O happens here.

use std::os::fd::AsFd;
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};

mod answers;
mod detect;
mod interrupt;
mod keys;
mod probe;
pub mod progress;
#[cfg(test)]
mod pty;
mod screen;
#[cfg(test)]
mod testing;
pub mod tmux;
mod window;

pub use detect::{CellSize, Detection, Mode, capabilities, examine, no_color};
pub use interrupt::{Interrupt, end_by};
pub use keys::WaitKeys;
pub use screen::Screen;
pub use window::{WindowSize, window_size};

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

/// Whether `fd` has something to read, waiting at most `timeout` for it.
fn readable(fd: impl AsFd, timeout: Duration) -> rustix::io::Result<bool> {
    let timeout = Timespec {
        tv_sec: timeout.as_secs() as i64,
        tv_nsec: i64::from(timeout.subsec_nanos()),
    };
    let mut fds = [PollFd::new(&fd, PollFlags::IN)];
    Ok(poll(&mut fds, Some(&timeout))? > 0)
}
