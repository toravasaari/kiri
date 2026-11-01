//! The pager end to end: kiri in a pseudo-terminal, with keys typed into
//! it.

use std::io::{Read, Write};
use std::os::fd::BorrowedFd;
use std::os::unix::process::CommandExt;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use rustix::process::Resource;

use crate::common::{Scratch, command, fixtures, read_terminal, read_terminal_quiet, text};

/// The alternate screen, which the pager draws on.
const ENTER: &str = "\x1b[?1049h";
const LEAVE: &str = "\x1b[?1049l";

/// Mouse reporting with SGR encoding, which the pager turns on when it
/// opens and off when it closes.
const MOUSE_ON: &str = "\x1b[?1000h\x1b[?1006h";
const MOUSE_OFF: &str = "\x1b[?1006l\x1b[?1000l";

/// A document of `count` paragraphs, one row each.
fn paragraphs(count: usize) -> String {
    (1..=count).map(|n| format!("Line {n}\n\n")).collect()
}

/// Runs kiri with `args` in a terminal of 40 × `rows` cells, in a scratch
/// directory called `name` that holds `document.md` of 3 rows and `long.md`
/// of [`long`]. Pipes `stdin` in when given, and types each of `keys` after
/// a frame of the pager, the first after the first frame. `--mode ansi`
/// unless `args` choose a mode. Returns how kiri exited and everything it
/// wrote to the terminal.
fn in_terminal(
    name: &str,
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    keys: &[&[u8]],
) -> (ExitStatus, String) {
    let mut typed = 0;
    in_terminal_with(name, args, rows, stdin, |frames, terminal, _| {
        if typed < frames.min(keys.len()) {
            terminal.write_all(keys[typed]).unwrap();
            typed += 1;
        }
    })
}

/// [`in_terminal`] with `act` called after each read of the terminal,
/// with the number of frames drawn so far, the terminal side and kiri's
/// process.
fn in_terminal_with(
    name: &str,
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    act: impl FnMut(usize, &mut std::fs::File, &std::process::Child),
) -> (ExitStatus, String) {
    in_terminal_after(name, |_| {}, args, rows, stdin, act)
}

/// [`in_terminal_with`], with `prepare` called with the scratch directory
/// before kiri starts.
fn in_terminal_after(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    act: impl FnMut(usize, &mut std::fs::File, &std::process::Child),
) -> (ExitStatus, String) {
    let (status, seen, stderr) = in_terminal_told(name, prepare, args, rows, stdin, act);
    assert_eq!(stderr, "");
    (status, seen)
}

/// [`in_terminal_after`], with what kiri wrote to standard error returned
/// last instead of expected to be empty.
fn in_terminal_told(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    act: impl FnMut(usize, &mut std::fs::File, &std::process::Child),
) -> (ExitStatus, String, String) {
    in_terminal_run(name, prepare, args, rows, stdin, Setup::default(), act)
}

/// [`in_terminal_after`], with what kiri writes to standard error also on
/// the terminal, among what it writes to standard output.
fn in_terminal_both(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    rows: u16,
    act: impl FnMut(usize, &mut std::fs::File, &std::process::Child),
) -> (ExitStatus, String) {
    let setup = Setup {
        both: true,
        ..Setup::default()
    };
    let (status, seen, _) = in_terminal_run(name, prepare, args, rows, None, setup, act);
    (status, seen)
}

/// How [`in_terminal_run`] sets kiri up besides its arguments.
#[derive(Default)]
struct Setup<'a> {
    /// Standard error on the terminal too; what kiri writes to it is then
    /// on the terminal instead of returned.
    both: bool,
    /// Environment variables set for kiri.
    env: &'a [(&'a str, &'a str)],
    /// Done on a thread of its own once kiri has started, with the
    /// terminal side and the scratch directory, whether or not kiri
    /// writes anything.
    meanwhile: Option<fn(&mut std::fs::File, &std::path::Path)>,
    /// How long the terminal stays silent before `act` is called again
    /// with nothing new, for a test that waits for nothing to happen.
    quiet: Option<Duration>,
}

/// [`in_terminal_told`], set up as `setup` says.
fn in_terminal_run(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    setup: Setup,
    mut act: impl FnMut(usize, &mut std::fs::File, &std::process::Child),
) -> (ExitStatus, String, String) {
    let act = |seen: &[u8], terminal: &mut std::fs::File, child: &std::process::Child| {
        let frames = text(seen).matches("\x1b[?2026l").count();
        act(frames, terminal, child);
    };
    in_terminal_seeing(name, prepare, args, rows, stdin, setup, act)
}

/// [`in_terminal_run`], with `act` given everything kiri wrote to the
/// terminal so far instead of the number of frames.
fn in_terminal_seeing(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    setup: Setup,
    act: impl FnMut(&[u8], &mut std::fs::File, &std::process::Child),
) -> (ExitStatus, String, String) {
    let dir = Scratch::new(name);
    std::fs::write(dir.join("document.md"), paragraphs(3)).unwrap();
    std::fs::write(dir.join("long.md"), long()).unwrap();
    prepare(&dir);
    let (mut terminal, tty) = crate::common::pty();
    let window = rustix::termios::Winsize {
        ws_col: 40,
        ws_row: rows,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    rustix::termios::tcsetwinsize(&tty, window).unwrap();
    let mut kiri = command(&dir);
    if !args.contains(&"--mode") {
        kiri.args(["--mode", "ansi"]);
    }
    kiri.current_dir(&*dir)
        .envs(setup.env.iter().copied())
        .args(args)
        .stdin(match stdin {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        })
        .stderr(match setup.both {
            true => Stdio::from(tty.try_clone().unwrap()),
            false => Stdio::piped(),
        })
        .stdout(Stdio::from(tty));
    // SAFETY: setsid, ioctl, getrlimit and setrlimit are async-signal-safe
    // and touch no memory.
    unsafe {
        kiri.pre_exec(|| {
            rustix::process::setsid()?;
            // Standard output is the terminal by now.
            rustix::process::ioctl_tiocsctty(BorrowedFd::borrow_raw(1))?;
            // No core dump when a test ends kiri with SIGQUIT.
            let mut core = rustix::process::getrlimit(Resource::Core);
            core.current = Some(0);
            rustix::process::setrlimit(Resource::Core, core)?;
            Ok(())
        });
    }
    let mut child = kiri.spawn().unwrap();
    // The command holds the program's side of the terminal. Close it, so
    // that reading the terminal side ends once kiri exits.
    drop(kiri);
    if let Some(meanwhile) = setup.meanwhile {
        let mut terminal = terminal.try_clone().unwrap();
        let dir = dir.to_path_buf();
        std::thread::spawn(move || meanwhile(&mut terminal, &dir));
    }
    if let Some(document) = stdin {
        let mut input = child.stdin.take().unwrap();
        input.write_all(document.as_bytes()).unwrap();
    }
    // kiri gets 10 s at most.
    let limit = Duration::from_secs(10);
    let seen = read_terminal_quiet(&mut terminal, &mut child, limit, setup.quiet, act);
    let status = child.wait().unwrap();
    let mut stderr = String::new();
    if let Some(mut told) = child.stderr.take() {
        told.read_to_string(&mut stderr).unwrap();
    }
    (status, text(&seen), stderr)
}

/// A document longer than a window of 5 rows.
fn long() -> String {
    paragraphs(10)
}

#[test]
fn a_long_document_is_paged_until_q() {
    let (status, seen) = in_terminal("pager-long", &["long.md"], 5, None, &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(seen.ends_with(LEAVE), "{seen:?}");
    // The bottom row is the status line's.
    assert!(seen.contains("Line 2"), "{seen:?}");
    assert!(!seen.contains("Line 3"), "{seen:?}");
    assert!(seen.contains("\x1b[5;1H\x1b[0;1m kiri "), "{seen:?}");
    assert!(seen.contains(" long.md "), "{seen:?}");
}

#[test]
fn the_pager_captures_the_mouse_and_scrolls_on_the_wheel() {
    let wheel = b"\x1b[<65;1;1M";
    let (status, seen) = in_terminal("pager-mouse", &["long.md"], 5, None, &[wheel, b"q"]);
    assert!(status.success());
    let opened = &seen[..seen.find("\x1b[?2026h").expect("a frame")];
    assert!(opened.contains(ENTER), "{seen:?}");
    assert!(opened.contains(MOUSE_ON), "{seen:?}");
    let closed = &seen[seen.rfind("\x1b[?2026l").expect("a frame")..];
    let off = closed.find(MOUSE_OFF).expect("mouse reporting off");
    assert!(off < closed.find(LEAVE).unwrap(), "{seen:?}");
    // Three lines down: Line 1, the blank line after it and Line 2.
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    assert_eq!(frames.len(), 2, "{seen:?}");
    assert!(!frames[0].contains("Line 4"), "{seen:?}");
    assert!(frames[1].contains("Line 4"), "{seen:?}");
    assert!(!frames[1].contains("Line 2"), "{seen:?}");
}

#[test]
fn no_mouse_leaves_the_mouse_to_the_terminal() {
    let args = &["--no-mouse", "long.md"];
    let (status, seen) = in_terminal("pager-no-mouse", args, 5, None, &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(!seen.contains("\x1b[?1000"), "{seen:?}");
    assert!(!seen.contains("\x1b[?1006"), "{seen:?}");
}

#[test]
fn a_printed_document_asks_for_no_mouse_events() {
    let (status, seen) = in_terminal("pager-print-mouse", &["document.md"], 24, None, &[]);
    assert!(status.success());
    assert!(seen.contains("Line 3"), "{seen:?}");
    assert!(!seen.contains("\x1b[?1000"), "{seen:?}");
}

#[test]
fn ctrl_c_quits_and_leaves_the_screen() {
    let (status, seen) = in_terminal("pager-ctrl-c", &["long.md"], 5, None, &[b"\x03"]);
    assert!(status.success());
    assert!(seen.ends_with(LEAVE), "{seen:?}");
}

#[test]
fn a_pipe_gets_an_image_file_printed_even_with_p() {
    let dir = Scratch::new("pager-pipe-image");
    let icon = fixtures().join("images/icon.png");
    let out = crate::common::kiri_on(&dir, &icon, &["-p", "--mode", "kitty"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(!stdout.contains(ENTER), "{stdout:?}");
    assert_eq!(stdout.matches("\x1b_Ga=T,").count(), 1, "{stdout:?}");
    let without = crate::common::kiri_on(&dir, &icon, &["--mode", "kitty"]);
    assert_eq!(stdout, text(&without.stdout));
}

#[test]
fn the_doctor_report_is_paged_only_when_paging_is_always() {
    let (status, seen) = in_terminal("pager-doctor-auto", &["--doctor"], 5, None, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert!(seen.contains("Settings"), "{seen:?}");
    let (status, seen) = in_terminal("pager-doctor-p", &["--doctor", "-p"], 5, None, &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    let raw = ["--doctor", "--raw", "-p"];
    let (status, seen) = in_terminal("pager-doctor-raw", &raw, 5, None, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
}

/// Runs kiri with `args` in a scratch directory called `name`, after
/// `prepare` is called with it, with standard output a terminal of 40 × 5
/// cells of 10 × 20 pixels but no terminal to read keys from. Returns how
/// kiri exited, everything it wrote to the terminal and what it wrote to
/// standard error.
fn without_keys(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
) -> (ExitStatus, String, String) {
    let dir = Scratch::new(name);
    prepare(&dir);
    let (mut terminal, tty) = crate::common::pty();
    let window = rustix::termios::Winsize {
        ws_col: 40,
        ws_row: 5,
        ws_xpixel: 400,
        ws_ypixel: 100,
    };
    rustix::termios::tcsetwinsize(&tty, window).unwrap();
    let mut kiri = command(&dir);
    kiri.current_dir(&*dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(tty))
        .stderr(Stdio::piped());
    // SAFETY: setsid is async-signal-safe and touches no memory.
    unsafe {
        // A session of its own and no controlling terminal: standard
        // output is a terminal, but there is none to read keys from.
        kiri.pre_exec(|| {
            rustix::process::setsid()?;
            Ok(())
        });
    }
    let mut child = kiri.spawn().unwrap();
    drop(kiri);
    let output = read_terminal(
        &mut terminal,
        &mut child,
        Duration::from_secs(10),
        |_, _, _| {},
    );
    let status = child.wait().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    (status, text(&output), stderr)
}

#[test]
fn without_a_terminal_for_keys_p_prints_an_image_file_as_without_it() {
    // Larger than the window, so that the window decides its size.
    let prepare = |dir: &std::path::Path| {
        image::RgbaImage::from_pixel(800, 400, image::Rgba([255, 0, 0, 255]))
            .save(dir.join("a.png"))
            .unwrap();
    };
    let args = ["--mode", "kitty", "-p", "a.png"];
    let (status, seen, stderr) = without_keys("pager-no-keys-image", prepare, &args);
    assert!(status.success(), "{stderr}");
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    assert_eq!(
        stderr,
        "kiri: no terminal to read keys from, so the document is printed\n"
    );
    let args = ["--mode", "kitty", "a.png"];
    let (status, printed, stderr) = without_keys("pager-no-keys-image-auto", prepare, &args);
    assert!(status.success(), "{stderr}");
    assert_eq!(seen, printed);
}

#[test]
fn a_signal_to_end_kiri_leaves_the_screen_first() {
    use std::os::unix::process::ExitStatusExt;

    use rustix::process::{Pid, Signal, kill_process};

    for signal in [Signal::TERM, Signal::HUP, Signal::INT, Signal::QUIT] {
        let mut sent = false;
        let (status, seen) =
            in_terminal_with("pager-signal", &["long.md"], 5, None, |frames, _, child| {
                if frames > 0 && !sent {
                    let pid = Pid::from_raw(child.id() as i32).unwrap();
                    kill_process(pid, signal).unwrap();
                    sent = true;
                }
            });
        assert!(seen.ends_with(LEAVE), "{signal:?}: {seen:?}");
        // The mouse goes off before the screen is left.
        let closed = &seen[seen.rfind("\x1b[?2026l").expect("a frame")..];
        let off = closed.find(MOUSE_OFF).expect("mouse reporting off");
        assert!(off < closed.find(LEAVE).unwrap(), "{signal:?}: {seen:?}");
        assert_eq!(status.signal(), Some(signal.as_raw()), "{signal:?}");
    }
}

#[test]
fn a_signal_after_a_program_drew_the_diagrams_leaves_the_screen_first() {
    use std::os::unix::process::ExitStatusExt;

    use rustix::process::{Pid, Signal, kill_process};

    // kiri catches Ctrl-C while the program draws, and the pager catches
    // it once the document shows.
    let prepare = |dir: &std::path::Path| {
        let script = dir.join("render.sh");
        std::fs::write(
            &script,
            "echo '<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"20\"/>' > \"$2\"\n",
        )
        .unwrap();
        let config = format!(
            "[diagrams.mermaid]\ncommand = [\"/bin/sh\", \"{}\", \"{{input}}\", \"{{output}}\"]\n",
            script.display()
        );
        crate::common::write(&dir.join("config/kiri/config.toml"), &config);
        let document = format!("```mermaid\nflowchart TD\n```\n\n{}", long());
        std::fs::write(dir.join("diagram.md"), document).unwrap();
    };
    let mut sent = false;
    let (status, seen) = in_terminal_after(
        "pager-signal-diagrams",
        prepare,
        &["--mode", "kitty", "diagram.md"],
        5,
        None,
        |frames, _, child| {
            if frames > 0 && !sent {
                let pid = Pid::from_raw(child.id() as i32).unwrap();
                kill_process(pid, Signal::INT).unwrap();
                sent = true;
            }
        },
    );
    assert!(seen.contains("\x1b_G"), "the diagram is drawn: {seen:?}");
    assert!(seen.ends_with(LEAVE), "{seen:?}");
    assert_eq!(status.signal(), Some(Signal::INT.as_raw()));
}

#[test]
fn the_pager_shows_why_a_diagram_was_not_drawn_and_finds_it() {
    let prepare = |dir: &std::path::Path| {
        let script = dir.join("render.sh");
        std::fs::write(&script, "echo 'Error: bad' >&2\nexit 1\n").unwrap();
        let config = format!(
            "render_errors = \"inline\"\n\
             [diagrams.mermaid]\ncommand = [\"/bin/sh\", \"{}\", \"{{input}}\", \"{{output}}\"]\n",
            script.display()
        );
        crate::common::write(&dir.join("config/kiri/config.toml"), &config);
        let document = format!("{}```mermaid\nflowchart TD\n```\n", long());
        std::fs::write(dir.join("diagram.md"), document).unwrap();
    };
    let keys: [&[u8]; 8] = [b"/", b"d", b"r", b"a", b"w", b"n", b"\r", b"q"];
    let mut typed = 0;
    let (status, seen) = in_terminal_after(
        "pager-render-error",
        prepare,
        &["--mode", "kitty", "diagram.md"],
        7,
        None,
        |frames, terminal, _| {
            if typed < frames.min(keys.len()) {
                terminal.write_all(keys[typed]).unwrap();
                typed += 1;
            }
        },
    );
    assert!(status.success());
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    // Below the end of the first page, and shown once found.
    assert!(!frames[0].contains("could not be"), "{seen:?}");
    let found = frames.last().unwrap();
    assert!(found.contains("✗ mermaid diagram could not be"), "{seen:?}");
    assert!(found.contains("/drawn 1/1"), "{seen:?}");
}

#[test]
fn a_file_that_cannot_be_shown_is_told_at_its_place_when_the_files_fit() {
    let prepare = |dir: &std::path::Path| {
        std::fs::write(dir.join("a.md"), "Alpha text.\n").unwrap();
        std::fs::write(dir.join("b.md"), "Beta text.\n").unwrap();
    };
    let args = ["a.md", "missing.md", "b.md"];
    let (status, seen) = in_terminal_both("pager-missing-fits", prepare, &args, 24, |_, _, _| {});
    assert_eq!(status.code(), Some(1), "{seen:?}");
    assert!(!seen.contains(ENTER), "{seen:?}");
    // As printed: told once, between the files around it, and not shown.
    let mut rest = seen.as_str();
    for part in ["Alpha text.", "kiri: missing.md: ", "b.md", "Beta text."] {
        let at = rest
            .find(part)
            .unwrap_or_else(|| panic!("{part}: {seen:?}"));
        rest = &rest[at + part.len()..];
    }
    assert_eq!(seen.matches("missing.md").count(), 1, "{seen:?}");
    assert!(!seen.contains('✗'), "{seen:?}");
}

#[test]
fn a_front_matter_warning_is_told_at_its_place_when_the_files_fit() {
    let invalid = std::fs::read_to_string(fixtures().join("front-matter-invalid.md")).unwrap();
    let prepare = |dir: &std::path::Path| {
        std::fs::write(dir.join("a.md"), "Alpha text.\n").unwrap();
        std::fs::write(dir.join("c.md"), invalid).unwrap();
        std::fs::write(dir.join("b.md"), "Beta text.\n").unwrap();
    };
    let args = ["a.md", "c.md", "b.md"];
    let (status, seen) = in_terminal_both("pager-warning-fits", prepare, &args, 40, |_, _, _| {});
    assert_eq!(status.code(), Some(0), "{seen:?}");
    assert!(!seen.contains(ENTER), "{seen:?}");
    // As printed: the warning comes after the file before it and before
    // its own file's header.
    let mut rest = seen.as_str();
    for part in ["Alpha text.", "kiri: c.md: front matter", "c.md", "b.md"] {
        let at = rest
            .find(part)
            .unwrap_or_else(|| panic!("{part}: {seen:?}"));
        rest = &rest[at + part.len()..];
    }
}

/// [`in_terminal`] in a window of 5 rows, with copies of an image beside
/// the documents, `a.png` and `b.png`.
fn with_images(name: &str, args: &[&str], keys: &[&[u8]]) -> (ExitStatus, String) {
    let icon = crate::common::fixtures().join("images/icon.png");
    let prepare = |dir: &std::path::Path| {
        for name in ["a.png", "b.png"] {
            std::fs::copy(&icon, dir.join(name)).unwrap();
        }
    };
    let mut typed = 0;
    in_terminal_after(name, prepare, args, 5, None, |frames, terminal, _| {
        if typed < frames.min(keys.len()) {
            terminal.write_all(keys[typed]).unwrap();
            typed += 1;
        }
    })
}

#[test]
fn image_files_alone_are_paged_only_when_paging_is_always() {
    let args = ["--mode", "kitty", "a.png", "b.png"];
    let (status, seen) = with_images("pager-images", &args, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 2, "{seen:?}");
    let args = ["--mode", "kitty", "-p", "a.png", "b.png"];
    let (status, seen) = with_images("pager-images-p", &args, &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(seen.contains("a.png"), "{seen:?}");
}

#[test]
fn image_files_with_a_document_are_paged_when_they_are_long() {
    let args = ["--mode", "kitty", "a.png", "document.md", "b.png"];
    let (status, seen) = with_images("pager-images-document", &args, &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
}

#[test]
fn an_image_file_on_its_own_is_paged_only_when_paging_is_always() {
    let args = ["--mode", "kitty", "a.png"];
    let (status, seen) = with_images("pager-image", &args, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    let args = ["--mode", "kitty", "-p", "a.png"];
    let (status, seen) = with_images("pager-image-p", &args, &[b"r", b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(seen.ends_with(LEAVE), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    // The status line of a file on its own, without its place among files.
    assert!(text_of(frames[0]).contains(" a.png "), "{seen:?}");
    assert!(!text_of(frames[0]).contains("1/1"), "{seen:?}");
    assert!(text_of(&seen).contains("nothing to reload"), "{seen:?}");
}

#[test]
fn paging_always_in_the_configuration_pages_an_image_file_on_its_own() {
    let prepare = |dir: &std::path::Path| {
        std::fs::copy(fixtures().join("images/icon.png"), dir.join("a.png")).unwrap();
        crate::common::write(
            &dir.join("config/kiri/config.toml"),
            "paging = \"always\"\n",
        );
    };
    let (status, seen) = in_terminal_after(
        "pager-image-config",
        prepare,
        &["--mode", "kitty", "a.png"],
        5,
        None,
        |frames, terminal, _| {
            if frames == 1 {
                terminal.write_all(b"q").unwrap();
            }
        },
    );
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
}

#[test]
fn p_pages_an_image_from_standard_input() {
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"20\"/>\n";
    let args = ["--mode", "kitty", "--format", "svg"];
    let (status, seen) = in_terminal("pager-image-stdin", &args, 5, Some(svg), &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    let args = ["--mode", "kitty", "--format", "svg", "-p"];
    let (status, seen) = in_terminal("pager-image-stdin-p", &args, 5, Some(svg), &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(text_of(&seen).contains(" <stdin> "), "{seen:?}");
}

/// Writes a configuration in `dir` whose program draws every Mermaid
/// diagram as a square SVG, and `diagram.mmd`.
fn drawn_diagram(dir: &std::path::Path) {
    let script = dir.join("render.sh");
    std::fs::write(
        &script,
        "echo '<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"20\"/>' > \"$2\"\n",
    )
    .unwrap();
    let config = format!(
        "[diagrams.mermaid]\ncommand = [\"/bin/sh\", \"{}\", \"{{input}}\", \"{{output}}\"]\n",
        script.display()
    );
    crate::common::write(&dir.join("config/kiri/config.toml"), &config);
    std::fs::write(dir.join("diagram.mmd"), "flowchart TD\n").unwrap();
}

#[test]
fn a_diagram_file_on_its_own_is_paged_only_when_paging_is_always() {
    let args = ["--mode", "kitty", "diagram.mmd"];
    let (status, seen) = in_steps_with("pager-diagram", drawn_diagram, &args, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    let reload = Step {
        frames: 1,
        keys: &|_| b"r",
    };
    let steps = [reload, quit_after(2)];
    let args = ["--mode", "kitty", "-p", "diagram.mmd"];
    let (status, seen) = in_steps_with("pager-diagram-p", drawn_diagram, &args, &steps);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(seen.ends_with(LEAVE), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    assert!(text_of(frames[0]).contains(" diagram.mmd "), "{seen:?}");
    assert!(!text_of(frames[0]).contains("1/1"), "{seen:?}");
    // Not the diagram's code: it is drawn.
    assert!(!text_of(frames[0]).contains("flowchart"), "{seen:?}");
    assert!(text_of(&seen).contains("nothing to reload"), "{seen:?}");
}

/// A configuration whose Mermaid command draws a square SVG once `go` is
/// in the scratch directory `dir`, or after 8 s, and `diagram.md`, a
/// diagram and a line of text.
fn drawn_on_cue(dir: &std::path::Path) {
    let script = dir.join("render.sh");
    let go = dir.join("go");
    let started = dir.join("started");
    let draw = format!(
        "echo started > '{}'\n\
         i=0\n\
         while [ ! -f '{}' ] && [ $i -lt 160 ]; do sleep 0.05; i=$((i + 1)); done\n\
         echo '<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"20\"/>' > \"$2\"\n",
        started.display(),
        go.display()
    );
    std::fs::write(&script, draw).unwrap();
    let config = format!(
        "[diagrams.mermaid]\ncommand = [\"/bin/sh\", \"{}\", \"{{input}}\", \"{{output}}\"]\n",
        script.display()
    );
    crate::common::write(&dir.join("config/kiri/config.toml"), &config);
    let document = "```mermaid\nflowchart TD\n```\n\nBelow\n";
    std::fs::write(dir.join("diagram.md"), document).unwrap();
}

/// kiri with `args` and `env`, and `stdin` piped in when given, in a
/// terminal of 12 rows, in a scratch directory that `prepare` fills: `go`
/// is written there once the first frame is drawn, and `q` typed once
/// `done` holds for the frames drawn whole and the directory. kiri must
/// succeed. Returns the frames and what kiri wrote on standard error.
fn quit_when(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    stdin: Option<&str>,
    env: &[(&str, &str)],
    done: impl Fn(&[String], &std::path::Path) -> bool,
) -> (Vec<String>, String) {
    let dir = std::cell::RefCell::new(std::path::PathBuf::new());
    let prepare = |scratch: &std::path::Path| {
        *dir.borrow_mut() = scratch.to_owned();
        prepare(scratch);
    };
    let (mut go, mut quit) = (false, false);
    let act = |seen: &[u8], terminal: &mut std::fs::File, _: &std::process::Child| {
        let frames = whole_frames(&text(seen));
        if !go && !frames.is_empty() {
            std::fs::write(dir.borrow().join("go"), "").unwrap();
            go = true;
        }
        if !quit && done(&frames, &dir.borrow()) {
            terminal.write_all(b"q").unwrap();
            quit = true;
        }
    };
    let setup = Setup {
        both: false,
        env,
        ..Setup::default()
    };
    let (status, seen, stderr) = in_terminal_seeing(name, prepare, args, 12, stdin, setup, act);
    assert!(status.success(), "{stderr}");
    (whole_frames(&seen), stderr)
}

/// The frames in `seen` that were drawn to their end.
fn whole_frames(seen: &str) -> Vec<String> {
    (seen.split("\x1b[?2026h").skip(1))
        .filter(|frame| frame.contains("\x1b[?2026l"))
        .map(str::to_owned)
        .collect()
}

/// Whether one of `frames` sent an image.
fn image_sent(frames: &[String], _: &std::path::Path) -> bool {
    frames.iter().any(|frame| frame.contains("\x1b_Ga=T,"))
}

/// kiri on `diagram.md` of [`drawn_on_cue`] with `args` and `env` in a
/// terminal of 12 rows: the diagram is drawn once the first frame is, and
/// `q` typed once a frame with an image sent in it is. Returns the frames.
fn initial_load(name: &str, args: &[&str], env: &[(&str, &str)]) -> Vec<String> {
    let mut args = args.to_vec();
    args.extend(["--mode", "kitty", "diagram.md"]);
    let (frames, stderr) = quit_when(name, drawn_on_cue, &args, None, env, image_sent);
    assert_eq!(stderr, "");
    frames
}

#[test]
fn an_initial_load_in_the_background_shows_the_text_before_the_diagram() {
    let background = [("KIRI_INITIAL_LOAD", "background")];
    for (args, env) in [
        (&["--initial-load", "background", "-p"][..], &[][..]),
        (&["--initial-load", "background", "-f"], &[]),
        (&["-p"], &background),
    ] {
        let frames = initial_load("pager-initial-load", args, env);
        let first = &frames[0];
        // The diagram's code, with the band below it.
        assert!(
            text_of(first).contains("flowchart TD"),
            "{args:?}: {frames:?}"
        );
        assert!(
            text_of(first).contains("[rendering: 0/1]"),
            "{args:?}: {frames:?}"
        );
        assert!(first.contains("\x1b_Ga=t,"), "{args:?}: {frames:?}");
        assert!(!first.contains("\x1b_Ga=T,"), "{args:?}: {frames:?}");
        let drawn = frames.iter().find(|frame| frame.contains("\x1b_Ga=T,"));
        let drawn = text_of(drawn.unwrap());
        assert!(!drawn.contains("flowchart"), "{args:?}: {frames:?}");
        assert!(drawn.contains(" diagram.md "), "{args:?}: {frames:?}");
        assert!(!drawn.contains("[rendering"), "{args:?}: {frames:?}");
    }
}

#[test]
fn quitting_while_the_initial_load_draws_stops_the_program() {
    // The command notes its process and never draws.
    let prepare = |dir: &std::path::Path| {
        drawn_on_cue(dir);
        let pid = dir.join("pid");
        let draw = format!(
            "echo $$ > '{}.new'\nmv '{0}.new' '{0}'\nexec sleep 60\n",
            pid.display()
        );
        std::fs::write(dir.join("render.sh"), draw).unwrap();
    };
    // Once the program runs, with its process noted, as the scratch
    // directory goes with kiri.
    let pid = std::cell::Cell::new(None);
    let running = |frames: &[String], dir: &std::path::Path| {
        let noted = std::fs::read_to_string(dir.join("pid")).ok();
        pid.set(noted.and_then(|noted| noted.trim().parse().ok()));
        !frames.is_empty() && pid.get().is_some()
    };
    let args = [
        "--initial-load",
        "background",
        "-p",
        "--mode",
        "kitty",
        "diagram.md",
    ];
    let name = "pager-initial-load-quit";
    let (frames, stderr) = quit_when(name, prepare, &args, None, &[], running);
    assert_eq!(stderr, "");
    let last = text_of(frames.last().unwrap());
    assert!(last.contains("[rendering: 0/1]"), "{frames:?}");
    assert!(last.contains("flowchart TD"), "{frames:?}");
    let pid = rustix::process::Pid::from_raw(pid.get().unwrap()).unwrap();
    // Gone, once whoever takes it in has reaped it.
    let start = std::time::Instant::now();
    while rustix::process::test_kill_process(pid).is_ok() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{pid:?} still runs"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_initial_load_from_standard_input_goes_in_the_background_with_p() {
    let document = "```mermaid\nflowchart TD\n```\n\nBelow\n";
    let args = ["--initial-load", "background", "-p", "--mode", "kitty"];
    let name = "pager-initial-load-stdin";
    let (frames, stderr) = quit_when(name, drawn_on_cue, &args, Some(document), &[], image_sent);
    assert_eq!(stderr, "");
    let first = text_of(&frames[0]);
    assert!(first.contains("flowchart TD"), "{frames:?}");
    assert!(first.contains("[rendering: 0/1]"), "{frames:?}");
    assert!(!frames[0].contains("\x1b_Ga=T,"), "{frames:?}");
    let drawn = frames.iter().find(|frame| frame.contains("\x1b_Ga=T,"));
    assert!(
        !text_of(drawn.unwrap()).contains("[rendering"),
        "{frames:?}"
    );
}

#[test]
fn an_initial_load_in_the_background_with_nothing_to_load_shows_as_waiting() {
    // Images off, a terminal that shows none, and a document without any.
    for (args, file) in [
        (&["-I", "--mode", "kitty"][..], "diagram.md"),
        (&["--mode", "ansi"], "diagram.md"),
        (&["--mode", "kitty"], "long.md"),
    ] {
        let shown = |initial_load: &str| {
            let mut all = vec!["--initial-load", initial_load, "-p"];
            all.extend(args);
            all.push(file);
            let quit = |frames: &[String], _: &std::path::Path| !frames.is_empty();
            let name = "pager-initial-load-nothing";
            let (frames, stderr) = quit_when(name, drawn_on_cue, &all, None, &[], quit);
            assert_eq!(stderr, "", "{all:?}");
            frames[0].clone()
        };
        let background = shown("background");
        assert!(!text_of(&background).contains("[rendering"), "{args:?}");
        assert_eq!(background, shown("wait"), "{args:?}");
    }
}

/// [`drawn_on_cue`], with a command that fails instead of drawing, and
/// the reasons only on standard error.
fn failing_on_cue(dir: &std::path::Path) {
    drawn_on_cue(dir);
    let fail = format!(
        "i=0\n\
         while [ ! -f '{}' ] && [ $i -lt 160 ]; do sleep 0.05; i=$((i + 1)); done\n\
         echo 'Error: bad' >&2\nexit 1\n",
        dir.join("go").display()
    );
    std::fs::write(dir.join("render.sh"), fail).unwrap();
    let config = dir.join("config/kiri/config.toml");
    let config_text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(
        &config,
        format!("render_errors = \"stderr\"\n{config_text}"),
    )
    .unwrap();
}

/// What a test does once `when` holds for the frames drawn whole since
/// the stage before acted, and the scratch directory: `then` may change
/// the files there, and returns the keys to type.
struct Stage<'a> {
    when: &'a dyn Fn(&[String], &std::path::Path) -> bool,
    then: &'a dyn Fn(&std::path::Path) -> &'static [u8],
}

/// kiri with `args` in a terminal of 5 rows, in a scratch directory that
/// `prepare` fills, with `stages` done in order, and with standard error
/// on the terminal too when `both`. kiri must succeed. Returns what it
/// wrote on the terminal and on standard error.
fn in_stages(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    both: bool,
    stages: &[Stage],
) -> (String, String) {
    let dir = std::cell::RefCell::new(std::path::PathBuf::new());
    let prepare = |scratch: &std::path::Path| {
        *dir.borrow_mut() = scratch.to_owned();
        prepare(scratch);
    };
    let (mut done, mut since) = (0, 0);
    let act = |seen: &[u8], terminal: &mut std::fs::File, _: &std::process::Child| {
        let frames = whole_frames(&text(seen));
        if let Some(stage) = stages.get(done)
            && (stage.when)(&frames[since..], &dir.borrow())
        {
            terminal.write_all((stage.then)(&dir.borrow())).unwrap();
            (done, since) = (done + 1, frames.len());
        }
    };
    let setup = Setup {
        both,
        ..Setup::default()
    };
    let (status, seen, stderr) = in_terminal_seeing(name, prepare, args, 5, None, setup, act);
    assert!(status.success(), "{stderr}");
    (seen, stderr)
}

/// Whether one of `frames` shows nothing loading.
fn loaded(frames: &[String], _: &std::path::Path) -> bool {
    frames
        .iter()
        .any(|frame| !text_of(frame).contains("[rendering"))
}

/// Whether one of `frames` says the document was reloaded.
fn reloaded(frames: &[String], _: &std::path::Path) -> bool {
    frames
        .iter()
        .any(|frame| text_of(frame).contains("reloaded"))
}

/// What kiri on `args` with `--initial-load wait -p --mode kitty` writes
/// on standard error, quitting at once, in a scratch directory that
/// `prepare` fills.
fn told_waiting(name: &str, prepare: impl FnOnce(&std::path::Path), files: &[&str]) -> String {
    let args = [
        &["--initial-load", "wait", "-p", "--mode", "kitty"][..],
        files,
    ]
    .concat();
    let quit = |frames: &[String], _: &std::path::Path| !frames.is_empty();
    let (frames, stderr) = quit_when(name, prepare, &args, None, &[], quit);
    assert!(!text_of(&frames[0]).contains("[rendering"), "{frames:?}");
    assert_ne!(stderr, "");
    stderr
}

const BACKGROUND: [&str; 5] = ["--initial-load", "background", "-p", "--mode", "kitty"];

#[test]
fn an_initial_load_in_the_background_tells_a_failed_diagram_after_the_pager() {
    let quit = Stage {
        when: &loaded,
        then: &|_| b"q",
    };
    let go = Stage {
        when: &|frames, _| !frames.is_empty(),
        then: &|dir| {
            std::fs::write(dir.join("go"), "").unwrap();
            b""
        },
    };
    let args = [&BACKGROUND[..], &["diagram.md"]].concat();
    let name = "pager-initial-load-failed";
    let (seen, stderr) = in_stages(name, failing_on_cue, &args, false, &[go, quit]);
    let frames = whole_frames(&seen);
    let first = text_of(&frames[0]);
    assert!(first.contains("[rendering: 0/1]"), "{frames:?}");
    assert!(text_of(frames.last().unwrap()).contains("flowchart TD"));
    // The lines waiting writes before the pager opens.
    let at_once = |dir: &std::path::Path| {
        failing_on_cue(dir);
        std::fs::write(dir.join("go"), "").unwrap();
    };
    let waiting = told_waiting("pager-initial-load-failed-wait", at_once, &["diagram.md"]);
    assert!(waiting.contains("Error: bad"), "{waiting}");
    assert_eq!(stderr, waiting);
    // Once the pager has left its screen.
    let go = Stage {
        when: &|frames, _| !frames.is_empty(),
        then: &|dir| {
            std::fs::write(dir.join("go"), "").unwrap();
            b""
        },
    };
    let quit = Stage {
        when: &loaded,
        then: &|_| b"q",
    };
    let name = "pager-initial-load-failed-after";
    let (seen, _) = in_stages(name, failing_on_cue, &args, true, &[go, quit]);
    let (_, after) = seen.rsplit_once(LEAVE).unwrap();
    assert!(after.contains("Error: bad"), "{seen:?}");
}

/// What the progress line says where Space or Enter opens the pager.
const HINT: &str = "(space: open now)";

/// kiri with `args` in a terminal of 12 rows, with standard error on it
/// too, in a scratch directory that `prepare` fills, with `act` called
/// with what kiri wrote so far, the terminal and the directory each time
/// it writes. Returns how kiri exited and what it wrote.
fn watching(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    mut act: impl FnMut(&str, &mut std::fs::File, &std::path::Path),
) -> (ExitStatus, String) {
    let dir = std::cell::RefCell::new(std::path::PathBuf::new());
    let prepare = |scratch: &std::path::Path| {
        *dir.borrow_mut() = scratch.to_owned();
        prepare(scratch);
    };
    let act = |seen: &[u8], terminal: &mut std::fs::File, _: &std::process::Child| {
        act(&text(seen), terminal, &dir.borrow());
    };
    let setup = Setup {
        both: true,
        ..Setup::default()
    };
    let (status, seen, _) = in_terminal_seeing(name, prepare, args, 12, None, setup, act);
    (status, seen)
}

/// [`drawn_on_cue`] with `diagram.md` longer than a window of 12 rows.
fn long_on_cue(dir: &std::path::Path) {
    drawn_on_cue(dir);
    let document = format!("```mermaid\nflowchart TD\n```\n\n{}", long());
    std::fs::write(dir.join("diagram.md"), document).unwrap();
}

#[test]
fn space_or_enter_opens_the_pager_before_the_slow_diagram_is_drawn() {
    for key in [&b" "[..], b"\r"] {
        let (mut typed, mut go, mut quit) = (false, false, false);
        let act = |seen: &str, terminal: &mut std::fs::File, dir: &std::path::Path| {
            if !typed && seen.contains(HINT) {
                terminal.write_all(key).unwrap();
                typed = true;
            }
            let frames = whole_frames(seen);
            if !go && !frames.is_empty() {
                std::fs::write(dir.join("go"), "").unwrap();
                go = true;
            }
            if !quit && image_sent(&frames, dir) {
                terminal.write_all(b"q").unwrap();
                quit = true;
            }
        };
        let args = ["--mode", "kitty", "-p", "diagram.md"];
        let (status, seen) = watching("pager-skip", drawn_on_cue, &args, act);
        assert!(status.success(), "{key:?}: {seen:?}");
        let (before, pager) = seen.split_once(ENTER).unwrap();
        assert!(before.contains(HINT), "{key:?}: {seen:?}");
        let frames = whole_frames(pager);
        let first = text_of(&frames[0]);
        assert!(first.contains("flowchart TD"), "{key:?}: {frames:?}");
        assert!(first.contains("[rendering: 0/1]"), "{key:?}: {frames:?}");
        assert!(!frames[0].contains("\x1b_Ga=T,"), "{key:?}: {frames:?}");
        assert!(image_sent(&frames, std::path::Path::new("")), "{frames:?}");
    }
}

#[test]
fn space_opens_the_pager_for_several_files_before_the_slow_diagram_is_drawn() {
    let (mut typed, mut go, mut quit) = (false, false, false);
    let act = |seen: &str, terminal: &mut std::fs::File, dir: &std::path::Path| {
        if !typed && seen.contains(HINT) {
            terminal.write_all(b" ").unwrap();
            typed = true;
        }
        let frames = whole_frames(seen);
        if !go && !frames.is_empty() {
            std::fs::write(dir.join("go"), "").unwrap();
            go = true;
        }
        if !quit && image_sent(&frames, dir) {
            terminal.write_all(b"q").unwrap();
            quit = true;
        }
    };
    let args = ["--mode", "kitty", "-p", "diagram.md", "document.md"];
    let (status, seen) = watching("pager-skip-files", drawn_on_cue, &args, act);
    assert!(status.success(), "{seen:?}");
    let (before, pager) = seen.split_once(ENTER).unwrap();
    assert!(before.contains(HINT), "{seen:?}");
    let frames = whole_frames(pager);
    let first = text_of(&frames[0]);
    assert!(first.contains("flowchart TD"), "{frames:?}");
    assert!(first.contains("[rendering: 0/1]"), "{frames:?}");
    assert!(image_sent(&frames, std::path::Path::new("")), "{frames:?}");
}

#[test]
fn a_diagram_that_fails_after_a_skipped_wait_is_told_after_the_pager() {
    for (files, told) in [
        (&["diagram.md"][..], "kiri: mermaid diagram on line 1: "),
        (
            &["diagram.md", "document.md"],
            "kiri: diagram.md: mermaid diagram on line 1: ",
        ),
    ] {
        let (mut typed, mut go, mut quit) = (false, false, false);
        let act = |seen: &str, terminal: &mut std::fs::File, dir: &std::path::Path| {
            if !typed && seen.contains(HINT) {
                terminal.write_all(b" ").unwrap();
                typed = true;
            }
            let frames = whole_frames(seen);
            if !go && !frames.is_empty() {
                std::fs::write(dir.join("go"), "").unwrap();
                go = true;
            }
            if !quit && loaded(&frames[frames.len().min(1)..], dir) {
                terminal.write_all(b"q").unwrap();
                quit = true;
            }
        };
        let args = [&["--mode", "kitty", "-p"][..], files].concat();
        let (status, seen) = watching("pager-skip-failed", failing_on_cue, &args, act);
        assert!(status.success(), "{files:?}: {seen:?}");
        let (before, pager) = seen.split_once(ENTER).unwrap();
        assert!(before.contains(HINT), "{files:?}: {seen:?}");
        let frames = whole_frames(pager);
        assert!(
            text_of(&frames[0]).contains("[rendering: 0/1]"),
            "{frames:?}"
        );
        let (_, after) = seen.rsplit_once(LEAVE).unwrap();
        let line = format!("{told}sh exited with status 1: Error: bad");
        assert!(after.contains(&line), "{files:?}: {after:?}");
        assert!(!pager.rsplit_once(LEAVE).unwrap().0.contains("Error: bad"));
    }
}

#[test]
fn without_standard_error_on_a_terminal_space_does_not_skip_the_wait() {
    // Space while the diagram is drawn, which nothing then shows.
    let meanwhile = |terminal: &mut std::fs::File, dir: &std::path::Path| {
        crate::common::wait_until(Duration::from_secs(5), || dir.join("started").exists());
        let _ = terminal.write_all(b" ");
        // Nothing should happen for this space: half a second is ample
        // for kiri to act on a key, so the diagram is let go after it.
        std::thread::sleep(Duration::from_millis(500));
        let _ = std::fs::write(dir.join("go"), "");
    };
    let mut quit = false;
    let act = |seen: &[u8], terminal: &mut std::fs::File, _: &std::process::Child| {
        if !quit && !whole_frames(&text(seen)).is_empty() {
            terminal.write_all(b"q").unwrap();
            quit = true;
        }
    };
    let setup = Setup {
        meanwhile: Some(meanwhile),
        ..Setup::default()
    };
    let args = ["--mode", "kitty", "-p", "diagram.md"];
    let name = "pager-skip-no-stderr";
    let (status, seen, stderr) =
        in_terminal_seeing(name, drawn_on_cue, &args, 12, None, setup, act);
    assert!(status.success(), "{stderr}");
    assert_eq!(stderr, "");
    let frames = whole_frames(&seen);
    // Waited for the diagram.
    assert!(frames[0].contains("\x1b_Ga=T,"), "{frames:?}");
    assert!(!text_of(&frames[0]).contains("[rendering"), "{frames:?}");
}

#[test]
fn another_key_does_nothing_while_the_images_load_and_is_not_kept() {
    let (mut typed, mut quit) = (false, false);
    let act = |seen: &str, terminal: &mut std::fs::File, dir: &std::path::Path| {
        if !typed && seen.contains(HINT) {
            // To the end, were it kept for the pager.
            terminal.write_all(b"G").unwrap();
            // Nothing should happen for this key. Gives kiri time to read it
            // before the diagram is let go, so a key that is kept would show.
            std::thread::sleep(Duration::from_millis(200));
            std::fs::write(dir.join("go"), "").unwrap();
            typed = true;
        }
        if !quit && !whole_frames(seen).is_empty() {
            terminal.write_all(b"q").unwrap();
            quit = true;
        }
    };
    let args = ["--mode", "kitty", "-p", "diagram.md"];
    let (status, seen) = watching("pager-skip-other", long_on_cue, &args, act);
    assert!(status.success(), "{seen:?}");
    assert!(typed, "no hint: {seen:?}");
    let frames = whole_frames(seen.split_once(ENTER).unwrap().1);
    // Waited for the diagram, at the top.
    assert!(frames[0].contains("\x1b_Ga=T,"), "{frames:?}");
    let first = text_of(&frames[0]);
    assert!(!first.contains("[rendering"), "{frames:?}");
    assert!(first.contains("Line 2"), "{frames:?}");
    assert!(!first.contains("Line 10"), "{frames:?}");
}

#[test]
fn ctrl_c_while_the_images_load_ends_kiri_as_before() {
    use std::os::unix::process::ExitStatusExt;

    for args in [
        &["--mode", "kitty", "-p", "diagram.md"][..],
        &["--mode", "kitty", "-P", "diagram.md"],
    ] {
        let mut typed = false;
        let act = |seen: &str, terminal: &mut std::fs::File, _: &std::path::Path| {
            if !typed && seen.contains("loading images") {
                terminal.write_all(b"\x03").unwrap();
                typed = true;
            }
        };
        let (status, seen) = watching("pager-skip-ctrl-c", drawn_on_cue, args, act);
        assert_eq!(status.signal(), Some(libc::SIGINT), "{args:?}: {seen:?}");
        assert!(!seen.contains(ENTER), "{args:?}: {seen:?}");
    }
}

#[test]
fn the_hint_shows_only_where_the_pager_surely_opens() {
    // The document, whether it fits a window of 12 rows, and the options.
    for (long, args, hint) in [
        (false, &["-p"][..], true),
        (true, &[][..], true),
        (false, &[][..], false),
        (true, &["-P"][..], false),
    ] {
        let (mut go, mut quit) = (false, false);
        let act = |seen: &str, terminal: &mut std::fs::File, dir: &std::path::Path| {
            if !go && seen.contains("loading images") {
                std::fs::write(dir.join("go"), "").unwrap();
                go = true;
            }
            if !quit && seen.contains("\x1b[?2026l") {
                terminal.write_all(b"q").unwrap();
                quit = true;
            }
        };
        let prepare = |dir: &std::path::Path| match long {
            true => long_on_cue(dir),
            false => drawn_on_cue(dir),
        };
        let all = [&["--mode", "kitty"][..], args, &["diagram.md"]].concat();
        let (status, seen) = watching("pager-skip-hint", prepare, &all, act);
        assert!(status.success(), "{args:?}: {seen:?}");
        assert!(seen.contains("loading images"), "{args:?}: {seen:?}");
        assert_eq!(seen.contains(HINT), hint, "{args:?}: {seen:?}");
    }
}

/// [`drawn_diagram`], with `a.md`, a diagram other than `diagram.mmd`'s,
/// and `missing.md`, an image whose file is missing.
fn several_files(dir: &std::path::Path) {
    drawn_diagram(dir);
    std::fs::write(dir.join("a.md"), "```mermaid\nflowchart LR\n```\n").unwrap();
    std::fs::write(dir.join("missing.md"), "![gone](missing.png)\n").unwrap();
}

#[test]
fn several_files_load_their_images_in_the_background_too() {
    let files = ["a.md", "missing.md"];
    let args = [&BACKGROUND[..], &files].concat();
    let name = "pager-initial-load-files";
    let (frames, stderr) = quit_when(name, several_files, &args, None, &[], image_sent);
    let first = text_of(&frames[0]);
    assert!(first.contains("flowchart LR"), "{frames:?}");
    assert!(first.contains("[rendering: 0/2]"), "{frames:?}");
    assert!(!frames[0].contains("\x1b_Ga=T,"), "{frames:?}");
    let drawn = frames.iter().find(|frame| frame.contains("\x1b_Ga=T,"));
    assert!(!text_of(drawn.unwrap()).contains("flowchart"), "{frames:?}");
    // Under the file's name, as waiting tells it before the pager opens.
    let waiting = told_waiting("pager-initial-load-files-wait", several_files, &files);
    assert_eq!(waiting, "kiri: missing.md: missing.png: file not found\n");
    assert_eq!(stderr, waiting);
}

#[test]
fn several_files_without_a_terminal_for_keys_print_as_when_waiting() {
    let shown = |how: &str| {
        let args = [
            "--mode",
            "kitty",
            "--initial-load",
            how,
            "-p",
            "a.md",
            "missing.md",
        ];
        let (status, seen, stderr) =
            without_keys("pager-initial-load-files-no-keys", several_files, &args);
        assert!(status.success(), "{stderr}");
        (seen, stderr)
    };
    let (seen, stderr) = shown("background");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    assert!(
        stderr.contains("kiri: missing.md: missing.png: file not found\n"),
        "{stderr}"
    );
    assert_eq!((seen, stderr), shown("wait"));
}

#[test]
fn a_diagram_file_waits_for_its_drawing_also_among_several_files() {
    let quit = |frames: &[String], _: &std::path::Path| !frames.is_empty();
    for files in [&["diagram.mmd"][..], &["diagram.mmd", "a.md"]] {
        let args = [&BACKGROUND[..], files].concat();
        let name = "pager-initial-load-diagram-file";
        let (frames, stderr) = quit_when(name, several_files, &args, None, &[], quit);
        assert_eq!(stderr, "");
        // Drawn at the top of the first frame.
        assert!(frames[0].contains("\x1b_Ga=T,"), "{files:?}: {frames:?}");
        let first = text_of(&frames[0]);
        let loading = first.contains("[rendering: 0/1]");
        assert_eq!(loading, files.len() > 1, "{files:?}: {frames:?}");
    }
}

/// [`drawn_on_cue`], with a command that fails until `ok` is in the
/// scratch directory `dir`, and the reasons only on standard error.
fn failing_until_ok(dir: &std::path::Path) {
    failing_on_cue(dir);
    let draw = format!(
        "[ -f '{}' ] || {{ echo 'Error: bad' >&2; exit 1; }}\n\
         echo '<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"20\"/>' > \"$2\"\n",
        dir.join("ok").display()
    );
    std::fs::write(dir.join("render.sh"), draw).unwrap();
}

#[test]
fn a_diagram_that_failed_and_then_drew_in_the_background_is_not_told() {
    let again = Stage {
        when: &loaded,
        then: &|dir| {
            std::fs::write(dir.join("ok"), "").unwrap();
            b"r"
        },
    };
    let quit = Stage {
        when: &image_sent,
        then: &|_| b"q",
    };
    let args = [&BACKGROUND[..], &["diagram.md"]].concat();
    let name = "pager-initial-load-failed-then-drew";
    let (seen, stderr) = in_stages(name, failing_until_ok, &args, false, &[again, quit]);
    assert!(seen.contains("\x1b_Ga=T,"), "{seen:?}");
    assert_eq!(stderr, "");
}

#[test]
fn a_failed_diagram_whose_block_is_gone_is_not_told() {
    let gone = Stage {
        when: &loaded,
        then: &|dir| {
            std::fs::write(dir.join("diagram.md"), "Gone\n").unwrap();
            b"r"
        },
    };
    let quit = Stage {
        when: &reloaded,
        then: &|_| b"q",
    };
    let args = [&BACKGROUND[..], &["diagram.md"]].concat();
    let name = "pager-initial-load-failed-gone";
    let (seen, stderr) = in_stages(name, failing_until_ok, &args, false, &[gone, quit]);
    assert!(text_of(&seen).contains("Gone"), "{seen:?}");
    assert_eq!(stderr, "");
}

#[test]
fn a_missing_image_that_came_in_the_background_is_told_once_after_reloads() {
    let prepare = |dir: &std::path::Path| {
        std::fs::write(dir.join("missing.md"), "![gone](missing.png)\n").unwrap();
    };
    let again = || Stage {
        when: &loaded,
        then: &|_| b"r",
    };
    let quit = Stage {
        when: &reloaded,
        then: &|_| b"q",
    };
    let reload = Stage {
        when: &reloaded,
        then: &|_| b"r",
    };
    let args = [&BACKGROUND[..], &["missing.md"]].concat();
    let name = "pager-initial-load-missing";
    let (seen, stderr) = in_stages(name, prepare, &args, false, &[again(), reload, quit]);
    assert!(text_of(&seen).contains("[image: gone]"), "{seen:?}");
    let waiting = told_waiting("pager-initial-load-missing-wait", prepare, &["missing.md"]);
    assert!(waiting.contains("missing.png"), "{waiting}");
    assert_eq!(stderr, waiting);
}

/// A document of `top` lines in one paragraph, a diagram of five lines
/// and three paragraphs of a line each: with the diagram counted at three
/// rows, it takes `top` + 10 rows.
fn counted(top: usize) -> String {
    let top: Vec<String> = (1..=top).map(|n| format!("Top {n}")).collect();
    let diagram = "```mermaid\nflowchart TD\n  A --> B\n  B --> C\n  C --> D\n  D --> E\n```";
    format!(
        "{}\n\n{diagram}\n\nLine 1\n\nLine 2\n\nLine 3\n",
        top.join("  \n")
    )
}

#[test]
fn paging_on_auto_loads_in_the_background_what_is_longer_than_the_window_with_three_rows_an_image()
{
    // 13 rows in a window of 12: the pager opens at once.
    let longer = |dir: &std::path::Path| {
        drawn_diagram(dir);
        std::fs::write(dir.join("counted.md"), counted(3)).unwrap();
    };
    let args = [
        "--initial-load",
        "background",
        "--mode",
        "kitty",
        "counted.md",
    ];
    let name = "pager-initial-load-auto-longer";
    let (frames, stderr) = quit_when(name, longer, &args, None, &[], image_sent);
    assert_eq!(stderr, "");
    assert!(!frames.is_empty(), "printed, not paged");
    let first = text_of(&frames[0]);
    assert!(first.contains("[rendering: 0/1]"), "{frames:?}");
    assert!(first.contains("D --> E"), "{frames:?}");
    // 12 rows: it waits, and then, with the diagram drawn shorter, prints.
    let fits = |dir: &std::path::Path| {
        drawn_diagram(dir);
        std::fs::write(dir.join("counted.md"), counted(2)).unwrap();
    };
    let name = "pager-initial-load-auto-fits";
    let (status, seen, stderr) = in_terminal_told(name, fits, &args, 12, None, |_, _, _| {});
    assert!(status.success(), "{stderr}");
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert!(!text_of(&seen).contains("D --> E"), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    // Several files, longer than the window as their images come.
    let files = |dir: &std::path::Path| {
        several_files(dir);
        std::fs::write(dir.join("counted.md"), counted(3)).unwrap();
    };
    let args = [
        "--initial-load",
        "background",
        "--mode",
        "kitty",
        "a.md",
        "counted.md",
    ];
    let name = "pager-initial-load-auto-files";
    let (frames, stderr) = quit_when(name, files, &args, None, &[], image_sent);
    assert_eq!(stderr, "");
    assert!(!frames.is_empty(), "printed, not paged");
    assert!(
        text_of(&frames[0]).contains("[rendering: 0/2]"),
        "{frames:?}"
    );
}

#[test]
fn an_initial_load_in_the_background_waits_where_the_pager_does_not_open() {
    let prepare = |dir: &std::path::Path| {
        drawn_diagram(dir);
        let document = format!("```mermaid\nflowchart TD\n```\n\n{}", long());
        std::fs::write(dir.join("diagram.md"), document).unwrap();
    };
    // Printed.
    let args = [
        "--mode",
        "kitty",
        "--initial-load",
        "background",
        "-P",
        "diagram.md",
    ];
    let (status, seen) = in_steps_with("pager-initial-load-print", prepare, &args, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    // Printed, as there is no terminal to read keys from.
    let args = [
        "--mode",
        "kitty",
        "--initial-load",
        "background",
        "-p",
        "diagram.md",
    ];
    let (status, seen, stderr) = without_keys("pager-initial-load-no-keys", prepare, &args);
    assert!(status.success(), "{stderr}");
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{seen:?}");
    assert_eq!(
        stderr,
        "kiri: no terminal to read keys from, so the document is printed\n"
    );
}

/// What a test types into the pager and when: the keys that `keys` returns
/// once `frames` frames are drawn, after it may have changed the files in
/// the scratch directory it is given.
struct Step<'a> {
    frames: usize,
    keys: &'a dyn Fn(&std::path::Path) -> &'static [u8],
}

/// [`in_steps`] on files `prepare` makes.
fn in_steps_with(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    steps: &[Step],
) -> (ExitStatus, String) {
    in_steps_after(name, prepare, args, None, steps)
}

fn in_steps_after(
    name: &str,
    prepare: impl FnOnce(&std::path::Path),
    args: &[&str],
    stdin: Option<&str>,
    steps: &[Step],
) -> (ExitStatus, String) {
    let dir = std::cell::RefCell::new(std::path::PathBuf::new());
    let prepare = |scratch: &std::path::Path| {
        *dir.borrow_mut() = scratch.to_owned();
        prepare(scratch);
    };
    let mut done = 0;
    in_terminal_after(name, prepare, args, 5, stdin, |frames, terminal, _| {
        if let Some(step) = steps.get(done).filter(|step| frames >= step.frames) {
            terminal.write_all((step.keys)(&dir.borrow())).unwrap();
            done += 1;
        }
    })
}

fn quit_after(frames: usize) -> Step<'static> {
    Step {
        frames,
        keys: &|_| b"q",
    }
}

#[test]
fn follow_takes_on_off_or_auto_and_one_flag_of_them() {
    let dir = crate::common::Scratch::new("follow-values");
    for value in ["yes", "Auto"] {
        let out = command(&dir)
            .args(["--follow", value, "-"])
            .output()
            .unwrap();
        assert!(!out.status.success(), "{value}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!("invalid value '{value}'")),
            "{stderr}"
        );
    }
    for flags in [
        ["-f", "-F"],
        ["--fa", "-F"],
        ["-f", "--fa"],
        ["--fa", "--follow=on"],
    ] {
        let out = command(&dir).args(flags).arg("-").output().unwrap();
        assert!(!out.status.success(), "{flags:?}");
    }
}

/// `frame` without its escape sequences.
fn text_of(frame: &str) -> String {
    let mut plain = String::new();
    let mut chars = frame.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            plain.push(c);
            continue;
        }
        // A CSI sequence ends at a letter; the graphics ones at the
        // string terminator.
        for c in chars.by_ref() {
            if c.is_ascii_alphabetic() || c == '\\' {
                break;
            }
        }
    }
    plain
}

/// The width in pixels of each image sent to the terminal in `seen`, in
/// the order they were sent.
fn sent_widths(seen: &str) -> Vec<String> {
    seen.split("\x1b_Ga=T,")
        .skip(1)
        .filter_map(|image| image.split(',').find(|key| key.starts_with("s=")))
        .map(str::to_owned)
        .collect()
}

/// A red PNG of `width` × 16 pixels at `path`, last modified at `time`.
fn png(path: &std::path::Path, width: u32, time: std::time::SystemTime) {
    image::RgbaImage::from_pixel(width, 16, image::Rgba([255, 0, 0, 255]))
        .save(path)
        .unwrap();
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(time).unwrap();
}

#[test]
fn a_reload_shows_an_image_whose_file_changed_and_keeps_the_others() {
    let long_ago = std::time::UNIX_EPOCH + Duration::from_secs(1000);
    let prepare = |dir: &std::path::Path| {
        png(&dir.join("a.png"), 16, long_ago);
        png(&dir.join("b.png"), 16, long_ago);
        let document = format!("![a](a.png)\n\n![b](b.png)\n\n{}", long());
        std::fs::write(dir.join("images.md"), document).unwrap();
    };
    // `a.png` is wider, and later than it was; `b.png` is wider too, but
    // as late as it was.
    let change = |dir: &std::path::Path| {
        png(&dir.join("a.png"), 32, std::time::SystemTime::now());
        png(&dir.join("b.png"), 32, long_ago);
        b"r" as &'static [u8]
    };
    let steps = [
        Step {
            frames: 1,
            keys: &change,
        },
        // The frame that says it is reloading, the reload's, and the one
        // where `a.png` came.
        quit_after(4),
    ];
    let args = ["--mode", "kitty", "images.md"];
    let (status, seen) = in_steps_with("pager-reload-images", prepare, &args, &steps);
    assert!(status.success());
    // Only `a.png` is sent again, once it has loaded; the terminal still
    // holds `b.png` as it was. Meanwhile `a.png` shows its alt text, with
    // the band, sent apart and placed over the screen, below it.
    let widths = sent_widths(&seen);
    assert_eq!(widths, ["s=20", "s=20", "s=40"], "{seen:?}");
    assert!(seen.contains("[image: a]"), "{seen:?}");
    assert_eq!(seen.matches("\x1b_Ga=t,").count(), 1, "{seen:?}");
    assert!(seen.contains("\x1b_Ga=p,"), "{seen:?}");
}

/// Writes `document` into the named pipe at `path` once a reader opens it,
/// on a thread of its own.
fn write_pipe(path: &std::path::Path, document: String) {
    let path = path.to_owned();
    std::thread::spawn(move || {
        let mut pipe = std::fs::File::options().write(true).open(path).unwrap();
        let _ = pipe.write_all(document.as_bytes());
    });
}

#[test]
fn a_named_pipe_reloads_nothing() {
    use std::os::unix::fs::OpenOptionsExt;

    let prepare = |dir: &std::path::Path| {
        let path = dir.join("pipe.md");
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `name` is a C string that lives through the call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        write_pipe(&path, long());
    };
    // Another document waits in the pipe, should kiri open it again.
    let again = |dir: &std::path::Path| {
        write_pipe(&dir.join("pipe.md"), "Again\n".to_owned());
        b"r" as &'static [u8]
    };
    let quit = |dir: &std::path::Path| {
        // Lets the writer's open go through, should kiri not have opened
        // the pipe again.
        let _ = std::fs::File::options()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(dir.join("pipe.md"));
        b"q" as &'static [u8]
    };
    let steps = [
        Step {
            frames: 1,
            keys: &again,
        },
        Step {
            frames: 2,
            keys: &quit,
        },
    ];
    let (status, seen) = in_steps_with("pager-reload-fifo", prepare, &["pipe.md"], &steps);
    assert!(status.success());
    assert!(text_of(&seen).contains("nothing to reload"), "{seen:?}");
    assert!(!seen.contains("Again"), "{seen:?}");
}

/// What kiri draws in the pager for a document with a code line wider
/// than reading mode's area, with `args` besides `--reading -p`.
fn wide_code(name: &str, args: &[&str]) -> String {
    let wide = |dir: &std::path::Path| {
        let code = format!("Text.\n\n```\n{}\n```\n", "a".repeat(200));
        std::fs::write(dir.join("wide.md"), code).unwrap();
    };
    let mut quit = false;
    let mut all = vec!["--mode", "kitty", "--reading", "-p"];
    all.extend(args);
    all.push("wide.md");
    let (status, seen) = in_terminal_after(name, wide, &all, 10, None, |frames, terminal, _| {
        if frames > 0 && !quit {
            terminal.write_all(b"q").unwrap();
            quit = true;
        }
    });
    assert!(status.success());
    seen
}

/// The images sent in `seen`, by the keys of their transmission.
fn images(seen: &str) -> Vec<&str> {
    (seen.split("\x1b_Ga=T").skip(1))
        .filter_map(|keys| keys.split(';').next())
        .collect()
}

#[test]
fn wide_scroll_scrolls_a_wide_code_block_in_the_pager_and_wrap_wraps_it() {
    let wrapped = wide_code("pager-wide-wrap", &[]);
    let scrolling = wide_code("pager-wide-scroll", &["--wide", "scroll"]);
    let again = wide_code("pager-wide-wrap-again", &["--wide", "wrap"]);
    // Wrapped, one part of the page; scrolling, the band besides.
    assert_eq!(images(&wrapped).len(), 1, "{:?}", images(&wrapped));
    assert_eq!(images(&scrolling).len(), 2, "{:?}", images(&scrolling));
    assert_eq!(images(&again).len(), 1, "{:?}", images(&again));
}

/// [`wide_code`], with the environment `env`, the configuration file
/// `config` and `args` besides `--mode kitty --reading -p`.
fn wide_code_set(name: &str, env: &[(&str, &str)], config: Option<&str>, args: &[&str]) -> String {
    let prepare = |dir: &std::path::Path| {
        let code = format!("Text.\n\n```\n{}\n```\n", "a".repeat(200));
        std::fs::write(dir.join("wide.md"), code).unwrap();
        if let Some(config) = config {
            std::fs::create_dir_all(dir.join("config/kiri")).unwrap();
            std::fs::write(dir.join("config/kiri/config.toml"), config).unwrap();
        }
    };
    let mut quit = false;
    let mut all = vec!["--mode", "kitty", "--reading", "-p"];
    all.extend(args);
    all.push("wide.md");
    let setup = Setup {
        env,
        ..Setup::default()
    };
    let act = |frames: usize, terminal: &mut std::fs::File, _: &std::process::Child| {
        if frames > 0 && !quit {
            terminal.write_all(b"q").unwrap();
            quit = true;
        }
    };
    let (status, seen, stderr) = in_terminal_run(name, prepare, &all, 10, None, setup, act);
    assert!(status.success(), "{stderr}");
    seen
}

/// The variable and the file set the wide setting as the flag does, and
/// the flag comes first, then the variable, then the file.
#[test]
fn wide_comes_from_the_flag_the_variable_or_the_file() {
    let scrolls = |seen: String| images(&seen).len() == 2;
    assert!(!scrolls(wide_code_set(
        "pager-wide-default",
        &[],
        None,
        &[]
    )));
    let variable = wide_code_set("pager-wide-variable", &[("KIRI_WIDE", "scroll")], None, &[]);
    assert!(scrolls(variable), "KIRI_WIDE=scroll");
    let file = wide_code_set("pager-wide-file", &[], Some("wide = \"scroll\"\n"), &[]);
    assert!(scrolls(file), "wide = \"scroll\" in the file");
    let beaten = wide_code_set(
        "pager-wide-variable-first",
        &[("KIRI_WIDE", "wrap")],
        Some("wide = \"scroll\"\n"),
        &[],
    );
    assert!(!scrolls(beaten), "KIRI_WIDE=wrap comes before the file");
    let flagged = wide_code_set(
        "pager-wide-flag-first",
        &[("KIRI_WIDE", "wrap")],
        None,
        &["--wide", "scroll"],
    );
    assert!(scrolls(flagged), "--wide scroll comes before KIRI_WIDE");
}

#[test]
fn printing_always_wraps() {
    let dir = Scratch::new("print-wide-scroll");
    let code = format!("Text.\n\n```\n{}\n```\n", "a".repeat(200));
    let path = dir.join("wide.md");
    std::fs::write(&path, code).unwrap();
    let print = |wide: &str| {
        let out = command(&dir)
            .args([
                "--mode",
                "kitty",
                "--reading",
                "--width",
                "40",
                "--wide",
                wide,
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out.stderr));
        out.stdout
    };
    assert_eq!(print("scroll"), print("wrap"));
}

/// The ids of the images sent in `seen`, in order.
fn sent_ids(seen: &str) -> Vec<String> {
    seen.split("\x1b_Ga=T,U=1,")
        .skip(1)
        .filter_map(|rest| {
            let id = rest.split(',').find_map(|part| part.strip_prefix("i="))?;
            Some(id.to_owned())
        })
        .collect()
}

#[test]
fn m_loads_no_images_when_images_are_off() {
    let prepare = |dir: &std::path::Path| {
        crate::common::quadrants(8, 8)
            .save(dir.join("p.png"))
            .unwrap();
        let document = "![block](p.png)\n\nA line with a badge <img src=\"p.png\" width=\"24\" alt=\"badge\"> in it.\n";
        std::fs::write(dir.join("document.md"), document).unwrap();
    };
    // `M` once the first frame is drawn; `q` once its frame is drawn and
    // the terminal has been silent for a moment, which is when a picture
    // that was loading would have come: nothing loads, so nothing comes.
    let (mut step, mut length) = (0, 0);
    let act = |seen: &[u8], terminal: &mut std::fs::File, _: &std::process::Child| {
        let shown = text(seen);
        let frames = shown.matches("\x1b[?2026l").count();
        let silent = seen.len() == length;
        length = seen.len();
        match step {
            0 if frames > 0 => {
                terminal.write_all(b"M").unwrap();
                step = 1;
            }
            1 if frames > 1 && silent => {
                terminal.write_all(b"q").unwrap();
                step = 2;
            }
            _ => {}
        }
    };
    let args = [
        "-p",
        "--no-images",
        "--mode",
        "kitty",
        "--width",
        "80",
        "document.md",
    ];
    let setup = Setup {
        quiet: Some(Duration::from_millis(300)),
        ..Setup::default()
    };
    let (status, seen, stderr) =
        in_terminal_seeing("pager-m-no-images", prepare, &args, 10, None, setup, act);
    assert_eq!(stderr, "");
    assert!(status.success());
    // Only the page of text, as it is with `--reading` and no images: the
    // pictures never load.
    assert_eq!(sent_ids(&seen).len(), 1, "{seen:?}");
}

/// kiri with `args` in a terminal of 12 rows, quitting once the images
/// have come, in a scratch directory with `missing.md`, which shows an
/// image that is not there, and `plain.md`. Returns what it wrote on
/// standard error.
fn told_for_missing(name: &str, args: &[&str]) -> String {
    let prepare = |dir: &std::path::Path| {
        std::fs::write(dir.join("missing.md"), "![gone](missing.png)\n").unwrap();
        std::fs::write(dir.join("plain.md"), "Plain\n").unwrap();
    };
    let (_, stderr) = quit_when(name, prepare, args, None, &[], loaded);
    stderr
}

#[test]
fn a_warning_of_images_that_came_in_the_background_names_its_file_only_among_several() {
    let alone = [&BACKGROUND[..], &["missing.md"]].concat();
    let stderr = told_for_missing("pager-late-alone", &alone);
    assert!(stderr.starts_with("kiri: missing.png: "), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    let several = [&BACKGROUND[..], &["plain.md", "missing.md"]].concat();
    let stderr = told_for_missing("pager-late-several", &several);
    assert!(
        stderr.starts_with("kiri: missing.md: missing.png: "),
        "{stderr}"
    );
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
}

#[test]
fn a_warning_of_images_waited_for_names_its_file_only_among_several() {
    let prepare = |dir: &std::path::Path| {
        std::fs::write(dir.join("missing.md"), "![gone](missing.png)\n").unwrap();
        std::fs::write(dir.join("plain.md"), "Plain\n").unwrap();
    };
    let stderr = told_waiting("pager-wait-alone", prepare, &["missing.md"]);
    assert!(stderr.starts_with("kiri: missing.png: "), "{stderr}");
    let stderr = told_waiting("pager-wait-several", prepare, &["plain.md", "missing.md"]);
    assert!(
        stderr.starts_with("kiri: missing.md: missing.png: "),
        "{stderr}"
    );
}

#[test]
fn the_settings_warnings_are_told_before_the_pager_opens_for_an_image_alone() {
    let prepare = |dir: &std::path::Path| {
        std::fs::copy(fixtures().join("images/icon.png"), dir.join("a.png")).unwrap();
    };
    let setup = Setup {
        env: &[("KIRI_TABLE_ROWS", "stripes")],
        ..Setup::default()
    };
    let mut typed = false;
    let act = |frames: usize, terminal: &mut std::fs::File, _: &std::process::Child| {
        if frames >= 1 && !typed {
            terminal.write_all(b"q").unwrap();
            typed = true;
        }
    };
    let args = ["--mode", "kitty", "-p", "a.png"];
    let (status, seen, stderr) =
        in_terminal_run("pager-image-warning", prepare, &args, 5, None, setup, act);
    assert!(status.success(), "{seen:?}");
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert_eq!(
        stderr,
        "kiri: KIRI_TABLE_ROWS: `stripes` is not off, zebra, lines, indent or underline; \
         left out\n"
    );
}

/// Types `q` once the first frame is drawn.
fn quit_at_first(typed: &mut bool) -> impl FnMut(usize, &mut std::fs::File, &std::process::Child) {
    move |frames, terminal, _| {
        if frames >= 1 && !*typed {
            terminal.write_all(b"q").unwrap();
            *typed = true;
        }
    }
}

#[test]
fn following_pages_neither_an_image_alone_nor_a_diagram_alone() {
    let prepare = |dir: &std::path::Path| {
        drawn_diagram(dir);
        std::fs::copy(fixtures().join("images/icon.png"), dir.join("a.png")).unwrap();
    };
    for (name, file) in [
        ("pager-follow-image", "a.png"),
        ("pager-follow-diagram", "diagram.mmd"),
    ] {
        let args = ["--mode", "kitty", "-f", file];
        let (status, seen) = in_terminal_after(name, prepare, &args, 5, None, |_, _, _| {});
        assert!(status.success(), "{seen:?}");
        assert!(!seen.contains(ENTER), "{file}: {seen:?}");
        assert_eq!(seen.matches("\x1b_Ga=T,").count(), 1, "{file}: {seen:?}");
    }
}

/// A configuration whose Mermaid command writes `called` in the scratch
/// directory before it draws, and `flow.mmd`.
fn calls_when_drawing(dir: &std::path::Path) {
    drawn_diagram(dir);
    let script = dir.join("render.sh");
    let draw = std::fs::read_to_string(&script).unwrap();
    let called = format!("touch '{}'\n{draw}", dir.join("called").display());
    std::fs::write(&script, called).unwrap();
    std::fs::rename(dir.join("diagram.mmd"), dir.join("flow.mmd")).unwrap();
}

/// Whether the program that draws diagrams was called, in the window of
/// `calls_when_drawing`, once `M` and `r` were typed and 600 ms went by.
fn called_after_m_and_a_reload(name: &str, args: &[&str]) -> bool {
    let called = std::cell::Cell::new(false);
    let quit = |dir: &std::path::Path| {
        std::thread::sleep(Duration::from_millis(600));
        called.set(dir.join("called").exists());
        b"q" as &'static [u8]
    };
    let steps = [
        Step {
            frames: 1,
            keys: &|_| b"M",
        },
        Step {
            frames: 2,
            keys: &|_| b"r",
        },
        Step {
            frames: 3,
            keys: &quit,
        },
    ];
    let (status, seen) = in_steps_with(name, calls_when_drawing, args, &steps);
    assert!(status.success(), "{seen:?}");
    called.get()
}

#[test]
fn a_diagram_in_text_stays_source_after_m_and_after_a_reload() {
    // Where the program is called, the wait is long enough to see it.
    let drawn = ["--mode", "kitty", "-p", "flow.mmd"];
    assert!(called_after_m_and_a_reload("pager-diagram-called", &drawn));
    let text = ["--mode", "kitty", "--format", "text", "-p", "flow.mmd"];
    assert!(!called_after_m_and_a_reload("pager-text-diagram", &text));
}

#[test]
fn one_image_left_by_a_broken_configuration_is_paged_as_a_file_alone() {
    let prepare = |dir: &std::path::Path| {
        std::fs::write(dir.join("a.md"), "Alpha\n").unwrap();
        std::fs::copy(fixtures().join("images/icon.png"), dir.join("pic.png")).unwrap();
        crate::common::write(&dir.join("config/kiri/config.toml"), "images = [\n");
    };
    let mut typed = false;
    let args = ["--mode", "kitty", "-p", "a.md", "pic.png"];
    let (status, seen, stderr) = in_terminal_told(
        "pager-broken-one-left",
        prepare,
        &args,
        5,
        None,
        quit_at_first(&mut typed),
    );
    assert_eq!(status.code(), Some(1), "{seen:?}");
    assert!(stderr.contains("config.toml"), "{stderr}");
    // As a document on its own: named by the status line, without its place
    // among files.
    let first = seen.split("\x1b[?2026h").nth(1).map(text_of).unwrap();
    assert!(first.contains(" pic.png "), "{seen:?}");
    assert!(!first.contains("1/1"), "{seen:?}");
}
