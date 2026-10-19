//! The kiri binary end to end: CSV and TSV files shown as a table.

use std::path::Path;
use std::process::Output;

mod common;

use common::{Scratch, command, fixtures, kiri_on, kiri_piped, shown, text};

/// kiri on the file `name` in `dir` holding `content`, with `args`.
fn kiri_on_content(dir: &Path, name: &str, content: &[u8], args: &[&str]) -> Output {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    kiri_on(dir, &path, args)
}

const TABLE: &str = "  ╭──────┬──────╮\n  │ name │ size │\n  ├──────┼──────┤\n  \
                     │ *a*  │ b, c │\n  ╰──────┴──────╯\n";

#[test]
fn a_csv_file_is_a_table() {
    let dir = Scratch::new("csv-file");
    let out = kiri_on_content(
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
        let out = kiri_on_content(&dir, name, b"name\tsize\n*a*\tb, c\n", &["--mode", "plain"]);
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
    let out = kiri_on_content(
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
    let out = kiri_on_content(&dir, "t.csv", b"", &["--mode", "plain"]);
    assert_eq!(shown(&out), "");
}

#[test]
fn a_table_not_in_utf_8_is_an_error() {
    let dir = Scratch::new("csv-latin1");
    let out = kiri_on_content(&dir, "t.csv", b"name\nK\xf6ln\n", &["--mode", "plain"]);
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

#[test]
fn rows_past_the_cell_limit_are_counted_below_the_table() {
    let dir = Scratch::new("csv-limit");
    let source = "x\n".repeat(kiri::parse::csv::MAX_CELLS + 2);
    let out = kiri_on_content(&dir, "t.csv", source.as_bytes(), &["--mode", "plain"]);
    let shown = shown(&out);
    assert_eq!(
        shown.matches("│ x │").count(),
        kiri::parse::csv::MAX_CELLS,
        "{}",
        shown.len()
    );
    assert!(
        shown.ends_with("╰───╯\n\n  … 2 more rows not shown\n"),
        "{:?}",
        &shown[shown.len() - 80..]
    );
}

#[test]
fn a_header_past_the_cell_limit_is_an_error() {
    let dir = Scratch::new("csv-too-wide");
    let header = vec!["x"; kiri::parse::csv::MAX_CELLS + 1].join(",");
    let out = kiri_on_content(&dir, "t.csv", header.as_bytes(), &["--mode", "plain"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!(
            "kiri: {}: too wide to show: 100 001 columns, and kiri shows at most 100 000 cells\n",
            dir.join("t.csv").display()
        )
    );
    assert_eq!(text(&out.stdout), "");
}
