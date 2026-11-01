//! The keys read while kiri waits for a document's images before the
//! pager opens: Space or Enter opens it at once. The terminal reads keys
//! one at a time without echo meanwhile, and Ctrl-C and the other signal
//! keys still send their signals.

use std::fs::File;
use std::io::Read;
use std::time::Duration;

use rustix::termios::{LocalModes, OptionalActions, Termios, tcgetattr, tcsetattr};

use super::probe;

/// The controlling terminal set up to read keys while kiri waits, until
/// dropped, which sets it back as it was.
pub struct WaitKeys {
    tty: File,
    original: Termios,
}

impl WaitKeys {
    /// Keys from the controlling terminal; `None` without one, or in the
    /// background, where reading it would stop kiri.
    pub fn open() -> Option<Self> {
        let tty = probe::controlling_terminal()?;
        if probe::runs_in_background(&tty) {
            return None;
        }
        Self::on(tty)
    }

    /// Keys from `tty`.
    fn on(tty: File) -> Option<Self> {
        let original = tcgetattr(&tty).ok()?;
        let mut keys = original.clone();
        keys.local_modes
            .remove(LocalModes::ICANON | LocalModes::ECHO);
        tcsetattr(&tty, OptionalActions::Now, &keys).ok()?;
        Some(Self { tty, original })
    }

    /// Keys from a pseudo-terminal of a test, with `typed` typed before
    /// they are read; the terminal side, which must live as long.
    #[cfg(test)]
    pub(crate) fn typed_ahead(typed: &[u8]) -> (Self, File) {
        use std::io::Write;
        let (mut terminal, tty) = super::pty::pty();
        terminal.write_all(typed).unwrap();
        (
            Self::on(tty).expect("a pseudo-terminal to set up"),
            terminal,
        )
    }

    /// Whether Space or Enter was pressed, waiting at most `timeout` for
    /// a key; other keys, and answers to queries that come late, go
    /// unheeded.
    pub fn skip(&mut self, timeout: Duration) -> bool {
        // A signal that comes meanwhile is for the caller to look at.
        if !super::readable(&self.tty, timeout).unwrap_or(false) {
            return false;
        }
        let mut buffer = [0; 256];
        match self.tty.read(&mut buffer) {
            Ok(read) => skips(&buffer[..read]),
            Err(_) => false,
        }
    }
}

impl Drop for WaitKeys {
    fn drop(&mut self) {
        // Flush discards what was typed and not read, so that no key of
        // the wait reaches the pager or the shell.
        let _ = tcsetattr(&self.tty, OptionalActions::Flush, &self.original);
    }
}

/// Whether `bytes`, read at once, hold Space or Enter typed as a key
/// rather than inside an escape sequence, such as an answer to a query.
fn skips(bytes: &[u8]) -> bool {
    bytes.first() != Some(&ESC)
        && bytes
            .iter()
            .any(|byte| matches!(byte, b' ' | b'\r' | b'\n'))
}

/// The byte that starts an escape sequence.
const ESC: u8 = 0x1b;

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::term::testing::pty;

    #[test]
    fn space_or_enter_skips_and_other_keys_do_not() {
        for (typed, skipped) in [
            (&b" "[..], true),
            (b"\r", true),
            (b"\n", true),
            (b"x", false),
            (b"q", false),
            (b"\x1b[A", false),
            (b"\x1b]11;rgb:0000/0000/0000\x1b\\", false),
        ] {
            assert_eq!(skips(typed), skipped, "{typed:?}");
        }
    }

    #[test]
    fn keys_come_one_at_a_time_without_echo_and_signals_still_work() {
        let (mut terminal, tty) = pty();
        let original = tcgetattr(&tty).unwrap();
        {
            let mut keys = WaitKeys::on(tty.try_clone().unwrap()).unwrap();
            let modes = tcgetattr(&tty).unwrap().local_modes;
            assert!(!modes.contains(LocalModes::ICANON), "{modes:?}");
            assert!(!modes.contains(LocalModes::ECHO), "{modes:?}");
            assert!(modes.contains(LocalModes::ISIG), "{modes:?}");
            assert!(!keys.skip(Duration::from_millis(10)));
            // No line end needed.
            terminal.write_all(b"x").unwrap();
            assert!(!keys.skip(Duration::from_secs(1)));
            terminal.write_all(b" ").unwrap();
            assert!(keys.skip(Duration::from_secs(1)));
            // Typed while kiri waits, for no one: gone with the mode.
            terminal.write_all(b"j").unwrap();
            std::thread::sleep(Duration::from_millis(50));
        }
        let restored = tcgetattr(&tty).unwrap();
        assert_eq!(restored.local_modes, original.local_modes);
        assert_eq!(rustix::io::ioctl_fionread(&tty).unwrap(), 0);
    }
}
