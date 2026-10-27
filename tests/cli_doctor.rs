//! `kiri --doctor` end to end: the report as Markdown in a pipe, and
//! rendered in a terminal.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use kiri::caps::Capabilities;
use kiri::config::{Environment, Options, Origin, Passthrough, Setting, choose};
use kiri::doctor::{Facts, report};
use kiri::term::tmux::Pane;
use kiri::term::{Detection, Mode, WindowSize};
use kiri::theme::Theme;

mod common;

use common::{Scratch, assert_succeeded, pty, text, write};

/// `kiri --doctor` with `args`, with its home directory in `dir`. It runs
/// in a session of its own, so it has no controlling terminal to ask
/// whatever terminal the tests run in.
fn doctor(dir: &Path, args: &[&str]) -> Command {
    let mut command = common::command(dir);
    command
        .env_remove("XDG_CONFIG_HOME")
        .arg("--doctor")
        .args(args);
    // SAFETY: setsid is async-signal-safe and touches no memory.
    unsafe {
        command.pre_exec(|| {
            rustix::process::setsid()?;
            Ok(())
        });
    }
    command
}

/// The report `kiri --doctor` with `args` writes into a pipe.
fn piped(dir: &Path, args: &[&str]) -> String {
    let out = doctor(dir, args).output().unwrap();
    assert_succeeded(&out);
    text(&out.stdout)
}

/// The report with the parts that change between builds replaced: kiri's
/// version, and the image formats and the diagram renderers, which depend
/// on the build's features; and the rows for where diagram programs work,
/// one or two, which depend on the directories the tests run in.
fn stable(report: &str) -> String {
    let mut lines: Vec<&str> = report
        .lines()
        .map(|line| {
            if line.contains("PNG") {
                "[image formats]"
            } else if line.starts_with("| `mermaid` | built-in") {
                "[built-in mermaid renderer]"
            } else if line.starts_with("| `mermaid` | command") {
                "[mermaid command]"
            } else if line.starts_with("| Diagram work files |") {
                "[diagram work files]"
            } else {
                line
            }
        })
        .collect();
    lines.dedup_by(|a, b| a == b && *a == "[diagram work files]");
    lines
        .join("\n")
        .replace(env!("CARGO_PKG_VERSION"), "[version]")
}

#[test]
fn a_pipe_gets_the_report_as_markdown() {
    let dir = Scratch::new("doctor-pipe");
    let report = piped(&dir, &[]);
    assert!(!report.contains('\x1b'), "{report:?}");
    insta::assert_snapshot!(stable(&report));
}

#[test]
fn settings_tell_where_they_come_from() {
    let dir = Scratch::new("doctor-origins");
    write(
        &dir.join("home/.config/kiri/config.toml"),
        "images = false\n",
    );
    write(&dir.join("home/themes/mine.toml"), "");
    let mut command = doctor(
        &dir,
        &[
            "--no-margins",
            "--mode",
            "ansi",
            "--width",
            "72",
            "--max-image-size",
            "5",
            "-P",
        ],
    );
    command.env("KIRI_THEME", dir.join("home/themes/mine.toml"));
    let out = command.output().unwrap();
    assert_succeeded(&out);
    let report = text(&out.stdout);
    for row in [
        "| Mode | ansi | `--mode` |",
        "| Theme | `~/themes/mine.toml` | `KIRI_THEME=~/themes/mine.toml` |",
        "| Margins | off | `--no-margins` |",
        "| Images | off | configuration file |",
        "| Largest image | 5 MB | `--max-image-size` |",
        "| Paging | never | `-P` |",
        "| Width | 72 cells | `--width` |",
        "| Configuration file | `~/.config/kiri/config.toml` | default | read |",
    ] {
        assert!(report.contains(row), "{row}\n{report}");
    }
}

/// Each flag that chooses a setting, as the report names it: by its long
/// name, or by its short one where it has no long one.
#[test]
fn flags_tell_where_they_come_from() {
    let dir = Scratch::new("doctor-flags");
    write(&dir.join("mine.toml"), "");
    let theme = dir.join("mine.toml");
    let theme = theme.to_str().unwrap();
    for (args, row) in [
        (&["-I"][..], "| Images | off | `--no-images` |"),
        (&["--no-images"][..], "| Images | off | `--no-images` |"),
        (&["--images"][..], "| Images | on | `--images` |"),
        (&["-M"][..], "| Margins | off | `--no-margins` |"),
        (&["--light"][..], "| Theme | light paper | `--light` |"),
        (&["--dark"][..], "| Theme | dark paper | `--dark` |"),
        (&["--dark", "--theme", theme][..], "| `--theme` |"),
        (&["-P"][..], "| Paging | never | `-P` |"),
        (&["--mode", "plain"][..], "| Mode | plain | `--mode` |"),
        (&["--mode=auto"][..], "| Mode | auto | `--mode` |"),
        (&["--diagrams=off"][..], "| Diagrams | off | `--diagrams` |"),
        (&["--width", "50"][..], "| Width | 50 cells | `--width` |"),
        (
            &["--max-image-size", "10"][..],
            "| Largest image | 10 MB | `--max-image-size` |",
        ),
    ] {
        let report = piped(&dir, args);
        assert!(report.contains(row), "{args:?}: {row}\n{report}");
    }
}

#[test]
fn theme_toml_is_the_theme_only_when_it_exists() {
    let dir = Scratch::new("doctor-theme-toml");
    let row = "| Theme | built-in | default |";
    assert!(piped(&dir, &[]).contains(row));
    write(&dir.join("home/.config/kiri/theme.toml"), "");
    let report = piped(&dir, &[]);
    let row = "| Theme | `~/.config/kiri/theme.toml` | default |";
    assert!(report.contains(row), "{row}\n{report}");
}

/// Asserts that the report of `kiri --doctor` with `args` and the
/// environment variables `env` has the row `row`.
fn check_row(dir: &Path, args: &[&str], env: &[(&str, &str)], row: &str) {
    let mut command = doctor(dir, args);
    command.envs(env.iter().copied());
    let out = command.output().unwrap();
    assert_succeeded(&out);
    let report = text(&out.stdout);
    assert!(report.contains(row), "{args:?} {env:?}: {row}\n{report}");
}

/// Writes `content` as the configuration file of `dir`'s home directory.
fn write_config(dir: &Path, content: &str) {
    write(&dir.join("home/.config/kiri/config.toml"), content);
}

#[test]
fn an_environment_variable_shows_its_value() {
    let dir = Scratch::new("doctor-env");
    let row = "| Images | off | `KIRI_NO_IMAGES=1` |";
    check_row(&dir, &[], &[("KIRI_NO_IMAGES", "1")], row);
}

#[test]
fn paging_tells_where_it_comes_from() {
    let dir = Scratch::new("doctor-paging");
    check_row(&dir, &[], &[], "| Paging | auto | default |");
    check_row(&dir, &["-p"], &[], "| Paging | always | `-p` |");
    let row = "| Paging | never | `--paging` |";
    check_row(&dir, &["--paging", "never"], &[], row);
    let row = "| Paging | always | `KIRI_PAGING=1` |";
    check_row(&dir, &[], &[("KIRI_PAGING", "1")], row);
}

#[test]
fn front_matter_tells_where_it_comes_from() {
    let dir = Scratch::new("doctor-front-matter");
    check_row(&dir, &[], &[], "| Front matter | table | default |");
    let row = "| Front matter | off | `--front-matter` |";
    check_row(
        &dir,
        &["--front-matter", "off"],
        &[("KIRI_FRONT_MATTER", "code")],
        row,
    );
    let row = "| Front matter | hide | `KIRI_FRONT_MATTER=hide` |";
    check_row(&dir, &[], &[("KIRI_FRONT_MATTER", "hide")], row);
    write_config(&dir, "front_matter = \"code\"\n");
    let row = "| Front matter | code | configuration file |";
    check_row(&dir, &[], &[], row);
}

#[test]
fn headings_tell_where_they_come_from() {
    let dir = Scratch::new("doctor-headings");
    check_row(&dir, &[], &[], "| Headings | auto | default |");
    let row = "| Headings | plain | `--headings` |";
    check_row(
        &dir,
        &["--headings", "plain"],
        &[("KIRI_HEADINGS", "sized")],
        row,
    );
    let row = "| Headings | images | `KIRI_HEADINGS=images` |";
    check_row(&dir, &[], &[("KIRI_HEADINGS", "images")], row);
    write_config(&dir, "headings = \"sized\"\n");
    let row = "| Headings | sized | configuration file |";
    check_row(&dir, &[], &[], row);
}

#[test]
fn table_rows_tell_where_they_come_from() {
    let dir = Scratch::new("doctor-table-rows");
    check_row(&dir, &[], &[], "| Table rows | zebra | default |");
    let row = "| Table rows | off | `--table-rows` |";
    check_row(
        &dir,
        &["--table-rows", "off"],
        &[("KIRI_TABLE_ROWS", "lines")],
        row,
    );
    let row = "| Table rows | lines | `KIRI_TABLE_ROWS=lines` |";
    check_row(&dir, &[], &[("KIRI_TABLE_ROWS", "lines")], row);
}

#[test]
fn diagrams_tell_where_they_come_from() {
    let dir = Scratch::new("doctor-diagrams");
    check_row(&dir, &[], &[], "| Diagrams | on | default |");
    let row = "| Diagrams | on | `--diagrams` |";
    check_row(&dir, &["--diagrams", "on"], &[("KIRI_DIAGRAMS", "0")], row);
    let row = "| Diagrams | off | `KIRI_DIAGRAMS=off` |";
    check_row(&dir, &[], &[("KIRI_DIAGRAMS", "off")], row);
    write_config(&dir, "diagrams = false\n");
    let row = "| Diagrams | off | configuration file |";
    check_row(&dir, &[], &[], row);
}

#[test]
fn the_mouse_tells_where_it_comes_from() {
    let dir = Scratch::new("doctor-mouse");
    check_row(&dir, &[], &[], "| Mouse | on | default |");
    let row = "| Mouse | off | `--no-mouse` |";
    check_row(&dir, &["--no-mouse"], &[("KIRI_MOUSE", "1")], row);
    let row = "| Mouse | off | `KIRI_MOUSE=off` |";
    check_row(&dir, &[], &[("KIRI_MOUSE", "off")], row);
    write_config(&dir, "mouse = false\n");
    let row = "| Mouse | off | configuration file |";
    check_row(&dir, &[], &[], row);
    let row = "| Mouse | on | `--mouse` |";
    check_row(&dir, &["--mouse"], &[], row);
}

#[test]
fn render_errors_tell_where_they_come_from() {
    let dir = Scratch::new("doctor-render-errors");
    check_row(&dir, &[], &[], "| Render errors | both | default |");
    let row = "| Render errors | stderr | `--render-errors` |";
    check_row(
        &dir,
        &["--render-errors", "stderr"],
        &[("KIRI_RENDER_ERRORS", "inline")],
        row,
    );
    let row = "| Render errors | inline | `KIRI_RENDER_ERRORS=inline` |";
    check_row(&dir, &[], &[("KIRI_RENDER_ERRORS", "inline")], row);
    write_config(&dir, "render_errors = \"inline\"\n");
    let row = "| Render errors | inline | configuration file |";
    check_row(&dir, &[], &[], row);
}

#[test]
fn the_diagrams_time_limit_and_panels_tell_where_they_come_from() {
    let dir = Scratch::new("doctor-diagram-settings");
    let report = piped(&dir, &[]);
    for row in [
        "| Diagram time limit | 20 s | default |",
        "| Diagram panels | on | default |",
    ] {
        assert!(report.contains(row), "{row}\n{report}");
    }
    write_config(
        &dir,
        "[diagrams]\ntimeout = 2.5\nbackground_panel = false\n",
    );
    let report = piped(&dir, &[]);
    for row in [
        "| Diagram time limit | 2.5 s | configuration file |",
        "| Diagram panels | off | configuration file |",
    ] {
        assert!(report.contains(row), "{row}\n{report}");
    }
}

#[test]
fn the_search_case_tells_where_it_comes_from() {
    let dir = Scratch::new("doctor-search-case");
    check_row(&dir, &[], &[], "| Search case | smart | default |");
    write_config(&dir, "search_case = \"literal\"\n");
    let row = "| Search case | literal | configuration file |";
    check_row(&dir, &[], &[], row);
}

#[test]
fn following_tells_where_it_comes_from() {
    let dir = Scratch::new("doctor-follow");
    check_row(&dir, &[], &[], "| Follow | off | default |");
    write_config(&dir, "follow = \"on\"\n");
    let row = "| Follow | on | configuration file |";
    check_row(&dir, &[], &[], row);
    check_row(&dir, &["-F"], &[], "| Follow | off | `-F` |");
    let row = "| Follow | off | `KIRI_FOLLOW=0` |";
    check_row(&dir, &[], &[("KIRI_FOLLOW", "0")], row);
    let row = "| Follow | auto | `KIRI_FOLLOW=auto` |";
    check_row(&dir, &[], &[("KIRI_FOLLOW", "auto")], row);
    check_row(&dir, &["--fa"], &[], "| Follow | auto | `--fa` |");
    let row = "| Follow | auto | `--follow` |";
    check_row(&dir, &["--follow", "auto"], &[], row);
}

#[test]
fn the_follow_jump_tells_where_it_comes_from() {
    let dir = Scratch::new("doctor-follow-jump");
    check_row(&dir, &[], &[], "| Follow jump | first | default |");
    write_config(&dir, "follow_jump = \"most\"\n");
    let row = "| Follow jump | most | configuration file |";
    check_row(&dir, &[], &[], row);
    let row = "| Follow jump | last | `--follow-jump` |";
    check_row(&dir, &["--follow-jump", "last"], &[], row);
    let row = "| Follow jump | last | `KIRI_FOLLOW_JUMP=last` |";
    check_row(&dir, &[], &[("KIRI_FOLLOW_JUMP", "last")], row);
}

#[test]
fn the_initial_load_tells_where_it_comes_from() {
    let dir = Scratch::new("doctor-initial-load");
    check_row(&dir, &[], &[], "| Initial load | wait | default |");
    write_config(&dir, "initial_load = \"background\"\n");
    let row = "| Initial load | background | configuration file |";
    check_row(&dir, &[], &[], row);
    let row = "| Initial load | wait | `KIRI_INITIAL_LOAD=wait` |";
    check_row(&dir, &[], &[("KIRI_INITIAL_LOAD", "wait")], row);
    let row = "| Initial load | background | `--initial-load` |";
    check_row(
        &dir,
        &["--initial-load", "background"],
        &[("KIRI_INITIAL_LOAD", "wait")],
        row,
    );
}

#[test]
fn the_passthrough_tells_where_it_comes_from() {
    let dir = Scratch::new("doctor-passthrough");
    check_row(&dir, &[], &[], "| Passthrough | detect | default |");
    write_config(&dir, "passthrough = \"none\"\n");
    let row = "| Passthrough | none | configuration file |";
    check_row(&dir, &[], &[], row);
    let row = "| Passthrough | tmux | `KIRI_PASSTHROUGH=tmux` |";
    check_row(&dir, &[], &[("KIRI_PASSTHROUGH", "tmux")], row);
    let row = "| Passthrough | none | `--passthrough` |";
    check_row(
        &dir,
        &["--passthrough", "none"],
        &[("KIRI_PASSTHROUGH", "tmux")],
        row,
    );
}

#[test]
fn an_unknown_kiri_paging_is_a_problem_in_the_report() {
    let dir = Scratch::new("doctor-paging-unknown");
    let mut command = doctor(&dir, &[]);
    command.env("KIRI_PAGING", "joo");
    let out = command.output().unwrap();
    assert_succeeded(&out);
    let report = text(&out.stdout);
    let problem = "- `KIRI_PAGING`: `joo` is not always, auto, never, 1 or 0; left out";
    assert!(report.contains(problem), "{report}");
    assert!(report.contains("| Paging | auto | default |"), "{report}");
}

#[test]
fn a_broken_configuration_file_is_a_problem_in_the_report() {
    let dir = Scratch::new("doctor-broken-config");
    write(&dir.join("home/.config/kiri/config.toml"), "colour = 1\n");
    let report = piped(&dir, &[]);
    assert!(report.contains("## Problems"), "{report}");
    // The error shows the line in the file, in a code block.
    assert!(
        report.contains("- `~/.config/kiri/config.toml`:\n\n  ```text\n"),
        "{report}"
    );
    assert!(report.contains("  unknown field `colour`"), "{report}");
    assert!(
        report
            .contains("| Configuration file | `~/.config/kiri/config.toml` | default | not read |"),
        "{report}"
    );
}

#[test]
fn a_problem_hides_the_home_directory_inside_its_text_too() {
    let dir = Scratch::new("doctor-home-in-error");
    let home = dir.join("home");
    let config = format!("theme = \"{}/x.toml\" oops\n", home.display());
    write(&home.join(".config/kiri/config.toml"), &config);
    let report = piped(&dir, &[]);
    assert!(report.contains("## Problems"), "{report}");
    assert!(report.contains("theme = \"~/x.toml\" oops"), "{report}");
    assert!(!report.contains(&*home.to_string_lossy()), "{report}");
}

#[test]
fn a_theme_that_does_not_load_is_a_problem_in_the_report() {
    let dir = Scratch::new("doctor-broken-theme");
    let report = piped(&dir, &["--theme", "/nonexistent/mine.toml"]);
    assert!(report.contains("## Problems"), "{report}");
    assert!(report.contains("- `/nonexistent/mine.toml`: "), "{report}");
    assert!(
        report.contains("| Theme | `/nonexistent/mine.toml` | `--theme` |"),
        "{report}"
    );
}

/// The heading sizes of a theme that does not load are the default ones,
/// and the row says so instead of naming the theme.
#[test]
fn heading_sizes_of_a_theme_that_does_not_load_are_the_defaults() {
    let dir = Scratch::new("doctor-broken-theme-sizes");
    let theme = dir.join("mine.toml");
    write(&theme, "[h1]\nscale = 9\n");
    let report = piped(&dir, &["--theme", theme.to_str().unwrap()]);
    assert!(
        report.contains("h1.scale must be between 0.5 and 7"),
        "{report}"
    );
    assert!(
        report.contains("| Heading sizes | 2.5, 2, 1.75, 1.5, 1.25, 1 | default |"),
        "{report}"
    );
}

#[test]
fn the_files_tell_where_their_paths_come_from() {
    let dir = Scratch::new("doctor-file-origins");
    write(&dir.join("home/kiri.toml"), "");
    let mut command = doctor(&dir, &[]);
    command
        .env("KIRI_CONFIG", dir.join("home/kiri.toml"))
        .env("XDG_CONFIG_HOME", dir.join("home/xdg"));
    let out = command.output().unwrap();
    assert_succeeded(&out);
    let report = text(&out.stdout);
    for row in [
        "| Configuration file | `~/kiri.toml` | `KIRI_CONFIG=~/kiri.toml` | read |",
        "| Theme file | `~/xdg/kiri/theme.toml` | `XDG_CONFIG_HOME=~/xdg` | not found |",
    ] {
        assert!(report.contains(row), "{row}\n{report}");
    }
}

#[test]
fn the_files_tell_where_diagram_programs_work() {
    // A short path, whatever TMPDIR is, so that a cache directory in it
    // is short enough.
    let dir = Scratch::in_dir(Path::new("/tmp"), "dw");
    write(&dir.join("home/file"), "");
    std::fs::create_dir_all(dir.join("home/tmp")).unwrap();
    let rows = |cache: Option<&Path>| {
        let mut command = doctor(&dir, &[]);
        command.env("TMPDIR", dir.join("home/tmp"));
        if let Some(cache) = cache {
            command.env("XDG_CACHE_HOME", cache);
        }
        let out = command.output().unwrap();
        assert_succeeded(&out);
        let report = text(&out.stdout);
        (report.lines())
            .filter(|line| line.starts_with("| Diagram work files"))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let default = &rows(None)[0];
    let start = "| Diagram work files | `~/.cache/kiri/diagrams` | default |";
    assert!(default.starts_with(start), "{default}");
    let cache = dir.join("c");
    let work = cache.join("kiri/diagrams");
    assert!(
        work.as_os_str().len() <= 49,
        "the scratch directory's path is too long"
    );
    assert_eq!(
        rows(Some(&cache)),
        [format!(
            "| Diagram work files | `{}` | `XDG_CACHE_HOME={}` | usable |",
            work.display(),
            cache.display()
        )]
    );
    // Where the cache directory cannot be made, or its path is too long
    // for the sockets a browser makes, the temporary directory is used.
    let temp = "| Diagram work files | `~/tmp` | `TMPDIR=~/tmp` | usable |";
    assert_eq!(
        rows(Some(&dir.join("home/file"))),
        [
            "| Diagram work files | `~/file/kiri/diagrams` | `XDG_CACHE_HOME=~/file` \
             | not usable: `~/file` is not a directory |",
            temp
        ]
    );
    let long = dir.join("home").join("x".repeat(60));
    let row = rows(Some(&long));
    assert_eq!(row.len(), 2, "{row:?}");
    assert!(
        row[0].ends_with("| not usable: its path is longer than 49 bytes |"),
        "{row:?}"
    );
    assert_eq!(row[1], temp);
    assert!(!long.exists());
}

#[test]
fn values_with_pipes_backticks_and_control_characters_stay_in_their_cells() {
    let dir = Scratch::new("doctor-escapes");
    let mut command = doctor(&dir, &[]);
    command
        .env("TERM", "x|y`z")
        .env("KIRI_NO_IMAGES", "a|b``c\td");
    let out = command.output().unwrap();
    assert_succeeded(&out);
    let report = text(&out.stdout);
    // Rendered, each row keeps its two or three cells and the values as
    // they are, as code, with the tab shown as an escape.
    let rendered = common::render(
        &report,
        &kiri::render::Inputs::new(Capabilities::plain(100)),
    );
    let line = |needle: &str| {
        rendered
            .lines()
            .find(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("{needle}\n{rendered}"))
            .to_owned()
    };
    let terminal = line("Terminal ");
    assert!(terminal.contains("none, `TERM=x|y`z` "), "{terminal}");
    assert_eq!(terminal.matches('│').count(), 3, "{terminal}");
    let images = line("Images ");
    assert!(
        images.contains("`KIRI_NO_IMAGES=a|b``c\\x09d` "),
        "{images}"
    );
    assert_eq!(images.matches('│').count(), 4, "{images}");
}

#[test]
fn no_problems_no_problems_section() {
    let dir = Scratch::new("doctor-no-problems");
    assert!(!piped(&dir, &[]).contains("Problems"));
}

#[test]
fn the_report_lists_the_image_formats_this_build_decodes() {
    let dir = Scratch::new("doctor-formats");
    let report = piped(&dir, &[]);
    let line = report.lines().find(|line| line.contains("PNG")).unwrap();
    for format in ["PNG", "JPEG", "GIF", "WebP", "SVG"] {
        assert!(line.contains(format), "{format}: {line}");
    }
    assert_eq!(line.contains("BMP"), cfg!(feature = "full"), "{line}");
}

#[test]
fn the_report_names_the_built_in_mermaid_renderer() {
    let dir = Scratch::new("doctor-mermaid");
    for (args, status) in [
        (&[][..], "used"),
        (&["--diagrams", "off"][..], "not used: diagrams are off"),
    ] {
        let report = piped(&dir, args);
        assert!(report.contains("## Diagrams"), "{report}");
        let row = match kiri::diagrams::BuiltIn::version("mermaid") {
            Some(version) => format!("| `mermaid` | built-in: {version} | {status} |"),
            None => "| `mermaid` | built-in | not in this build |".to_owned(),
        };
        assert!(report.contains(&row), "{row}\n{report}");
    }
    let row = report_row(&piped(&dir, &[]), "| `mermaid` |");
    assert_eq!(row.contains("merman"), cfg!(feature = "mermaid"), "{row}");
}

#[test]
fn the_report_names_the_built_in_formula_renderer() {
    let dir = Scratch::new("doctor-math");
    for (args, status) in [
        (&[][..], "used"),
        (&["--diagrams", "off"][..], "not used: diagrams are off"),
    ] {
        let report = piped(&dir, args);
        let row = "| `math` | built-in: RaTeX 0.1.15 |";
        assert_eq!(report_row(&report, row), format!("{row} {status} |"));
    }
}

#[test]
fn the_report_names_the_mermaid_command() {
    let dir = Scratch::new("doctor-mermaid-command");
    let row = |args: &[&str]| report_row(&piped(&dir, args), "| `mermaid` | command");
    let none = "| `mermaid` | command: none configured | not used |";
    let nothing = match cfg!(feature = "mermaid") {
        true => none,
        false => "| `mermaid` | command: `mmdc` not found | not used |",
    };
    // Unless Homebrew has mmdc on this machine.
    let homebrew = [
        "/opt/homebrew/bin/mmdc",
        "/home/linuxbrew/.linuxbrew/bin/mmdc",
    ];
    if !homebrew.iter().any(|path| Path::new(path).exists()) {
        assert_eq!(row(&[]), nothing);
    }
    // Found where nvm installs programs, in a build without merman, but
    // not looked for when the configuration file says so.
    let nvm = "home/.nvm/versions/node/v24.16.0/bin/mmdc";
    common::executable(&dir.join(nvm), "#!/bin/sh\n");
    let config = dir.join("home/.config/kiri/config.toml");
    write(&config, "[diagrams.mermaid]\nauto_detect = false\n");
    let off = match cfg!(feature = "mermaid") {
        true => none,
        false => "| `mermaid` | command: `mmdc` not looked for: `auto_detect = false` | not used |",
    };
    assert_eq!(row(&[]), off);
    std::fs::remove_file(&config).unwrap();
    let found = match cfg!(feature = "mermaid") {
        true => none.to_owned(),
        false => "| `mermaid` | command found: `~/.nvm/versions/node/v24.16.0/bin/mmdc -i {input} \
                  -o {output} -t {theme} -b transparent -c {config} -s {scale}` | used |"
            .to_owned(),
    };
    assert_eq!(row(&[]), found);
    write(
        &dir.join("home/.config/kiri/config.toml"),
        "[diagrams.mermaid]\ncommand = [\"mmdc\", \"-i\", \"{input}\", \"-o\", \"{output}\"]\n",
    );
    let configured = "| `mermaid` | command configured: `mmdc -i {input} -o {output}` |";
    assert_eq!(row(&[]), format!("{configured} used |"));
    assert_eq!(
        row(&["--diagrams", "off"]),
        format!("{configured} not used: diagrams are off |")
    );
    let built_in = report_row(&piped(&dir, &[]), "| `mermaid` | built-in");
    let status = match cfg!(feature = "mermaid") {
        true => "| not used: a command is configured |",
        false => "| not in this build |",
    };
    assert!(built_in.ends_with(status), "{built_in}");
}

/// The line of `report` that starts with `start`.
fn report_row(report: &str, start: &str) -> String {
    report
        .lines()
        .find(|line| line.starts_with(start))
        .unwrap_or_else(|| panic!("{start}\n{report}"))
        .to_owned()
}

#[test]
fn doctor_takes_no_file_or_format() {
    let dir = Scratch::new("doctor-conflicts");
    write(&dir.join("document.md"), "Text\n");
    let document = dir.join("document.md");
    for args in [
        vec![document.to_str().unwrap()],
        vec!["--format", "markdown"],
    ] {
        let out = doctor(&dir, &args).output().unwrap();
        assert!(!out.status.success(), "{args:?}");
        let stderr = text(&out.stderr);
        assert!(stderr.contains("cannot be used with"), "{stderr}");
    }
}

#[test]
fn a_terminal_gets_the_report_rendered_with_its_test_image() {
    let dir = Scratch::new("doctor-terminal");
    let output = in_terminal(&dir, &["--mode", "kitty"], None);
    assert!(!output.contains("## Terminal"), "{output:?}");
    assert!(output.contains("terminal features"), "{output:?}");
    assert!(output.contains("kiri --doctor --raw"), "{output:?}");
    assert_eq!(common::graphics_commands(&output).len(), 1, "{output:?}");
}

/// Without a controlling terminal to ask, the report tells of the terminal
/// on stdout, as a run that renders a document sees it.
#[test]
fn without_a_controlling_terminal_the_report_tells_of_stdout() {
    let dir = Scratch::new("doctor-no-tty");
    let size = rustix::termios::Winsize {
        ws_col: 100,
        ws_row: 30,
        ws_xpixel: 1200,
        ws_ypixel: 600,
    };
    let output = in_terminal(&dir, &[], Some(size));
    assert!(output.contains("100 × 30 cells"), "{output:?}");
    assert!(output.contains("12 × 20 pixels"), "{output:?}");
    assert!(output.contains("\x1b["), "{output:?}");
}

/// What `kiri --doctor` with `args` writes into a terminal, of the window
/// size `size` when one is given.
fn in_terminal(dir: &Path, args: &[&str], size: Option<rustix::termios::Winsize>) -> String {
    let (mut terminal, tty) = pty();
    if let Some(size) = size {
        rustix::termios::tcsetwinsize(&tty, size).unwrap();
    }
    let mut child = doctor(dir, args).stdout(Stdio::from(tty)).spawn().unwrap();
    // Read while kiri writes, so that a report larger than the terminal
    // holds unread does not hang the test.
    let output = common::read_terminal(
        &mut terminal,
        &mut child,
        Duration::from_secs(10),
        |_, _, _| {},
    );
    assert!(child.wait().unwrap().success());
    text(&output)
}

#[test]
fn raw_writes_the_markdown_into_a_terminal_too() {
    let dir = Scratch::new("doctor-raw-terminal");
    let output = in_terminal(&dir, &["--raw", "--mode", "kitty"], None);
    assert!(!output.contains('\x1b'), "{output:?}");
    assert!(output.contains("## Terminal"), "{output:?}");
    assert!(output.contains("| Mode | kitty | `--mode` |"), "{output:?}");
    assert!(!output.contains("--raw"), "{output:?}");
}

#[test]
fn raw_leaves_out_the_sample() {
    let dir = Scratch::new("doctor-raw");
    let report = piped(&dir, &["--raw"]);
    assert!(report.contains("## Image formats"), "{report}");
    assert!(!report.contains("## Sample"), "{report}");
    assert!(!report.contains("data:"), "{report}");
    let markdown = piped(&dir, &[]);
    assert!(markdown.contains("## Sample"), "{markdown}");
    assert!(!markdown.contains("--raw"), "{markdown}");
}

#[test]
fn raw_needs_doctor() {
    let dir = Scratch::new("doctor-raw-alone");
    let with_file = common::kiri(&dir, "Text\n", &["--raw"]);
    let alone = common::command(&dir).arg("--raw").output().unwrap();
    for out in [with_file, alone] {
        assert!(!out.status.success());
        let stderr = text(&out.stderr);
        assert!(stderr.contains("--raw needs --doctor"), "{stderr}");
        assert!(stderr.contains("Usage: kiri "), "{stderr}");
    }
}

/// The report on a kitty 0.47.1 in a window without pixels that told its
/// cell size, with `TERM` and the settings of a user with a home directory,
/// as it is rendered.
#[test]
fn the_report_on_kitty() {
    let env = Environment {
        home: Some("/home/u".into()),
        term: Some("xterm-kitty".into()),
        colorterm: Some("truecolor".into()),
        ..Environment::default()
    };
    let detection = Detection {
        caps: Capabilities {
            height: Some(40),
            cell_pixels: Some((9, 19)),
            dark_background: Some(true),
            ..Capabilities::kitty(120)
        },
        window: Some(WindowSize {
            columns: 120,
            rows: 40,
            width: 0,
            height: 0,
        }),
        answers: Some(
            b"\x1b[5;1R\x1b[5;3R\x1b[5;5R\x1b_Gi=31;OK\x1b\\\
              \x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\\x1bP>|kitty(0.47.1)\x1b\\\
              \x1b[6;19;9t\x1b[?62;c"
                .to_vec(),
        ),
        background: false,
        tmux: None,
    };
    let choices = choose(&Options::default(), &env, &Default::default());
    let facts = Facts {
        env: &env,
        mode: Setting::new(Mode::Auto, Origin::Default),
        width: None,
        max_image_size: Setting::new(10, Origin::Default),
        detection: &detection,
        choices: &choices,
        theme: &Theme::default(),
        config_error: None,
        theme_error: None,
        output: kiri::doctor::Output::Rendered,
    };
    insta::assert_snapshot!(stable(&report(&facts)));
}

/// The terminal section of the report on a kitty behind tmux, where
/// kiri found `pane` with the passthrough setting `passthrough`.
fn tmux_report(pane: Option<Pane>, passthrough: Passthrough) -> String {
    let env = Environment::default();
    let detection = Detection {
        caps: Capabilities {
            text_sizing: false,
            hyperlinks: false,
            styled_underlines: false,
            tmux_passthrough: true,
            ..Capabilities::kitty(80)
        },
        window: None,
        answers: Some(b"\x1b_Gi=31;OK\x1b\\\x1bP>|tmux 3.8\x1b\\\x1b[?1;2;4c".to_vec()),
        background: false,
        tmux: pane,
    };
    let options = Options {
        passthrough: Some(Setting::new(passthrough, Origin::Flag("--passthrough"))),
        ..Options::default()
    };
    let choices = choose(&options, &env, &Default::default());
    let facts = Facts {
        env: &env,
        mode: Setting::new(Mode::Auto, Origin::Default),
        width: None,
        max_image_size: Setting::new(10, Origin::Default),
        detection: &detection,
        choices: &choices,
        theme: &Theme::default(),
        config_error: None,
        theme_error: None,
        output: kiri::doctor::Output::Markdown,
    };
    let report = report(&facts);
    let start = report.find("## Terminal").unwrap();
    let end = report[start..].find("| Window").unwrap();
    report[start..start + end].to_owned()
}

#[test]
fn the_doctor_tells_the_tmux_pane() {
    let pane = |passthrough: &str, turned_on| Pane {
        client: Some("kitty(0.47.1)".into()),
        passthrough: Some(passthrough.into()),
        turned_on,
        focus_events: Some(true),
        ..Pane::default()
    };
    let report = tmux_report(Some(pane("off", true)), Passthrough::Detect);
    let row = "| tmux | passthrough for `kitty(0.47.1)`, allow-passthrough `on`, \
               turned on by kiri, focus-events `on` |";
    assert!(report.contains(row), "{report}");
    assert!(report.contains("| Terminal | `tmux 3.8` |"), "{report}");
    let report = tmux_report(Some(pane("all", false)), Passthrough::Detect);
    let row = "| tmux | passthrough for `kitty(0.47.1)`, allow-passthrough `all`, \
               focus-events `on` |";
    assert!(report.contains(row), "{report}");
}

#[test]
fn the_doctor_says_what_focus_events_off_means() {
    let pane = |focus_events| Pane {
        client: Some("kitty(0.47.1)".into()),
        passthrough: Some("on".into()),
        turned_on: false,
        focus_events,
        ..Pane::default()
    };
    let report = tmux_report(Some(pane(Some(false))), Passthrough::Detect);
    let row = "| tmux | passthrough for `kitty(0.47.1)`, allow-passthrough `on`, \
               focus-events `off`: images come back after a window switch only \
               with `set -g focus-events on` in tmux.conf, or with Ctrl+L |";
    assert!(report.contains(row), "{report}");
    let report = tmux_report(Some(pane(None)), Passthrough::Detect);
    let row = "| tmux | passthrough for `kitty(0.47.1)`, allow-passthrough `on`, \
               focus-events unknown |";
    assert!(report.contains(row), "{report}");
}

#[test]
fn the_doctor_says_when_passthrough_stays_off() {
    let pane = Pane {
        client: Some("kitty(0.47.1)".into()),
        passthrough: Some("off".into()),
        turned_on: false,
        focus_events: Some(true),
        ..Pane::default()
    };
    let report = tmux_report(Some(pane), Passthrough::Detect);
    let row = "| tmux | passthrough for `kitty(0.47.1)`, allow-passthrough `off`: \
               images cannot pass |";
    assert!(report.contains(row), "{report}");
}

#[test]
fn the_doctor_tells_when_tmux_was_not_asked() {
    let nothing = Pane {
        client: None,
        passthrough: None,
        turned_on: false,
        focus_events: None,
        ..Pane::default()
    };
    let report = tmux_report(Some(nothing.clone()), Passthrough::Tmux);
    let row = "| tmux | passthrough; tmux not asked, as passthrough is `tmux` |";
    assert!(report.contains(row), "{report}");
    let report = tmux_report(Some(nothing), Passthrough::Detect);
    let row = "| tmux | passthrough; tmux did not answer |";
    assert!(report.contains(row), "{report}");
}

#[test]
fn without_passthrough_the_doctor_has_no_tmux_row() {
    let report = tmux_report(None, Passthrough::None);
    assert!(!report.contains("\n| tmux |"), "{report}");
}

#[test]
fn inside_tmux_with_passthrough_none_tmux_is_left_alone() {
    // With none, kiri runs no tmux command, so this is safe inside a
    // real tmux too.
    let dir = Scratch::new("doctor-tmux-none");
    let mut command = doctor(&dir, &["--passthrough", "none"]);
    command
        .env("TMUX", "/tmp/kiri-no-such-tmux,1,0")
        .env("TMUX_PANE", "%1");
    let out = command.output().unwrap();
    assert_succeeded(&out);
    let report = text(&out.stdout);
    assert!(!report.contains("\n| tmux |"), "{report}");
    assert!(
        report.contains("| Passthrough | none | `--passthrough` |"),
        "{report}"
    );
}

#[test]
fn passthrough_tmux_on_a_pipe_outside_tmux_runs_no_tmux_command() {
    let dir = Scratch::new("doctor-tmux-pipe");
    // A tmux that leaves a mark when it runs.
    let mark = dir.join("tmux-ran");
    let script = format!("#!/bin/sh\necho \"$@\" >> '{}'\n", mark.display());
    common::executable(&dir.join("bin/tmux"), &script);
    let mut command = doctor(&dir, &["--passthrough", "tmux"]);
    command.env("PATH", dir.join("bin"));
    let out = command.output().unwrap();
    assert_succeeded(&out);
    let report = text(&out.stdout);
    assert!(!report.contains("\n| tmux |"), "{report}");
    assert!(
        report.contains("| Passthrough | tmux | `--passthrough` |"),
        "{report}"
    );
    assert!(
        !mark.exists(),
        "tmux ran: {:?}",
        std::fs::read_to_string(&mark)
    );
}

/// The terminal's name is its own answer, so Markdown in it stays text.
#[test]
fn the_terminal_name_is_shown_as_code() {
    let env = Environment::default();
    let detection = Detection {
        caps: Capabilities::kitty(80),
        window: None,
        answers: Some(b"\x1bP>|evil *x* ![i](p.png)\x1b\\\x1b[?62;c".to_vec()),
        background: false,
        tmux: None,
    };
    let choices = choose(&Options::default(), &env, &Default::default());
    let facts = Facts {
        env: &env,
        mode: Setting::new(Mode::Auto, Origin::Default),
        width: None,
        max_image_size: Setting::new(10, Origin::Default),
        detection: &detection,
        choices: &choices,
        theme: &Theme::default(),
        config_error: None,
        theme_error: None,
        output: kiri::doctor::Output::Markdown,
    };
    let report = report(&facts);
    assert!(
        report.contains("| Terminal | `evil *x* ![i](p.png)` |"),
        "{report}"
    );
}

/// In the background of its terminal, kiri does not ask it, and says so.
#[test]
fn in_the_background_the_terminal_is_not_asked() {
    let env = Environment::default();
    let detection = Detection {
        caps: Capabilities::ansi(80),
        window: None,
        answers: None,
        background: true,
        tmux: None,
    };
    let choices = choose(&Options::default(), &env, &Default::default());
    let facts = Facts {
        env: &env,
        mode: Setting::new(Mode::Auto, Origin::Default),
        width: None,
        max_image_size: Setting::new(10, Origin::Default),
        detection: &detection,
        choices: &choices,
        theme: &Theme::default(),
        config_error: None,
        theme_error: None,
        output: kiri::doctor::Output::Markdown,
    };
    let report = report(&facts);
    assert!(
        report.contains("| Terminal | not asked: kiri runs in the background |"),
        "{report}"
    );
}

/// A heading size that kiri rounded shows what the theme wrote beside what
/// is drawn.
#[test]
fn heading_sizes_show_their_rounding() {
    let env = Environment::default();
    let detection = Detection {
        caps: Capabilities::kitty(80),
        window: None,
        answers: None,
        background: false,
        tmux: None,
    };
    let choices = choose(&Options::default(), &env, &Default::default());
    let theme = kiri::theme::parse("[h1]\nscale = 3\n[h5]\nscale = 1.3\n").unwrap();
    let facts = Facts {
        env: &env,
        mode: Setting::new(Mode::Auto, Origin::Default),
        width: None,
        max_image_size: Setting::new(10, Origin::Default),
        detection: &detection,
        choices: &choices,
        theme: &theme,
        config_error: None,
        theme_error: None,
        output: kiri::doctor::Output::Markdown,
    };
    let report = report(&facts);
    assert!(
        report.contains(
            "| Heading sizes | 3, 2, 1.75, 1.5, 1.3 (drawn 1.27 = 2 · 7/11), 1 | default |"
        ),
        "{report}"
    );
}
