//! `kiri --doctor` end to end: the report as Markdown in a pipe, and
//! rendered in a terminal.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use kiri::caps::Capabilities;
use kiri::config::{Environment, Options, Origin, Setting, choose};
use kiri::doctor::{Facts, report};
use kiri::term::{Detection, Mode, WindowSize};

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
/// on the build's features.
fn stable(report: &str) -> String {
    report
        .replace(env!("CARGO_PKG_VERSION"), "[version]")
        .lines()
        .map(|line| {
            if line.contains("PNG") {
                "[image formats]"
            } else if line.starts_with("| `mermaid` | built-in") {
                "[built-in mermaid renderer]"
            } else if line.starts_with("| `mermaid` | command") {
                "[mermaid command]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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

#[test]
fn an_environment_variable_shows_its_value() {
    let dir = Scratch::new("doctor-env");
    let mut command = doctor(&dir, &[]);
    command.env("KIRI_NO_IMAGES", "1");
    let out = command.output().unwrap();
    assert_succeeded(&out);
    let report = text(&out.stdout);
    let row = "| Images | off | `KIRI_NO_IMAGES=1` |";
    assert!(report.contains(row), "{report}");
}

#[test]
fn paging_tells_where_it_comes_from() {
    let dir = Scratch::new("doctor-paging");
    for (args, variable, row) in [
        (&[][..], None, "| Paging | auto | default |"),
        (&["-p"][..], None, "| Paging | always | `-p` |"),
        (
            &["--paging", "never"][..],
            None,
            "| Paging | never | `--paging` |",
        ),
        (&[][..], Some("1"), "| Paging | always | `KIRI_PAGING=1` |"),
    ] {
        let mut command = doctor(&dir, args);
        if let Some(value) = variable {
            command.env("KIRI_PAGING", value);
        }
        let out = command.output().unwrap();
        assert_succeeded(&out);
        let report = text(&out.stdout);
        assert!(report.contains(row), "{row}\n{report}");
    }
}

#[test]
fn front_matter_tells_where_it_comes_from() {
    let dir = Scratch::new("doctor-front-matter");
    for (args, variable, row) in [
        (&[][..], None, "| Front matter | table | default |"),
        (
            &["--front-matter", "off"][..],
            Some("code"),
            "| Front matter | off | `--front-matter` |",
        ),
        (
            &[][..],
            Some("hide"),
            "| Front matter | hide | `KIRI_FRONT_MATTER=hide` |",
        ),
    ] {
        let mut command = doctor(&dir, args);
        if let Some(value) = variable {
            command.env("KIRI_FRONT_MATTER", value);
        }
        let out = command.output().unwrap();
        assert_succeeded(&out);
        let report = text(&out.stdout);
        assert!(report.contains(row), "{row}\n{report}");
    }
    write(
        &dir.join("home/.config/kiri/config.toml"),
        "front_matter = \"code\"\n",
    );
    let report = piped(&dir, &[]);
    let row = "| Front matter | code | configuration file |";
    assert!(report.contains(row), "{row}\n{report}");
}

#[test]
fn table_rows_tell_where_they_come_from() {
    let dir = Scratch::new("doctor-table-rows");
    for (args, variable, row) in [
        (&[][..], None, "| Table rows | zebra | default |"),
        (
            &["--table-rows", "off"][..],
            Some("lines"),
            "| Table rows | off | `--table-rows` |",
        ),
        (
            &[][..],
            Some("lines"),
            "| Table rows | lines | `KIRI_TABLE_ROWS=lines` |",
        ),
    ] {
        let mut command = doctor(&dir, args);
        if let Some(value) = variable {
            command.env("KIRI_TABLE_ROWS", value);
        }
        let out = command.output().unwrap();
        assert_succeeded(&out);
        let report = text(&out.stdout);
        assert!(report.contains(row), "{row}\n{report}");
    }
}

#[test]
fn diagrams_tell_where_they_come_from() {
    let dir = Scratch::new("doctor-diagrams");
    for (args, variable, row) in [
        (&[][..], None, "| Diagrams | on | default |"),
        (
            &["--diagrams", "on"][..],
            Some("0"),
            "| Diagrams | on | `--diagrams` |",
        ),
        (
            &[][..],
            Some("off"),
            "| Diagrams | off | `KIRI_DIAGRAMS=off` |",
        ),
    ] {
        let mut command = doctor(&dir, args);
        if let Some(value) = variable {
            command.env("KIRI_DIAGRAMS", value);
        }
        let out = command.output().unwrap();
        assert_succeeded(&out);
        let report = text(&out.stdout);
        assert!(report.contains(row), "{row}\n{report}");
    }
    write(
        &dir.join("home/.config/kiri/config.toml"),
        "diagrams = false\n",
    );
    let report = piped(&dir, &[]);
    let row = "| Diagrams | off | configuration file |";
    assert!(report.contains(row), "{row}\n{report}");
}

#[test]
fn the_search_case_tells_where_it_comes_from() {
    let dir = Scratch::new("doctor-search-case");
    let report = piped(&dir, &[]);
    let row = "| Search case | smart | default |";
    assert!(report.contains(row), "{row}\n{report}");
    write(
        &dir.join("home/.config/kiri/config.toml"),
        "search_case = \"literal\"\n",
    );
    let report = piped(&dir, &[]);
    let row = "| Search case | literal | configuration file |";
    assert!(report.contains(row), "{row}\n{report}");
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
fn the_report_names_the_mermaid_command() {
    let dir = Scratch::new("doctor-mermaid-command");
    let row = |args: &[&str]| report_row(&piped(&dir, args), "| `mermaid` | command");
    let nothing = match cfg!(feature = "mermaid") {
        true => "| `mermaid` | command: none configured | not used |",
        false => "| `mermaid` | command: `mmdc` not found | not used |",
    };
    assert_eq!(row(&[]), nothing);
    // Found where nvm installs programs, in a build without merman.
    let nvm = "home/.nvm/versions/node/v24.16.0/bin/mmdc";
    common::executable(&dir.join(nvm), "#!/bin/sh\n");
    let found = match cfg!(feature = "mermaid") {
        true => nothing.to_owned(),
        false => "| `mermaid` | command found: `~/.nvm/versions/node/v24.16.0/bin/mmdc -i {input} \
                  -o {output} -t {theme} -b transparent -c {config}` | used |"
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
    let (mut terminal, tty) = pty();
    let mut child = doctor(&dir, &["--mode", "kitty"])
        .stdout(Stdio::from(tty))
        .spawn()
        .unwrap();
    let status = child.wait().unwrap();
    assert!(status.success());
    // Reading ends with an error once no one holds the terminal side.
    let mut output = Vec::new();
    let _ = terminal.read_to_end(&mut output);
    let output = text(&output);
    assert!(!output.contains("## Terminal"), "{output:?}");
    assert!(output.contains("kiri doctor"), "{output:?}");
    assert!(output.contains("kiri --doctor --raw"), "{output:?}");
    assert_eq!(common::graphics_commands(&output).len(), 1, "{output:?}");
}

/// Without a controlling terminal to ask, the report tells of the terminal
/// on stdout, as a run that renders a document sees it.
#[test]
fn without_a_controlling_terminal_the_report_tells_of_stdout() {
    let dir = Scratch::new("doctor-no-tty");
    let (mut terminal, tty) = pty();
    let size = rustix::termios::Winsize {
        ws_col: 100,
        ws_row: 30,
        ws_xpixel: 1200,
        ws_ypixel: 600,
    };
    rustix::termios::tcsetwinsize(&tty, size).unwrap();
    let mut child = doctor(&dir, &[]).stdout(Stdio::from(tty)).spawn().unwrap();
    assert!(child.wait().unwrap().success());
    // Reading ends with an error once no one holds the terminal side.
    let mut output = Vec::new();
    let _ = terminal.read_to_end(&mut output);
    let output = text(&output);
    assert!(output.contains("100 × 30 cells"), "{output:?}");
    assert!(output.contains("12 × 20 pixels"), "{output:?}");
    assert!(output.contains("\x1b["), "{output:?}");
}

/// What `kiri --doctor` with `args` writes into a terminal.
fn in_terminal(dir: &Path, args: &[&str]) -> String {
    let (mut terminal, tty) = pty();
    let mut child = doctor(dir, args).stdout(Stdio::from(tty)).spawn().unwrap();
    assert!(child.wait().unwrap().success());
    // Reading ends with an error once no one holds the terminal side.
    let mut output = Vec::new();
    let _ = terminal.read_to_end(&mut output);
    text(&output)
}

#[test]
fn raw_writes_the_markdown_into_a_terminal_too() {
    let dir = Scratch::new("doctor-raw-terminal");
    let output = in_terminal(&dir, &["--raw", "--mode", "kitty"]);
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
    };
    let choices = choose(&Options::default(), &env, &Default::default());
    let facts = Facts {
        env: &env,
        mode: Setting::new(Mode::Auto, Origin::Default),
        width: None,
        max_image_size: Setting::new(10, Origin::Default),
        detection: &detection,
        choices: &choices,
        config_error: None,
        theme_error: None,
        output: kiri::doctor::Output::Rendered,
    };
    insta::assert_snapshot!(stable(&report(&facts)));
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
    };
    let choices = choose(&Options::default(), &env, &Default::default());
    let facts = Facts {
        env: &env,
        mode: Setting::new(Mode::Auto, Origin::Default),
        width: None,
        max_image_size: Setting::new(10, Origin::Default),
        detection: &detection,
        choices: &choices,
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
    };
    let choices = choose(&Options::default(), &env, &Default::default());
    let facts = Facts {
        env: &env,
        mode: Setting::new(Mode::Auto, Origin::Default),
        width: None,
        max_image_size: Setting::new(10, Origin::Default),
        detection: &detection,
        choices: &choices,
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
