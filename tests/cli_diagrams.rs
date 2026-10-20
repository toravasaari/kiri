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
