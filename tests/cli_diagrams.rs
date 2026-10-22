//! The kiri binary end to end with Mermaid diagrams.

mod common;

use common::{Scratch, kiri, sent, text};

const FLOWCHART: &str = "# Plan\n\n```mermaid\nflowchart TD\n    A --> B\n```\n";
const BROKEN: &str = "# Plan\n\n```mermaid\nflowchart TD\n    A[Unclosed --> B\n```\n";

#[cfg(feature = "mermaid")]
#[test]
fn draws_the_fixtures_diagrams_and_warns_about_the_broken_one() {
    let dir = Scratch::new("mermaid-fixture");
    let fixture = common::fixtures().join("mermaid.md");
    let out = common::kiri_on(&dir, &fixture, &["--mode", "kitty"]);
    assert!(out.status.success());
    assert_eq!(
        text(&out.stderr),
        "kiri: mermaid diagram on line 95: Diagram parse error (flowchart-v2): \
         Unterminated node label (missing `]`)\n"
    );
    let stdout = text(&out.stdout);
    assert_eq!(sent(&stdout).len(), 9);
    assert!(stdout.contains("A[Unclosed label --> B"), "{stdout:?}");
    assert!(!stdout.contains("sequenceDiagram"), "{stdout:?}");
}

#[cfg(feature = "mermaid")]
#[test]
fn names_a_piped_diagram_by_its_first_line() {
    let dir = Scratch::new("mermaid-piped");
    let out = common::kiri_piped(&dir, &["--mode", "kitty"], BROKEN.as_bytes());
    assert!(out.status.success());
    assert_eq!(
        text(&out.stderr),
        "kiri: mermaid diagram \"flowchart TD\": Diagram parse error (flowchart-v2): \
         Unterminated node label (missing `]`)\n"
    );
}

#[test]
fn diagrams_are_code_without_images() {
    let dir = Scratch::new("mermaid-no-images");
    for args in [&["--mode", "kitty", "--no-images"][..], &["--mode", "ansi"]] {
        let out = kiri(&dir, BROKEN, args);
        assert!(out.status.success());
        assert_eq!(text(&out.stderr), "", "{args:?}");
        let out = kiri(&dir, FLOWCHART, args);
        let stdout = text(&out.stdout);
        assert!(sent(&stdout).is_empty(), "{args:?}: {stdout:?}");
        assert!(stdout.contains("flowchart TD"), "{args:?}: {stdout:?}");
    }
}

#[cfg(not(feature = "mermaid"))]
#[test]
fn diagrams_are_code_in_a_build_without_mermaid_when_mmdc_is_not_looked_for() {
    let dir = Scratch::new("mermaid-not-built");
    // mmdc on PATH, which kiri would find.
    let called = dir.join("called");
    fake_mmdc(&dir.join("bin"), &called);
    let config = dir.join("kiri.toml");
    common::write(&config, "[diagrams.mermaid]\nauto_detect = false\n");
    let run = |source: &str| {
        let document = dir.join("document.md");
        std::fs::write(&document, source).unwrap();
        common::command(&dir)
            .env("KIRI_CONFIG", &config)
            .env("PATH", dir.join("bin"))
            .args(["--mode", "kitty"])
            .arg(&document)
            .output()
            .unwrap()
    };
    let out = run(BROKEN);
    assert_eq!(text(&out.stderr), "");
    let out = run(FLOWCHART);
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    assert!(sent(&stdout).is_empty(), "{stdout:?}");
    assert!(stdout.contains("flowchart TD"), "{stdout:?}");
    assert!(!called.exists());
}

/// Runs kiri in kitty mode on `source` with `args` and `KIRI_DIAGRAMS` set
/// to `variable`, and with `config` as its configuration file.
fn with_settings(
    dir: &std::path::Path,
    source: &str,
    args: &[&str],
    variable: Option<&str>,
    config: &str,
) -> std::process::Output {
    common::write(&dir.join("config/kiri/config.toml"), config);
    let document = dir.join("document.md");
    std::fs::write(&document, source).unwrap();
    let mut command = common::command(dir);
    command.args(["--mode", "kitty"]).args(args).arg(&document);
    if let Some(value) = variable {
        command.env("KIRI_DIAGRAMS", value);
    }
    command.output().unwrap()
}

#[test]
fn diagrams_off_shows_them_as_code_without_rendering_them() {
    let dir = Scratch::new("mermaid-off");
    for (args, variable, config) in [
        (&["--diagrams", "off"][..], None, ""),
        (&["--diagrams", "off"][..], Some("on"), "diagrams = true\n"),
        (&[][..], Some("0"), ""),
        (&[][..], Some("off"), "diagrams = true\n"),
        (&[][..], None, "diagrams = false\n"),
        (&[][..], None, "[diagrams]\nenabled = false\n"),
    ] {
        let case = format!("{args:?} {variable:?} {config:?}");
        // A broken diagram is not rendered, so it gets no warning.
        let out = with_settings(&dir, BROKEN, args, variable, config);
        assert!(out.status.success(), "{case}");
        assert_eq!(text(&out.stderr), "", "{case}");
        let out = with_settings(&dir, FLOWCHART, args, variable, config);
        assert_eq!(text(&out.stderr), "", "{case}");
        let stdout = text(&out.stdout);
        assert!(sent(&stdout).is_empty(), "{case}: {stdout:?}");
        assert!(stdout.contains("flowchart TD"), "{case}: {stdout:?}");
    }
}

#[cfg(feature = "mermaid")]
#[test]
fn diagrams_on_overrides_the_environment_and_the_configuration_file() {
    let dir = Scratch::new("mermaid-on");
    for (args, variable, config) in [
        (&["--diagrams", "on"][..], Some("0"), "diagrams = false\n"),
        (&[][..], Some("1"), "diagrams = false\n"),
        (&[][..], Some(""), "diagrams = true\n"),
        (&[][..], Some("1"), "[diagrams]\nenabled = false\n"),
    ] {
        let case = format!("{args:?} {variable:?} {config:?}");
        let out = with_settings(&dir, FLOWCHART, args, variable, config);
        assert_eq!(text(&out.stderr), "", "{case}");
        assert_eq!(sent(&text(&out.stdout)).len(), 1, "{case}");
    }
}

#[test]
fn an_unknown_kiri_diagrams_is_left_out_with_a_warning() {
    let dir = Scratch::new("mermaid-unknown-variable");
    let out = with_settings(&dir, FLOWCHART, &[], Some("nope"), "diagrams = false\n");
    assert!(out.status.success());
    assert_eq!(
        text(&out.stderr),
        "kiri: KIRI_DIAGRAMS: `nope` is not on, off, 1 or 0; left out\n"
    );
    assert!(sent(&text(&out.stdout)).is_empty());
}

#[test]
fn help_explains_diagrams() {
    let dir = Scratch::new("mermaid-help");
    let out = common::command(&dir).arg("--help").output().unwrap();
    let help = text(&out.stdout);
    assert!(help.contains("--diagrams <SWITCH>"), "{help}");
    assert!(help.contains("[possible values: on, off]"), "{help}");
}

/// A configuration file that draws Mermaid with the shell script `script`,
/// which gets the input file, the output file and the theme. Through sh,
/// so the test does not run a file it has just written.
fn external(dir: &std::path::Path, script: &str) -> String {
    let path = dir.join("render.sh");
    std::fs::write(&path, script).unwrap();
    format!(
        "[diagrams.mermaid]\ncommand = [\"/bin/sh\", \"{}\", \"{{input}}\", \"{{output}}\", \"{{theme}}\"]\n",
        path.display()
    )
}

/// A stand-in for mmdc: an SVG as high as the source has lines, apart
/// from directives.
const DRAWS: &str = "lines=$(grep -vc '^%%' \"$1\")\n\
    printf '<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"%s\"/>' \
    $((lines * 20)) > \"$2\"\n";

#[test]
fn a_configured_command_draws_the_diagrams() {
    let dir = Scratch::new("mermaid-external");
    let config = external(&dir, DRAWS);
    let source = format!("{FLOWCHART}\n```mermaid\nsequenceDiagram\n```\n");
    let out = with_settings(&dir, &source, &[], None, &config);
    assert_eq!(text(&out.stderr), "");
    let images = sent(&text(&out.stdout));
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].keys["s"], "200");
    assert_eq!(images[0].keys["v"], "40");
    assert_eq!(images[1].keys["v"], "20");
}

#[test]
fn format_text_shows_a_document_s_diagrams_as_written() {
    let dir = Scratch::new("mermaid-text");
    let called = dir.join("called");
    let script = format!("touch '{}'\n{DRAWS}", called.display());
    let out = with_settings(
        &dir,
        FLOWCHART,
        &["-f", "text"],
        None,
        &external(&dir, &script),
    );
    assert!(out.status.success());
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    assert!(sent(&stdout).is_empty(), "{stdout:?}");
    let shown = common::strip_own_sequences(&stdout);
    for line in ["```mermaid", "flowchart TD", "A --> B"] {
        assert!(shown.contains(line), "{line:?} in {shown:?}");
    }
    assert!(!called.exists());
}

#[test]
fn a_command_that_fails_leaves_the_code_with_a_warning() {
    let dir = Scratch::new("mermaid-external-fails");
    let config = external(&dir, "echo 'Error: Parse error on line 2:' >&2\nexit 1\n");
    let out = with_settings(&dir, FLOWCHART, &[], None, &config);
    assert!(out.status.success());
    // Not drawn by merman either, in a build that has it.
    assert_eq!(
        text(&out.stderr),
        "kiri: mermaid diagram on line 3: sh exited with status 1: \
         Error: Parse error on line 2:\n"
    );
    let stdout = text(&out.stdout);
    assert!(sent(&stdout).is_empty(), "{stdout:?}");
    assert!(stdout.contains("flowchart TD"), "{stdout:?}");
}

#[test]
fn a_command_that_writes_no_svg_leaves_the_code_with_a_warning() {
    let dir = Scratch::new("mermaid-external-not-svg");
    let config = external(&dir, "echo 'not an SVG' > \"$2\"\n");
    let out = with_settings(&dir, FLOWCHART, &[], None, &config);
    assert!(out.status.success());
    let stderr = text(&out.stderr);
    assert!(
        stderr.starts_with("kiri: mermaid diagram on line 3: "),
        "{stderr:?}"
    );
    assert_eq!(stderr.lines().count(), 1, "{stderr:?}");
    let stdout = text(&out.stdout);
    assert!(sent(&stdout).is_empty(), "{stdout:?}");
    assert!(stdout.contains("flowchart TD"), "{stdout:?}");
}

#[test]
fn a_configured_command_gets_the_theme_for_the_page() {
    let dir = Scratch::new("mermaid-external-theme");
    let record = dir.join("theme");
    let script = format!("echo \"$3\" >> '{}'\n{DRAWS}", record.display());
    let config = external(&dir, &script);
    for args in [&["--light"][..], &["--dark"]] {
        let out = with_settings(&dir, FLOWCHART, args, None, &config);
        assert_eq!(text(&out.stderr), "", "{args:?}");
    }
    assert_eq!(std::fs::read_to_string(&record).unwrap(), "default\ndark\n");
}

/// A stand-in for mermaid-cli in `bin`: `mmdc`, a Node script, and the
/// `node` it runs, which draws an SVG as high as the source has lines,
/// apart from directives, and notes in `called` the theme it got and
/// whether it got a configuration file.
fn fake_mmdc(bin: &std::path::Path, called: &std::path::Path) {
    common::executable(&bin.join("mmdc"), "#!/usr/bin/env node\n");
    let node = format!(
        "#!/bin/sh\n\
         PATH=/usr/bin:/bin\n\
         shift\n\
         while [ $# -gt 0 ]; do\n\
           case $1 in -i) input=$2 ;; -o) output=$2 ;; -t) theme=$2 ;; -c) config=$2 ;; esac\n\
           shift\n\
         done\n\
         [ -f \"$config\" ] && theme=\"$theme config\"\n\
         echo \"$theme\" >> '{}'\n\
         lines=$(grep -vc '^%%' \"$input\")\n\
         printf '<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"%s\"/>' \
         $((lines * 20)) > \"$output\"\n",
        called.display()
    );
    common::executable(&bin.join("node"), &node);
}

#[test]
fn a_build_without_merman_finds_mmdc_by_itself() {
    let dir = Scratch::new("mermaid-find-mmdc");
    let called = dir.join("called");
    // On PATH, and in an nvm directory that is not on it.
    let nvm = dir.join("home/.nvm/versions/node/v24.16.0/bin");
    for (bin, path) in [(dir.join("bin"), dir.join("bin")), (nvm, dir.join("empty"))] {
        let _ = std::fs::remove_file(&called);
        fake_mmdc(&bin, &called);
        let document = dir.join("document.md");
        std::fs::write(&document, FLOWCHART).unwrap();
        let out = common::command(&dir)
            .env("PATH", &path)
            .args(["--mode", "kitty", "--dark"])
            .arg(&document)
            .output()
            .unwrap();
        assert_eq!(text(&out.stderr), "", "{}", bin.display());
        let images = sent(&text(&out.stdout));
        assert_eq!(images.len(), 1, "{}", bin.display());
        let called = std::fs::read_to_string(&called).ok();
        if cfg!(feature = "mermaid") {
            // merman draws it.
            assert_eq!(called, None);
        } else {
            assert_eq!(called.as_deref(), Some("dark config\n"));
            assert_eq!(images[0].keys["v"], "40");
        }
        let _ = std::fs::remove_dir_all(&bin);
    }
}

/// Runs kiri in kitty mode on `source` with the configuration `config`,
/// its stderr a terminal: what the terminal got, and the document.
fn on_terminal(dir: &std::path::Path, source: &str, config: &str) -> (String, String) {
    use std::io::Read;
    common::write(&dir.join("config/kiri/config.toml"), config);
    let document = dir.join("document.md");
    std::fs::write(&document, source).unwrap();
    let (mut terminal, tty) = common::pty();
    let out = common::command(dir)
        .args(["--mode", "kitty"])
        .arg(&document)
        .stderr(std::process::Stdio::from(tty))
        .output()
        .unwrap();
    assert!(out.status.success());
    // Reading ends with an error once no one holds the terminal side.
    let mut stderr = Vec::new();
    let _ = terminal.read_to_end(&mut stderr);
    (text(&stderr), text(&out.stdout))
}

#[test]
fn slow_diagrams_show_their_progress_on_a_terminal() {
    let dir = Scratch::new("mermaid-progress");
    let config = external(&dir, &format!("sleep 0.8\n{DRAWS}"));
    let (stderr, stdout) = on_terminal(&dir, FLOWCHART, &config);
    // Wiped at the end; 1/1 may show for a moment before.
    let shown = "\r\x1b[2Kkiri: rendering diagrams 0/1…";
    let done = "\r\x1b[2Kkiri: rendering diagrams 1/1…";
    let wiped = "\r\x1b[2K";
    assert!(
        [format!("{shown}{wiped}"), format!("{shown}{done}{wiped}")].contains(&stderr),
        "{stderr:?}"
    );
    assert_eq!(sent(&stdout).len(), 1);
    // Quick ones show nothing.
    let config = external(&dir, DRAWS);
    let (stderr, stdout) = on_terminal(&dir, FLOWCHART, &config);
    assert_eq!(stderr, "");
    assert_eq!(sent(&stdout).len(), 1);
}

#[test]
fn slow_diagrams_show_no_progress_when_stderr_is_not_a_terminal() {
    let dir = Scratch::new("mermaid-progress-piped");
    let config = external(&dir, &format!("sleep 0.8\n{DRAWS}"));
    let out = with_settings(&dir, FLOWCHART, &[], None, &config);
    assert!(out.status.success());
    assert_eq!(text(&out.stderr), "");
    assert_eq!(sent(&text(&out.stdout)).len(), 1);
}

/// Whether the processes in `pids` have all ended, or are zombies being
/// reaped, within 2 s.
fn all_gone(pids: &[String]) -> bool {
    common::wait_until(std::time::Duration::from_secs(2), || {
        !pids.iter().any(|pid| {
            let stat = std::path::Path::new("/proc").join(pid).join("stat");
            std::fs::read_to_string(stat).is_ok_and(|stat| {
                stat.rsplit_once(") ")
                    .is_some_and(|(_, state)| !state.starts_with('Z'))
            })
        })
    })
}

#[test]
fn a_signal_stops_the_programs_removes_their_files_and_ends_kiri() {
    use std::os::unix::process::ExitStatusExt;
    use std::time::Duration;

    use rustix::process::{Pid, Signal, kill_process};

    // Ctrl-C, a request to end and a terminal that closes.
    for signal in [Signal::INT, Signal::TERM, Signal::HUP] {
        // A short name, so that the cache directory is used.
        let dir = Scratch::new("s");
        let started = dir.join("started");
        // Each diagram's program starts a program of its own and waits
        // for it.
        let script = format!("sleep 30 &\necho $! >> '{}'\nwait\n", started.display());
        common::write(
            &dir.join("config/kiri/config.toml"),
            &external(&dir, &script),
        );
        let source = format!("{FLOWCHART}\n```mermaid\nsequenceDiagram\n```\n");
        let document = dir.join("document.md");
        std::fs::write(&document, source).unwrap();
        let temp = dir.join("temp");
        std::fs::create_dir_all(&temp).unwrap();
        let mut kiri = common::command(&dir)
            .env("TMPDIR", &temp)
            .env("XDG_CACHE_HOME", dir.join("x"))
            .args(["--mode", "kitty"])
            .arg(&document)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let pids = || -> Vec<String> {
            let started = std::fs::read_to_string(&started).unwrap_or_default();
            started.lines().map(str::to_owned).collect()
        };
        common::wait_until(Duration::from_secs(10), || pids().len() >= 2);
        assert_eq!(pids().len(), 2, "{signal:?}: both programs start");
        let pid = Pid::from_raw(kiri.id() as i32).unwrap();
        kill_process(pid, signal).unwrap();
        // kiri gets 10 s at most.
        let status = common::wait_or_kill(&mut kiri, Duration::from_secs(10))
            .unwrap_or_else(|| panic!("{signal:?}: kiri did not end"));
        let out = kiri.wait_with_output().unwrap();
        let gone = all_gone(&pids());
        if !gone {
            for pid in pids() {
                let _ = std::process::Command::new("kill").arg(pid).status();
            }
        }
        // Ended by the signal, as without the programs: a shell shows
        // status 130 after Ctrl-C.
        assert_eq!(status.signal(), Some(signal.as_raw()), "{status:?}");
        assert_eq!(text(&out.stderr), "", "{signal:?}");
        assert_eq!(text(&out.stdout), "", "{signal:?}");
        assert!(gone, "{signal:?}: the programs' own programs still run");
        for dir in [temp, dir.join("x/kiri/diagrams")] {
            let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
            assert!(left.is_empty(), "{signal:?}: {left:?}");
        }
    }
}

#[test]
fn ctrl_backslash_stops_the_programs_removes_their_files_and_ends_kiri() {
    use std::os::unix::process::ExitStatusExt;
    use std::time::Duration;

    use rustix::process::{Pid, Signal, kill_process};

    // A short name, so that the cache directory is used.
    let dir = Scratch::new("q");
    let started = dir.join("started");
    // Each diagram's program starts a program of its own and waits for it.
    let script = format!("sleep 30 &\necho $! >> '{}'\nwait\n", started.display());
    common::write(
        &dir.join("config/kiri/config.toml"),
        &external(&dir, &script),
    );
    let source = format!("{FLOWCHART}\n```mermaid\nsequenceDiagram\n```\n");
    let document = dir.join("document.md");
    std::fs::write(&document, source).unwrap();
    let temp = dir.join("temp");
    std::fs::create_dir_all(&temp).unwrap();
    let mut kiri = common::command(&dir)
        .env("TMPDIR", &temp)
        .env("XDG_CACHE_HOME", dir.join("x"))
        .args(["--mode", "kitty"])
        .arg(&document)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pids = || -> Vec<String> {
        let started = std::fs::read_to_string(&started).unwrap_or_default();
        started.lines().map(str::to_owned).collect()
    };
    common::wait_until(Duration::from_secs(10), || pids().len() >= 2);
    assert_eq!(pids().len(), 2, "both programs start");
    let pid = Pid::from_raw(kiri.id() as i32).unwrap();
    // What Ctrl-\ sends.
    kill_process(pid, Signal::QUIT).unwrap();
    // kiri gets 10 s at most.
    let status = common::wait_or_kill(&mut kiri, Duration::from_secs(10))
        .unwrap_or_else(|| panic!("kiri did not end"));
    let out = kiri.wait_with_output().unwrap();
    let gone = all_gone(&pids());
    if !gone {
        for pid in pids() {
            let _ = std::process::Command::new("kill").arg(pid).status();
        }
    }
    // Ended by the signal, as without the programs.
    assert_eq!(status.signal(), Some(Signal::QUIT.as_raw()), "{status:?}");
    assert_eq!(text(&out.stderr), "");
    assert_eq!(text(&out.stdout), "");
    assert!(gone, "the programs' own programs still run");
    for dir in [temp, dir.join("x/kiri/diagrams")] {
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert!(left.is_empty(), "{left:?}");
    }
}

#[test]
fn a_signal_ignored_when_kiri_starts_stays_ignored() {
    use std::time::Duration;

    use rustix::process::{Pid, Signal, kill_process};

    let dir = Scratch::new("mermaid-ignored");
    let started = dir.join("started");
    let script = format!("touch '{}'\nsleep 1\n{DRAWS}", started.display());
    common::write(
        &dir.join("config/kiri/config.toml"),
        &external(&dir, &script),
    );
    let document = dir.join("document.md");
    std::fs::write(&document, FLOWCHART).unwrap();
    // As nohup and a shell's background jobs leave them.
    let mut kiri = std::process::Command::new("/bin/sh")
        .env_clear()
        .env("HOME", dir.join("home"))
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .args(["-c", "trap '' INT TERM HUP\nexec \"$0\" \"$@\""])
        .arg(env!("CARGO_BIN_EXE_kiri"))
        .args(["--mode", "kitty"])
        .arg(&document)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = common::wait_until(Duration::from_secs(10), || started.exists());
    assert!(start, "the program starts");
    let pid = Pid::from_raw(kiri.id() as i32).unwrap();
    for signal in [Signal::INT, Signal::TERM, Signal::HUP] {
        kill_process(pid, signal).unwrap();
    }
    let status =
        common::wait_or_kill(&mut kiri, Duration::from_secs(10)).expect("kiri did not end");
    let out = kiri.wait_with_output().unwrap();
    assert!(status.success(), "{status:?}: {}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    assert_eq!(sent(&text(&out.stdout)).len(), 1);
}

#[test]
fn ctrl_c_ends_kiri_at_once_when_no_diagram_runs_a_program() {
    use std::os::unix::process::ExitStatusExt;
    use std::time::Duration;

    use rustix::process::{Pid, Signal, kill_process};

    // A command is configured, but the document has no diagram for it.
    let dir = Scratch::new("mermaid-no-diagram");
    common::write(&dir.join("config/kiri/config.toml"), &external(&dir, DRAWS));
    // An SVG that takes a while to read and then does not load, many
    // times over: loading the images takes far longer than the test
    // waits.
    let rects: String = (0..150_000)
        .map(|n| {
            format!(
                "<rect x=\"{}\" y=\"{}\" width=\"1\" height=\"1\"/>",
                n % 100,
                n / 1000
            )
        })
        .collect();
    let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{rects}<unclosed");
    let size = svg.len();
    std::fs::write(dir.join("slow.svg"), svg).unwrap();
    let images: String = (0..200).map(|n| format!("![](slow.svg?{n})\n\n")).collect();
    let document = dir.join("document.md");
    std::fs::write(&document, format!("# Images\n\n{images}")).unwrap();
    let mut kiri = common::command(&dir)
        .args(["--mode", "kitty"])
        .arg(&document)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // Loading the images: kiri has read more bytes than one of them has.
    let io = std::path::Path::new("/proc")
        .join(kiri.id().to_string())
        .join("io");
    let read = || {
        let io = std::fs::read_to_string(&io).unwrap_or_default();
        (io.lines())
            .find_map(|line| line.strip_prefix("rchar: ")?.parse::<usize>().ok())
            .unwrap_or(0)
    };
    let loading = common::wait_until(Duration::from_secs(5), || read() > size);
    if !loading {
        kiri.kill().unwrap();
        kiri.wait().unwrap();
        panic!("kiri does not load the images");
    }
    let pid = Pid::from_raw(kiri.id() as i32).unwrap();
    kill_process(pid, Signal::INT).unwrap();
    let status = common::wait_or_kill(&mut kiri, Duration::from_secs(5));
    let status = status.map(|status| status.signal());
    assert_eq!(status, Some(Some(Signal::INT.as_raw())), "Ctrl-C ends kiri");
}

#[test]
fn ctrl_c_ends_kiri_when_the_pager_cannot_open_and_it_prints_instead() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::time::Duration;

    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::process::{Pid, Signal, kill_process};

    let dir = Scratch::new("mermaid-printed");
    common::write(&dir.join("config/kiri/config.toml"), &external(&dir, DRAWS));
    // Longer than a terminal holds unread, so that printing it waits.
    let lines: String = (0..20_000).map(|n| format!("Line {n}\n\n")).collect();
    let document = dir.join("document.md");
    std::fs::write(&document, format!("{FLOWCHART}\n{lines}")).unwrap();
    // The terminal side is not read.
    let (terminal, tty) = common::pty();
    let mut command = common::command(&dir);
    command
        .args(["--mode", "kitty", "-p"])
        .arg(&document)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(tty))
        .stderr(std::process::Stdio::piped());
    // SAFETY: setsid is async-signal-safe and touches no memory.
    unsafe {
        // A session of its own and no controlling terminal: standard
        // output is a terminal, but there is none to read keys from.
        command.pre_exec(|| {
            rustix::process::setsid()?;
            Ok(())
        });
    }
    let mut kiri = command.spawn().unwrap();
    let mut stderr = BufReader::new(kiri.stderr.take().unwrap());
    let mut line = String::new();
    stderr.read_line(&mut line).unwrap();
    assert_eq!(
        line,
        "kiri: no terminal to read keys from, so the document is printed\n"
    );
    // Printing, and waiting for the terminal: the document reaches it.
    let mut fds = [PollFd::new(&terminal, PollFlags::IN)];
    let timeout = Timespec::try_from(Duration::from_secs(5)).unwrap();
    if poll(&mut fds, Some(&timeout)).unwrap() == 0 {
        // Writing to a terminal nobody reads, it would wait for ever.
        kiri.kill().unwrap();
        kiri.wait().unwrap();
        panic!("kiri does not print");
    }
    let pid = Pid::from_raw(kiri.id() as i32).unwrap();
    kill_process(pid, Signal::INT).unwrap();
    let status = common::wait_or_kill(&mut kiri, Duration::from_secs(5));
    let status = status.map(|status| status.signal());
    assert_eq!(status, Some(Some(Signal::INT.as_raw())), "Ctrl-C ends kiri");
}

#[test]
fn a_program_s_files_are_in_kiri_s_cache_directory_until_it_is_done() {
    use std::os::unix::fs::PermissionsExt;

    // A short name, so that the cache directory's path is short enough
    // for the sockets a browser makes in it.
    let dir = Scratch::new("c");
    let record = dir.join("tmpdir");
    let script = format!("printf '%s' \"$TMPDIR\" > '{}'\n{DRAWS}", record.display());
    let config = external(&dir, &script);
    common::write(&dir.join("config/kiri/config.toml"), &config);
    let document = dir.join("document.md");
    std::fs::write(&document, FLOWCHART).unwrap();
    let temp = dir.join("temp");
    std::fs::create_dir_all(&temp).unwrap();
    for (variable, cache) in [
        (Some(dir.join("x")), dir.join("x/kiri")),
        (None, dir.join("home/.cache/kiri")),
    ] {
        let mut command = common::command(&dir);
        command.env("TMPDIR", &temp);
        if let Some(variable) = &variable {
            command.env("XDG_CACHE_HOME", variable);
        }
        let out = command
            .args(["--mode", "kitty"])
            .arg(&document)
            .output()
            .unwrap();
        assert_eq!(text(&out.stderr), "", "{variable:?}");
        assert_eq!(sent(&text(&out.stdout)).len(), 1, "{variable:?}");
        // The program's temporary directory was its own, in the cache.
        let tmpdir = std::fs::read_to_string(&record).unwrap();
        let diagrams = cache.join("diagrams");
        assert_eq!(
            std::path::Path::new(&tmpdir).parent(),
            Some(diagrams.as_path()),
            "{variable:?}"
        );
        let mode = std::fs::metadata(&cache).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "{variable:?}");
        assert_eq!(std::fs::read_dir(&diagrams).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(&temp).unwrap().count(), 0);
    }
}

#[test]
fn with_an_empty_tmpdir_a_program_s_files_go_in_tmp() {
    let dir = Scratch::new("empty-tmpdir");
    let record = dir.join("tmpdir");
    let script = format!("printf '%s' \"$TMPDIR\" > '{}'\n{DRAWS}", record.display());
    let config = external(&dir, &script);
    common::write(&dir.join("config/kiri/config.toml"), &config);
    let document = dir.join("document.md");
    std::fs::write(&document, FLOWCHART).unwrap();
    // A file where the cache directory would be, so it cannot be used.
    std::fs::write(dir.join("file"), "").unwrap();
    let out = common::command(&dir)
        .current_dir(&*dir)
        .env("TMPDIR", "")
        .env("XDG_CACHE_HOME", dir.join("file"))
        .args(["--mode", "kitty"])
        .arg(&document)
        .output()
        .unwrap();
    assert_eq!(text(&out.stderr), "");
    assert_eq!(sent(&text(&out.stdout)).len(), 1);
    // As `--doctor` says: an empty TMPDIR is /tmp, not the current
    // directory.
    let tmpdir = std::fs::read_to_string(&record).unwrap();
    let tmpdir = std::path::Path::new(&tmpdir);
    assert_eq!(tmpdir.parent(), Some(std::path::Path::new("/tmp")));
    let name = tmpdir.file_name().unwrap().to_string_lossy();
    assert!(name.starts_with("kiri-diagram-"), "{name}");
    assert!(!tmpdir.exists());
}

#[test]
fn the_configuration_file_sets_the_time_limit() {
    let dir = Scratch::new("mermaid-timeout");
    let config = format!(
        "[diagrams]\ntimeout = 0.5\n\n{}",
        external(&dir, &format!("sleep 5\n{DRAWS}"))
    );
    let start = std::time::Instant::now();
    let out = with_settings(&dir, FLOWCHART, &[], None, &config);
    assert!(out.status.success());
    assert_eq!(
        text(&out.stderr),
        "kiri: mermaid diagram on line 3: sh did not finish in 0.5 s\n"
    );
    assert!(sent(&text(&out.stdout)).is_empty());
    assert!(start.elapsed() < std::time::Duration::from_secs(4));
}

#[test]
fn a_diagram_for_the_other_page_lies_on_a_panel_unless_turned_off() {
    let dir = Scratch::new("mermaid-panel");
    let dark = "# Plan\n\n```mermaid\n%%{init: {\"theme\": \"dark\"}}%%\nflowchart TD\n```\n";
    let command = external(&dir, DRAWS);
    let width = |args: &[&str], config: &str| {
        let out = with_settings(&dir, dark, args, None, config);
        assert_eq!(text(&out.stderr), "", "{args:?} {config:?}");
        let images = sent(&text(&out.stdout));
        assert_eq!(images.len(), 1, "{args:?} {config:?}");
        images[0].keys["s"].clone()
    };
    // Eight pixels of panel on each side, on a light page: 216 pixels,
    // sent as whole cells of 10.
    assert_eq!(width(&["--light"], &command), "220");
    assert_eq!(width(&["--dark"], &command), "200");
    let off = format!("[diagrams]\nbackground_panel = false\n\n{command}");
    assert_eq!(width(&["--light"], &off), "200");
}

// A Mermaid file on its own

/// The flowchart of `FLOWCHART`, as a file of its own.
const FLOWCHART_FILE: &str = "flowchart TD\n    A --> B\n";

/// Runs kiri with `args` on the file `name` of `source` in `dir`, with
/// `config` as its configuration file; in kitty mode unless `args` choose
/// a mode.
fn on_file(
    dir: &std::path::Path,
    name: &str,
    source: &str,
    args: &[&str],
    config: &str,
) -> std::process::Output {
    common::write(&dir.join("config/kiri/config.toml"), config);
    let path = dir.join(name);
    std::fs::write(&path, source).unwrap();
    let mut command = common::command(dir);
    if !args.contains(&"--mode") {
        command.args(["--mode", "kitty"]);
    }
    command.args(args).arg(&path).output().unwrap()
}

/// Fails unless `out` shows `source`, a diagram, as code, without a word
/// on standard error.
fn assert_shown_as_code(out: &std::process::Output, source: &str, case: &str) {
    assert!(out.status.success(), "{case}: {}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "", "{case}");
    let stdout = text(&out.stdout);
    assert!(sent(&stdout).is_empty(), "{case}: {stdout:?}");
    let shown = common::strip_own_sequences(&stdout);
    for line in source.lines() {
        assert!(shown.contains(line), "{case}: {line:?} in {shown:?}");
    }
}

#[test]
fn a_mermaid_file_is_drawn_on_its_own_like_an_image() {
    let dir = Scratch::new("mermaid-file");
    let config = external(&dir, DRAWS);
    // What DRAWS draws for the flowchart, as an image file.
    let svg = dir.join("same.svg");
    std::fs::write(
        &svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="40"/>"#,
    )
    .unwrap();
    for args in [&[][..], &["--width", "5"]] {
        let image = common::kiri_on(&dir, &svg, &[&["--mode", "kitty"][..], args].concat());
        assert_eq!(sent(&text(&image.stdout)).len(), 1, "{args:?}");
        for name in ["flow.mmd", "flow.mermaid", "FLOW.MMD"] {
            let out = on_file(&dir, name, FLOWCHART_FILE, args, &config);
            assert!(out.status.success(), "{name}: {}", text(&out.stderr));
            assert_eq!(text(&out.stderr), "", "{name}");
            assert_eq!(text(&out.stdout), text(&image.stdout), "{name} {args:?}");
        }
    }
}

#[test]
fn format_mermaid_draws_standard_input_or_a_file_of_another_name() {
    let dir = Scratch::new("mermaid-format");
    common::write(&dir.join("config/kiri/config.toml"), &external(&dir, DRAWS));
    for format in ["mermaid", "Mermaid", "mmd"] {
        let args = ["--mode", "kitty", "-f", format];
        let out = common::kiri_piped(&dir, &args, FLOWCHART_FILE.as_bytes());
        assert_eq!(text(&out.stderr), "", "{format}");
        assert_eq!(sent(&text(&out.stdout)).len(), 1, "{format}");
        let out = on_file(
            &dir,
            "diagram.txt",
            FLOWCHART_FILE,
            &args[2..],
            &external(&dir, DRAWS),
        );
        assert_eq!(text(&out.stderr), "", "{format}");
        assert_eq!(sent(&text(&out.stdout)).len(), 1, "{format}");
    }
}

#[test]
fn a_mermaid_file_is_drawn_even_with_images_off() {
    let dir = Scratch::new("mermaid-file-images-off");
    let config = format!("images = false\n{}", external(&dir, DRAWS));
    for args in [&[][..], &["-I"]] {
        let out = on_file(&dir, "flow.mmd", FLOWCHART_FILE, args, &config);
        assert_eq!(text(&out.stderr), "", "{args:?}");
        assert_eq!(sent(&text(&out.stdout)).len(), 1, "{args:?}");
    }
}

#[test]
fn a_mermaid_file_that_is_not_drawn_shows_its_source() {
    let dir = Scratch::new("mermaid-file-not-drawn");
    let called = dir.join("called");
    let config = external(&dir, &format!("touch '{}'\n{DRAWS}", called.display()));
    for args in [
        &["--diagrams", "off"][..],
        &["--mode", "ansi"],
        &["--mode", "plain"],
        &["-f", "text"],
    ] {
        let out = on_file(&dir, "flow.mmd", FLOWCHART_FILE, args, &config);
        assert_shown_as_code(&out, FLOWCHART_FILE, &format!("{args:?}"));
    }
    let off = format!("[diagrams]\nenabled = false\n\n{config}");
    let out = on_file(&dir, "flow.mmd", FLOWCHART_FILE, &[], &off);
    assert_shown_as_code(&out, FLOWCHART_FILE, "enabled = false");
    assert!(!called.exists());
    // Plain output gets it as it is, as `--format text` does.
    let out = on_file(
        &dir,
        "flow.mmd",
        FLOWCHART_FILE,
        &["--mode", "plain"],
        &config,
    );
    assert_eq!(text(&out.stdout), FLOWCHART_FILE);
}

#[test]
fn a_mermaid_file_that_fails_to_draw_shows_its_source_and_is_an_error() {
    let dir = Scratch::new("mermaid-file-fails");
    let config = external(&dir, "echo 'Error: Parse error on line 2:' >&2\nexit 1\n");
    let out = on_file(&dir, "flow.mmd", FLOWCHART_FILE, &[], &config);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!(
            "kiri: {}: sh exited with status 1: Error: Parse error on line 2:\n",
            dir.join("flow.mmd").display()
        )
    );
    let stdout = text(&out.stdout);
    assert!(sent(&stdout).is_empty(), "{stdout:?}");
    assert!(stdout.contains("flowchart TD"), "{stdout:?}");
    // Standard input is named as in other errors.
    let args = ["--mode", "kitty", "-f", "mermaid"];
    let out = common::kiri_piped(&dir, &args, FLOWCHART_FILE.as_bytes());
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        "kiri: <stdin>: sh exited with status 1: Error: Parse error on line 2:\n"
    );
}

#[test]
fn a_mermaid_file_not_in_utf_8_is_an_error() {
    let dir = Scratch::new("mermaid-file-latin1");
    let path = dir.join("flow.mmd");
    std::fs::write(&path, b"flowchart TD\n    A[\xe4] --> B\n").unwrap();
    let out = common::kiri_on(&dir, &path, &["--mode", "plain"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!("kiri: {}: not UTF-8 text\n", path.display())
    );
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn a_mermaid_file_takes_the_theme_and_the_panel_of_the_page() {
    let dir = Scratch::new("mermaid-file-theme");
    let record = dir.join("theme");
    let config = external(
        &dir,
        &format!("echo \"$3\" >> '{}'\n{DRAWS}", record.display()),
    );
    let light = on_file(&dir, "flow.mmd", FLOWCHART_FILE, &["--light"], &config);
    let dark = on_file(&dir, "flow.mmd", FLOWCHART_FILE, &["--dark"], &config);
    assert_eq!(std::fs::read_to_string(&record).unwrap(), "default\ndark\n");
    // The page's background behind it, as behind a document.
    for (out, page) in [(light, "48;2;255;255;255m"), (dark, "48;2;30;30;30m")] {
        assert_eq!(text(&out.stderr), "");
        let stdout = text(&out.stdout);
        assert_eq!(sent(&stdout).len(), 1);
        assert!(stdout.contains(page), "{stdout:?}");
    }
    let dark_diagram = format!("%%{{init: {{\"theme\": \"dark\"}}}}%%\n{FLOWCHART_FILE}");
    let out = on_file(&dir, "dark.mmd", &dark_diagram, &["--light"], &config);
    let images = sent(&text(&out.stdout));
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].keys["s"], "220");
}

#[cfg(not(feature = "mermaid"))]
#[test]
fn a_mermaid_file_without_a_renderer_shows_its_source_and_a_hint() {
    let dir = Scratch::new("mermaid-file-no-renderer");
    let config = "[diagrams.mermaid]\nauto_detect = false\n";
    let out = on_file(&dir, "flow.mmd", FLOWCHART_FILE, &[], config);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stderr),
        format!(
            "kiri: {}: no program to draw Mermaid diagrams; see --doctor\n",
            dir.join("flow.mmd").display()
        )
    );
    let shown = common::strip_own_sequences(&text(&out.stdout));
    assert!(shown.contains("flowchart TD"), "{shown:?}");
    // Not when diagrams are off, or the terminal cannot show them.
    for args in [&["--diagrams", "off"][..], &["--mode", "ansi"]] {
        let out = on_file(&dir, "flow.mmd", FLOWCHART_FILE, args, config);
        assert_shown_as_code(&out, FLOWCHART_FILE, &format!("{args:?}"));
    }
}

#[cfg(feature = "mermaid")]
#[test]
fn merman_draws_a_mermaid_file() {
    let dir = Scratch::new("mermaid-file-merman");
    let out = on_file(&dir, "flow.mmd", FLOWCHART_FILE, &[], "");
    assert_eq!(text(&out.stderr), "");
    assert_eq!(sent(&text(&out.stdout)).len(), 1);
    let broken = "flowchart TD\n    A[Unclosed --> B\n";
    let out = on_file(&dir, "broken.mmd", broken, &[], "");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!(
            "kiri: {}: Diagram parse error (flowchart-v2): \
             Unterminated node label (missing `]`)\n",
            dir.join("broken.mmd").display()
        )
    );
    assert!(text(&out.stdout).contains("A[Unclosed --> B"));
}

#[test]
fn help_names_the_mermaid_format() {
    let dir = Scratch::new("mermaid-format-help");
    let out = common::command(&dir).arg("--help").output().unwrap();
    let help = text(&out.stdout);
    assert!(
        help.contains("markdown, csv, tsv, mermaid (a diagram"),
        "{help}"
    );
}

// Where a diagram's error goes

/// A stand-in for mmdc that fails.
const FAILS: &str = "echo 'Error: bad' >&2\nexit 1\n";

/// Runs kiri in kitty mode on `source` with `args`, `KIRI_RENDER_ERRORS`
/// set to `variable`, and `config` as its configuration file.
fn with_render_errors(
    dir: &std::path::Path,
    source: &str,
    args: &[&str],
    variable: Option<&str>,
    config: &str,
) -> std::process::Output {
    common::write(&dir.join("config/kiri/config.toml"), config);
    let document = dir.join("document.md");
    std::fs::write(&document, source).unwrap();
    let mut command = common::command(dir);
    command
        .args(["--mode", "kitty", "--width", "120"])
        .args(args);
    if let Some(value) = variable {
        command.env("KIRI_RENDER_ERRORS", value);
    }
    command.arg(&document).output().unwrap()
}

/// What kiri tells in the document of `FAILS`'s diagram.
const INLINE: &str = "✗ mermaid diagram could not be drawn: sh exited with status 1: Error: bad";

#[test]
fn render_errors_go_into_the_document_on_stderr_or_both() {
    let dir = Scratch::new("render-errors");
    let config = external(&dir, FAILS);
    let warning = "kiri: mermaid diagram on line 3: sh exited with status 1: Error: bad\n";
    for (args, variable, file, inline, stderr) in [
        (&[][..], None, "", true, true),
        (&["--render-errors", "inline"][..], None, "", true, false),
        (&["--render-errors", "stderr"][..], None, "", false, true),
        (
            &["--render-errors", "both"][..],
            Some("inline"),
            "render_errors = \"stderr\"\n",
            true,
            true,
        ),
        (
            &[][..],
            Some("stderr"),
            "render_errors = \"inline\"\n",
            false,
            true,
        ),
        (
            &[][..],
            Some(""),
            "render_errors = \"inline\"\n",
            true,
            false,
        ),
        (&[][..], None, "render_errors = \"stderr\"\n", false, true),
        (&[][..], Some("inline"), "", true, false),
    ] {
        let case = format!("{args:?} {variable:?} {file:?}");
        let config = format!("{file}{config}");
        let out = with_render_errors(&dir, FLOWCHART, args, variable, &config);
        assert!(out.status.success(), "{case}: {}", text(&out.stderr));
        let expected = if stderr { warning } else { "" };
        assert_eq!(text(&out.stderr), expected, "{case}");
        let shown = common::strip_own_sequences(&text(&out.stdout));
        assert!(shown.contains("flowchart TD"), "{case}: {shown:?}");
        assert_eq!(shown.contains(INLINE), inline, "{case}: {shown:?}");
    }
}

#[test]
fn render_errors_leave_the_warnings_about_images_alone() {
    let dir = Scratch::new("render-errors-images");
    let config = external(&dir, FAILS);
    let source = format!("![gone](missing.png)\n\n{FLOWCHART}");
    let out = with_render_errors(&dir, &source, &["--render-errors", "inline"], None, &config);
    let missing = dir.join("missing.png");
    assert_eq!(
        text(&out.stderr),
        format!("kiri: {}: file not found\n", missing.display())
    );
    assert!(common::strip_own_sequences(&text(&out.stdout)).contains(INLINE));
}

#[test]
fn an_unknown_kiri_render_errors_is_left_out_with_a_warning() {
    let dir = Scratch::new("render-errors-unknown");
    let config = format!("render_errors = \"inline\"\n{}", external(&dir, FAILS));
    let out = with_render_errors(&dir, FLOWCHART, &[], Some("never"), &config);
    assert!(out.status.success());
    assert_eq!(
        text(&out.stderr),
        "kiri: KIRI_RENDER_ERRORS: `never` is not inline, stderr or both; left out\n"
    );
    assert!(common::strip_own_sequences(&text(&out.stdout)).contains(INLINE));
    let out = with_render_errors(&dir, FLOWCHART, &["--render-errors", "never"], None, "");
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("invalid value 'never'"));
}

#[test]
fn help_explains_render_errors() {
    let dir = Scratch::new("render-errors-help");
    let out = common::command(&dir).arg("--help").output().unwrap();
    let help = text(&out.stdout);
    assert!(help.contains("--render-errors <WHERE>"), "{help}");
    assert!(
        help.contains("[possible values: inline, stderr, both]"),
        "{help}"
    );
}

#[test]
fn a_mermaid_file_that_fails_tells_why_below_its_source_as_the_setting_says() {
    let dir = Scratch::new("mermaid-file-render-errors");
    let config = external(&dir, FAILS);
    let path = dir.join("flow.mmd");
    std::fs::write(&path, FLOWCHART_FILE).unwrap();
    // The setting as an option, as KIRI_RENDER_ERRORS or in the
    // configuration file, for the file and for standard input.
    for (args, variable, file, inline, stderr) in [
        (&[][..], None, "", true, true),
        (&["--render-errors", "both"][..], None, "", true, true),
        (&["--render-errors", "inline"][..], None, "", true, false),
        (&["--render-errors", "stderr"][..], None, "", false, true),
        (&[][..], Some("inline"), "", true, false),
        (
            &[][..],
            Some("stderr"),
            "render_errors = \"inline\"\n",
            false,
            true,
        ),
        (&[][..], None, "render_errors = \"inline\"\n", true, false),
        (&[][..], None, "render_errors = \"stderr\"\n", false, true),
    ] {
        common::write(
            &dir.join("config/kiri/config.toml"),
            &format!("{file}{config}"),
        );
        for piped in [false, true] {
            let case = format!("{args:?} {variable:?} {file:?} piped: {piped}");
            let mut command = common::command(&dir);
            command
                .current_dir(&*dir)
                .args(["--mode", "kitty", "--width", "120"])
                .args(args);
            if let Some(value) = variable {
                command.env("KIRI_RENDER_ERRORS", value);
            }
            let (out, name) = if piped {
                command.args(["--format", "mermaid", "-"]);
                let out = common::piped(command, FLOWCHART_FILE.as_bytes());
                (out, "<stdin>".to_owned())
            } else {
                (
                    command.arg(&path).output().unwrap(),
                    path.display().to_string(),
                )
            };
            // A failure whichever way it is told.
            assert_eq!(out.status.code(), Some(1), "{case}");
            let warning = format!("kiri: {name}: sh exited with status 1: Error: bad\n");
            let expected = if stderr { warning.as_str() } else { "" };
            assert_eq!(text(&out.stderr), expected, "{case}");
            let stdout = text(&out.stdout);
            assert!(sent(&stdout).is_empty(), "{case}: {stdout:?}");
            let shown = common::strip_own_sequences(&stdout);
            assert!(shown.contains("A --> B"), "{case}: {shown:?}");
            assert_eq!(shown.contains(INLINE), inline, "{case}: {shown:?}");
            // Below the source, not above it.
            if inline {
                assert!(shown.find("A --> B") < shown.find(INLINE), "{shown:?}");
            }
        }
    }
}
