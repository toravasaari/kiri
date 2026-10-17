//! Tables: borders, column widths and stacking.

use kiri::term::Capabilities;
use kiri::theme::Theme;

mod common;

use common::{WIDTH, render, render_source, visible};

#[test]
fn inline_code_kept_together_in_a_narrow_table_cell() {
    let source = "| A | B |\n|---|---|\n| see (`a b`). | x |\n";
    let together = kiri::theme::parse("inline_code_wrap = false\n").unwrap();
    // The first column is nine cells wide.
    let plain = Capabilities::plain(21);
    let out = render_source(source, &plain, &together);
    assert!(out.contains("│ see       │"), "{out}");
    assert!(out.contains("│ (`a b`).  │"), "{out}");
    let out = render_source(source, &plain, &Theme::default());
    assert!(out.contains("│ see (`a   │"), "{out}");
}

#[test]
fn a_table_stacks_only_when_spaced_inline_code_cannot_wrap_in_it() {
    let source = "| Notes | Command |\n|---|---|\n\
                  | a fairly long note about the step | `git commit -m message` |\n";
    // By default the code wraps at its spaces like the text beside it.
    let out = render_source(source, &Capabilities::plain(28), &Theme::default());
    assert!(out.contains("│ a fairly  │ `git     │"), "{out}");
    assert!(out.contains("│ long note │ commit   │"), "{out}");
    // Kept together, the code stays whole while the notes keep room,
    let together = kiri::theme::parse("inline_code_wrap = false\n").unwrap();
    let out = render_source(source, &Capabilities::plain(50), &together);
    assert!(
        out.contains("│ a fairly long    │ `git commit -m message` │"),
        "{out}"
    );
    // then wraps at its spaces like by default before the table stacks,
    let narrow = Capabilities::plain(30);
    let out = render_source(source, &narrow, &together);
    assert!(out.contains("│ a fairly   │ `git      │"), "{out}");
    assert!(out.contains("│ long note  │ commit -m │"), "{out}");
    assert_eq!(out, render_source(source, &narrow, &Theme::default()));
    // and stacks only when not even its words fit.
    let out = render_source(source, &Capabilities::plain(24), &together);
    assert!(out.starts_with("  Notes: a fairly long\n"), "{out}");
}

#[test]
fn plain_tables() {
    insta::assert_snapshot!(render("tables.md", &Capabilities::plain(WIDTH)));
}

#[test]
fn kitty_tables() {
    insta::assert_snapshot!(visible(&render("tables.md", &Capabilities::kitty(WIDTH))));
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
        render_source(&source, &Capabilities::plain(24), &Theme::default()),
        "  > City: Helsinki\n  > Notes: Capital\n  > ──────────────────\n  \
         > City: Oulu\n  > Notes: North\n\n  \
         - City: Helsinki\n    Notes: Capital\n    ──────────────────\n    \
         City: Oulu\n    Notes: North\n"
    );
}
