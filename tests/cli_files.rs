//! Several files on the command line, shown one after another, each under
//! a header that names it.

use std::path::Path;
use std::process::Output;

mod common;

use common::{Scratch, command, fixtures, kiri_piped, sent, text, write};

/// kiri in `dir` with `args`, as a pipe gets it.
fn kiri_in(dir: &Path, args: &[&str]) -> Output {
    command(dir).current_dir(dir).args(args).output().unwrap()
}

/// Whether `parts` come in this order in `text`.
fn in_order(text: &str, parts: &[&str]) -> bool {
    let mut rest = text;
    for part in parts {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    true
}

/// A scratch directory called `name` with `a.md` and `b.md`.
fn two_documents(name: &str) -> Scratch {
    let dir = Scratch::new(name);
    write(&dir.join("a.md"), "Alpha text.\n");
    write(&dir.join("b.md"), "Beta text.\n");
    dir
}

#[test]
fn several_files_are_shown_in_order_under_their_names() {
    let dir = two_documents("files-order");
    let out = kiri_in(&dir, &["--mode", "plain", "b.md", "a.md"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    let rule = "─".repeat(20);
    assert!(
        in_order(
            &stdout,
            &[
                &rule,
                "b.md",
                &rule,
                "Beta text.",
                &rule,
                "a.md",
                &rule,
                "Alpha text."
            ]
        ),
        "{stdout}"
    );
}

#[test]
fn one_file_has_no_header() {
    let dir = two_documents("files-one");
    let out = kiri_in(&dir, &["--mode", "plain", "a.md"]);
    assert_eq!(text(&out.stdout).trim(), "Alpha text.");
}

#[test]
fn a_file_that_cannot_be_shown_is_told_and_the_others_are_shown() {
    let dir = two_documents("files-missing");
    let out = kiri_in(&dir, &["--mode", "plain", "a.md", "missing.md", "b.md"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.starts_with("kiri: missing.md: "), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    let stdout = text(&out.stdout);
    assert!(
        in_order(&stdout, &["Alpha text.", "Beta text."]),
        "{stdout}"
    );
    assert!(!stdout.contains("missing.md"), "{stdout}");
}

#[test]
fn each_document_s_images_are_relative_to_its_own_directory() {
    let dir = Scratch::new("files-images");
    for (name, width) in [("a", 30), ("b", 50)] {
        write(&dir.join(name).join("notes.md"), "![Figure](fig.png)\n");
        image::RgbaImage::new(width, 2)
            .save(dir.join(name).join("fig.png"))
            .unwrap();
    }
    let out = kiri_in(&dir, &["--mode", "kitty", "a/notes.md", "b/notes.md"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    let widths: Vec<String> = sent(&stdout)
        .iter()
        .map(|sent| sent.keys["s"].clone())
        .collect();
    // In cells of the default 10 × 20 pixels, a pixel is a CSS pixel.
    assert_eq!(widths, ["30", "50"]);
}

#[test]
fn image_files_are_shown_on_their_own_under_their_names() {
    let dir = Scratch::new("files-image-files");
    // Relative names, so that a long checkout path does not wrap the
    // headers.
    let out = command(&dir)
        .current_dir(fixtures().join("images"))
        .args(["--mode", "kitty", "icon.png", "pattern.png"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    assert_eq!(stdout.matches("\x1b_Ga=T,").count(), 2, "{stdout:?}");
    assert!(
        in_order(&stdout, &["icon.png", "\x1b_G", "pattern.png", "\x1b_G"]),
        "{stdout:?}"
    );
}

#[test]
fn image_files_need_a_terminal_that_draws_images() {
    let dir = two_documents("files-image-ansi");
    let icon = fixtures().join("images/icon.png");
    let out = command(&dir)
        .current_dir(&*dir)
        .args(["--mode", "ansi", "a.md"])
        .arg(&icon)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!(
            "kiri: {}: the terminal cannot show images\n",
            icon.display()
        )
    );
    assert!(text(&out.stdout).contains("Alpha text."));
}

#[test]
fn a_directory_among_files_shows_its_readme() {
    let dir = two_documents("files-directory");
    write(&dir.join("project/README.md"), "Project readme.\n");
    let out = kiri_in(&dir, &["--mode", "plain", "a.md", "project"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    let readme = Path::new("project").join("README.md");
    let readme = readme.display().to_string();
    assert!(
        in_order(
            &stdout,
            &["a.md", "Alpha text.", &readme, "Project readme."]
        ),
        "{stdout}"
    );
}

#[test]
fn standard_input_can_be_one_of_the_files() {
    let dir = two_documents("files-stdin");
    let out = kiri_piped(&dir, &["--mode", "plain", "a.md", "-"], b"Piped text.\n");
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        in_order(&stdout, &["a.md", "Alpha text.", "<stdin>", "Piped text."]),
        "{stdout}"
    );
}

#[test]
fn standard_input_can_be_given_only_once() {
    let dir = two_documents("files-stdin-twice");
    let out = kiri_piped(&dir, &["a.md", "-", "-"], b"Piped text.\n");
    assert_eq!(out.status.code(), Some(2));
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("- (standard input) can be given only once"),
        "{stderr}"
    );
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn format_applies_to_every_file() {
    let dir = two_documents("files-format");
    write(&dir.join("c.md"), "# Not a heading\n");
    let out = kiri_in(&dir, &["--mode", "ansi", "-f", "text", "a.md", "c.md"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    // As code, without the heading's style.
    assert!(stdout.contains("Not a heading"), "{stdout:?}");
    assert!(!stdout.contains("4;34mNot a heading"), "{stdout:?}");
}

#[test]
fn text_in_plain_output_is_written_as_it_is_under_its_name() {
    let dir = two_documents("files-text-plain");
    write(&dir.join("c.txt"), "  indented *text*\n");
    let out = kiri_in(&dir, &["--mode", "plain", "-f", "text", "a.md", "c.txt"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        in_order(&stdout, &["c.txt", "\n  indented *text*\n"]),
        "{stdout:?}"
    );
}

#[test]
fn warnings_name_the_file_they_are_about() {
    let dir = two_documents("files-warnings");
    write(&dir.join("c.md"), "![Lost](no-such.png)\n");
    let out = kiri_in(&dir, &["--mode", "kitty", "a.md", "c.md"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stderr),
        "kiri: c.md: no-such.png: file not found\n"
    );
}

#[test]
fn help_tells_about_several_files() {
    let dir = Scratch::new("files-help");
    let out = kiri_in(&dir, &["--help"]);
    let stdout = text(&out.stdout);
    assert!(stdout.contains("[FILE]..."), "{stdout}");
    assert!(
        stdout.contains("Several files are shown one after another"),
        "{stdout}"
    );
}

/// A configuration file in `dir` whose command draws Mermaid as an empty
/// SVG 200 × 40 CSS pixels, and fails on a source that says `broken`.
fn draws_mermaid(dir: &Path) {
    let script = dir.join("render.sh");
    std::fs::write(
        &script,
        "if grep -q broken \"$1\"; then echo 'Parse error' >&2; exit 1; fi\n\
         printf '<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"40\"/>' > \"$2\"\n",
    )
    .unwrap();
    let config = format!(
        "[diagrams.mermaid]\ncommand = [\"/bin/sh\", \"{}\", \"{{input}}\", \"{{output}}\"]\n",
        script.display()
    );
    write(&dir.join("config/kiri/config.toml"), &config);
}

#[test]
fn a_diagram_file_among_files_is_drawn_on_its_own() {
    let dir = two_documents("files-diagram");
    draws_mermaid(&dir);
    write(&dir.join("flow.mmd"), "flowchart TD\n    A --> B\n");
    write(&dir.join("c.md"), "![Icon](icon.png)\n");
    std::fs::copy(fixtures().join("images/icon.png"), dir.join("icon.png")).unwrap();
    for (args, images) in [(&[][..], 2), (&["-I"], 1)] {
        let out = kiri_in(
            &dir,
            &[&["--mode", "kitty", "a.md", "flow.mmd", "c.md"], args].concat(),
        );
        assert!(out.status.success(), "{args:?}: {}", text(&out.stderr));
        assert_eq!(text(&out.stderr), "", "{args:?}");
        let stdout = text(&out.stdout);
        assert_eq!(sent(&stdout).len(), images, "{args:?}");
        assert!(!stdout.contains("flowchart"), "{args:?}: {stdout:?}");
        assert!(
            in_order(&stdout, &["a.md", "flow.mmd", "\x1b_G", "c.md"]),
            "{stdout:?}"
        );
    }
}

#[test]
fn a_diagram_file_that_fails_to_draw_shows_its_source_and_fails() {
    let dir = two_documents("files-diagram-fails");
    draws_mermaid(&dir);
    write(&dir.join("broken.mmd"), "flowchart TD\n    broken\n");
    let out = kiri_in(&dir, &["--mode", "kitty", "broken.mmd", "a.md"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.starts_with("kiri: broken.mmd: "), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    let stdout = text(&out.stdout);
    assert!(
        in_order(&stdout, &["broken.mmd", "broken", "a.md", "Alpha text."]),
        "{stdout:?}"
    );
}

#[test]
fn a_broken_configuration_file_stops_the_documents_but_not_the_images() {
    let dir = two_documents("files-broken-config");
    write(&dir.join("config/kiri/config.toml"), "images = [\n");
    let icon = fixtures().join("images/icon.png");
    let out = command(&dir)
        .current_dir(&*dir)
        .args(["--mode", "kitty", "a.md"])
        .arg(&icon)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    // Told once, as the configuration file's error.
    let stderr = text(&out.stderr);
    assert!(stderr.starts_with("kiri: "), "{stderr}");
    assert!(stderr.contains("config.toml"), "{stderr}");
    assert_eq!(stderr.matches("kiri: ").count(), 1, "{stderr}");
    let stdout = text(&out.stdout);
    assert!(!stdout.contains("Alpha"), "{stdout:?}");
    assert_eq!(sent(&stdout).len(), 1, "{stdout:?}");
}

#[test]
fn more_image_files_than_kiri_can_keep_open_are_shown() {
    use std::os::unix::process::CommandExt;

    let dir = Scratch::new("files-many-images");
    let icon = fixtures().join("images/icon.png");
    let mut kiri = command(&dir);
    kiri.args(["--mode", "kitty"]).args(vec![&icon; 64]);
    // SAFETY: getrlimit and setrlimit are async-signal-safe and touch no
    // memory.
    unsafe {
        kiri.pre_exec(|| {
            use rustix::process::{Resource, getrlimit, setrlimit};
            let mut files = getrlimit(Resource::Nofile);
            files.current = Some(32);
            setrlimit(Resource::Nofile, files)?;
            Ok(())
        });
    }
    let out = kiri.output().unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout).matches("\x1b_Ga=T,").count(), 64);
}
