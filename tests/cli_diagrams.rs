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
fn diagrams_are_code_in_a_build_without_mermaid() {
    let dir = Scratch::new("mermaid-not-built");
    let out = kiri(&dir, BROKEN, &["--mode", "kitty"]);
    assert_eq!(text(&out.stderr), "");
    let out = kiri(&dir, FLOWCHART, &["--mode", "kitty"]);
    let stdout = text(&out.stdout);
    assert!(sent(&stdout).is_empty(), "{stdout:?}");
    assert!(stdout.contains("flowchart TD"), "{stdout:?}");
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

/// A stand-in for mmdc: an SVG as high as the source has lines.
const DRAWS: &str = "lines=$(wc -l < \"$1\")\n\
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
