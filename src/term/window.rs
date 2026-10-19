//! The size of a terminal window.

use std::os::fd::AsFd;

/// The size of a terminal window, from one query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    /// Columns and rows of cells.
    pub columns: u16,
    pub rows: u16,
    /// Width and height in pixels; 0 when the terminal does not tell.
    pub width: u16,
    pub height: u16,
}

impl WindowSize {
    /// Size of one cell in pixels, when the terminal reports its size in
    /// pixels.
    pub fn cell_pixels(&self) -> Option<(u16, u16)> {
        let cell_width = self.width / self.columns.max(1);
        let cell_height = self.height / self.rows.max(1);
        (cell_width > 0 && cell_height > 0).then_some((cell_width, cell_height))
    }
}

/// The size of the terminal window `fd` refers to, if it is a terminal
/// that reports one.
pub fn window_size(fd: impl AsFd) -> Option<WindowSize> {
    let size = rustix::termios::tcgetwinsize(fd).ok()?;
    (size.ws_col > 0).then_some(WindowSize {
        columns: size.ws_col,
        rows: size.ws_row,
        width: size.ws_xpixel,
        height: size.ws_ypixel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::testing::{pty, size};

    #[test]
    fn reads_the_window_size_in_cells_and_pixels() {
        let (_terminal, tty) = pty();
        rustix::termios::tcsetwinsize(&tty, size(80, 24, 800, 480)).unwrap();
        assert_eq!(
            window_size(&tty),
            Some(WindowSize {
                columns: 80,
                rows: 24,
                width: 800,
                height: 480,
            })
        );
    }

    #[test]
    fn a_pipe_has_no_window_size() {
        let (reader, _writer) = std::io::pipe().unwrap();
        assert_eq!(window_size(&reader), None);
    }
}
