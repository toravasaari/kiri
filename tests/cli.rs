//! The `kiri` binary end to end: options, loading and warnings.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A directory of its own for one test, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("kiri-cli-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The kiri binary with nothing from the environment of the tests: no
/// colour settings, and the home and configuration directories in `dir`,
/// so the user's theme stays out.
fn command(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kiri"));
    command
        .env_clear()
        .env("HOME", dir.join("home"))
        .env("XDG_CONFIG_HOME", dir.join("config"));
    command
}

/// Runs kiri on a document of `source` in `dir`, with `args` before the file.
fn kiri(dir: &Path, source: &str, args: &[&str]) -> Output {
    let document = dir.join("document.md");
    std::fs::write(&document, source).unwrap();
    command(dir).args(args).arg(&document).output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

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
fn reads_the_theme_from_the_configuration_directory() {
    let dir = Scratch::new("theme");
    let theme = dir.join("config/kiri/theme.toml");
    std::fs::create_dir_all(theme.parent().unwrap()).unwrap();
    std::fs::write(&theme, "not a theme").unwrap();
    let out = kiri(&dir, "Text\n", &[]);
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains(&theme.display().to_string()), "{stderr}");
}

#[test]
fn help_explains_max_image_size() {
    let dir = Scratch::new("help");
    let out = command(&dir).arg("--help").output().unwrap();
    let help = text(&out.stdout);
    assert!(help.contains("--max-image-size <MB>"), "{help}");
    assert!(help.contains("[default: 10]"), "{help}");
}

#[test]
fn reads_the_theme_from_home_without_a_configuration_directory() {
    let dir = Scratch::new("theme-home");
    let theme = dir.join("home/.config/kiri/theme.toml");
    std::fs::create_dir_all(theme.parent().unwrap()).unwrap();
    std::fs::write(&theme, "not a theme").unwrap();
    for xdg in [None, Some("")] {
        let document = dir.join("document.md");
        std::fs::write(&document, "Text\n").unwrap();
        let mut command = command(&dir);
        match xdg {
            Some(value) => command.env("XDG_CONFIG_HOME", value),
            None => command.env_remove("XDG_CONFIG_HOME"),
        };
        let out = command.arg(&document).output().unwrap();
        assert!(!out.status.success(), "{xdg:?}");
        let stderr = text(&out.stderr);
        assert!(stderr.contains(&theme.display().to_string()), "{stderr}");
    }
}

/// A theme file at `path` that kiri refuses, so its error message tells
/// which theme kiri chose.
fn broken_theme(path: &Path) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "not a theme").unwrap();
    path.to_owned()
}

/// Writes `text` to `path`, creating its directory.
fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Fails unless kiri failed with an error that names `path`.
fn assert_fails_naming(out: &Output, path: &Path) {
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(
        stderr.starts_with(&format!("kiri: {}: ", path.display())),
        "{stderr}"
    );
}

#[test]
fn the_configuration_file_chooses_the_theme_relative_to_itself() {
    let dir = Scratch::new("config-theme");
    let theme = broken_theme(&dir.join("config/kiri/themes/mine.toml"));
    write(
        &dir.join("config/kiri/config.toml"),
        "theme = \"themes/mine.toml\"\n",
    );
    assert_fails_naming(&kiri(&dir, "Text\n", &[]), &theme);
}

#[test]
fn the_configuration_file_theme_comes_before_theme_toml() {
    let dir = Scratch::new("config-before-theme-toml");
    broken_theme(&dir.join("config/kiri/theme.toml"));
    let theme = broken_theme(&dir.join("mine.toml"));
    write(
        &dir.join("config/kiri/config.toml"),
        &format!("theme = \"{}\"\n", theme.display()),
    );
    assert_fails_naming(&kiri(&dir, "Text\n", &[]), &theme);
}

#[test]
fn the_theme_option_comes_before_the_configuration_file() {
    let dir = Scratch::new("option-before-config");
    broken_theme(&dir.join("config/kiri/mine.toml"));
    write(
        &dir.join("config/kiri/config.toml"),
        "theme = \"mine.toml\"\n",
    );
    let theme = broken_theme(&dir.join("option.toml"));
    let out = kiri(&dir, "Text\n", &["--theme", theme.to_str().unwrap()]);
    assert_fails_naming(&out, &theme);
}

#[test]
fn kiri_config_names_another_configuration_file() {
    let dir = Scratch::new("kiri-config");
    broken_theme(&dir.join("config/kiri/default.toml"));
    write(
        &dir.join("config/kiri/config.toml"),
        "theme = \"default.toml\"\n",
    );
    let theme = broken_theme(&dir.join("other/theme.toml"));
    let config = dir.join("other/kiri.toml");
    write(&config, "theme = \"theme.toml\"\n");
    let document = dir.join("document.md");
    std::fs::write(&document, "Text\n").unwrap();
    let out = command(&dir)
        .env("KIRI_CONFIG", &config)
        .arg(&document)
        .output()
        .unwrap();
    assert_fails_naming(&out, &theme);
}

#[test]
fn a_configuration_file_that_kiri_config_names_must_exist() {
    let dir = Scratch::new("kiri-config-missing");
    let config = dir.join("missing.toml");
    let document = dir.join("document.md");
    std::fs::write(&document, "Text\n").unwrap();
    let out = command(&dir)
        .env("KIRI_CONFIG", &config)
        .arg(&document)
        .output()
        .unwrap();
    assert_fails_naming(&out, &config);
}

#[test]
fn a_mistake_in_the_configuration_file_is_an_error() {
    let dir = Scratch::new("config-mistake");
    let config = dir.join("config/kiri/config.toml");
    write(&config, "colour = \"blue\"\n");
    let out = kiri(&dir, "Text\n", &[]);
    assert_fails_naming(&out, &config);
    assert!(text(&out.stderr).contains("unknown field `colour`"));
}

#[test]
fn renders_with_an_empty_configuration_file() {
    let dir = Scratch::new("config-empty");
    write(&dir.join("config/kiri/config.toml"), "");
    let out = kiri(&dir, "Text\n", &["--mode", "plain"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout).trim(), "Text");
}

#[test]
fn kiri_theme_comes_between_the_option_and_the_configuration_file() {
    let dir = Scratch::new("kiri-theme");
    broken_theme(&dir.join("config/kiri/mine.toml"));
    write(
        &dir.join("config/kiri/config.toml"),
        "theme = \"mine.toml\"\n",
    );
    let env_theme = broken_theme(&dir.join("env.toml"));
    let option_theme = broken_theme(&dir.join("option.toml"));
    let document = dir.join("document.md");
    std::fs::write(&document, "Text\n").unwrap();
    let run = |args: &[&std::ffi::OsStr]| {
        command(&dir)
            .env("KIRI_THEME", &env_theme)
            .args(args)
            .arg(&document)
            .output()
            .unwrap()
    };
    assert_fails_naming(&run(&[]), &env_theme);
    let out = run(&["--theme".as_ref(), option_theme.as_os_str()]);
    assert_fails_naming(&out, &option_theme);
}

/// The background colour code of the light and the dark paper theme.
const LIGHT_PAGE: &str = "48;2;255;255;255m";
const DARK_PAGE: &str = "48;2;30;30;30m";

#[test]
fn light_and_dark_draw_the_document_on_a_page() {
    let dir = Scratch::new("paper");
    for (option, page) in [("--light", LIGHT_PAGE), ("--dark", DARK_PAGE)] {
        let out = kiri(&dir, "Text\n", &["--mode", "kitty", option]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        let stdout = text(&out.stdout);
        assert!(stdout.contains(page), "{option}: {stdout:?}");
    }
}

#[test]
fn light_and_dark_cannot_be_given_together() {
    let dir = Scratch::new("paper-both");
    let out = kiri(&dir, "Text\n", &["--light", "--dark"]);
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("cannot be used with"), "{stderr}");
}

#[test]
fn a_paper_theme_comes_before_kiri_theme_and_the_configuration_file() {
    let dir = Scratch::new("paper-before-env");
    broken_theme(&dir.join("config/kiri/mine.toml"));
    write(
        &dir.join("config/kiri/config.toml"),
        "theme = \"mine.toml\"\n",
    );
    let env_theme = broken_theme(&dir.join("env.toml"));
    let document = dir.join("document.md");
    std::fs::write(&document, "Text\n").unwrap();
    let out = command(&dir)
        .env("KIRI_THEME", &env_theme)
        .args(["--mode", "kitty", "--light"])
        .arg(&document)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains(LIGHT_PAGE));
}

#[test]
fn the_theme_option_comes_before_a_paper_theme() {
    let dir = Scratch::new("theme-before-paper");
    let theme = broken_theme(&dir.join("option.toml"));
    let out = kiri(
        &dir,
        "Text\n",
        &["--light", "--theme", theme.to_str().unwrap()],
    );
    assert_fails_naming(&out, &theme);
}

#[test]
fn light_and_dark_choose_the_code_colours_for_another_theme_too() {
    let dir = Scratch::new("paper-code");
    let theme = dir.join("mine.toml");
    write(
        &theme,
        "code_theme_light = \"InspiredGitHub\"\ncode_theme_dark = \"base16-ocean.dark\"\n",
    );
    let code = "```\ncode\n```\n";
    let theme = theme.to_str().unwrap();
    // The background of InspiredGitHub and of base16-ocean.dark.
    for (option, background) in [
        ("--light", "48;2;255;255;255m"),
        ("--dark", "48;2;43;48;59m"),
    ] {
        let out = kiri(&dir, code, &["--mode", "kitty", "--theme", theme, option]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        let stdout = text(&out.stdout);
        assert!(stdout.contains(background), "{option}: {stdout:?}");
    }
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
