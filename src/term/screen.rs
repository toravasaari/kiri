//! The pager's screen: the alternate screen, and raw mode for keys.

use std::fs::File;
use std::io::{self, Read, Stdout, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use signal_hook::SigId;
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};

use rustix::termios::{OptionalActions, Termios, tcgetattr, tcsetattr};

use super::probe;
use super::window::{WindowSize, window_size};

/// Raw mode on a terminal until dropped, also when a panic unwinds: keys
/// arrive one by one without echo, and Ctrl+C is a key rather than a
/// signal.
pub(crate) struct RawMode {
    tty: File,
    original: Termios,
}

impl RawMode {
    pub(crate) fn enter(tty: &File) -> Option<Self> {
        let original = tcgetattr(tty).ok()?;
        let mut raw = original.clone();
        raw.make_raw();
        tcsetattr(tty, OptionalActions::Now, &raw).ok()?;
        let tty = tty.try_clone().ok()?;
        Some(Self { tty, original })
    }
}

impl RawMode {
    /// Ends raw mode.
    fn restore(&self) {
        // Flush also discards input that arrived but was not read, such as
        // probe answers after the timeout, so they do not show up at the
        // shell prompt. Keys typed meanwhile go with them. Answers that
        // arrive later still can show up; nothing here can wait for a
        // terminal that may never answer.
        let _ = tcsetattr(&self.tty, OptionalActions::Flush, &self.original);
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Leaving the pager's screen: the alternate screen and raw mode end once,
/// when the screen is dropped or, before that, when a panic hook runs.
struct Leave {
    raw: RawMode,
    done: AtomicBool,
}

impl Leave {
    /// Leaves the alternate screen on `out` and ends raw mode, unless that
    /// has been done.
    fn run(&self, out: &mut impl Write) {
        if self.done.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = out.write_all(LEAVE_SCREEN);
        let _ = out.flush();
        self.raw.restore();
    }

    fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }
}

/// The terminal while the pager shows a document: the alternate screen on
/// the output, and the controlling terminal in raw mode for keys. Both end
/// when it is dropped, also when a panic unwinds.
pub struct Screen<W: Write> {
    tty: File,
    out: W,
    /// What has been written and not yet flushed to `out`; dropped when a
    /// panic has left the screen, so no part of a frame lands on the main
    /// screen.
    buffer: Vec<u8>,
    leave: Arc<Leave>,
    /// Whether a panic hook of the screen's own is set.
    hooked: bool,
    /// The signal that asked kiri to end, 0 before one came.
    signal: Arc<AtomicUsize>,
    /// The handlers that set it.
    handlers: Vec<SigId>,
}

impl Screen<Stdout> {
    /// The pager's screen on standard output, with keys from the
    /// controlling terminal, so a piped document can be paged too. `None`
    /// without a controlling terminal or in the background, where raw mode
    /// would stop kiri until it is brought to the foreground.
    pub fn open() -> Option<Self> {
        let tty = probe::controlling_terminal()?;
        if probe::runs_in_background(&tty) {
            return None;
        }
        let mut screen = Self::on(tty, std::io::stdout())?;
        screen.catch_signals();
        screen.catch_panics();
        Some(screen)
    }
}

impl<W: Write> Screen<W> {
    /// The pager's screen on `out`, with keys from `tty`.
    fn on(tty: File, mut out: W) -> Option<Self> {
        let raw = RawMode::enter(&tty)?;
        out.write_all(ENTER_SCREEN).ok()?;
        out.flush().ok()?;
        Some(Self {
            tty,
            out,
            buffer: Vec::new(),
            leave: Arc::new(Leave {
                raw,
                done: AtomicBool::new(false),
            }),
            hooked: false,
            signal: Arc::new(AtomicUsize::new(0)),
            handlers: Vec::new(),
        })
    }

    /// Makes the signals that would end kiri end the screen instead:
    /// [`read`](Self::read) fails once one comes, and [`close`](Self::close)
    /// then ends kiri with it, after the terminal is restored. Ctrl+C sends
    /// no signal in raw mode; these come from elsewhere, such as `kill` or
    /// a terminal that closes.
    fn catch_signals(&mut self) {
        for signal in [SIGTERM, SIGHUP, SIGINT, SIGQUIT] {
            let flag = Arc::clone(&self.signal);
            if let Ok(id) = signal_hook::flag::register_usize(signal, flag, signal as usize) {
                self.handlers.push(id);
            }
        }
    }

    /// Makes a panic leave the screen before its message is written, so the
    /// message stays on the main screen.
    fn catch_panics(&mut self) {
        let leave = Arc::clone(&self.leave);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Standard output is the screen's output: see `open`.
            leave.run(&mut std::io::stdout());
            previous(info);
        }));
        self.hooked = true;
    }

    /// The signal that asked kiri to end, if one came.
    fn signal(&self) -> Option<i32> {
        match self.signal.load(Ordering::SeqCst) {
            0 => None,
            signal => Some(signal as i32),
        }
    }

    /// Restores the terminal, and then ends kiri with the signal that
    /// asked it to end, if one came.
    pub fn close(self) {
        // Read after the drop, so a signal that comes while the terminal
        // is restored still counts.
        let flag = Arc::clone(&self.signal);
        drop(self);
        match flag.load(Ordering::SeqCst) {
            0 => {}
            signal => {
                let _ = signal_hook::low_level::emulate_default_handler(signal as i32);
            }
        }
    }

    /// The size of the window now.
    pub fn size(&self) -> Option<WindowSize> {
        window_size(&self.tty)
    }

    /// The bytes of the keys pressed, waiting at most `timeout` for the
    /// first; empty when none came, and an error when the terminal has
    /// gone away or a signal asked kiri to end.
    pub fn read(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};

        let timeout = Timespec {
            tv_sec: timeout.as_secs() as i64,
            tv_nsec: i64::from(timeout.subsec_nanos()),
        };
        let mut fds = [PollFd::new(&self.tty, PollFlags::IN)];
        let ready = poll(&mut fds, Some(&timeout));
        if self.signal().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "ended by a signal",
            ));
        }
        match ready {
            Ok(0) | Err(rustix::io::Errno::INTR) => return Ok(Vec::new()),
            Ok(_) => {}
            Err(error) => return Err(error.into()),
        }
        let mut buffer = [0; 1024];
        match self.tty.read(&mut buffer)? {
            // Ready but empty: the terminal has gone away.
            0 => Err(io::ErrorKind::UnexpectedEof.into()),
            read => Ok(buffer[..read].to_vec()),
        }
    }
}

impl<W: Write> Write for Screen<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.write_all(&self.buffer)?;
        self.buffer.clear();
        self.out.flush()
    }
}

impl<W: Write> Drop for Screen<W> {
    fn drop(&mut self) {
        if !self.leave.is_done() {
            let _ = self.flush();
        }
        self.leave.run(&mut self.out);
        // Last, so a signal that comes until the terminal is restored is
        // caught rather than lost.
        for &id in &self.handlers {
            signal_hook::low_level::unregister(id);
        }
        // Back to the default hook, as kiri sets no other. The hook cannot
        // be taken back while a panic runs it.
        if self.hooked && !std::thread::panicking() {
            let _ = std::panic::take_hook();
        }
    }
}

/// The alternate screen, with the cursor hidden.
const ENTER_SCREEN: &[u8] = b"\x1b[?1049h\x1b[?25l";
/// Back to the main screen as it was, with the cursor shown.
const LEAVE_SCREEN: &[u8] = b"\x1b[?25h\x1b[?1049l";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::testing::{pty, size};

    #[test]
    fn raw_mode_ends_when_a_panic_unwinds() {
        use rustix::termios::LocalModes;

        let (_terminal, tty) = pty();
        let original = tcgetattr(&tty).unwrap();
        let result = std::panic::catch_unwind(|| {
            let _raw = RawMode::enter(&tty).unwrap();
            let modes = tcgetattr(&tty).unwrap().local_modes;
            assert!(!modes.contains(LocalModes::ICANON));
            panic!("the probe fails while the terminal is raw");
        });
        assert!(result.is_err());
        let restored = tcgetattr(&tty).unwrap();
        assert_eq!(restored.local_modes, original.local_modes);
        assert_eq!(restored.input_modes, original.input_modes);
    }

    #[test]
    fn ending_raw_mode_discards_unread_answers() {
        use std::io::Write;

        use rustix::event::{PollFd, PollFlags, Timespec, poll};

        let (mut terminal, tty) = pty();
        {
            let _raw = RawMode::enter(&tty).unwrap();
            // A late answer that nothing reads.
            terminal.write_all(b"\x1b[?62;c\n").unwrap();
            let mut fds = [PollFd::new(&tty, PollFlags::IN)];
            let timeout = Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            };
            assert_eq!(poll(&mut fds, Some(&timeout)).unwrap(), 1);
        }
        assert_eq!(rustix::io::ioctl_fionread(&tty).unwrap(), 0);
    }

    /// A screen on a pseudo-terminal, writing to memory, and the
    /// terminal's side of the pseudo-terminal.
    fn screen() -> (Screen<Vec<u8>>, File, File) {
        let (terminal, tty) = pty();
        let probe = tty.try_clone().unwrap();
        let screen = Screen::on(tty, Vec::new()).expect("the screen opens");
        (screen, terminal, probe)
    }

    #[test]
    fn the_screen_is_the_alternate_screen_in_raw_mode() {
        use rustix::termios::LocalModes;

        let (screen, _terminal, tty) = screen();
        assert_eq!(screen.out, b"\x1b[?1049h\x1b[?25l");
        let modes = tcgetattr(&tty).unwrap().local_modes;
        assert!(!modes.contains(LocalModes::ICANON));
    }

    #[test]
    fn the_screen_ends_when_a_panic_unwinds() {
        use std::sync::{Arc, Mutex};

        /// Output that outlives the screen.
        #[derive(Clone)]
        struct Shared(Arc<Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let (_terminal, tty) = pty();
        let original = tcgetattr(&tty).unwrap();
        let out = Shared(Arc::new(Mutex::new(Vec::new())));
        let result = std::panic::catch_unwind({
            let tty = tty.try_clone().unwrap();
            let out = out.clone();
            move || {
                let _screen = Screen::on(tty, out).expect("the screen opens");
                panic!("the pager fails");
            }
        });
        assert!(result.is_err());
        let written = out.0.lock().unwrap().clone();
        assert!(written.ends_with(b"\x1b[?25h\x1b[?1049l"), "{written:?}");
        let restored = tcgetattr(&tty).unwrap();
        assert_eq!(restored.local_modes, original.local_modes);
    }

    #[test]
    fn the_screen_reads_the_keys_pressed() {
        use std::io::Write;

        let (mut screen, mut terminal, _tty) = screen();
        terminal.write_all(b"j\x1b[B").unwrap();
        let keys = screen.read(Duration::from_secs(5)).unwrap();
        assert_eq!(keys, b"j\x1b[B");
    }

    #[test]
    fn the_screen_reads_nothing_when_no_key_comes() {
        let (mut screen, _terminal, _tty) = screen();
        let keys = screen.read(Duration::from_millis(10)).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn the_screen_tells_the_window_size() {
        let (screen, _terminal, tty) = screen();
        rustix::termios::tcsetwinsize(&tty, size(100, 30, 1000, 600)).unwrap();
        let window = screen.size().expect("a pseudo-terminal has a size");
        assert_eq!((window.columns, window.rows), (100, 30));
    }

    #[test]
    fn the_screen_fails_when_the_terminal_goes_away() {
        let (mut screen, terminal, _tty) = screen();
        drop(terminal);
        assert!(screen.read(Duration::from_secs(5)).is_err());
    }

    /// Run on its own by [`a_panic_message_comes_after_the_screen`]: opens
    /// a screen on standard output, leaves part of a frame unwritten and
    /// panics.
    #[test]
    #[ignore = "run by a_panic_message_comes_after_the_screen"]
    fn panic_on_a_screen() {
        use std::io::Write;

        let (_terminal, tty) = pty();
        let mut screen = Screen::on(tty, std::io::stdout()).expect("the screen opens");
        screen.catch_panics();
        screen.write_all(b"[part of a frame]").unwrap();
        panic!("[the pager fails]");
    }

    #[test]
    fn a_panic_message_comes_after_the_screen() {
        use std::process::{Command, Stdio};

        let (mut reader, writer) = std::io::pipe().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "term::screen::tests::panic_on_a_screen"])
            .args(["--ignored", "--nocapture", "--test-threads", "1"])
            .stdin(Stdio::null())
            .stdout(writer.try_clone().unwrap())
            .stderr(writer)
            .spawn()
            .unwrap();
        let mut output = Vec::new();
        let mut buffer = [0; 4096];
        // The child gets 10 s at most, so a hanging child fails the test
        // instead of hanging it.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            use rustix::event::{PollFd, PollFlags, Timespec, poll};

            let left = deadline.checked_duration_since(std::time::Instant::now());
            let timeout = left.map(|left| Timespec::try_from(left).unwrap());
            let mut fds = [PollFd::new(&reader, PollFlags::IN)];
            if left.is_none() || poll(&mut fds, timeout.as_ref()).unwrap() == 0 {
                child.kill().unwrap();
                panic!("the child did not finish: {output:?}");
            }
            // Reading ends once the child, which holds the writing end,
            // exits.
            match reader.read(&mut buffer).unwrap() {
                0 => break,
                read => output.extend_from_slice(&buffer[..read]),
            }
        }
        let _ = child.wait();
        let output = String::from_utf8_lossy(&output);
        let leave = output.find("\x1b[?1049l").expect("the screen is left");
        let message = output
            .find("[the pager fails]")
            .expect("the message is written");
        assert!(leave < message, "{output:?}");
        assert_eq!(output.matches("\x1b[?1049l").count(), 1, "{output:?}");
        assert!(!output.contains("[part of a frame]"), "{output:?}");
    }
}
