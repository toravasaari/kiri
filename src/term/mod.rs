//! The terminal itself: finding out what it can do, its window and the
//! pager's screen. All of kiri's terminal I/O happens here.

mod answers;
mod detect;
mod probe;
mod screen;
#[cfg(test)]
mod testing;
mod window;

pub use detect::{CellSize, Detection, Mode, capabilities, examine, no_color};
pub use screen::Screen;
pub use window::{WindowSize, window_size};

/// Whether stdout is a terminal.
pub fn stdout_is_terminal() -> bool {
    rustix::termios::isatty(std::io::stdout())
}

/// Whether stdin is a terminal.
pub fn stdin_is_terminal() -> bool {
    rustix::termios::isatty(std::io::stdin())
}
