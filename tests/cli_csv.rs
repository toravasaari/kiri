//! The kiri binary end to end: CSV and TSV files shown as a table.

use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};

mod common;

use common::{Scratch, command, fixtures, text};

/// kiri on the file `name` in `dir` holding `content`, with `args`.
fn kiri_on(dir: &Path, name: &str, content: &[u8], args: &[&str]) -> Output {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    command(dir).args(args).arg(&path).output().unwrap()
}

/// kiri with `args`, reading `stdin` from a pipe.
fn kiri_piped(dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = command(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

/// The standard output of `out`, which must have succeeded quietly.
fn shown(out: &Output) -> String {
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    text(&out.stdout)
}

const TABLE: &str = "  ╭──────┬──────╮\n  │ name │ size │\n  ├──────┼──────┤\n  \
                     │ *a*  │ b, c │\n  ╰──────┴──────╯\n";

#[test]
fn a_csv_file_is_a_table() {
    let dir = Scratch::new("csv-file");
    let out = kiri_on(
        &dir,
        "t.csv",
        b"name,size\n*a*,\"b, c\"\n",
        &["--mode", "plain"],
    );
    assert_eq!(shown(&out), TABLE);
}

#[test]
fn a_tsv_file_is_a_table() {
    let dir = Scratch::new("tsv-file");
    for name in ["t.tsv", "t.TAB"] {
        let out = kiri_on(&dir, name, b"name\tsize\n*a*\tb, c\n", &["--mode", "plain"]);
        assert_eq!(shown(&out), TABLE, "{name}");
    }
}

#[test]
fn the_fixtures_are_tables() {
    let dir = Scratch::new("csv-fixtures");
    for name in ["table.csv", "table.tsv"] {
        let out = command(&dir)
            .args(["--mode", "plain", "--width", "64"])
            .arg(fixtures().join(name))
            .output()
            .unwrap();
        assert!(shown(&out).starts_with("  ╭"), "{name}");
    }
}

#[test]
fn format_tells_a_table_on_standard_input() {
    let dir = Scratch::new("csv-stdin");
    let out = kiri_piped(
        &dir,
        &["--mode", "plain", "--format", "csv"],
        b"name,size\n*a*,\"b, c\"\n",
    );
    assert_eq!(shown(&out), TABLE);
    let out = kiri_piped(
        &dir,
        &["--mode", "plain", "-f", "tsv", "-"],
        b"name\tsize\n*a*\tb, c\n",
    );
    assert_eq!(shown(&out), TABLE);
}

#[test]
fn format_tells_a_table_under_another_name() {
    let dir = Scratch::new("csv-format");
    let out = kiri_on(
        &dir,
        "t.txt",
        b"name,size\n*a*,\"b, c\"\n",
        &["--mode", "plain", "-f", "csv"],
    );
    assert_eq!(shown(&out), TABLE);
}

#[test]
fn an_empty_file_shows_nothing() {
    let dir = Scratch::new("csv-empty");
    let out = kiri_on(&dir, "t.csv", b"", &["--mode", "plain"]);
    assert_eq!(shown(&out), "");
}

#[test]
fn a_table_not_in_utf_8_is_an_error() {
    let dir = Scratch::new("csv-latin1");
    let out = kiri_on(&dir, "t.csv", b"name\nK\xf6ln\n", &["--mode", "plain"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!("kiri: {}: not UTF-8 text\n", dir.join("t.csv").display())
    );
    assert_eq!(text(&out.stdout), "");
    let out = kiri_piped(&dir, &["--mode", "plain", "-f", "csv"], b"K\xf6ln\n");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stderr), "kiri: <stdin>: not UTF-8 text\n");
}
