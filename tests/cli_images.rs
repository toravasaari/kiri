//! The kiri binary end to end: turning images on and off, the size limit
//! and warnings.

use std::path::{Path, PathBuf};
use std::process::Output;

mod common;

use common::{Scratch, command, fixtures, kiri, kiri_on, text, write};

#[test]
fn warns_about_an_image_that_does_not_load_and_shows_its_alt_text() {
    let dir = Scratch::new("missing");
    let out = kiri(&dir, "![Lost](no-such.png)\n", &["--mode", "kitty"]);
    assert!(out.status.success());
    let missing = dir.join("no-such.png");
    assert_eq!(
        text(&out.stderr),
        format!("kiri: {}: file not found\n", missing.display())
    );
    let stdout = text(&out.stdout);
    // Dim and italic, as everywhere else an image is not drawn.
    assert!(stdout.contains("\x1b[0;2;3m[image: Lost]"), "{stdout:?}");
    assert!(!stdout.contains("\x1b_G"), "{stdout:?}");
}

#[test]
fn warns_about_an_image_url_without_a_file() {
    let dir = Scratch::new("no-path");
    let out = kiri(&dir, "![Nothing](file://)\n", &["--mode", "kitty"]);
    assert!(out.status.success());
    assert_eq!(text(&out.stderr), "kiri: file://: the URL names no file\n");
}

#[test]
fn does_not_warn_about_images_shown_as_alt_text() {
    let dir = Scratch::new("alt-text");
    let source = "Text ![Lost](no-such.png) here.\n\n| ![Cell](no-such.png) |\n|---|\n";
    let out = kiri(&dir, source, &["--mode", "kitty"]);
    assert!(out.status.success());
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    assert!(stdout.contains("[image: Lost]"), "{stdout:?}");
    assert!(stdout.contains("[image: Cell]"), "{stdout:?}");
}

#[test]
fn refuses_image_files_above_ten_megabytes_by_default() {
    let dir = Scratch::new("default-limit");
    // Sparse: the size is all that is read.
    let big = std::fs::File::create(dir.join("big.png")).unwrap();
    big.set_len(10_000_001).unwrap();
    let out = kiri(&dir, "![Big](big.png)\n", &["--mode", "kitty"]);
    assert!(out.status.success());
    assert_eq!(
        text(&out.stderr),
        format!(
            "kiri: {}: larger than the 10 MB limit; see --max-image-size\n",
            dir.join("big.png").display()
        )
    );
}

#[test]
fn max_image_size_changes_the_limit() {
    let dir = Scratch::new("option");
    let photo = fixtures().join("images/photo-cmyk.jpg");
    let source = format!("![Photo]({})\n", photo.display());
    let refused = kiri(&dir, &source, &["--mode", "kitty", "--max-image-size", "1"]);
    let loaded = kiri(&dir, &source, &["--mode", "kitty", "--max-image-size", "2"]);
    assert!(refused.status.success());
    assert_eq!(
        text(&refused.stderr),
        format!(
            "kiri: {}: larger than the 1 MB limit; see --max-image-size\n",
            photo.display()
        )
    );
    assert!(loaded.status.success());
    assert_eq!(text(&loaded.stderr), "");
    assert!(text(&loaded.stdout).contains("\x1b_G"));
}

#[test]
fn max_image_size_is_at_most_a_hundred_megabytes() {
    let dir = Scratch::new("too-large");
    for value in ["101", "0", "ten"] {
        let out = kiri(&dir, "Text\n", &["--max-image-size", value]);
        assert_eq!(out.status.code(), Some(2), "{value}");
        let stderr = text(&out.stderr);
        assert!(
            stderr.contains(&format!(
                "invalid value '{value}' for '--max-image-size <MB>'"
            )),
            "{stderr}"
        );
        assert!(
            stderr.contains("whole number of MB from 1 to 100"),
            "{stderr}"
        );
    }
    let out = kiri(&dir, "Text\n", &["--max-image-size", "100"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
}

#[test]
fn help_explains_max_image_size_and_images() {
    let dir = Scratch::new("help");
    let out = command(&dir).arg("--help").output().unwrap();
    let help = text(&out.stdout);
    assert!(help.contains("--max-image-size <MB>"), "{help}");
    assert!(help.contains("[default: 10]"), "{help}");
    assert!(help.contains("--images"), "{help}");
    assert!(help.contains("-f, --format <FORMAT>"), "{help}");
    assert!(
        help.contains("Images and diagrams are drawn only in terminals"),
        "{help}"
    );
}

/// A document with an image that loads and one that does not, in `dir`.
fn images_document(dir: &Path) -> PathBuf {
    let icon = fixtures().join("images/icon.png");
    let broken = fixtures().join("images/not-an-image.png");
    let document = dir.join("document.md");
    std::fs::write(
        &document,
        format!(
            "![Icon]({})\n\n![Broken]({})\n",
            icon.display(),
            broken.display()
        ),
    )
    .unwrap();
    document
}

/// Fails unless both images of `images_document` are alt text, without
/// warnings: neither was read.
fn assert_images_off(out: &Output) {
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    assert!(stdout.contains("[image: Icon]"), "{stdout:?}");
    assert!(stdout.contains("[image: Broken]"), "{stdout:?}");
    assert!(!stdout.contains("\x1b_G"), "{stdout:?}");
}

/// Fails unless the icon of `images_document` is drawn.
fn assert_images_on(out: &Output) {
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("not-an-image.png"));
    assert!(text(&out.stdout).contains("\x1b_G"));
}

#[test]
fn draws_images_unless_turned_off() {
    let dir = Scratch::new("images-on");
    let document = images_document(&dir);
    let out = command(&dir)
        .args(["--mode", "kitty"])
        .arg(&document)
        .output()
        .unwrap();
    assert_images_on(&out);
}

#[test]
fn no_images_shows_alt_text_without_reading_images() {
    let dir = Scratch::new("no-images");
    let document = images_document(&dir);
    for option in ["--no-images", "-I"] {
        let out = command(&dir)
            .args(["--mode", "kitty", option])
            .arg(&document)
            .output()
            .unwrap();
        assert_images_off(&out);
    }
}

#[test]
fn kiri_no_images_turns_images_off_unless_empty_or_0() {
    let dir = Scratch::new("kiri-no-images");
    let document = images_document(&dir);
    let run = |value: &str| {
        command(&dir)
            .env("KIRI_NO_IMAGES", value)
            .args(["--mode", "kitty"])
            .arg(&document)
            .output()
            .unwrap()
    };
    assert_images_off(&run("1"));
    assert_images_on(&run(""));
    assert_images_on(&run("0"));
}

#[test]
fn the_configuration_file_can_turn_images_off() {
    let dir = Scratch::new("config-images");
    let document = images_document(&dir);
    write(&dir.join("config/kiri/config.toml"), "images = false\n");
    let run = |env: Option<&str>| {
        let mut command = command(&dir);
        if let Some(value) = env {
            command.env("KIRI_NO_IMAGES", value);
        }
        command
            .args(["--mode", "kitty"])
            .arg(&document)
            .output()
            .unwrap()
    };
    assert_images_off(&run(None));
    assert_images_off(&run(Some("")));
    // The environment comes first.
    assert_images_on(&run(Some("0")));
}

#[test]
fn no_images_comes_before_the_environment_and_the_configuration_file() {
    let dir = Scratch::new("no-images-first");
    let document = images_document(&dir);
    write(&dir.join("config/kiri/config.toml"), "images = true\n");
    let out = command(&dir)
        .env("KIRI_NO_IMAGES", "0")
        .args(["--mode", "kitty", "-I"])
        .arg(&document)
        .output()
        .unwrap();
    assert_images_off(&out);
}

#[test]
fn images_comes_before_the_environment_and_the_configuration_file() {
    let dir = Scratch::new("images-first");
    let document = images_document(&dir);
    write(&dir.join("config/kiri/config.toml"), "images = false\n");
    let run = |env: Option<&str>| {
        let mut command = command(&dir);
        if let Some(value) = env {
            command.env("KIRI_NO_IMAGES", value);
        }
        command
            .args(["--mode", "kitty", "--images"])
            .arg(&document)
            .output()
            .unwrap()
    };
    assert_images_on(&run(None));
    assert_images_on(&run(Some("1")));
}

#[test]
fn images_and_no_images_cannot_be_given_together() {
    let dir = Scratch::new("images-both");
    let document = images_document(&dir);
    for no_images in ["-I", "--no-images"] {
        let out = command(&dir)
            .args(["--mode", "kitty", "--images", no_images])
            .arg(&document)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{no_images}");
        assert!(
            text(&out.stderr).contains("cannot be used with"),
            "{}",
            text(&out.stderr)
        );
    }
}

/// Fails unless `out` shows one image on its own and nothing else.
fn assert_image_alone(out: &Output) {
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    // A large image goes in chunks, the first of which places it.
    assert_eq!(stdout.matches("\x1b_Ga=T,").count(), 1, "{stdout:?}");
    assert!(!stdout.contains("[image"), "{stdout:?}");
}

#[test]
fn shows_an_image_file_on_its_own() {
    let dir = Scratch::new("image-file");
    for name in [
        "icon.png",
        "photo-landscape.jpg",
        "animated.gif",
        "lossy.webp",
    ] {
        let out = kiri_on(
            &dir,
            &fixtures().join("images").join(name),
            &["--mode", "kitty"],
        );
        assert_image_alone(&out);
    }
}

#[test]
fn shows_an_svg_file_on_its_own() {
    let dir = Scratch::new("svg-file");
    for name in ["pattern.svg", "pattern.svgz"] {
        let out = kiri_on(
            &dir,
            &fixtures().join("images").join(name),
            &["--mode", "kitty"],
        );
        assert_image_alone(&out);
    }
}

#[test]
fn tells_an_image_by_its_content_whatever_its_name() {
    let dir = Scratch::new("image-content");
    let picture = dir.join("picture");
    std::fs::copy(fixtures().join("images/icon.png"), &picture).unwrap();
    assert_image_alone(&kiri_on(&dir, &picture, &["--mode", "kitty"]));
}

#[test]
fn a_markdown_file_is_markdown_whatever_it_starts_with() {
    let dir = Scratch::new("markdown-content");
    // BM starts a BMP image, P1 a PNM image.
    for (name, source) in [
        ("notes.md", "BMW and Audi\n"),
        ("notes.txt", "BMW and Audi\n"),
        ("notes", "P1 is the first priority\n"),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, source).unwrap();
        let out = kiri_on(&dir, &path, &["--mode", "kitty"]);
        assert!(out.status.success(), "{name}: {}", text(&out.stderr));
        assert_eq!(text(&out.stdout).trim(), source.trim(), "{name}");
    }
}

#[test]
fn an_image_file_is_shown_even_with_images_off() {
    let dir = Scratch::new("image-file-images-off");
    let icon = fixtures().join("images/icon.png");
    assert_image_alone(&kiri_on(&dir, &icon, &["--mode", "kitty", "-I"]));
    let out = command(&dir)
        .env("KIRI_NO_IMAGES", "1")
        .args(["--mode", "kitty"])
        .arg(&icon)
        .output()
        .unwrap();
    assert_image_alone(&out);
    write(&dir.join("config/kiri/config.toml"), "images = false\n");
    assert_image_alone(&kiri_on(&dir, &icon, &["--mode", "kitty"]));
}

#[test]
fn an_image_file_above_the_size_limit_is_an_error() {
    let dir = Scratch::new("image-file-limit");
    let photo = fixtures().join("images/photo-cmyk.jpg");
    let out = kiri_on(&dir, &photo, &["--mode", "kitty", "--max-image-size", "1"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!(
            "kiri: {}: larger than the 1 MB limit; see --max-image-size\n",
            photo.display()
        )
    );
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn an_image_file_that_does_not_decode_is_an_error() {
    let dir = Scratch::new("image-file-broken");
    // Text named .png: the name makes it an image.
    for name in ["not-an-image.png", "truncated.png"] {
        let path = fixtures().join("images").join(name);
        let out = kiri_on(&dir, &path, &["--mode", "kitty"]);
        assert_eq!(out.status.code(), Some(1), "{name}");
        let stderr = text(&out.stderr);
        assert!(
            stderr.starts_with(&format!("kiri: {}: ", path.display())),
            "{stderr}"
        );
        assert_eq!(text(&out.stdout), "", "{name}");
    }
}

#[test]
fn an_image_file_needs_a_terminal_that_draws_images() {
    let dir = Scratch::new("image-file-no-graphics");
    let icon = fixtures().join("images/icon.png");
    for mode in ["plain", "ansi"] {
        let out = kiri_on(&dir, &icon, &["--mode", mode]);
        assert_eq!(out.status.code(), Some(1), "{mode}");
        assert_eq!(
            text(&out.stderr),
            format!(
                "kiri: {}: the terminal cannot show images\n",
                icon.display()
            ),
            "{mode}"
        );
        assert_eq!(text(&out.stdout), "", "{mode}");
    }
}

#[test]
fn a_file_neither_text_nor_an_image_is_an_error() {
    let dir = Scratch::new("binary-file");
    let path = dir.join("data.bin");
    std::fs::write(&path, b"\x00\x01\x02\xff binary").unwrap();
    let out = kiri_on(&dir, &path, &["--mode", "kitty"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!(
            "kiri: {}: neither Markdown nor an image kiri knows\n",
            path.display()
        )
    );
}

#[test]
fn format_tells_what_the_file_is() {
    let dir = Scratch::new("format");
    let notes = dir.join("notes.png");
    std::fs::write(&notes, "BMW and Audi\n").unwrap();
    for option in ["-f", "--format"] {
        let out = kiri_on(&dir, &notes, &["--mode", "kitty", option, "markdown"]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert_eq!(text(&out.stdout).trim(), "BMW and Audi");
    }
    let drawing = dir.join("drawing.xml");
    std::fs::copy(fixtures().join("images/pattern.svg"), &drawing).unwrap();
    assert_image_alone(&kiri_on(&dir, &drawing, &["--mode", "kitty", "-f", "svg"]));
    assert_image_alone(&kiri_on(
        &dir,
        &drawing,
        &["--mode", "kitty", "-f", "image"],
    ));
    let icon = fixtures().join("images/icon.png");
    for format in ["png", "PNG", "image"] {
        assert_image_alone(&kiri_on(&dir, &icon, &["--mode", "kitty", "-f", format]));
    }
}

#[test]
fn a_file_not_in_the_format_given_is_an_error() {
    let dir = Scratch::new("wrong-format");
    let photo = fixtures().join("images/photo-landscape.jpg");
    let readme = fixtures().join("images/README.md");
    for (path, format, message) in [
        (&photo, "png", "broken image: "),
        (&photo, "svg", "broken image: "),
        (&photo, "markdown", "not UTF-8 text"),
        (&readme, "image", "not an image format kiri knows"),
    ] {
        let out = kiri_on(&dir, path, &["--mode", "kitty", "-f", format]);
        assert_eq!(out.status.code(), Some(1), "{format}");
        let stderr = text(&out.stderr);
        let start = format!("kiri: {}: {message}", path.display());
        assert!(stderr.starts_with(&start), "{format}: {stderr}");
        assert_eq!(text(&out.stdout), "", "{format}");
    }
}

#[test]
fn format_takes_only_formats_kiri_knows() {
    let dir = Scratch::new("unknown-format");
    let out = kiri(&dir, "Text\n", &["-f", "docx"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("invalid value 'docx' for '--format <FORMAT>'"),
        "{stderr}"
    );
    assert!(
        stderr.contains(
            "give markdown, csv, tsv, text, image, or an image format such as svg, png or jpg"
        ),
        "{stderr}"
    );
}

#[test]
fn an_image_extension_may_be_in_capitals() {
    let dir = Scratch::new("capitals");
    for (from, to) in [("icon.png", "ICON.PNG"), ("pattern.svg", "PATTERN.SVG")] {
        let path = dir.join(to);
        std::fs::copy(fixtures().join("images").join(from), &path).unwrap();
        assert_image_alone(&kiri_on(&dir, &path, &["--mode", "kitty"]));
    }
}

#[test]
fn an_image_may_come_through_a_pipe() {
    use std::io::Write;
    use std::process::Stdio;

    let dir = Scratch::new("pipe");
    let mut child = command(&dir)
        .args(["--mode", "kitty", "/dev/stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let png = std::fs::read(fixtures().join("images/icon.png")).unwrap();
    child.stdin.take().unwrap().write_all(&png).unwrap();
    assert_image_alone(&child.wait_with_output().unwrap());
}

#[test]
fn a_directory_is_an_error() {
    let dir = Scratch::new("directory");
    for args in [
        &["--mode", "kitty"][..],
        &["--mode", "kitty", "-f", "image"],
    ] {
        let out = kiri_on(&dir, &dir, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        let stderr = text(&out.stderr);
        let start = format!("kiri: {}: ", dir.display());
        assert!(stderr.starts_with(&start), "{args:?}: {stderr}");
    }
}

#[test]
fn an_empty_file_is_an_empty_document_unless_named_as_an_image() {
    let dir = Scratch::new("empty");
    let empty = dir.join("empty");
    std::fs::write(&empty, "").unwrap();
    let out = kiri_on(&dir, &empty, &["--mode", "kitty"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    let empty_png = dir.join("empty.png");
    std::fs::write(&empty_png, "").unwrap();
    let out = kiri_on(&dir, &empty_png, &["--mode", "kitty"]);
    assert_eq!(out.status.code(), Some(1));
    let start = format!("kiri: {}: ", empty_png.display());
    assert!(
        text(&out.stderr).starts_with(&start),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn an_image_file_at_the_size_limit_is_read() {
    let dir = Scratch::new("at-limit");
    // Sparse and no image: the size is all that matters here.
    for (size, refused) in [(1_000_000, false), (1_000_001, true)] {
        let path = dir.join(format!("{size}.png"));
        std::fs::File::create(&path).unwrap().set_len(size).unwrap();
        let out = kiri_on(&dir, &path, &["--mode", "kitty", "--max-image-size", "1"]);
        assert_eq!(out.status.code(), Some(1), "{size}");
        let stderr = text(&out.stderr);
        assert_eq!(
            stderr.contains("larger than the 1 MB limit"),
            refused,
            "{stderr}"
        );
    }
}

#[test]
fn width_limits_an_image_file() {
    let dir = Scratch::new("image-width");
    let photo = fixtures().join("images/photo-landscape.jpg");
    let out = kiri_on(&dir, &photo, &["--mode", "kitty", "--width", "10"]);
    assert_image_alone(&out);
    let stdout = text(&out.stdout);
    assert!(stdout.contains(",c=10,"), "{:?}", &stdout[..200]);
}
