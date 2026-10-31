//! Reading mode from the command line: `--reading`.

use std::process::Command;

use crate::common::{Scratch, assert_succeeded, command, kiri, text, write};

const DOCUMENT: &str = "# Title\n\nSome *text*.\n\n> [!NOTE]\n> A note.\n";

const NEEDS_IMAGES: &str =
    "kiri: reading mode needs a terminal that draws images; showing the document as text\n";

#[test]
fn reading_mode_draws_the_text_as_images() {
    let dir = Scratch::new("reading");
    let out = kiri(
        &dir,
        DOCUMENT,
        &["--reading", "--mode", "kitty", "--width", "80"],
    );
    assert_succeeded(&out);
    let shown = text(&out.stdout);
    assert!(shown.contains("\x1b_Ga=T"), "no image sent");
    assert!(shown.contains('\u{10EEEE}'), "no placeholders");
    assert!(!shown.contains("Title"), "the heading as text");
    assert!(!shown.contains("A note"), "the alert as text");
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn text_in_any_script_leaves_standard_error_alone() {
    let dir = Scratch::new("reading-scripts");
    let source = "日本語のテキスト, ไทย, ខ្មែរ and ລາວ.\n";
    let out = kiri(
        &dir,
        source,
        &["--reading", "--mode", "kitty", "--width", "80"],
    );
    assert_succeeded(&out);
}

#[test]
fn without_images_reading_mode_warns_and_shows_the_text() {
    let dir = Scratch::new("reading-ansi");
    let args = ["--mode", "ansi", "--width", "80"];
    let plain = kiri(&dir, DOCUMENT, &args);
    let reading = kiri(&dir, DOCUMENT, &[&["--reading"][..], &args].concat());
    assert!(reading.status.success());
    assert_eq!(text(&reading.stderr), NEEDS_IMAGES);
    assert_eq!(reading.stdout, plain.stdout);
}

#[test]
fn several_files_warn_once() {
    let dir = Scratch::new("reading-files");
    let (one, two) = (dir.join("one.md"), dir.join("two.md"));
    std::fs::write(&one, DOCUMENT).unwrap();
    std::fs::write(&two, DOCUMENT).unwrap();
    let out = crate::common::command(&dir)
        .args(["--reading", "--mode", "ansi"])
        .args([&one, &two])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(text(&out.stderr), NEEDS_IMAGES);
}

/// `--no-reading` turns reading mode off whatever the variable and the file
/// say, and it cannot be given with `--reading`.
#[test]
fn no_reading_turns_it_off_whatever_the_variable_and_the_file_say() {
    let dir = Scratch::new("reading-no");
    let document = dir.join("document.md");
    std::fs::write(&document, "# Title\n\nSome text.\n").unwrap();
    write(&dir.join("config/kiri/config.toml"), "reading = true\n");
    let draws_images = |command: &mut Command| {
        let out = command
            .args(["--mode", "kitty", "--width", "80"])
            .arg(&document)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out.stderr));
        text(&out.stdout).contains("\x1b_Ga=T")
    };
    assert!(
        draws_images(&mut command(&dir)),
        "reading = true in the file"
    );
    assert!(
        !draws_images(command(&dir).env("KIRI_READING", "1").arg("--no-reading")),
        "--no-reading comes before KIRI_READING and the file"
    );
    let out = command(&dir)
        .args(["--reading", "--no-reading"])
        .arg(&document)
        .output()
        .unwrap();
    assert!(!out.status.success(), "--reading and --no-reading conflict");
}

/// Reading mode is off unless the flag, the variable or the file sets it,
/// and the flag comes before the variable, which comes before the file.
#[test]
fn reading_comes_from_the_flag_the_variable_or_the_file() {
    let dir = Scratch::new("reading-levels");
    let document = dir.join("document.md");
    std::fs::write(&document, "# Title\n\nSome text.\n").unwrap();
    // Reading mode draws the heading as an image; without it there is none.
    let draws_images = |command: &mut Command| {
        let out = command
            .args(["--mode", "kitty", "--width", "80"])
            .arg(&document)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out.stderr));
        text(&out.stdout).contains("\x1b_Ga=T")
    };
    assert!(!draws_images(&mut command(&dir)), "off unless set");
    assert!(
        draws_images(command(&dir).env("KIRI_READING", "1")),
        "KIRI_READING=1"
    );
    assert!(draws_images(command(&dir).arg("--reading")), "--reading");
    write(&dir.join("config/kiri/config.toml"), "reading = true\n");
    assert!(
        draws_images(&mut command(&dir)),
        "reading = true in the file"
    );
    assert!(
        !draws_images(command(&dir).env("KIRI_READING", "0")),
        "KIRI_READING=0 comes before the file"
    );
    assert!(
        draws_images(command(&dir).env("KIRI_READING", "0").arg("--reading")),
        "--reading comes before KIRI_READING"
    );
}
