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
