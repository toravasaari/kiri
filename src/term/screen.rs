//! The pager's screen: the alternate screen, and raw mode for keys.

use std::fs::File;
use std::io::{self, Read, Stdout, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use signal_hook::SigId;
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};

use rustix::termios::{OptionalActions, Termios, tcgetattr, tcsetattr};

use super::tmux::Wrapping;
use super::window::{WindowSize, window_size};
use super::{interrupt, probe};

/// The signals that would end kiri, caught while the pager's screen is
/// open unless they are ignored: one that comes sets a flag instead.
/// Dropping it takes the handlers away.
struct Signals {
    /// The signal that came, 0 before one did.
    came: Arc<AtomicUsize>,
    /// The signals caught.
    caught: Vec<i32>,
    handlers: Vec<SigId>,
}

impl Signals {
    /// None caught.
    fn none() -> Self {
        Signals {
            came: Arc::new(AtomicUsize::new(0)),
            caught: Vec::new(),
            handlers: Vec::new(),
        }
    }

    /// SIGTERM, SIGHUP, SIGINT and SIGQUIT caught from now on, except one
    /// that is ignored, as under `nohup`. Ctrl+C sends no signal in raw
    /// mode; these come from elsewhere, such as `kill` or a terminal that
    /// closes, or from Ctrl+C before raw mode.
    fn catch() -> Self {
        let mut signals = Self::none();
        // One that is ignored stays so.
        for signal in [SIGTERM, SIGHUP, SIGINT, SIGQUIT] {
            if interrupt::ignored(signal) {
                continue;
            }
            let flag = Arc::clone(&signals.came);
            if let Ok(id) = signal_hook::flag::register_usize(signal, flag, signal as usize) {
                signals.caught.push(signal);
                signals.handlers.push(id);
            }
        }
        signals
    }

    /// Makes the signals end kiri again, as they do by default, for a
    /// screen that did not open; one that came meanwhile ends kiri now.
    /// Taking the handlers away would not do: signal-hook's own handler
    /// stays, and would leave the signals doing nothing while the
    /// document is printed instead. The new handlers stay as long as
    /// kiri runs.
    fn give_back(self) {
        let ends = Arc::new(AtomicBool::new(true));
        for &signal in &self.caught {
            let _ = signal_hook::flag::register_conditional_default(signal, Arc::clone(&ends));
        }
        if let Some(signal) = interrupt::came(&self.came) {
            interrupt::end_by(signal);
        }
    }
}

impl Drop for Signals {
    fn drop(&mut self) {
        for &id in &self.handlers {
            signal_hook::low_level::unregister(id);
        }
    }
}

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
    /// The signals that ask kiri to end. The last field, so that its
    /// handlers go only after the terminal is restored, and a signal that
    /// comes until then is caught rather than lost.
    signals: Signals,
}

impl Screen<Wrapping<Stdout>> {
    /// The pager's screen on standard output, with keys from the
    /// controlling terminal, so a piped document can be paged too, with
    /// kitty's graphics wrapped for tmux if `wrap`. `None`
    /// without a controlling terminal or in the background, where raw mode
    /// would stop kiri until it is brought to the foreground. `caught` runs
    /// once the screen catches the signals, before the terminal is raw,
    /// such as to drop a guard that would otherwise end kiri by a signal
    /// with the terminal raw.
    pub fn open(caught: impl FnOnce(), wrap: bool) -> Option<Self> {
        let tty = probe::controlling_terminal()?;
        if probe::runs_in_background(&tty) {
            return None;
        }
        let out = Wrapping::new(std::io::stdout(), wrap);
        let mut screen = Self::catching_on(tty, out, caught)?;
        screen.catch_panics();
        Some(screen)
    }
}

impl<W: Write> Screen<W> {
    /// The pager's screen on `out`, with keys from `tty`, catching the
    /// signals that would end kiri: [`read`](Self::read) fails once one
    /// comes, and [`close`](Self::close) then ends kiri with it, after the
    /// terminal is restored. They are caught before the terminal is raw,
    /// so that none can end kiri and leave it so; `caught` runs in
    /// between.
    fn catching_on(tty: File, out: W, caught: impl FnOnce()) -> Option<Self> {
        let signals = Signals::catch();
        caught();
        Self::on(tty, out, signals)
    }

    /// The pager's screen on `out`, with keys from `tty`, and `signals`
    /// caught while it is open. When it cannot open, the signals end kiri
    /// again.
    fn on(tty: File, mut out: W, signals: Signals) -> Option<Self> {
        let Some(raw) = RawMode::enter(&tty) else {
            signals.give_back();
            return None;
        };
        if out
            .write_all(ENTER_SCREEN)
            .and_then(|()| out.flush())
            .is_err()
        {
            // The terminal restored before a signal that came ends kiri.
            drop(raw);
            signals.give_back();
            return None;
        }
        Some(Self {
            tty,
            out,
            buffer: Vec::new(),
            leave: Arc::new(Leave {
                raw,
                done: AtomicBool::new(false),
            }),
            hooked: false,
            signals,
        })
    }

    /// Makes a panic leave the screen before its message is written, so the
    /// message stays on the main screen. A panic that
    /// [`crate::panics::catch`] catches, such as a load's in the
    /// background, which then fails, neither leaves the screen nor writes
    /// over it.
    fn catch_panics(&mut self) {
        let leave = Arc::clone(&self.leave);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if crate::panics::catching() {
                return;
            }
            // Standard output is the screen's output: see `open`.
            leave.run(&mut std::io::stdout());
            previous(info);
        }));
        self.hooked = true;
    }

    /// The signal that asked kiri to end, if one came.
    fn signal(&self) -> Option<i32> {
        interrupt::came(&self.signals.came)
    }

    /// Restores the terminal, and then ends kiri with the signal that
    /// asked it to end, if one came.
    pub fn close(self) {
        self.close_then(|| {});
    }

    /// Restores the terminal, runs `after`, such as waiting for programs
    /// that are to end, and then ends kiri with the signal that asked it
    /// to end, if one came.
    pub fn close_then(self, after: impl FnOnce()) {
        // Read after the drop, so a signal that comes while the terminal
        // is restored still counts.
        let flag = Arc::clone(&self.signals.came);
        drop(self);
        after();
        if let Some(signal) = interrupt::came(&flag) {
            interrupt::end_by(signal);
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
        let ready = super::readable(&self.tty, timeout);
        if self.signal().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "ended by a signal",
            ));
        }
        match ready {
            Ok(false) | Err(rustix::io::Errno::INTR) => return Ok(Vec::new()),
            Ok(true) => {}
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
        // Back to the default hook, as kiri sets no other. The hook cannot
        // be taken back while a panic runs it.
        if self.hooked && !std::thread::panicking() {
            let _ = std::panic::take_hook();
        }
    }
}

/// The alternate screen, with the cursor hidden, and focus reporting,
/// which tells the pager when its tmux pane is shown again.
const ENTER_SCREEN: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[?1004h";
/// Back to the main screen as it was, with the cursor shown.
const LEAVE_SCREEN: &[u8] = b"\x1b[?1004l\x1b[?25h\x1b[?1049l";

#[cfg(test)]
mod tests;
