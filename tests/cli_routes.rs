//! What differs between kiri on one file and kiri on several, as a pipe
//! sees it. These tests pin the differences that stay.

use std::path::Path;
use std::process::Output;

mod common;

use common::{Scratch, command, fixtures, sent, text, write};

/// kiri in `dir` with the environment `env` and `args`, as a pipe gets it.
fn kiri_with(dir: &Path, env: &[(&str, &str)], args: &[&str]) -> Output {
    command(dir)
        .current_dir(dir)
        .envs(env.iter().copied())
        .args(args)
        .output()
        .unwrap()
}

/// `a.md` and `b.md` of a line each, and `icon.png` and `other.png`.
fn scratch(name: &str) -> Scratch {
    let dir = Scratch::new(name);
    write(&dir.join("a.md"), "Alpha text.\n");
    write(&dir.join("b.md"), "Beta text.\n");
    for image in ["icon.png", "other.png"] {
        std::fs::copy(fixtures().join("images/icon.png"), dir.join(image)).unwrap();
    }
    dir
}

/// What the unknown paging in `KIRI_PAGING` is told as.
const PAGING: &str = "kiri: KIRI_PAGING: `joo` is not always, auto, never, 1 or 0; left out\n";

#[test]
fn the_settings_warnings_are_told_for_a_document_alone_and_for_several_files() {
    let dir = scratch("routes-settings-warnings");
    let env = [("KIRI_PAGING", "joo")];
    for files in [&["a.md"][..], &["a.md", "b.md"], &["icon.png", "other.png"]] {
        let args = [&["--mode", "kitty"], files].concat();
        let out = kiri_with(&dir, &env, &args);
        assert!(out.status.success(), "{files:?}: {}", text(&out.stderr));
        assert_eq!(text(&out.stderr), PAGING, "{files:?}");
    }
}

#[test]
fn an_image_alone_that_is_printed_leaves_the_settings_warnings_untold() {
    let dir = scratch("routes-settings-warnings-image");
    let env = [("KIRI_PAGING", "joo")];
    let out = kiri_with(&dir, &env, &["--mode", "kitty", "icon.png"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    assert_eq!(sent(&text(&out.stdout)).len(), 1);
}

#[test]
fn a_file_alone_that_cannot_be_opened_is_told_before_any_settings_warning() {
    let dir = scratch("routes-missing");
    let env = [("KIRI_PAGING", "joo")];
    let out = kiri_with(&dir, &env, &["--mode", "plain", "missing.md"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.starts_with("kiri: missing.md: "), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    // Among several files, the warnings come first and the file is told at
    // its place.
    let out = kiri_with(&dir, &env, &["--mode", "plain", "a.md", "missing.md"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    let told = stderr.strip_prefix(PAGING);
    assert!(
        told.is_some_and(|told| told.starts_with("kiri: missing.md: ")),
        "{stderr}"
    );
}

/// What kiri says when reading mode is asked for and cannot be shown.
const READING: &str =
    "kiri: reading mode needs a terminal that draws images; showing the document as text\n";

#[test]
fn reading_mode_that_cannot_be_shown_is_told_before_a_document_is_written() {
    let dir = scratch("routes-reading");
    for files in [&["a.md"][..], &["a.md", "b.md"]] {
        let args = [&["--mode", "plain", "--reading"], files].concat();
        let out = kiri_with(&dir, &[], &args);
        assert!(out.status.success(), "{files:?}: {}", text(&out.stderr));
        assert_eq!(text(&out.stderr), READING, "{files:?}");
    }
}

#[test]
fn reading_mode_is_not_told_for_text_written_as_it_is_alone_but_is_among_several() {
    let dir = scratch("routes-reading-as-is");
    write(&dir.join("c.txt"), "  indented *text*\n");
    let args = ["--mode", "plain", "--reading", "--format", "text"];
    let out = kiri_with(&dir, &[], &[&args[..], &["c.txt"]].concat());
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "  indented *text*\n");
    assert_eq!(text(&out.stderr), "");
    let out = kiri_with(&dir, &[], &[&args[..], &["c.txt", "a.md"]].concat());
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), READING);
}

#[test]
fn the_order_of_the_front_matter_and_reading_warnings_differs_for_one_file_and_several() {
    let dir = scratch("routes-warning-order");
    let invalid = std::fs::read_to_string(fixtures().join("front-matter-invalid.md")).unwrap();
    write(&dir.join("c.md"), &invalid);
    let args = ["--mode", "ansi", "--reading"];
    // One file: the front matter is read first, then reading mode is
    // found not to be shown.
    let out = kiri_with(&dir, &[], &[&args[..], &["c.md"]].concat());
    let stderr = text(&out.stderr);
    let lines: Vec<_> = stderr.lines().collect();
    assert_eq!(lines.len(), 2, "{stderr}");
    assert!(
        lines[0].starts_with("kiri: front matter, line 4: "),
        "{stderr}"
    );
    assert_eq!(format!("{}\n", lines[1]), READING);
    // Several files: reading mode is told once for all of them, before the
    // first is written, and a file's own warning at its place.
    let out = kiri_with(&dir, &[], &[&args[..], &["a.md", "c.md"]].concat());
    let stderr = text(&out.stderr);
    let lines: Vec<_> = stderr.lines().collect();
    assert_eq!(lines.len(), 2, "{stderr}");
    assert_eq!(format!("{}\n", lines[0]), READING);
    assert!(
        lines[1].starts_with("kiri: c.md: front matter, line 4: "),
        "{stderr}"
    );
}

/// A configuration file in `dir` whose command draws every Mermaid
/// diagram as an empty SVG 200 × 40 CSS pixels, and `flow.mmd`.
fn draws_mermaid(dir: &Path) {
    let script = dir.join("render.sh");
    write(
        &script,
        "printf '<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"40\"/>' > \"$2\"\n",
    );
    let config = format!(
        "[diagrams.mermaid]\ncommand = [\"/bin/sh\", \"{}\", \"{{input}}\", \"{{output}}\"]\n",
        script.display()
    );
    write(&dir.join("config/kiri/config.toml"), &config);
    write(&dir.join("flow.mmd"), "flowchart TD\n    A --> B\n");
}

/// Whether `stdout` sets a background colour.
fn has_background(stdout: &str) -> bool {
    stdout.contains("48;2;")
}

#[test]
fn an_image_alone_is_printed_without_the_page_a_diagram_alone_gets() {
    let dir = scratch("routes-page");
    draws_mermaid(&dir);
    let page = |files: &[&str]| {
        let args = [&["--mode", "kitty", "--light"], files].concat();
        let out = kiri_with(&dir, &[], &args);
        assert!(out.status.success(), "{files:?}: {}", text(&out.stderr));
        text(&out.stdout)
    };
    // The image fills no rows beside it with the page's colour; the
    // diagram does, as a document's lines are.
    let image = page(&["icon.png"]);
    assert_eq!(sent(&image).len(), 1, "{image:?}");
    assert!(!has_background(&image), "{image:?}");
    let diagram = page(&["flow.mmd"]);
    assert_eq!(sent(&diagram).len(), 1, "{diagram:?}");
    assert!(has_background(&diagram), "{diagram:?}");
    // Among several files, each is a file of a document's look under its
    // header.
    let files = page(&["icon.png", "flow.mmd"]);
    assert_eq!(sent(&files).len(), 2, "{files:?}");
    assert!(files.contains("icon.png"), "{files:?}");
    assert!(has_background(&files), "{files:?}");
}

#[test]
fn a_diagram_file_alone_is_drawn_with_images_off_as_among_several() {
    let dir = scratch("routes-diagram-no-images");
    draws_mermaid(&dir);
    for files in [&["flow.mmd"][..], &["flow.mmd", "a.md"]] {
        let args = [&["--mode", "kitty", "-I"], files].concat();
        let out = kiri_with(&dir, &[], &args);
        assert!(out.status.success(), "{files:?}: {}", text(&out.stderr));
        assert_eq!(sent(&text(&out.stdout)).len(), 1, "{files:?}");
    }
}

#[test]
fn a_diagram_file_alone_that_fails_to_draw_shows_its_source_and_fails() {
    let dir = scratch("routes-diagram-fails");
    write(&dir.join("render.sh"), "echo 'Parse error' >&2\nexit 1\n");
    let config = format!(
        "render_errors = \"stderr\"\n[diagrams.mermaid]\ncommand = [\"/bin/sh\", \"{}\", \"{{input}}\", \"{{output}}\"]\n",
        dir.join("render.sh").display()
    );
    write(&dir.join("config/kiri/config.toml"), &config);
    write(&dir.join("broken.mmd"), "flowchart TD\n    broken\n");
    for (files, name) in [
        (&["broken.mmd"][..], "kiri: "),
        (&["broken.mmd", "a.md"], "kiri: broken.mmd: "),
    ] {
        let args = [&["--mode", "kitty"], files].concat();
        let out = kiri_with(&dir, &[], &args);
        assert_eq!(out.status.code(), Some(1), "{files:?}");
        let stderr = text(&out.stderr);
        assert!(stderr.starts_with(name), "{stderr}");
        assert!(stderr.contains("broken.mmd: "), "{stderr}");
        assert_eq!(stderr.lines().count(), 1, "{files:?}: {stderr}");
        assert!(text(&out.stdout).contains("broken"), "{files:?}");
    }
}

#[test]
fn the_images_of_a_pipe_alone_are_relative_to_the_current_directory_and_among_several_to_the_pipe()
{
    let dir = scratch("routes-fifo");
    std::fs::create_dir(dir.join("sub")).unwrap();
    let path = dir.join("sub/pipe.md");
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: `name` is a C string that lives through the call.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    // `icon.png` is in the current directory, and not beside the pipe.
    for (files, images) in [(&["sub/pipe.md"][..], 1), (&["sub/pipe.md", "a.md"], 0)] {
        let writer = {
            let path = path.clone();
            std::thread::spawn(move || {
                std::fs::write(path, "![icon](icon.png)\n").unwrap();
            })
        };
        let args = [&["--mode", "kitty"], files].concat();
        let out = kiri_with(&dir, &[], &args);
        writer.join().unwrap();
        assert!(out.status.success(), "{files:?}: {}", text(&out.stderr));
        assert_eq!(sent(&text(&out.stdout)).len(), images, "{files:?}");
    }
}

#[test]
fn reading_mode_is_not_told_for_an_image_or_a_diagram_alone_that_is_printed() {
    let dir = scratch("routes-reading-alone");
    draws_mermaid(&dir);
    let args = ["--mode", "ansi", "--reading"];
    let out = kiri_with(&dir, &[], &[&args[..], &["icon.png"]].concat());
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        "kiri: icon.png: the terminal cannot show images\n"
    );
    let out = kiri_with(&dir, &[], &[&args[..], &["flow.mmd"]].concat());
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    assert!(text(&out.stdout).contains("flowchart"));
}
