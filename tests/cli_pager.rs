//! The pager end to end: kiri in a pseudo-terminal, with keys typed into
//! it.

use std::io::{Read, Write};
use std::os::fd::BorrowedFd;
use std::os::unix::process::CommandExt;
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};

mod common;

use common::{Scratch, command, text};

/// The alternate screen, which the pager draws on.
const ENTER: &str = "\x1b[?1049h";
const LEAVE: &str = "\x1b[?1049l";

/// A document of `count` paragraphs, one row each.
fn paragraphs(count: usize) -> String {
    (1..=count).map(|n| format!("Line {n}\n\n")).collect()
}

/// Runs kiri, in a scratch directory called `name`, with `args` in a terminal of 40 × `rows` cells, with `stdin`
/// piped in when given, and types each of `keys` after a frame of the pager,
/// the first after the first frame. `--mode ansi` unless `args` choose a
/// mode. Returns
/// how kiri exited and everything it wrote to the terminal.
fn in_terminal(
    name: &str,
    args: &[&str],
    rows: u16,
    stdin: Option<&str>,
    keys: &[&[u8]],
) -> (ExitStatus, String) {
    let dir = Scratch::new(name);
    std::fs::write(dir.join("document.md"), paragraphs(3)).unwrap();
    let (mut terminal, tty) = common::pty();
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
        .args(args)
        .stdin(match stdin {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        })
        .stdout(Stdio::from(tty))
        .stderr(Stdio::piped());
    // SAFETY: setsid and ioctl are async-signal-safe and touch no memory.
    unsafe {
        kiri.pre_exec(|| {
            rustix::process::setsid()?;
            // Standard output is the terminal by now.
            rustix::process::ioctl_tiocsctty(BorrowedFd::borrow_raw(1))?;
            Ok(())
        });
    }
    let mut child = kiri.spawn().unwrap();
    // The command holds the program's side of the terminal. Close it, so
    // that reading the terminal side ends once kiri exits.
    drop(kiri);
    if let Some(document) = stdin {
        let mut input = child.stdin.take().unwrap();
        input.write_all(document.as_bytes()).unwrap();
    }
    let mut seen = Vec::new();
    let mut typed = 0;
    let mut buffer = [0; 4096];
    // kiri gets 10 s at most, so a hanging kiri fails the test instead of
    // hanging it.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.checked_duration_since(Instant::now());
        let timeout = left.map(|left| Timespec::try_from(left).unwrap());
        let mut fds = [PollFd::new(&terminal, PollFlags::IN)];
        if left.is_none() || poll(&mut fds, timeout.as_ref()).unwrap() == 0 {
            child.kill().unwrap();
            panic!("kiri did not finish: {:?}", text(&seen));
        }
        // Reading ends with an error once no one holds the program's side.
        let Ok(read @ 1..) = terminal.read(&mut buffer) else {
            break;
        };
        seen.extend_from_slice(&buffer[..read]);
        let frames = text(&seen).matches("\x1b[?2026l").count();
        if typed < frames.min(keys.len()) {
            terminal.write_all(keys[typed]).unwrap();
            typed += 1;
        }
    }
    let status = child.wait().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert_eq!(stderr, "");
    (status, text(&seen))
}

/// A document longer than a window of 5 rows.
fn long() -> String {
    paragraphs(10)
}

#[test]
fn a_long_document_is_paged_until_q() {
    let (status, seen) = in_terminal("pager-long", &["-"], 5, Some(&long()), &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
    assert!(seen.ends_with(LEAVE), "{seen:?}");
    assert!(seen.contains("Line 3"), "{seen:?}");
    assert!(!seen.contains("Line 4"), "{seen:?}");
}

#[test]
fn keys_scroll_the_document() {
    let (status, seen) = in_terminal("pager-keys", &["-"], 5, Some(&long()), &[b"G", b"q"]);
    assert!(status.success());
    let frames: Vec<_> = seen.split("\x1b[?2026h").skip(1).collect();
    assert_eq!(frames.len(), 2, "{seen:?}");
    assert!(!frames[0].contains("Line 10"), "{seen:?}");
    assert!(frames[1].contains("Line 10"), "{seen:?}");
}

#[test]
fn ctrl_c_quits_and_leaves_the_screen() {
    let (status, seen) = in_terminal("pager-ctrl-c", &["-"], 5, Some(&long()), &[b"\x03"]);
    assert!(status.success());
    assert!(seen.ends_with(LEAVE), "{seen:?}");
}

#[test]
fn a_short_document_is_printed() {
    let (status, seen) = in_terminal("pager-short", &["document.md"], 24, None, &[]);
    assert!(status.success());
    assert!(!seen.contains(ENTER), "{seen:?}");
    assert!(seen.contains("Line 3"), "{seen:?}");
}

#[test]
fn p_pages_a_short_document() {
    let (status, seen) = in_terminal("pager-p", &["-p", "document.md"], 24, None, &[b"q"]);
    assert!(status.success());
    assert!(seen.starts_with(ENTER), "{seen:?}");
}

#[test]
fn capital_p_prints_a_long_document() {
    for args in [&["-P", "-"][..], &["--paging", "never", "-"][..]] {
        let (status, seen) = in_terminal("pager-capital-p", args, 5, Some(&long()), &[]);
        assert!(status.success());
        assert!(!seen.contains(ENTER), "{args:?}: {seen:?}");
        assert!(seen.contains("Line 10"), "{args:?}: {seen:?}");
    }
}

#[test]
fn plain_mode_prints_without_the_pager() {
    let (status, seen) = in_terminal(
        "pager-plain",
        &["-p", "--mode", "plain", "-"],
        5,
        Some(&long()),
        &[],
    );
    assert!(status.success());
    assert!(!seen.contains('\x1b'), "{seen:?}");
}

#[test]
fn a_pipe_gets_the_document_printed_even_with_p() {
    let dir = Scratch::new("pager-pipe");
    let out = common::kiri(&dir, &long(), &["-p", "--mode", "ansi"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(!stdout.contains(ENTER), "{stdout:?}");
    assert!(stdout.contains("Line 10"), "{stdout:?}");
}
