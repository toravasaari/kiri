//! Tables: borders, column widths and stacking.

use kiri::render::{Inputs, TableRows};
use kiri::typeset::caps::Capabilities;
use kiri::typeset::color::Color;

use crate::common::{
    WIDTH, fixed_inputs, fixture, inputs, render, render_for, strip_own_sequences, visible,
};
use kiri::typeset::parse::csv::Separator;

#[test]
fn inline_code_kept_together_in_a_narrow_table_cell() {
    let source = "| A | B |\n|---|---|\n| see (`a b`). | x |\n";
    let together = kiri::typeset::theme::parse("inline_code_wrap = false\n").unwrap();
    // The first column is nine cells wide.
    let plain = Capabilities::plain(21);
    let out = render(source, &inputs(&plain, &together));
    assert!(out.contains("│ see       │"), "{out}");
    assert!(out.contains("│ (`a b`).  │"), "{out}");
    let out = render(source, &fixed_inputs(plain));
    assert!(out.contains("│ see (`a   │"), "{out}");
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
        render(&source, &fixed_inputs(Capabilities::plain(24))),
        "  > City: Helsinki\n  > Notes: Capital\n  > ──────────────────\n  \
         > City: Oulu\n  > Notes: North\n\n  \
         - City: Helsinki\n    Notes: Capital\n    ──────────────────\n    \
         City: Oulu\n    Notes: North\n"
    );
}

/// The CSV or TSV fixture `name` rendered for `caps`.
fn render_table_file(name: &str, separator: Separator, caps: Capabilities) -> String {
    let document = kiri::typeset::parse::csv::parse(&fixture(name), separator).unwrap();
    let mut out = Vec::new();
    kiri::render::render(&document, &fixed_inputs(caps), &mut out).unwrap();
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

#[test]
fn a_note_is_muted_text_below_the_table() {
    let mut document = kiri::typeset::parse::csv::parse("a,b\n1,2\n", Separator::Comma).unwrap();
    let note = "… 98\u{a0}000 more rows not shown";
    document
        .blocks
        .push(kiri::typeset::model::Block::Note(note.into()));
    let render = |caps: Capabilities| {
        let mut out = Vec::new();
        kiri::render::render(&document, &fixed_inputs(caps), &mut out).unwrap();
        String::from_utf8(out).unwrap()
    };
    let plain = render(Capabilities::plain(WIDTH));
    assert!(
        plain.ends_with("╰───┴───╯\n\n  … 98\u{a0}000 more rows not shown\n"),
        "{plain}"
    );
    let kitty = visible(&render(Capabilities::kitty(WIDTH)));
    assert!(
        kitty.contains("␛[0;2m… 98⍽000 more rows not shown"),
        "{kitty}"
    );
}

#[test]
fn kitty_tsv_file() {
    insta::assert_snapshot!(visible(&render_table_file(
        "table.tsv",
        Separator::Tab,
        Capabilities::kitty(WIDTH)
    )));
}

/// kitty with the terminal's colours known, as the probe finds them.
fn kitty_on(background: (u8, u8, u8), foreground: (u8, u8, u8)) -> Capabilities {
    let mut caps = Capabilities::kitty(WIDTH);
    caps.background = Some(Color::Rgb(background.0, background.1, background.2));
    caps.foreground = Some(Color::Rgb(foreground.0, foreground.1, foreground.2));
    caps
}

const ROWS: &str =
    "| City | Notes |\n|---|---|\n| Helsinki | Capital |\n| Oulu | North |\n| Turku | West |\n";

fn with_rows(caps: Capabilities, rows: TableRows) -> Inputs {
    Inputs {
        table_rows: rows,
        ..fixed_inputs(caps)
    }
}

#[test]
fn table_rows_are_striped_by_default_a_little_towards_the_text_colour() {
    // A dark page gets a lighter stripe, a light page a darker one.
    for (caps, stripe) in [
        (kitty_on((0, 0, 0), (200, 200, 200)), "48;2;12;12;12m"),
        (kitty_on((255, 255, 255), (31, 35, 40)), "48;2;242;242;242m"),
    ] {
        let out = render(ROWS, &fixed_inputs(caps));
        let lines: Vec<&str> = out.lines().collect();
        // The second body row, Oulu, is striped; the others are not, and
        // no rules go between them.
        assert!(lines[4].contains("Oulu"), "{out}");
        assert!(lines[4].contains(stripe), "{}", visible(lines[4]));
        for line in [lines[3], lines[5]] {
            assert!(!line.contains("48;2;"), "{}", visible(line));
        }
        assert_eq!(lines.len(), 7, "{out}");
    }
}

#[test]
fn zebra_draws_nothing_where_stripes_cannot_show() {
    let plain = "│ Helsinki │ Capital │\n  │ Oulu ";
    // Plain text, colours unknown, no colours, no 24-bit colour.
    let mut no_color = kitty_on((0, 0, 0), (200, 200, 200));
    no_color.color = false;
    let mut no_truecolor = kitty_on((0, 0, 0), (200, 200, 200));
    no_truecolor.truecolor = false;
    for caps in [
        Capabilities::plain(WIDTH),
        Capabilities::kitty(WIDTH),
        no_color,
        no_truecolor,
    ] {
        let out = render(ROWS, &fixed_inputs(caps));
        assert!(!out.contains("48;2;"), "{}", visible(&out));
        let out = strip_own_sequences(&out);
        assert!(out.contains(plain), "{out}");
    }
}

#[test]
fn table_rows_take_lines_or_nothing_as_asked() {
    let caps = kitty_on((0, 0, 0), (200, 200, 200));
    let out = render(ROWS, &with_rows(caps.clone(), TableRows::Lines));
    let out = strip_own_sequences(&out);
    assert!(out.contains("│ Helsinki │ Capital │\n  ├"), "{out}");
    let out = render(ROWS, &with_rows(caps, TableRows::Off));
    assert!(!out.contains("48;2;"), "{}", visible(&out));
    let out = strip_own_sequences(&out);
    assert!(out.contains("│ Helsinki │ Capital │\n  │ Oulu"), "{out}");
}

#[test]
fn inline_styles_keep_their_colours_on_a_striped_row() {
    let source = "| A | B |\n|---|---|\n| x | y |\n\
                  | `code` [link](https://example.com) <mark>marked</mark> | z |\n";
    let inputs = fixed_inputs(kitty_on((0, 0, 0), (200, 200, 200)));
    let striped = visible(&render(source, &inputs));
    let row = striped.lines().find(|line| line.contains("code")).unwrap();
    let alone = visible(&render(
        "`code` [link](https://example.com) <mark>marked</mark>\n",
        &inputs,
    ));
    // The style written just before `word`.
    let style = |text: &str, word: &str| {
        let at = text.find(&format!("m{word}")).unwrap();
        text[..at].rsplit('␛').next().unwrap().to_owned()
    };
    // Inline code and the mark keep their own colours.
    assert_eq!(style(row, "⍽code"), style(&alone, "⍽code"));
    assert_eq!(style(row, "marked"), style(&alone, "marked"));
    // The link keeps its colour, on the stripe.
    assert_eq!(
        style(row, "link"),
        format!("{};48;2;12;12;12", style(&alone, "link"))
    );
}

#[test]
fn every_underlined_cell_of_a_row_has_one_underline_colour() {
    use kiri::typeset::color::Underline;
    let source = "| A | B |\n|---|---|\n\
                  | **key** `code` [link](https://example.com) | y |\n| z | w |\n";
    let inputs = with_rows(Capabilities::kitty(WIDTH), TableRows::Underline);
    let lines = kiri::typeset::layout::layout(&kiri::typeset::parse::parse(source), &inputs);
    let row = lines
        .iter()
        .find(|line| line.spans.iter().any(|span| span.text.contains("link")))
        .unwrap();
    let colours: Vec<_> = (row.spans.iter())
        .filter(|span| span.style.underline != Underline::None)
        .map(|span| (span.text.clone(), span.style.underline_color))
        .collect();
    // The text, the padding, the border between the columns, the code and
    // the link: all underlined in the table border's colour.
    assert!(colours.len() >= 6, "{colours:?}");
    let border = inputs.theme.table_border;
    assert!(border.is_some());
    for (text, colour) in &colours {
        assert_eq!(*colour, border, "{text:?} in {colours:?}");
    }
}

#[test]
fn underline_draws_nothing_where_its_colour_cannot_show() {
    // Without underline styles there is no underline colour either, and
    // each cell's underline would take its own text colour.
    let caps = Capabilities::ansi(WIDTH);
    assert!(caps.color && !caps.styled_underlines);
    let out = render(ROWS, &with_rows(caps.clone(), TableRows::Underline));
    assert_eq!(out, render(ROWS, &with_rows(caps, TableRows::Off)));
}
