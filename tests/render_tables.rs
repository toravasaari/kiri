//! Tables: borders, column widths and stacking.

use kiri::render::Inputs;
use kiri::term::Capabilities;

mod common;

use common::{WIDTH, fixture, inputs, render, render_for, visible};
use kiri::parse::csv::Separator;

#[test]
fn inline_code_kept_together_in_a_narrow_table_cell() {
    let source = "| A | B |\n|---|---|\n| see (`a b`). | x |\n";
    let together = kiri::theme::parse("inline_code_wrap = false\n").unwrap();
    // The first column is nine cells wide.
    let plain = Capabilities::plain(21);
    let out = render(source, &inputs(&plain, &together));
    assert!(out.contains("│ see       │"), "{out}");
    assert!(out.contains("│ (`a b`).  │"), "{out}");
    let out = render(source, &Inputs::new(plain));
    assert!(out.contains("│ see (`a   │"), "{out}");
}

#[test]
fn a_table_stacks_only_when_spaced_inline_code_cannot_wrap_in_it() {
    let source = "| Notes | Command |\n|---|---|\n\
                  | a fairly long note about the step | `git commit -m message` |\n";
    // By default the code wraps at its spaces like the text beside it.
    let out = render(source, &Inputs::new(Capabilities::plain(28)));
    assert!(out.contains("│ a fairly │ `git      │"), "{out}");
    assert!(out.contains("│ long     │ commit -m │"), "{out}");
    // Kept together, the code stays whole while the notes keep room,
    let together = kiri::theme::parse("inline_code_wrap = false\n").unwrap();
    let out = render(source, &inputs(&Capabilities::plain(50), &together));
    assert!(
        out.contains("│ a fairly long    │ `git commit -m message` │"),
        "{out}"
    );
    // then wraps at its spaces like by default before the table stacks,
    let narrow = Capabilities::plain(30);
    let out = render(source, &inputs(&narrow, &together));
    assert!(out.contains("│ a fairly  │ `git       │"), "{out}");
    assert!(out.contains("│ long note │ commit -m  │"), "{out}");
    assert_eq!(out, render(source, &Inputs::new(narrow)));
    // and stacks only when not even its words fit.
    let out = render(source, &inputs(&Capabilities::plain(24), &together));
    assert!(out.starts_with("  Notes: a fairly long\n"), "{out}");
}

#[test]
fn plain_tables() {
    insta::assert_snapshot!(render_for("tables.md", Capabilities::plain(WIDTH)));
}

#[test]
fn kitty_tables() {
    insta::assert_snapshot!(visible(&render_for(
        "tables.md",
        Capabilities::kitty(WIDTH)
    )));
}

#[test]
fn stacked_tables_nest_in_quotes_and_list_items() {
    let table = "| City | Notes |\n|---|---|\n| Helsinki | Capital |\n| Oulu | North |\n";
    let quoted: String = table.lines().map(|line| format!("> {line}\n")).collect();
    let listed = table
        .replacen("| City", "- | City", 1)
        .replace("\n|", "\n  |");
    let source = format!("{quoted}\n{listed}");
    // Eighteen cells inside the quote and the item: too few for the
    // borders, enough for a header and a value on one line.
    assert_eq!(
        render(&source, &Inputs::new(Capabilities::plain(24))),
        "  > City: Helsinki\n  > Notes: Capital\n  > ──────────────────\n  \
         > City: Oulu\n  > Notes: North\n\n  \
         - City: Helsinki\n    Notes: Capital\n    ──────────────────\n    \
         City: Oulu\n    Notes: North\n"
    );
}

/// The CSV or TSV fixture `name` rendered for `caps`.
fn render_table_file(name: &str, separator: Separator, caps: Capabilities) -> String {
    let document = kiri::parse::csv::parse(&fixture(name), separator);
    let mut out = Vec::new();
    kiri::render::render(&document, &Inputs::new(caps), &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn csv_file() {
    insta::assert_snapshot!(render_table_file(
        "table.csv",
        Separator::Comma,
        Capabilities::plain(64)
    ));
}

#[test]
fn tsv_file() {
    insta::assert_snapshot!(render_table_file(
        "table.tsv",
        Separator::Tab,
        Capabilities::plain(64)
    ));
}

#[test]
fn kitty_csv_file() {
    insta::assert_snapshot!(visible(&render_table_file(
        "table.csv",
        Separator::Comma,
        Capabilities::kitty(WIDTH)
    )));
}
