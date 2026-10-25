//! The pager's screen: the alternate screen, and raw mode for keys.

use std::fs::File;
use std::io::{self, Read, Stdout, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use signal_hook::SigId;
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};

use rustix::termios::{OptionalActions, Termios, tcgetattr, tcsetattr};

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

impl Screen<Stdout> {
    /// The pager's screen on standard output, with keys from the
    /// controlling terminal, so a piped document can be paged too. `None`
    /// without a controlling terminal or in the background, where raw mode
    /// would stop kiri until it is brought to the foreground. `caught` runs
    /// once the screen catches the signals, before the terminal is raw,
    /// such as to drop a guard that would otherwise end kiri by a signal
    /// with the terminal raw.
    pub fn open(caught: impl FnOnce()) -> Option<Self> {
        let tty = probe::controlling_terminal()?;
        if probe::runs_in_background(&tty) {
            return None;
        }
        let mut screen = Self::catching_on(tty, std::io::stdout(), caught)?;
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

/// The alternate screen, with the cursor hidden.
const ENTER_SCREEN: &[u8] = b"\x1b[?1049h\x1b[?25l";
/// Back to the main screen as it was, with the cursor shown.
const LEAVE_SCREEN: &[u8] = b"\x1b[?25h\x1b[?1049l";

#[cfg(test)]
mod tests {
    use std::path::Path;

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
        let screen = Screen::on(tty, Vec::new(), Signals::none()).expect("the screen opens");
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
                let _screen = Screen::on(tty, out, Signals::none()).expect("the screen opens");
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
        let mut screen =
            Screen::on(tty, std::io::stdout(), Signals::none()).expect("the screen opens");
        screen.catch_panics();
        screen.write_all(b"[part of a frame]").unwrap();
        panic!("[the pager fails]");
    }

    /// Output that raises `signal` when the screen is entered: once the
    /// terminal is raw, before the screen has opened.
    struct RaiseOnEnter(i32);

    impl Write for RaiseOnEnter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if buf == ENTER_SCREEN {
                signal_hook::low_level::raise(self.0)?;
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Output that cannot enter the screen, after it raises `signal`, if
    /// one is given, as the terminal is raw.
    struct FailOnEnter(Option<i32>);

    impl Write for FailOnEnter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if buf != ENTER_SCREEN {
                return Ok(buf.len());
            }
            if let Some(signal) = self.0 {
                signal_hook::low_level::raise(signal)?;
            }
            Err(io::Error::other("no screen"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// The terminal that [`in_child`] gives a child test.
    const PTY: &str = "KIRI_TEST_PTY";

    /// A file that [`in_child`] names for a child test to leave a mark
    /// in.
    const MARK: &str = "KIRI_TEST_MARK";

    /// Run on its own by [`in_child`]: opens a screen on the terminal
    /// `KIRI_TEST_PTY` names, gets SIGTERM while it opens, and closes it.
    #[test]
    #[ignore = "run by in_child"]
    fn sigterm_while_the_screen_opens() {
        let tty = File::options()
            .read(true)
            .write(true)
            .open(std::env::var_os(PTY).unwrap())
            .unwrap();
        let screen =
            Screen::catching_on(tty, RaiseOnEnter(SIGTERM), || {}).expect("the screen opens");
        screen.close();
    }

    /// Run on its own by [`in_child`]: with SIGTERM ignored, as under
    /// `nohup`, opens a screen on the terminal `KIRI_TEST_PTY` names,
    /// gets SIGTERM while it is open, and closes it.
    #[test]
    #[ignore = "run by in_child"]
    fn ignored_sigterm_on_the_screen() {
        // SAFETY: setting a signal to be ignored needs no handler.
        unsafe { libc::signal(SIGTERM, libc::SIG_IGN) };
        let tty = File::options()
            .read(true)
            .write(true)
            .open(std::env::var_os(PTY).unwrap())
            .unwrap();
        let screen = Screen::catching_on(tty, Vec::new(), || {}).expect("the screen opens");
        signal_hook::low_level::raise(SIGTERM).unwrap();
        screen.close();
    }

    /// Run on its own by [`in_child`]: as kiri does after external
    /// programs drew diagrams, opens a screen on the terminal
    /// `KIRI_TEST_PTY` names while the drawing's signal guard is
    /// released, gets SIGTERM while it opens, and closes it.
    #[test]
    #[ignore = "run by in_child"]
    fn sigterm_after_drawing_while_the_screen_opens() {
        let tty = File::options()
            .read(true)
            .write(true)
            .open(std::env::var_os(PTY).unwrap())
            .unwrap();
        let interrupt = super::super::Interrupt::catch().unwrap();
        assert_eq!(interrupt.release(), None);
        let screen = Screen::catching_on(tty, RaiseOnEnter(SIGTERM), || drop(interrupt))
            .expect("the screen opens");
        screen.close();
    }

    /// Run on its own by [`in_child`]: as kiri does after external
    /// programs drew diagrams, fails to open a screen on the terminal
    /// `KIRI_TEST_PTY` names, and then gets SIGINT, as Ctrl-C sends while
    /// the document is printed instead.
    #[test]
    #[ignore = "run by in_child"]
    fn sigint_after_the_screen_fails_to_open() {
        let tty = File::options()
            .read(true)
            .write(true)
            .open(std::env::var_os(PTY).unwrap())
            .unwrap();
        let interrupt = super::super::Interrupt::catch().unwrap();
        assert_eq!(interrupt.release(), None);
        let screen = Screen::catching_on(tty, FailOnEnter(None), || drop(interrupt));
        assert!(screen.is_none());
        signal_hook::low_level::raise(SIGINT).unwrap();
    }

    /// Run on its own by [`in_child`]: gets SIGINT while a screen on the
    /// terminal `KIRI_TEST_PTY` names fails to open.
    #[test]
    #[ignore = "run by in_child"]
    fn sigint_while_the_screen_fails_to_open() {
        let tty = File::options()
            .read(true)
            .write(true)
            .open(std::env::var_os(PTY).unwrap())
            .unwrap();
        let screen = Screen::catching_on(tty, FailOnEnter(Some(SIGINT)), || {});
        assert!(screen.is_none());
    }

    /// Runs the ignored test `name` in a process of its own with a
    /// pseudo-terminal, and gives how it exited and the terminal after.
    fn in_child(name: &str) -> (std::process::ExitStatus, Termios) {
        in_child_marking(name, Path::new("/nonexistent/mark"))
    }

    /// [`in_child`], with the file `mark` for the child to leave a mark
    /// in.
    fn in_child_marking(name: &str, mark: &Path) -> (std::process::ExitStatus, Termios) {
        use std::process::{Command, Stdio};

        let (_terminal, tty) = pty();
        let path = std::fs::read_link(format!("/proc/self/fd/{}", {
            use std::os::fd::AsRawFd;
            tty.as_raw_fd()
        }))
        .unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &format!("term::screen::tests::{name}")])
            .args(["--ignored", "--test-threads", "1"])
            .env(PTY, path)
            .env(MARK, mark)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // 10 s at most, so a hanging child fails the test instead of
        // hanging it.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() > deadline {
                child.kill().unwrap();
                panic!("{name} did not finish");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        (status, tcgetattr(&tty).unwrap())
    }

    /// Run on its own by [`in_child`]: opens a screen on the terminal
    /// `KIRI_TEST_PTY` names, gets SIGTERM while it is open, and closes
    /// it with work after, which leaves a mark in `KIRI_TEST_MARK`.
    #[test]
    #[ignore = "run by in_child"]
    fn sigterm_on_a_screen_closed_with_work_after() {
        let tty = File::options()
            .read(true)
            .write(true)
            .open(std::env::var_os(PTY).unwrap())
            .unwrap();
        let screen = Screen::catching_on(tty, Vec::new(), || {}).expect("the screen opens");
        signal_hook::low_level::raise(SIGTERM).unwrap();
        screen.close_then(|| std::fs::write(std::env::var_os(MARK).unwrap(), "after").unwrap());
    }

    #[test]
    fn work_after_closing_runs_before_a_signal_ends_kiri() {
        use std::os::unix::process::ExitStatusExt;

        let scratch = crate::testing::Scratch::new("screen-close-then");
        let mark = scratch.join("mark");
        let (status, _) = in_child_marking("sigterm_on_a_screen_closed_with_work_after", &mark);
        assert_eq!(status.signal(), Some(SIGTERM), "{status}");
        assert_eq!(
            std::fs::read_to_string(&mark).ok().as_deref(),
            Some("after")
        );
    }

    #[test]
    fn a_signal_while_the_screen_opens_ends_kiri_after_the_terminal_is_restored() {
        use std::os::unix::process::ExitStatusExt;

        use rustix::termios::LocalModes;

        let (status, terminal) = in_child("sigterm_while_the_screen_opens");
        assert!(
            terminal.local_modes.contains(LocalModes::ICANON),
            "the terminal is left raw"
        );
        assert_eq!(status.signal(), Some(SIGTERM), "{status}");
    }

    #[test]
    fn a_signal_after_drawing_while_the_screen_opens_ends_kiri_after_the_terminal_is_restored() {
        use std::os::unix::process::ExitStatusExt;

        use rustix::termios::LocalModes;

        let (status, terminal) = in_child("sigterm_after_drawing_while_the_screen_opens");
        assert!(
            terminal.local_modes.contains(LocalModes::ICANON),
            "the terminal is left raw"
        );
        assert_eq!(status.signal(), Some(SIGTERM), "{status}");
    }

    #[test]
    fn ctrl_c_ends_kiri_when_the_screen_fails_to_open() {
        use std::os::unix::process::ExitStatusExt;

        use rustix::termios::LocalModes;

        for name in [
            "sigint_after_the_screen_fails_to_open",
            "sigint_while_the_screen_fails_to_open",
        ] {
            let (status, terminal) = in_child(name);
            assert!(
                terminal.local_modes.contains(LocalModes::ICANON),
                "{name}: the terminal is left raw"
            );
            assert_eq!(status.signal(), Some(SIGINT), "{name}: {status}");
        }
    }

    #[test]
    fn a_signal_ignored_at_start_stays_ignored_on_the_screen() {
        use rustix::termios::LocalModes;

        let (status, terminal) = in_child("ignored_sigterm_on_the_screen");
        assert!(status.success(), "{status}");
        assert!(terminal.local_modes.contains(LocalModes::ICANON));
    }

    /// Run on its own by
    /// [`a_panic_caught_on_another_thread_leaves_the_screen_open`]: opens a
    /// screen on standard output, and another thread panics and catches
    /// it, as a load in the background does; then the screen writes on.
    #[test]
    #[ignore = "run by a_panic_caught_on_another_thread_leaves_the_screen_open"]
    fn caught_panic_beside_a_screen() {
        use std::io::Write;

        let (_terminal, tty) = pty();
        let mut screen =
            Screen::on(tty, std::io::stdout(), Signals::none()).expect("the screen opens");
        screen.catch_panics();
        let caught =
            std::thread::spawn(|| crate::panics::catch(|| panic!("[a load fails]")).is_err());
        assert!(caught.join().unwrap());
        screen.write_all(b"[still on the screen]").unwrap();
        screen.flush().unwrap();
    }

    /// Run on its own by
    /// [`a_panic_nobody_catches_on_another_thread_leaves_the_screen`]:
    /// opens a screen on standard output, and another thread panics.
    #[test]
    #[ignore = "run by a_panic_nobody_catches_on_another_thread_leaves_the_screen"]
    fn uncaught_panic_beside_a_screen() {
        use std::io::Write;

        let (_terminal, tty) = pty();
        let mut screen =
            Screen::on(tty, std::io::stdout(), Signals::none()).expect("the screen opens");
        screen.catch_panics();
        let thread = std::thread::spawn(|| panic!("[a thread fails]"));
        assert!(thread.join().is_err());
        screen.write_all(b"[after the thread]").unwrap();
        screen.flush().unwrap();
    }

    #[test]
    fn a_panic_nobody_catches_on_another_thread_leaves_the_screen() {
        let output = child_output("term::screen::tests::uncaught_panic_beside_a_screen");
        let leave = output.find("\x1b[?1049l").expect("the screen is left");
        let message = output
            .find("[a thread fails]")
            .expect("the message is written");
        assert!(leave < message, "{output:?}");
    }

    #[test]
    fn a_panic_caught_on_another_thread_leaves_the_screen_open() {
        let output = child_output("term::screen::tests::caught_panic_beside_a_screen");
        let written = output
            .find("[still on the screen]")
            .expect("the screen writes on");
        let leave = output.find("\x1b[?1049l").expect("the screen is left");
        assert!(written < leave, "{output:?}");
        assert!(!output.contains("[a load fails]"), "{output:?}");
    }

    #[test]
    fn a_panic_message_comes_after_the_screen() {
        let output = child_output("term::screen::tests::panic_on_a_screen");
        let leave = output.find("\x1b[?1049l").expect("the screen is left");
        let message = output
            .find("[the pager fails]")
            .expect("the message is written");
        assert!(leave < message, "{output:?}");
        assert_eq!(output.matches("\x1b[?1049l").count(), 1, "{output:?}");
        assert!(!output.contains("[part of a frame]"), "{output:?}");
    }

    /// All that the ignored test `test`, run on its own in a child, writes
    /// on standard output and standard error.
    fn child_output(test: &str) -> String {
        use std::process::{Command, Stdio};

        let (mut reader, writer) = std::io::pipe().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test])
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
        String::from_utf8_lossy(&output).into_owned()
    }
}
