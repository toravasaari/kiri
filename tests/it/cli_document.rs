//! The kiri binary end to end: reading the document and writing it out.

use std::io::Read;
use std::process::Stdio;

use crate::common::{Scratch, colour_codes, command, fixture, kiri, shown, text, write};

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

#[test]
fn without_colours_marked_text_is_in_reverse_video() {
    // The whole way from NO_COLOR, with a theme that colours marks.
    let dir = Scratch::new("mark-without-colours");
    let theme = dir.join("theme.toml");
    std::fs::write(
        &theme,
        "[mark]\nforeground = \"#101010\"\nbackground = \"#fff8c5\"\n",
    )
    .unwrap();
    let document = dir.join("marked.md");
    std::fs::write(&document, "Some <mark>marked</mark> text\n").unwrap();
    let out = command(&dir)
        .env("NO_COLOR", "1")
        .args(["--mode", "kitty", "--theme"])
        .arg(&theme)
        .arg(&document)
        .output()
        .unwrap();
    let out = shown(&out);
    assert!(out.contains("\x1b[0;7mmarked\x1b[0m"), "{out:?}");
    assert!(colour_codes(&out).is_empty(), "{out:?}");
}

#[test]
fn front_matter_that_does_not_parse_is_a_warning() {
    let dir = Scratch::new("front-matter-warning");
    let out = kiri(
        &dir,
        &fixture("front-matter-invalid.md"),
        &["--mode", "plain"],
    );
    assert!(out.status.success());
    assert_eq!(
        text(&out.stderr),
        "kiri: front matter, line 4: while parsing a flow sequence, expected ',' or ']'\n"
    );
    assert!(text(&out.stdout).contains("tags: [does, not, close"));
}

#[test]
fn a_directory_shows_its_readme_with_images_relative_to_it() {
    let dir = Scratch::new("directory-readme");
    let project = dir.join("project");
    write(
        &project.join("README.md"),
        "Project *readme*.\n\n![Icon](icon.png)\n",
    );
    std::fs::copy(
        crate::common::fixtures().join("images/icon.png"),
        project.join("icon.png"),
    )
    .unwrap();
    let out = command(&dir)
        .current_dir(&*dir)
        .args(["--mode", "kitty", "project"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    assert!(stdout.contains("readme"), "{stdout:?}");
    assert_eq!(crate::common::sent(&stdout).len(), 1, "{stdout:?}");
}

#[test]
fn a_directory_shows_readme_md_before_readme() {
    let dir = Scratch::new("directory-readme-order");
    let shown = |dir: &std::path::Path| {
        let out = command(dir)
            .args(["--mode", "plain"])
            .arg(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out.stderr));
        text(&out.stdout).trim().to_owned()
    };
    write(&dir.join("README"), "Plain *readme*.\n");
    assert_eq!(shown(&dir), "Plain readme.");
    write(&dir.join("README.md"), "Markdown *readme*.\n");
    assert_eq!(shown(&dir), "Markdown readme.");
}

#[test]
fn a_directory_without_a_readme_is_an_error() {
    let dir = Scratch::new("directory-no-readme");
    // Only these exact names count.
    write(&dir.join("readme.md"), "Lower case.\n");
    write(&dir.join("README.markdown"), "Another extension.\n");
    let out = command(&dir).arg(&*dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!("kiri: {}: no README.md or README\n", dir.display())
    );
    assert_eq!(text(&out.stdout), "");
}
