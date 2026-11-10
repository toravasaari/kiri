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
    let screen = Screen::on(tty, Vec::new(), Signals::none(), false).expect("the screen opens");
    (screen, terminal, probe)
}

#[test]
fn the_screen_is_the_alternate_screen_in_raw_mode() {
    use rustix::termios::LocalModes;

    let (screen, _terminal, tty) = screen();
    // With focus reporting, which tells when a tmux pane comes back.
    assert_eq!(screen.out, b"\x1b[?1049h\x1b[?25l\x1b[?1004h");
    let modes = tcgetattr(&tty).unwrap().local_modes;
    assert!(!modes.contains(LocalModes::ICANON));
}

#[test]
fn the_screen_captures_the_mouse_only_when_asked() {
    let entered_and_left = |mouse| {
        let (_terminal, tty) = pty();
        let screen = Screen::on(tty, Vec::new(), Signals::none(), mouse).expect("the screen opens");
        let mut left = Vec::new();
        screen.leave.run(&mut left);
        (screen.out.clone(), left)
    };
    let (entered, left) = entered_and_left(true);
    assert_eq!(entered, [ENTER_SCREEN, b"\x1b[?1000h\x1b[?1006h"].concat());
    assert_eq!(left, [b"\x1b[?1006l\x1b[?1000l", LEAVE_SCREEN].concat());
    let (entered, left) = entered_and_left(false);
    assert_eq!(entered, ENTER_SCREEN);
    assert_eq!(left, LEAVE_SCREEN);
}

#[test]
fn the_screen_ends_when_a_panic_unwinds() {
    let (written, restored, original) = panic_on_the_screen(false);
    let leave = b"\x1b[?1004l\x1b[?25h\x1b[?1049l";
    assert!(written.ends_with(leave), "{written:?}");
    assert_eq!(restored.local_modes, original.local_modes);
}

#[test]
fn a_panic_turns_the_mouse_off_before_leaving_the_screen() {
    let (written, restored, original) = panic_on_the_screen(true);
    assert!(
        written.ends_with(&[MOUSE_OFF, LEAVE_SCREEN].concat()),
        "{written:?}"
    );
    assert_eq!(restored.local_modes, original.local_modes);
}

/// What a screen on a pseudo-terminal, with the mouse on if `mouse`,
/// writes when a panic unwinds through it, and the terminal's modes after
/// and before.
fn panic_on_the_screen(mouse: bool) -> (Vec<u8>, Termios, Termios) {
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
            let _screen = Screen::on(tty, out, Signals::none(), mouse).expect("the screen opens");
            panic!("the pager fails");
        }
    });
    assert!(result.is_err());
    let written = out.0.lock().unwrap().clone();
    (written, tcgetattr(&tty).unwrap(), original)
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
        Screen::on(tty, std::io::stdout(), Signals::none(), false).expect("the screen opens");
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
        Screen::catching_on(tty, RaiseOnEnter(SIGTERM), false, || {}).expect("the screen opens");
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
    let screen = Screen::catching_on(tty, Vec::new(), false, || {}).expect("the screen opens");
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
    let screen = Screen::catching_on(tty, RaiseOnEnter(SIGTERM), false, || drop(interrupt))
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
    let screen = Screen::catching_on(tty, FailOnEnter(None), false, || drop(interrupt));
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
    let screen = Screen::catching_on(tty, FailOnEnter(Some(SIGINT)), false, || {});
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
    let path = rustix::termios::ttyname(&tty, Vec::new())
        .unwrap()
        .into_string()
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
    let screen = Screen::catching_on(tty, Vec::new(), false, || {}).expect("the screen opens");
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

/// Run on its own by [`in_child`]: opens a screen on the terminal
/// `KIRI_TEST_PTY` names, and gets SIGINT while the work after closing
/// runs, which leaves a mark in `KIRI_TEST_MARK`.
#[test]
#[ignore = "run by in_child"]
fn sigint_during_the_work_after_the_screen_closes() {
    let tty = File::options()
        .read(true)
        .write(true)
        .open(std::env::var_os(PTY).unwrap())
        .unwrap();
    let screen = Screen::catching_on(tty, Vec::new(), false, || {}).expect("the screen opens");
    screen.close_then(|| {
        signal_hook::low_level::raise(SIGINT).unwrap();
        std::fs::write(std::env::var_os(MARK).unwrap(), "after").unwrap();
    });
}

#[test]
fn a_signal_during_the_work_after_closing_ends_kiri() {
    use std::os::unix::process::ExitStatusExt;

    let scratch = crate::testing::Scratch::new("screen-close-then-during");
    let mark = scratch.join("mark");
    let (status, _) = in_child_marking("sigint_during_the_work_after_the_screen_closes", &mark);
    assert_eq!(status.signal(), Some(SIGINT), "{status}");
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
        Screen::on(tty, std::io::stdout(), Signals::none(), false).expect("the screen opens");
    screen.catch_panics();
    let caught = std::thread::spawn(|| crate::panics::catch(|| panic!("[a load fails]")).is_err());
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
        Screen::on(tty, std::io::stdout(), Signals::none(), false).expect("the screen opens");
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

/// A pager frame with several images, written through a screen whose output
/// wraps graphics for tmux, comes out as one passthrough: the sends of the
/// frame are consecutive and the screen writes the frame at once.
#[test]
fn the_images_of_a_frame_go_through_tmux_in_one_passthrough() {
    use std::sync::{Arc, Mutex};

    use crate::present::encoder::Encoder;
    use crate::present::output::{Page, Writer};
    use crate::present::presenter::Presenter;
    use crate::present::scene::{Scene, Surface};
    use crate::term::tmux::Wrapping;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::images::size::{Placement, Shows, Size};
    use crate::typeset::images::{ImageKey, Images, Source};
    use crate::typeset::style::{Line, Span, Style};

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

    let mut images = Images::new();
    let mut lines = Vec::new();
    for n in 0..4u8 {
        let url = format!("{n}.png");
        let pixels = image::RgbaImage::from_pixel(40, 20, image::Rgba([n, 9, 9, 255]));
        images.insert(url.as_str(), Ok(Source::Raster(pixels)));
        let size = Size {
            width: 40,
            height: 20,
            columns: 4,
            rows: 1,
        };
        let key = ImageKey::from(url.as_str());
        lines.push(Line::new(vec![Span::new("text", Style::default())]));
        lines.push(Line::image(
            std::rc::Rc::new(Placement::block(key, size, Shows::Image)),
            0,
        ));
    }
    let caps = Capabilities {
        cell_pixels: Some((10, 20)),
        tmux_passthrough: true,
        ..Capabilities::kitty(20)
    };
    let out = Shared(Arc::new(Mutex::new(Vec::new())));
    let (_terminal, tty) = pty();
    let mut screen = Screen::on(
        tty,
        Wrapping::new(out.clone(), true),
        Signals::none(),
        false,
    )
    .expect("the screen opens");
    out.0.lock().unwrap().clear();
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
    let scene = Scene {
        window: (20, lines.len() as u16),
        surfaces: vec![Surface::Lines {
            lines: &lines,
            dim: false,
        }],
    };
    Presenter::default()
        .present(&scene, &mut writer, &mut screen)
        .unwrap();
    screen.flush().unwrap();
    let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
    assert_eq!(text.matches("a=T").count(), 4, "{text:?}");
    assert_eq!(text.matches("\x1bPtmux;").count(), 1, "{text:?}");
    // The passthrough is before the clear, and well under the budget.
    let close = text.find("\x1b\\\x1b[").unwrap_or(text.len());
    assert!(close < text.find("\x1b[2J").unwrap(), "{text:?}");
    assert!(close < 512 * 1024, "{close}");
}
