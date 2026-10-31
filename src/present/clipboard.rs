//! Copying text to the terminal's clipboard with OSC 52.

use std::io::{self, Write};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::term::tmux;

/// Writes the sequence that puts `text` on the clipboard, `ESC ] 52 ; c ;
/// <base64> ESC \`, to `out`. In tmux with passthrough, `through_tmux`,
/// the sequence is wrapped in it, so that it reaches kitty whatever tmux's
/// `set-clipboard` says, as long as `allow-passthrough` lets the images
/// through too. Without passthrough tmux gets it as it is and does with it
/// what its own `set-clipboard` setting says.
pub fn copy(text: &str, through_tmux: bool, out: &mut impl Write) -> io::Result<()> {
    let sequence = format!("\x1b]52;c;{}\x1b\\", STANDARD.encode(text));
    match through_tmux {
        true => out.write_all(tmux::passthrough(&sequence).as_bytes()),
        false => out.write_all(sequence.as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_text_as_base64_in_an_osc_52_sequence() {
        let mut out = Vec::new();
        copy("https://a.example/ü", false, &mut out).unwrap();
        assert_eq!(out, b"\x1b]52;c;aHR0cHM6Ly9hLmV4YW1wbGUvw7w=\x1b\\");
    }

    #[test]
    fn wraps_the_sequence_in_tmux_s_passthrough_when_asked() {
        let mut out = Vec::new();
        copy("a", true, &mut out).unwrap();
        assert_eq!(out, b"\x1bPtmux;\x1b\x1b]52;c;YQ==\x1b\x1b\\\x1b\\");
    }

    #[test]
    fn what_the_wrapping_writer_gets_comes_out_as_it_went_in() {
        // The terminal's output wraps graphics sequences only, and leaves
        // a wrapped sequence of another kind alone.
        let mut out = tmux::Wrapping::new(Vec::new(), true);
        copy("a", true, &mut out).unwrap();
        out.flush().unwrap();
        assert_eq!(
            out.into_inner(),
            b"\x1bPtmux;\x1b\x1b]52;c;YQ==\x1b\x1b\\\x1b\\"
        );
    }
}
