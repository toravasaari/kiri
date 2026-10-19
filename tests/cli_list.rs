//! `kiri --list-emojis` end to end.

use std::process::Output;

mod common;

use common::{Scratch, command, text};

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

/// The document a terminal gets starts with a title and a table for the
/// first group.
#[test]
fn the_document_starts_with_the_first_group() {
    let document = kiri::list::emoji_document();
    let start: Vec<_> = document.lines().take(14).collect();
    insta::assert_snapshot!(start.join("\n"));
}
