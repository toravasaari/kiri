//! `kiri --completions SHELL` end to end.

use std::process::Output;

use crate::common::{Scratch, command, text};

/// The shells the flag takes.
const SHELLS: [&str; 6] = ["bash", "zsh", "fish", "elvish", "powershell", "nushell"];

/// `kiri` with `args`, its output in a pipe.
fn run(args: &[&str]) -> Output {
    let dir = Scratch::new("completions");
    command(&dir).args(args).output().unwrap()
}

/// Each shell gets a script on standard output, and nothing else.
#[test]
fn each_shell_gets_a_script() {
    for shell in SHELLS {
        let out = run(&["--completions", shell]);
        assert!(out.status.success(), "{shell}: {}", text(&out.stderr));
        assert!(!out.stdout.is_empty(), "{shell}: no script");
        assert!(
            text(&out.stderr).is_empty(),
            "{shell}: {}",
            text(&out.stderr)
        );
    }
}

/// The script knows the flags, and the values of a flag that lists them.
#[test]
fn the_bash_script_names_flags_and_values() {
    let out = run(&["--completions", "bash"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let script = text(&out.stdout);
    assert!(script.contains("--paging"), "{script}");
    assert!(script.contains("--completions"), "{script}");
    // The word list of the --paging arm, not any other `auto` in the script.
    assert!(script.contains("\"always auto never\""), "{script}");
}

/// An unknown shell is a usage error that lists the six names.
#[test]
fn an_unknown_shell_lists_the_shells() {
    let out = run(&["--completions", "tcsh"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = text(&out.stderr);
    for shell in SHELLS {
        assert!(stderr.contains(shell), "{shell}: {stderr}");
    }
}

/// The flag shows no file and does no other report, so those are usage
/// errors.
#[test]
fn it_conflicts_with_files_and_reports() {
    for other in [
        &["README.md"][..],
        &["--format", "csv"],
        &["--doctor"],
        &["--list-emojis"],
        &["--raw"],
    ] {
        let mut args = vec!["--completions", "bash"];
        args.extend(other);
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2), "{other:?}");
        let stderr = text(&out.stderr);
        assert!(
            stderr.contains("cannot be used with"),
            "{other:?}: {stderr}"
        );
    }
}

/// A closed pipe takes no more, which is no failure, as with a document.
/// The reader is dropped before kiri starts, so there is no race with the
/// script being smaller than a pipe's buffer.
#[test]
fn a_closed_pipe_is_not_an_error() {
    let dir = Scratch::new("completions-closed");
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let out = command(&dir)
        .args(["--completions", "bash"])
        .stdout(writer)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stderr).is_empty(), "{}", text(&out.stderr));
}

/// The scripts are for the program `kiri`, not for the package's name.
#[test]
fn the_script_registers_kiri() {
    let out = run(&["--completions", "bash"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let script = text(&out.stdout);
    assert!(script.contains("complete -F _kiri"), "{script}");
    assert!(!script.contains("kiri-the-reader"), "{script}");
    assert!(!script.contains("kiri__the__reader"), "{script}");
}

/// `kiri --version` tells the program's name and its version.
#[test]
fn the_version_is_kiri_and_its_number() {
    let out = run(&["--version"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let expected = format!("kiri {}\n", env!("CARGO_PKG_VERSION"));
    assert_eq!(text(&out.stdout), expected);
}
