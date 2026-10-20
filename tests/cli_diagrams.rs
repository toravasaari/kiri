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
