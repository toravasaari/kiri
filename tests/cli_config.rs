//! The kiri binary end to end: the configuration file and choosing a
//! theme.

use std::path::{Path, PathBuf};
use std::process::Output;

mod common;

use common::{Scratch, command, kiri, text, write};

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
