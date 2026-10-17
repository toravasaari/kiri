//! The kiri binary end to end: reading the document and writing it out.

use std::io::Read;
use std::process::Stdio;

mod common;

use common::{Scratch, command, text};

#[test]
fn a_document_that_cannot_be_read_is_an_error() {
    let dir = Scratch::new("no-document");
    let missing = dir.join("missing.md");
    let out = command(&dir).arg(&missing).output().unwrap();
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(
        stderr.starts_with(&format!("kiri: {}: ", missing.display())),
        "{stderr}"
    );
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn a_closed_pipe_is_not_an_error() {
    let dir = Scratch::new("closed-pipe");
    // More than a pipe holds, so kiri is still writing when it closes.
    let document = dir.join("long.md");
    std::fs::write(&document, "A line of text.\n\n".repeat(20_000)).unwrap();
    let mut child = command(&dir)
        .args(["--mode", "plain"])
        .arg(&document)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(child.wait().unwrap().success(), "{stderr}");
    assert_eq!(stderr, "");
}

#[test]
fn no_color_turns_colours_off_in_a_forced_mode_too() {
    let dir = Scratch::new("no-color");
    let document = dir.join("colours.md");
    std::fs::write(&document, "# Title\n\n```rust\nfn main() {}\n```\n").unwrap();
    for (no_color, colours) in [(None, true), (Some(""), true), (Some("1"), false)] {
        let mut kiri = command(&dir);
        if let Some(value) = no_color {
            kiri.env("NO_COLOR", value);
        }
        let out = kiri
            .args(["--mode", "ansi"])
            .arg(&document)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out.stderr));
        let stdout = text(&out.stdout);
        assert_eq!(
            stdout.contains("\x1b[0;38;"),
            colours,
            "{no_color:?}: {stdout:?}"
        );
        // Bold stays.
        assert!(stdout.contains("\x1b[0;1"), "{no_color:?}: {stdout:?}");
    }
}
