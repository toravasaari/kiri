//! No committed file names a plan (AGENTS.md, "Source of truth"), as a test.
//!
//! Plans, specs and other working notes live in `.local/`, which is not in
//! git. The guard reads every text file that git has or would add (tracked,
//! and new ones that `.gitignore` does not leave out) and finds paths into
//! `.local/`. Two of its directories are named on purpose and pass: the debt
//! ledger, `.local/ledger/`, which AGENTS.md describes, and the screenshots
//! of `.local/render-check/`. `.gitignore`, which leaves `.local/` out, and
//! this file, which names the pattern, are not read.
//!
//! A part of the roadmap, such as `G2.6.4`, is no plan and may be named.
//! What it does not see: a plan's file name without its path.
//!
//! Without git's metadata (a source archive, a copy without `.git`), there
//! is nothing to tell committed files by: the test says so and passes.

use std::path::Path;
use std::process::Command;

/// The directories of `.local/` that committed files may name.
const NAMED: &[&str] = &["ledger", "render-check"];

/// The files that are not read, from the repository's root.
const NOT_READ: &[&str] = &[".gitignore", "tests/it/guard_plans.rs"];

#[test]
fn no_committed_file_names_a_plan() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // `.git` is a directory in a checkout and a file in a worktree.
    if !root.join(".git").exists() {
        eprintln!("no .git in {}: no committed files to read", root.display());
        return;
    }
    let listed = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(root)
        .output()
        .expect("git runs");
    assert!(
        listed.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let mut found = Vec::new();
    for path in String::from_utf8(listed.stdout).unwrap().split('\0') {
        if path.is_empty() || NOT_READ.contains(&path) {
            continue;
        }
        // A file deleted but not yet staged is still listed; a binary file
        // is no text.
        let Ok(text) = std::fs::read_to_string(root.join(path)) else {
            continue;
        };
        for (line, named) in references(&text) {
            found.push(format!("  {path}:{line}: {named}"));
        }
    }
    assert!(
        found.is_empty(),
        "committed files name working notes in .local/ (AGENTS.md, \"Source of truth\"):\n{}\n\
         Say what the note says in the file itself, or leave it out: plans stay out of \
         the committed tree.\n",
        found.join("\n")
    );
}

/// The paths into `.local/` in `text` that name something other than the
/// directories in `NAMED`, as (line number, the path's start).
fn references(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        for (at, _) in line.match_indices(".local/") {
            let before = line[..at].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                continue; // A host name: `printer.local/`.
            }
            let rest = &line[at + ".local/".len()..];
            let directory: String = rest
                .chars()
                .take_while(|&c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
                .collect();
            if !NAMED.contains(&directory.as_str()) {
                found.push((number + 1, format!(".local/{directory}")));
            }
        }
    }
    found
}

#[test]
fn the_ledger_and_render_checks_may_be_named_and_nothing_else() {
    let named = |text: &str| -> Vec<String> {
        references(text).into_iter().map(|(_, path)| path).collect()
    };
    assert_eq!(named("see .local/plans/G1.md"), [".local/plans"]);
    assert_eq!(named("(../.local/prompts/a.md)"), [".local/prompts"]);
    assert_eq!(named("in `.local/ledger-notes/`"), [".local/ledger-notes"]);
    assert_eq!(named("all of `.local/`"), [".local/"]);
    assert!(named("an entry in `.local/ledger/`").is_empty());
    assert!(named("out=.local/render-check/$branch").is_empty());
    assert!(named("http://printer.local/queue and a.local/b").is_empty());
    assert!(named("G2.6.4 does it").is_empty());
    assert_eq!(
        references("a\nb .local/reports/x\n"),
        [(2, ".local/reports".to_owned())]
    );
}
