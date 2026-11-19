//! Tests of a layout that grows with its document: a whole file shown as
//! text, or the table of a CSV, that only grew at its end is laid out on
//! from the last stretch it had, and comes to what a layout of the grown
//! document made afresh is.

use super::{code_block, inputs, rust, texts};
use crate::render::Inputs;
use crate::typeset::driver::{self, Content, stretches_done};
use crate::typeset::laid::Laid;
use crate::typeset::model::Document;
use crate::typeset::parse::csv::{self, Separator};
use crate::typeset::stretch::Tables;

/// `count` lines of a log, from line `from` on.
fn log(from: usize, count: usize) -> String {
    (from..from + count)
        .map(|n| {
            format!(
                "2026-03-01T10:00:{:02}Z INFO  [http.server] request {n} took {} ms\n",
                n % 60,
                n % 977
            )
        })
        .collect()
}

/// The inputs of the pager's cell layout, with what its long tables were
/// measured to ask for, `tables`.
fn pager(tables: Tables) -> Inputs {
    Inputs {
        tables: Some(tables),
        ..inputs()
    }
}

/// Checks that `grown`, a layout grown into `document`, is what a layout of
/// it made afresh with `inputs` is: as many lines, as tall, the same block
/// on every line and the same texts where both have lines, the same answers
/// of the map, and, laid out and coloured where a reader comes, the lines
/// of the whole layout.
fn assert_fresh(grown: &mut Laid, document: &Document, inputs: &Inputs, what: &str) {
    let content = Content::Document(document);
    let fresh = driver::laid_measured(content, 0, inputs);
    let lines = fresh.len();
    assert!(grown.is_whole(), "{what}");
    assert_eq!(grown.len(), lines, "{what}");
    assert_eq!(
        grown.height(&inputs.caps),
        fresh.height(&inputs.caps),
        "{what}"
    );
    assert_eq!(grown.blocks(), fresh.blocks(), "{what}");
    for (at, (line, other)) in grown.lines().iter().zip(fresh.lines()).enumerate() {
        assert_eq!(line.block, other.block, "{what}: the block of line {at}");
        if line.measured.is_none() && other.measured.is_none() {
            assert_eq!(
                texts(std::slice::from_ref(line)),
                texts(std::slice::from_ref(other)),
                "{what}: {at}"
            );
        }
    }
    // The whole layout, which every laid line of the grown one is held
    // against. A stretch that is only measured answers for the smallest and
    // the greatest place of its lines, so the grown layout's map is held
    // against the whole one where its line is laid, and against the fresh
    // one only where both are laid.
    let all = driver::laid(content, inputs);
    assert_eq!(all.len(), lines, "{what}");
    let laid = |laid: &Laid, line: usize| laid.lines()[line].measured.is_none();
    for line in (0..lines).step_by(37).chain([lines - 1]) {
        let start = all
            .span_of(line..(line + 5).min(lines))
            .map(|range| range.start);
        if laid(grown, line) {
            assert_eq!(grown.first_on(line), all.first_on(line), "{what}: {line}");
            if let Some(start) = &start {
                assert_eq!(grown.line_of(start), all.line_of(start), "{what}: {line}");
            }
        }
        if laid(grown, line) && laid(&fresh, line) {
            assert_eq!(grown.first_on(line), fresh.first_on(line), "{what}: {line}");
            if let Some(start) = &start {
                assert_eq!(grown.line_of(start), fresh.line_of(start), "{what}: {line}");
            }
        }
    }
    assert_eq!(grown.span_of(0..lines), fresh.span_of(0..lines), "{what}");
    // Read through, laid out and coloured as the reader comes, it is the
    // whole layout; a long one at a few places and its end.
    let tops: Vec<usize> = match lines > 20_000 {
        true => vec![0, lines / 3, lines - 900, lines - 300, lines - 40],
        false => (0..lines).step_by(211).collect(),
    };
    for top in tops {
        let near = top..(top + 40).min(lines);
        grown.show(content, inputs, near.clone());
        worked_with(grown, content, inputs, near.clone());
        assert_eq!(
            grown.lines()[near.clone()],
            all.lines()[near.clone()],
            "{what}: {near:?}"
        );
        for line in near {
            assert_eq!(grown.hit(line, 3), all.hit(line, 3), "{what}: {line}");
            assert_eq!(grown.first_on(line), all.first_on(line), "{what}: {line}");
        }
    }
}

/// [`worked`] with `inputs`.
fn worked_with(part: &mut Laid, content: Content, inputs: &Inputs, near: std::ops::Range<usize>) {
    let mut pieces = 0;
    while part.pending(near.clone()) {
        part.work(content, inputs, near.clone());
        pieces += 1;
        assert!(pieces < 10_000, "the work of a layout ends");
    }
}

#[test]
fn a_line_appended_to_a_log_of_100_000_lines_lays_out_its_last_stretch_alone() {
    let inputs = inputs();
    let before = log(0, 100_000);
    // Without colours, which the eager layout it is held against would take
    // minutes for in a test build: the tests of code below have them.
    let old = code_block(None, &before, true);
    let mut laid = driver::laid_measured(Content::Document(&old), 0, &inputs);
    laid.map();
    let whole = format!("{before}{}", log(100_000, 1));
    let new = code_block(None, &whole, true);
    let done = stretches_done();
    assert!(laid.grow(Content::Document(&new), &inputs));
    // The last stretch had 100 000 % 128 = 32 lines; with the new one it is
    // still the last, laid out again.
    assert_eq!(stretches_done() - done, 1, "stretches laid out or measured");
    let fresh = stretches_done();
    driver::laid_measured(Content::Document(&new), 0, &inputs);
    assert_eq!(stretches_done() - fresh, 782, "a fresh layout");
    assert_fresh(&mut laid, &new, &inputs, "log");
}

#[test]
fn ten_thousand_lines_appended_lay_out_the_stretches_from_the_old_last_on() {
    let inputs = inputs();
    let before = log(0, 100_000);
    let old = code_block(None, &before, true);
    let mut laid = driver::laid_measured(Content::Document(&old), 0, &inputs);
    let new = code_block(None, &format!("{before}{}", log(100_000, 10_000)), true);
    let done = stretches_done();
    assert!(laid.grow(Content::Document(&new), &inputs));
    // From line 99 968: 10 032 lines, in 79 stretches.
    assert_eq!(stretches_done() - done, 79);
    assert_fresh(&mut laid, &new, &inputs, "log");
}

/// Grows a layout of the first of `steps`, cut from `whole` at each, into
/// each of the others in turn, and holds it against a fresh one after each.
fn grown_by_steps(
    whole: &str,
    steps: &[usize],
    document: impl Fn(&str) -> Document,
    inputs: &Inputs,
    what: &str,
) {
    let first = document(&whole[..steps[0]]);
    let mut laid = driver::laid_measured(Content::Document(&first), 0, inputs);
    for (step, &cut) in steps[1..].iter().enumerate() {
        // The map, made before the growth or not, goes on with the layout.
        if step % 2 == 0 {
            laid.map();
        }
        let new = document(&whole[..cut]);
        assert!(
            laid.grow(Content::Document(&new), inputs),
            "{what} at {cut}"
        );
        assert_fresh(&mut laid, &new, inputs, &format!("{what}, grown to {cut}"));
    }
}

/// The byte where line `line` of `text` starts.
fn line_start(text: &str, line: usize) -> usize {
    text.split_inclusive('\n').take(line).map(str::len).sum()
}

#[test]
fn a_file_of_code_grown_inside_a_comment_and_a_string_is_laid_out_as_afresh() {
    driver::set_colour_budget(Some(std::time::Duration::MAX));
    let mut whole = rust(1800);
    // A string of several lines, cut inside below.
    let string = line_start(&whole, 1300);
    whole.insert_str(string, "let s = \"a string\nthat goes on\nover lines\";\n");
    let inputs = inputs();
    // Inside the comment of lines 1022 to 1025, in the middle of a line,
    // inside the string, and at the end.
    let steps = [
        line_start(&whole, 900),
        line_start(&whole, 1023),
        line_start(&whole, 1024) + 5,
        string + 15,
        whole.len(),
    ];
    for (what, language) in [("rust", Some("rust")), ("text", None)] {
        grown_by_steps(
            &whole,
            &steps,
            |code| code_block(language, code, true),
            &inputs,
            what,
        );
    }
}

/// A CSV of `rows` rows under a header, a quoted field with a line break in
/// every seventh, and one wider than any before from row `wide` on.
fn csv_rows(rows: usize, wide: usize) -> String {
    let mut source = "id,name,note\n".to_owned();
    for n in 1..=rows {
        let row = match n {
            _ if n >= wide => format!(
                "{n},{},\"wider than any before it\"\n",
                "a long name ".repeat(4)
            ),
            _ if n % 7 == 0 => format!("{n},name {n},\"two\nlines\"\n"),
            _ => format!("{n},name {n},note {n}\n"),
        };
        source.push_str(&row);
    }
    source
}

#[test]
fn a_csv_grown_across_a_quote_and_by_wider_rows_is_laid_out_as_afresh() {
    let whole = csv_rows(1500, 1400);
    let parse = |source: &str| csv::parse(source, Separator::Comma).unwrap();
    // The widths a pager opens the CSV with, kept through the growth, as an
    // automatic reload keeps them.
    let mut tables = Tables::default();
    let first = line_start(&whole, 600);
    driver::scanned(
        Content::Document(&parse(&whole[..first])),
        &inputs(),
        &mut tables,
    );
    let inputs = pager(tables);
    // Inside the quoted field of row 700, between its two lines, after
    // row 1000, inside the wider rows, and the whole.
    let quoted = whole.find("700,name 700,\"two\n").unwrap();
    let steps = [
        first,
        quoted + 16,
        quoted + 18,
        line_start(&whole, 1001),
        line_start(&whole, 1450) + 3,
        whole.len(),
    ];
    grown_by_steps(&whole, &steps, parse, &inputs, "csv");
}

#[test]
fn rows_appended_to_a_csv_make_cells_of_the_rows_near_the_view_only() {
    use crate::typeset::source_rows::cells_made;
    let before = csv_rows(100_000, usize::MAX);
    let old = csv::parse(&before, Separator::Comma).unwrap();
    let mut tables = Tables::default();
    driver::scanned(Content::Document(&old), &inputs(), &mut tables);
    let inputs = pager(tables);
    let mut laid = driver::laid_measured(Content::Document(&old), 0, &inputs);
    let whole = format!("{before}100001,name,note\n100002,name,note\n");
    let new = csv::parse_grown(&old, &whole, Separator::Comma)
        .unwrap()
        .unwrap();
    let (made, done) = (cells_made(), stretches_done());
    assert!(laid.grow(Content::Document(&new), &inputs));
    // The last stretch of 128 rows, laid out again: 100 000 % 128 = 32
    // rows and the two new ones, under the header its grid carries.
    assert_eq!(stretches_done() - done, 1);
    assert_eq!(cells_made() - made, (1 + 34) * 3);
}

#[test]
fn only_a_file_or_a_csv_that_grew_at_its_end_is_taken_for_grown() {
    use crate::typeset::compare::grown;
    use crate::typeset::parse::{file, parse};
    let before = log(0, 300);
    let text = |source: &str| file(source, None);
    let longer = format!("{before}{}", log(300, 2));
    assert!(grown(&text(&before), &text(&longer)));
    assert!(grown(&text(&before), &text(&before)), "as it was");
    let not = [
        (
            "a byte in the middle",
            before.replacen("request 150", "request 15o", 1),
        ),
        (
            "a line removed at the top",
            before.split_once('\n').unwrap().1.to_owned(),
        ),
        ("rotated", log(0, 2)),
        ("shorter", before[..before.len() - 1].to_owned()),
    ];
    for (what, after) in &not {
        assert!(!grown(&text(&before), &text(after)), "{what}");
    }
    assert!(
        !grown(&text(&before), &file(&longer, Some("log"))),
        "another language"
    );
    assert!(!grown(&parse(&before), &parse(&longer)), "Markdown");
    let csv = |source: &str, separator| csv::parse(source, separator).unwrap();
    let rows = csv_rows(300, usize::MAX);
    let more = format!("{rows}301,name,note\n");
    assert!(grown(
        &csv(&rows, Separator::Comma),
        &csv(&more, Separator::Comma)
    ));
    let wider = format!("{rows}301,name,note,a fourth\n");
    assert!(
        !grown(
            &csv(&rows, Separator::Comma),
            &csv(&wider, Separator::Comma)
        ),
        "a column more"
    );
    assert!(
        !grown(&csv(&rows, Separator::Comma), &csv(&more, Separator::Tab)),
        "TSV"
    );
    let changed = rows.replacen("name 150", "name 15o", 1);
    assert!(!grown(
        &csv(&rows, Separator::Comma),
        &csv(&format!("{changed}301,a,b\n"), Separator::Comma)
    ));
}

/// A source of numbers that the same seed repeats.
struct Dice(u64);

impl Dice {
    /// A number below `below`.
    fn below(&mut self, below: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % below.max(1)
    }
}

/// `count` lines of Rust or text from `dice`, ended by `end`: block
/// comments and strings over several lines, lines that wrap, tabs, wide
/// letters and empty lines.
fn random_lines(dice: &mut Dice, count: usize, end: &str) -> String {
    const LINES: [&str; 10] = [
        "fn f() { let x = 1; }",
        "/* a comment that goes on",
        "   and ends */ let y = 2;",
        "let s = \"a string",
        "that ends\";",
        "",
        "\tlet tabbed = \"a\\tb\"; // a comment long enough to wrap at sixty cells or so",
        "let wide = \"日本語の文字は幅が二つ\";",
        "// one line",
        "struct S { a: u8, b: u16 }",
    ];
    (0..count)
        .map(|_| format!("{}{end}", LINES[dice.below(LINES.len())]))
        .collect()
}

#[test]
fn a_file_grown_at_random_places_is_laid_out_as_afresh() {
    driver::set_colour_budget(Some(std::time::Duration::MAX));
    let inputs = inputs();
    let mut dice = Dice(1234);
    let (mut cases, mut grown) = (0, 0);
    while cases < 40 {
        cases += 1;
        let language = [Some("rust"), None][dice.below(2)];
        let end = ["\n", "\r\n"][dice.below(2)];
        let count = 200 + dice.below(500);
        let whole = random_lines(&mut dice, count, end);
        // Three growths at random places, a line cut in two among them.
        let mut cuts: Vec<usize> = (0..3).map(|_| dice.below(whole.len())).collect();
        cuts.push(whole.len());
        cuts.sort_unstable();
        cuts.retain(|&cut| whole.is_char_boundary(cut));
        let first = code_block(language, &whole[..cuts[0]], true);
        let mut laid = driver::laid_measured(Content::Document(&first), 0, &inputs);
        for (step, &cut) in cuts[1..].iter().enumerate() {
            if step % 2 == 0 {
                laid.map();
            }
            let new = code_block(language, &whole[..cut], true);
            let what = format!("case {cases}, {language:?}, {end:?}, grown to {cut}");
            // A block of no more than a stretch has nothing to go on from,
            // and is laid out afresh.
            if !laid.grow(Content::Document(&new), &inputs) {
                laid = driver::laid_measured(Content::Document(&new), 0, &inputs);
                continue;
            }
            grown += 1;
            assert_fresh(&mut laid, &new, &inputs, &what);
        }
    }
    assert!(grown >= 60, "{grown} growths went on");
}
