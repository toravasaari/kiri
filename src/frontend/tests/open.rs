//! What a file is shown as, in the simulated frontend: Markdown by its
//! extension, any other text as it is, and standard input by its content.
//! The `*b*` in the samples is how the tests tell: Markdown shows it
//! emphasised, without the stars.

use crate::frontend::sim::{Sim, plain};
use crate::typeset::input::Input;

const STARRED: &str = "{\"a\": \"*b*\"}\n";

/// What kiri prints for `files` on a window of 40 columns, as text.
fn printed(sim: Sim, files: &[&str]) -> String {
    let ran = sim.kiri(files).quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    plain(&ran.out)
}

#[test]
fn standard_input_that_is_json_is_shown_as_text() {
    let shown = printed(Sim::new(40, 20).stdin(STARRED), &["-"]);
    assert!(shown.contains("{\"a\": \"*b*\"}"), "{shown:?}");
}

#[test]
fn standard_input_that_is_a_diff_is_shown_as_text() {
    let diff = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-*old*\n+*new*\n";
    let shown = printed(Sim::new(40, 20).stdin(diff), &["-"]);
    assert!(
        shown.contains("-*old*") && shown.contains("+*new*"),
        "{shown:?}"
    );
}

#[test]
fn standard_input_that_is_markdown_stays_markdown() {
    for source in [
        "# Title\n\n*b*\n",
        "---\ntitle: Notes\n---\n\n*b*\n",
        "<div align=\"center\">\n\n*b*\n\n</div>\n",
        "[x] done *b*\n",
    ] {
        let shown = printed(Sim::new(40, 20).stdin(source), &["-"]);
        assert!(
            shown.contains('b') && !shown.contains("*b*"),
            "{source:?}: {shown:?}"
        );
    }
}

#[test]
fn format_decides_for_standard_input_over_its_content() {
    let sim = Sim::new(40, 20).stdin(STARRED).format(Input::Markdown);
    let shown = printed(sim, &["-"]);
    assert!(!shown.contains("*b*"), "{shown:?}");
    let sim = Sim::new(40, 20).stdin("# Title\n*b*\n").format(Input::Text);
    let shown = printed(sim, &["-"]);
    assert!(
        shown.contains("# Title") && shown.contains("*b*"),
        "{shown:?}"
    );
}

#[test]
fn only_a_markdown_extension_makes_a_file_markdown() {
    let sim = || {
        let source = "# Title\n*b*\n";
        Sim::new(40, 20)
            .file("notes.md", source)
            .file("notes.yaml", source)
            .file("notes.txt", source)
            .file("LICENSE", source)
    };
    let shown = printed(sim(), &["notes.md"]);
    assert!(
        shown.contains("Title") && !shown.contains('#') && !shown.contains("*b*"),
        "{shown:?}"
    );
    for name in ["notes.yaml", "notes.txt", "LICENSE"] {
        let shown = printed(sim(), &[name]);
        assert!(
            shown.contains("# Title") && shown.contains("*b*"),
            "{name}: {shown:?}"
        );
    }
}

#[test]
fn a_file_whose_name_tells_nothing_is_told_by_its_content() {
    // Shown as text either way, so the stars stay.
    let shown = printed(Sim::new(40, 20).file("deploy", STARRED), &["deploy"]);
    assert!(shown.contains("*b*"), "{shown:?}");
}

#[test]
fn a_directory_shows_its_readme_without_an_extension_as_markdown() {
    let sim = Sim::new(40, 20).file("docs/README", "# Title\n*b*\n");
    let shown = printed(sim, &["docs"]);
    assert!(
        shown.contains("Title") && !shown.contains("*b*"),
        "{shown:?}"
    );
    // The same file given by its name is text.
    let sim = Sim::new(40, 20).file("docs/README", "# Title\n*b*\n");
    let shown = printed(sim, &["docs/README"]);
    assert!(
        shown.contains("# Title") && shown.contains("*b*"),
        "{shown:?}"
    );
}
