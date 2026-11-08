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
    /// The height in rows, or `None` for a window that reports none, whose
    /// length is not known.
    pub fn height_in_rows(&self) -> Option<u16> {
        Some(self.rows).filter(|&rows| rows > 0)
    }

    /// Size of one cell in pixels, when the terminal reports its size in
    /// pixels and a height in rows.
    pub fn cell_pixels(&self) -> Option<(u16, u16)> {
        let rows = self.height_in_rows()?;
        let cell_width = self.width / self.columns.max(1);
        let cell_height = self.height / rows;
        (cell_width > 0 && cell_height > 0).then_some((cell_width, cell_height))
    }
}

/// The size of the terminal window `fd` refers to, if it is a terminal
/// that reports one: a window of no columns is unknown, and one of no rows
/// has no [height](WindowSize::height_in_rows).
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
    fn xterms_window_size_with_its_border_still_gives_the_cell_size() {
        // 100 × 30 cells of 12 × 23 pixels, and 2 pixels of border on each
        // side, as xterm reports them: the integer division drops the border.
        let (_terminal, tty) = pty();
        rustix::termios::tcsetwinsize(&tty, size(100, 30, 1204, 694)).unwrap();
        assert_eq!(window_size(&tty).unwrap().cell_pixels(), Some((12, 23)));
    }

    #[test]
    fn a_window_that_reports_no_columns_has_no_size() {
        let (_terminal, tty) = pty();
        rustix::termios::tcsetwinsize(&tty, size(0, 24, 0, 0)).unwrap();
        assert_eq!(window_size(&tty), None);
    }

    #[test]
    fn a_window_that_reports_no_rows_keeps_its_columns_and_has_no_height() {
        let (_terminal, tty) = pty();
        rustix::termios::tcsetwinsize(&tty, size(120, 0, 0, 0)).unwrap();
        let window = window_size(&tty).expect("the columns are known");
        assert_eq!(window.columns, 120);
        assert_eq!(window.height_in_rows(), None);
        rustix::termios::tcsetwinsize(&tty, size(120, 30, 0, 0)).unwrap();
        assert_eq!(window_size(&tty).unwrap().height_in_rows(), Some(30));
    }

    #[test]
    fn a_window_with_no_rows_has_no_cell_size_whatever_its_pixels() {
        let (_terminal, tty) = pty();
        rustix::termios::tcsetwinsize(&tty, size(120, 0, 2560, 1600)).unwrap();
        assert_eq!(window_size(&tty).unwrap().cell_pixels(), None);
        rustix::termios::tcsetwinsize(&tty, size(120, 40, 2400, 1600)).unwrap();
        assert_eq!(window_size(&tty).unwrap().cell_pixels(), Some((20, 40)));
    }

    #[test]
    fn a_pipe_has_no_window_size() {
        let (reader, _writer) = std::io::pipe().unwrap();
        assert_eq!(window_size(&reader), None);
    }
}
