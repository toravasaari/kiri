//! `kiri --list-emojis` end to end.

use std::process::Output;

use crate::common::{Scratch, command, text};

/// `kiri --list-emojis` with `args`, its output in a pipe.
fn list(args: &[&str]) -> Output {
    let dir = Scratch::new("list-emojis");
    command(&dir)
        .arg("--list-emojis")
        .args(args)
        .output()
        .unwrap()
}

/// In a pipe, each emoji is a line of its own: the emoji, its shortcodes
/// and its name, separated by tabs.
#[test]
fn a_pipe_gets_one_line_per_emoji() {
    let out = list(&[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout
            .lines()
            .any(|line| line == "😄\tsmile\tgrinning face with smiling eyes"),
        "{stdout}"
    );
    assert!(stdout.lines().count() > 1000, "{stdout}");
    for line in stdout.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 3, "{line:?}");
        assert!(fields.iter().all(|field| !field.is_empty()), "{line:?}");
    }
}

/// --list-emojis shows no file, so a file or a format is a usage error.
#[test]
fn a_file_or_a_format_is_a_usage_error() {
    for args in [&["README.md"][..], &["--format", "csv"], &["--doctor"]] {
        let out = list(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let stderr = text(&out.stderr);
        assert!(stderr.contains("cannot be used with"), "{stderr}");
    }
}

/// The document a terminal gets has a table for each group. The snapshot
/// starts at the first group, leaving out the count of emoji above it,
/// which changes with the emojis crate.
#[test]
fn the_document_starts_with_the_first_group() {
    let document = kiri::list::emoji_document();
    let start: Vec<_> = document
        .lines()
        .skip_while(|line| !line.starts_with("## "))
        .take(10)
        .collect();
    insta::assert_snapshot!(start.join("\n"));
}

/// What `kiri --list-emojis` with `args` writes into a terminal until
/// `enough` says so, and kiri is killed then: reading mode takes long to
/// lay the whole list out, and the test only needs to see which mode it
/// starts in. The list is printed, not paged, so that no keys are needed.
fn in_terminal(args: &[&str], enough: impl Fn(&str) -> bool) -> String {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::Duration;

    let dir = Scratch::new("list-emojis-terminal");
    let (mut terminal, tty) = crate::common::pty();
    let mut command = command(&dir);
    command
        .env_remove("KIRI_READING")
        .args(["--list-emojis", "-P", "--mode", "kitty"])
        .args(args)
        .stdout(Stdio::from(tty.try_clone().unwrap()));
    // SAFETY: setsid is async-signal-safe and touches no memory.
    unsafe {
        command.pre_exec(|| {
            rustix::process::setsid()?;
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    drop(command);
    let pid = rustix::process::Pid::from_child(&child);
    let output = crate::common::read_terminal(
        &mut terminal,
        tty,
        &mut child,
        Duration::from_secs(60),
        |seen, _, _| {
            if enough(&text(seen)) {
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            }
        },
    );
    let _ = child.wait();
    text(&output)
}

/// The start of kitty's graphics sequences: reading mode draws its text
/// as images, and the text layout draws none for a list of emoji.
const GRAPHICS: &str = "\x1b_G";

/// In a terminal that draws images the list is text, to copy from, unless
/// reading mode is named.
#[test]
fn a_terminal_gets_the_list_as_text_unless_reading_mode_is_named() {
    let output = in_terminal(&[], |seen| seen.contains(GRAPHICS));
    assert!(!output.contains(GRAPHICS), "reading mode: {output:.300?}");
    assert!(output.contains("smile"), "{output:.300?}");
}

#[test]
fn a_terminal_gets_the_list_in_reading_mode_when_it_is_named() {
    let output = in_terminal(&["--reading"], |seen| seen.contains(GRAPHICS));
    assert!(output.contains(GRAPHICS), "no image: {output:.300?}");
}
