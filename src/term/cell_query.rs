//! Asking the terminal for the size of its cells.

use std::io::{self, Write};

use super::tmux;
use crate::kitty::query;

/// Writes the query for the terminal's cell size, `CSI 16 t`, to `out`.
/// kitty answers it among the keys, as [`query::cell_size`] reads it. In
/// tmux with passthrough, `through_tmux`, the query is wrapped in it, so
/// that it reaches kitty: tmux answers it itself otherwise, from a pixel
/// size it may only guess.
pub fn ask(through_tmux: bool, out: &mut impl Write) -> io::Result<()> {
    match through_tmux {
        true => out.write_all(tmux::passthrough(query::CELL_SIZE).as_bytes()),
        false => out.write_all(query::CELL_SIZE.as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_with_csi_16_t() {
        let mut out = Vec::new();
        ask(false, &mut out).unwrap();
        assert_eq!(out, b"\x1b[16t");
    }

    #[test]
    fn wraps_the_query_in_tmux_s_passthrough_when_asked() {
        let mut out = Vec::new();
        ask(true, &mut out).unwrap();
        assert_eq!(out, b"\x1bPtmux;\x1b\x1b[16t\x1b\\");
    }
}
