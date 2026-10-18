//! `kiri --doctor` end to end: the report as Markdown in a pipe, and
//! rendered in a terminal.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use kiri::config::{Environment, Options, Origin, Setting, choose};
use kiri::doctor::{Facts, report};
use kiri::term::{Capabilities, Detection, Mode, WindowSize};

mod common;

use common::{Scratch, text, write};

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

fn assert_succeeded(out: &Output) {
    assert!(out.status.success(), "{}", text(&out.stderr));
}

/// The report with the parts that change between builds replaced: kiri's
/// version, and the image formats, which depend on the build's features.
fn stable(report: &str) -> String {
    report
        .replace(env!("CARGO_PKG_VERSION"), "[version]")
        .lines()
        .map(|line| match line.contains("PNG") {
            true => "[image formats]",
            false => line,
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
fn without_doctor_a_file_is_still_required() {
    let dir = Scratch::new("doctor-required");
    let out = common::command(&dir).output().unwrap();
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("<FILE>"), "{stderr}");
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

/// A pseudo-terminal: the terminal side, and the program's side.
fn pty() -> (std::fs::File, std::fs::File) {
    use std::ffi::OsStr;
    use std::fs::File;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    use rustix::fs::OFlags;
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};

    let terminal = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&terminal).unwrap();
    unlockpt(&terminal).unwrap();
    let name = ptsname(&terminal, Vec::new()).unwrap();
    let tty = File::options()
        .read(true)
        .write(true)
        .custom_flags(OFlags::NOCTTY.bits() as i32)
        .open(OsStr::from_bytes(name.as_bytes()))
        .unwrap();
    (File::from(terminal), tty)
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
