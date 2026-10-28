//! Reading mode from the command line: `--reading`.

mod common;

use common::{Scratch, assert_succeeded, kiri, text};

const DOCUMENT: &str = "# Title\n\nSome *text*.\n\n```\ncode\n```\n";

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
    assert!(shown.contains("\x1b_Ga=T"), "{shown:?}");
    assert!(shown.contains('\u{10EEEE}'), "{shown:?}");
    assert!(!shown.contains("Title"), "{shown:?}");
    // The code block shows as text, as without reading mode.
    assert!(shown.contains("code"), "{shown:?}");
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
    let out = common::command(&dir)
        .args(["--reading", "--mode", "ansi"])
        .args([&one, &two])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(text(&out.stderr), NEEDS_IMAGES);
}
