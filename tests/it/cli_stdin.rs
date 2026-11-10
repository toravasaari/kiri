//! The kiri binary end to end: a document or an image on standard input.

use std::io::{Read, Write};
use std::os::fd::BorrowedFd;
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::time::Duration;

use crate::common::{
    Scratch, assert_succeeded, command, fixtures, kiri_piped, piped, read_terminal, text, write,
};

#[test]
fn reads_the_document_from_a_pipe_without_a_file_name() {
    let dir = Scratch::new("stdin-pipe");
    let out = kiri_piped(&dir, &["--mode", "plain"], b"Piped *text*.\n");
    assert_succeeded(&out);
    assert_eq!(text(&out.stdout).trim(), "Piped text.");
}

#[test]
fn a_dash_reads_the_document_from_standard_input() {
    let dir = Scratch::new("stdin-dash");
    let out = kiri_piped(&dir, &["--mode", "plain", "-"], b"Dashed *text*.\n");
    assert_succeeded(&out);
    assert_eq!(text(&out.stdout).trim(), "Dashed text.");
}

#[test]
fn a_file_on_standard_input_is_read_too() {
    let dir = Scratch::new("stdin-file");
    let document = dir.join("document.md");
    write(&document, "From a *file*.\n");
    let out = command(&dir)
        .args(["--mode", "plain"])
        .stdin(std::fs::File::open(&document).unwrap())
        .output()
        .unwrap();
    assert_succeeded(&out);
    assert_eq!(text(&out.stdout).trim(), "From a file.");
}

#[test]
fn a_terminal_on_standard_input_still_needs_a_file_name() {
    let dir = Scratch::new("stdin-terminal");
    let (_terminal, tty) = crate::common::pty();
    let out = command(&dir).stdin(Stdio::from(tty)).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("<FILE>"), "{stderr}");
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn image_paths_are_relative_to_the_current_directory() {
    let dir = Scratch::new("stdin-images");
    std::fs::copy(fixtures().join("images/icon.png"), dir.join("icon.png")).unwrap();
    let out = kiri_piped(&dir, &["--mode", "kitty"], b"![Icon](icon.png)\n");
    assert_succeeded(&out);
    let stdout = text(&out.stdout);
    assert_eq!(crate::common::sent(&stdout).len(), 1, "{stdout:?}");
    assert!(!stdout.contains("[image"), "{stdout:?}");

    let out = kiri_piped(&dir, &["--mode", "kitty"], b"![Lost](lost.png)\n");
    assert!(out.status.success());
    assert_eq!(text(&out.stderr), "kiri: lost.png: file not found\n");
}

#[test]
fn an_image_on_standard_input_is_shown_on_its_own() {
    let dir = Scratch::new("stdin-image");
    let icon = std::fs::read(fixtures().join("images/icon.png")).unwrap();
    for args in [&["--mode", "kitty"][..], &["--mode", "kitty", "-"]] {
        let out = kiri_piped(&dir, args, &icon);
        assert_succeeded(&out);
        let stdout = text(&out.stdout);
        assert_eq!(stdout.matches("\x1b_Ga=T,").count(), 1, "{stdout:?}");
    }
    // SVG is text, so only the format tells it from Markdown.
    let svg = std::fs::read(fixtures().join("images/pattern.svg")).unwrap();
    let out = kiri_piped(&dir, &["--mode", "kitty", "--format", "svg"], &svg);
    assert_succeeded(&out);
    assert_eq!(text(&out.stdout).matches("\x1b_Ga=T,").count(), 1);
}

#[test]
fn format_makes_standard_input_markdown() {
    let dir = Scratch::new("stdin-format");
    let out = kiri_piped(
        &dir,
        &["--mode", "plain", "--format", "markdown"],
        b"P1 *text*\n",
    );
    assert_succeeded(&out);
    assert_eq!(text(&out.stdout).trim(), "P1 text");
}

/// Errors and warnings call standard input `<stdin>`.
#[test]
fn errors_name_standard_input() {
    let dir = Scratch::new("stdin-errors");
    let icon = std::fs::read(fixtures().join("images/icon.png")).unwrap();
    let photo = std::fs::read(fixtures().join("images/photo-cmyk.jpg")).unwrap();
    for (args, stdin, error) in [
        (
            &["--mode", "kitty"][..],
            &b"\x00\x01\x02\xff binary"[..],
            "neither Markdown nor an image kiri knows",
        ),
        (
            &["--mode", "kitty", "--format", "markdown"],
            b"\xff\xfe",
            "not UTF-8 text",
        ),
        (
            &["--mode", "plain"],
            &icon,
            "the terminal cannot show images",
        ),
        (
            &["--mode", "kitty", "--max-image-size", "1"],
            &photo,
            "larger than the 1 MB limit; see --max-image-size",
        ),
    ] {
        let out = kiri_piped(&dir, args, stdin);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(text(&out.stderr), format!("kiri: <stdin>: {error}\n"));
        assert_eq!(text(&out.stdout), "", "{args:?}");
    }
}

#[test]
fn doctor_does_not_read_standard_input() {
    let dir = Scratch::new("stdin-doctor");
    let mut doctor = command(&dir);
    doctor.arg("--doctor");
    // SAFETY: setsid is async-signal-safe and touches no memory.
    unsafe {
        // No controlling terminal to ask, whatever the tests run in.
        doctor.pre_exec(|| {
            rustix::process::setsid()?;
            Ok(())
        });
    }
    let out = piped(doctor, b"# Not a report\n");
    assert_succeeded(&out);
    let report = text(&out.stdout);
    assert!(report.contains("## Terminal"), "{report}");
    assert!(!report.contains("Not a report"), "{report}");

    let out = kiri_piped(&dir, &["--doctor", "-"], b"");
    assert_eq!(out.status.code(), Some(2));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("cannot be used with"), "{stderr}");
}

/// The terminal is asked on the controlling terminal, so the questions do
/// not go to the pipe and the answers do not come from it. The fake
/// terminal answers like kitty, so kiri uses its features: a large heading
/// and an image.
#[test]
fn the_terminal_is_asked_on_the_controlling_terminal_not_standard_input() {
    let dir = Scratch::new("stdin-probe");
    std::fs::copy(fixtures().join("images/icon.png"), dir.join("icon.png")).unwrap();
    let (mut terminal, tty) = crate::common::pty();
    let window = rustix::termios::Winsize {
        ws_col: 80,
        ws_row: 24,
        ws_xpixel: 720,
        ws_ypixel: 456,
    };
    rustix::termios::tcsetwinsize(&tty, window).unwrap();
    let mut kiri = command(&dir);
    kiri.current_dir(&*dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(tty.try_clone().unwrap()))
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
    drop(kiri);
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"# Title\n\n![Icon](icon.png)\n").unwrap();
    drop(stdin);

    let mut answered = false;
    // kiri gets 10 s at most.
    let seen = read_terminal(
        &mut terminal,
        tty,
        &mut child,
        Duration::from_secs(10),
        |seen, terminal, _| {
            if !answered && seen.windows(3).any(|w| w == b"\x1b[c") {
                terminal
                    .write_all(
                        b"\x1b[5;1R\x1b[5;3R\x1b[5;5R\x1b_Gi=31;OK\x1b\\\
                          \x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\\x1bP>|kitty(0.47.1)\x1b\\\
                          \x1b[?62;c",
                    )
                    .unwrap();
                answered = true;
            }
        },
    );
    let status = child.wait().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(status.success(), "{stderr}");
    assert_eq!(stderr, "");
    assert!(answered, "{:?}", text(&seen));
    // The document comes after the queries.
    let seen = text(&seen);
    let (_, output) = seen.split_once("\x1b[c").unwrap();
    assert!(output.contains("\x1b]66;s="), "{output:?}");
    assert_eq!(crate::common::sent(output).len(), 1, "{output:?}");
}
