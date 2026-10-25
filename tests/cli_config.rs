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

#[test]
fn no_margins_puts_the_text_at_the_edge() {
    let dir = Scratch::new("no-margins");
    let out = kiri(&dir, "Text\n", &["--mode", "plain"]);
    assert_eq!(text(&out.stdout), "  Text\n");
    for option in ["-M", "--no-margins"] {
        let out = kiri(&dir, "Text\n", &["--mode", "plain", option]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert_eq!(text(&out.stdout), "Text\n", "{option}");
    }
}

#[test]
fn an_unknown_kiri_paging_is_a_warning() {
    let dir = Scratch::new("paging-unknown");
    let document = dir.join("document.md");
    std::fs::write(&document, "Text\n").unwrap();
    let out = command(&dir)
        .env("KIRI_PAGING", "joo")
        .arg(&document)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout).trim(), "Text");
    assert_eq!(
        text(&out.stderr),
        "kiri: KIRI_PAGING: `joo` is not always, auto, never, 1 or 0; left out\n"
    );
}

#[test]
fn the_warnings_come_before_the_error_of_a_theme_that_does_not_load() {
    let dir = Scratch::new("warnings-before-theme");
    let theme = broken_theme(&dir.join("broken.toml"));
    let document = dir.join("document.md");
    std::fs::write(&document, "Text\n").unwrap();
    let out = command(&dir)
        .env("KIRI_PAGING", "joo")
        .env("KIRI_TABLE_ROWS", "stripes")
        .args(["--theme".as_ref(), theme.as_os_str()])
        .arg(&document)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    let warnings = "kiri: KIRI_PAGING: `joo` is not always, auto, never, 1 or 0; left out\n\
         kiri: KIRI_TABLE_ROWS: `stripes` is not off, zebra, lines, indent or underline; \
         left out\n";
    let error = stderr.strip_prefix(warnings);
    assert!(
        error.is_some_and(|error| error.starts_with(&format!("kiri: {}: ", theme.display()))),
        "{stderr}"
    );
}

#[test]
fn the_paging_flags_exclude_each_other() {
    let dir = Scratch::new("paging-flags");
    let document = dir.join("document.md");
    std::fs::write(&document, "Text\n").unwrap();
    for args in [&["-p", "-P"][..], &["-p", "--paging", "never"][..]] {
        let out = command(&dir).args(args).arg(&document).output().unwrap();
        assert!(!out.status.success(), "{args:?}");
        assert!(
            text(&out.stderr).contains("cannot be used with"),
            "{args:?}"
        );
    }
}

/// A document with front matter, and the Markdown after it.
const FRONT_MATTER: &str = "---\ntitle: A\n---\nBody\n";

#[test]
fn front_matter_is_a_table_unless_the_flag_says_otherwise() {
    let dir = Scratch::new("front-matter-flag");
    let shown = |args: &[&str]| {
        let out = kiri(&dir, FRONT_MATTER, &[&["--mode", "plain"], args].concat());
        assert!(out.status.success(), "{}", text(&out.stderr));
        text(&out.stdout)
    };
    let table = "  ╭───────┬───╮\n  │ title │ A │\n  ╰───────┴───╯\n\n  Body\n";
    assert_eq!(shown(&[]), table);
    assert_eq!(shown(&["--front-matter", "table"]), table);
    assert_eq!(
        shown(&["--front-matter", "code"]),
        "    title: A\n\n  Body\n"
    );
    assert_eq!(shown(&["--front-matter", "hide"]), "  Body\n");
    // Off, the text is Markdown: a thematic break and a heading.
    let off = shown(&["--front-matter", "off"]);
    assert!(off.contains("## title: A\n"), "{off}");
    let out = kiri(&dir, FRONT_MATTER, &["--front-matter", "list"]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("invalid value 'list'"));
}

#[test]
fn front_matter_comes_from_the_environment_and_the_configuration_file() {
    let dir = Scratch::new("front-matter-setting");
    let document = dir.join("document.md");
    std::fs::write(&document, FRONT_MATTER).unwrap();
    let shown = |command: &mut std::process::Command| {
        let out = command
            .args(["--mode", "plain"])
            .arg(&document)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out.stderr));
        (text(&out.stdout), text(&out.stderr))
    };
    write(
        &dir.join("config/kiri/config.toml"),
        "front_matter = \"hide\"\n",
    );
    assert_eq!(shown(&mut command(&dir)).0, "  Body\n");
    let (out, _) = shown(command(&dir).env("KIRI_FRONT_MATTER", "code"));
    assert_eq!(out, "    title: A\n\n  Body\n");
    let (out, _) = shown(
        command(&dir)
            .env("KIRI_FRONT_MATTER", "code")
            .args(["--front-matter", "table"]),
    );
    assert!(out.contains("│ title │ A │"), "{out}");
    let (out, warning) = shown(command(&dir).env("KIRI_FRONT_MATTER", "joo"));
    assert_eq!(out, "  Body\n");
    assert_eq!(
        warning,
        "kiri: KIRI_FRONT_MATTER: `joo` is not table, code, hide or off; left out\n"
    );
}

#[test]
fn front_matter_that_does_not_parse_is_a_warning_only_as_a_table() {
    let dir = Scratch::new("front-matter-invalid");
    let source = "---\ntags: [unclosed\n---\nBody\n";
    let warning = |args: &[&str]| text(&kiri(&dir, source, args).stderr);
    assert!(warning(&[]).starts_with("kiri: front matter, line 3: "));
    for style in ["code", "hide", "off"] {
        assert_eq!(warning(&["--front-matter", style]), "", "{style}");
    }
}

/// A table of two body rows.
const TABLE: &str = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n";

#[test]
fn table_rows_come_from_the_flag_the_environment_and_the_configuration_file() {
    let dir = Scratch::new("table-rows");
    let document = dir.join("document.md");
    std::fs::write(&document, TABLE).unwrap();
    let shown = |command: &mut std::process::Command| {
        let out = command
            .args(["--mode", "plain"])
            .arg(&document)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out.stderr));
        (text(&out.stdout), text(&out.stderr))
    };
    let lined = "  │ 1 │ 2 │\n  ├───┼───┤\n  │ 3 │ 4 │\n";
    let plain = "  │ 1 │ 2 │\n  │ 3 │ 4 │\n";
    // Zebra by default, which draws nothing without colours.
    assert!(shown(&mut command(&dir)).0.contains(plain));
    let (out, _) = shown(command(&dir).args(["--table-rows", "off"]));
    assert!(out.contains(plain), "{out}");
    write(
        &dir.join("config/kiri/config.toml"),
        "table_rows = \"off\"\n",
    );
    assert!(shown(&mut command(&dir)).0.contains(plain));
    let (out, _) = shown(command(&dir).env("KIRI_TABLE_ROWS", "lines"));
    assert!(out.contains(lined), "{out}");
    let (out, _) = shown(
        command(&dir)
            .env("KIRI_TABLE_ROWS", "lines")
            .args(["--table-rows", "off"]),
    );
    assert!(out.contains(plain), "{out}");
    let (out, warning) = shown(command(&dir).env("KIRI_TABLE_ROWS", "stripes"));
    assert!(out.contains(plain), "{out}");
    assert_eq!(
        warning,
        "kiri: KIRI_TABLE_ROWS: `stripes` is not off, zebra, lines, indent or underline; left out\n"
    );
    let out = kiri(&dir, TABLE, &["--table-rows", "stripes"]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("invalid value 'stripes'"));
}

#[test]
fn the_help_lists_the_values_of_each_setting() {
    let dir = Scratch::new("help-values");
    for flag in ["--help", "-h"] {
        let out = command(&dir).arg(flag).output().unwrap();
        assert!(out.status.success(), "{}", text(&out.stderr));
        insta::assert_snapshot!(flag.trim_start_matches('-'), text(&out.stdout));
    }
}

#[test]
fn the_help_wraps_to_the_terminal_width_up_to_100() {
    let dir = Scratch::new("help-width");
    for (columns, width) in [("60", 60), ("200", 100)] {
        for flag in ["--help", "-h"] {
            let out = command(&dir)
                .arg(flag)
                .env("COLUMNS", columns)
                .output()
                .unwrap();
            assert!(out.status.success(), "{}", text(&out.stderr));
            let help = text(&out.stdout);
            let widest = help.lines().map(|line| line.chars().count()).max();
            assert!(
                widest <= Some(width),
                "{flag} {columns}: {widest:?} in {help}"
            );
        }
    }
}

#[test]
fn the_short_help_gives_each_argument_one_line() {
    let dir = Scratch::new("help-short");
    let out = command(&dir)
        .arg("-h")
        .env("COLUMNS", "100")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    let help = text(&out.stdout);
    // A line that goes on from the one above is indented past the names;
    // only clap's list of values, in brackets, may go on.
    let mut longer = Vec::new();
    let mut in_values = false;
    for line in help.lines() {
        let goes_on = line.starts_with(&" ".repeat(12)) && !line.trim().is_empty();
        if goes_on && !in_values && !line.trim_start().starts_with('[') {
            longer.push(line);
        }
        if let Some(open) = line.rfind('[') {
            in_values = !line[open..].contains(']');
        } else if line.contains(']') {
            in_values = false;
        }
    }
    assert!(longer.is_empty(), "{longer:#?}");
}

#[test]
fn a_flag_takes_only_the_names_of_its_values() {
    let dir = Scratch::new("flag-values");
    let error = |args: &[&str]| {
        let out = command(&dir).args(args).output().unwrap();
        assert!(!out.status.success(), "{args:?}");
        text(&out.stderr)
    };
    let errors: Vec<String> = [
        &["--mode", "Kitty"][..],
        &["--paging", "1"][..],
        &["--front-matter", "yaml"][..],
        &["--table-rows", "zebras"][..],
        &["--diagrams", "1"][..],
        &["--render-errors", "never"][..],
    ]
    .iter()
    .map(|args| error(args))
    .collect();
    insta::assert_snapshot!(errors.join("\n---\n"));
}
