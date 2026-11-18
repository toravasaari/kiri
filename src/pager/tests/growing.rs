//! A followed file that only grows at its end: its layout goes on from the
//! last stretch it had, and the pager shows what it shows of the grown file
//! opened afresh. A file that changed otherwise is laid out afresh.

use crate::frontend::sim::{Ran, Sim};
use crate::present::seen::Row;
use crate::session::settings::{Follow, FollowJump};
use crate::typeset::caps::Capabilities;
use crate::typeset::driver::{set_colour_budget, stretches_done};

/// `count` lines of a log from line `from` on, one in 97 a needle.
fn log(from: usize, count: usize) -> String {
    (from..from + count)
        .map(|n| match n % 97 {
            0 => format!("2026-03-01T10:00:00Z ERROR [db.pool] needle {n} in a line long enough to wrap at sixty\n"),
            _ => format!("2026-03-01T10:00:{:02}Z INFO  [http.server] request {n}\n", n % 60),
        })
        .collect()
}

/// A CSV of the rows `from` to `to`, a header first when `from` is 1.
fn csv(from: usize, to: usize) -> String {
    let header = if from == 1 { "id,name,note\n" } else { "" };
    let rows: String = (from..=to)
        .map(|n| format!("{n},name {n},\"a note\nof two lines\"\n"))
        .collect();
    format!("{header}{rows}")
}

/// The rows of the document on the last screen of `ran`, without the
/// status line.
fn last_screen(ran: &Ran) -> Vec<Row> {
    let seen = ran.shown().last().expect("a frame");
    seen.rows[..seen.rows.len() - 1].to_vec()
}

/// A pager on a kitty terminal that follows `name`.
fn sim(name: &str, text: &str) -> Sim {
    Sim::new(60, 10)
        .caps(Capabilities::kitty(60))
        .file(name, text)
        .follow(Follow::On, FollowJump::First)
}

/// What the pager shows, after `keys` one after another, of `name`
/// followed while it holds `before` and then `after`, and of `name`
/// opened holding `after`.
fn grown_and_fresh(
    name: &str,
    (before, after): (&str, &str),
    keys: &[&str],
) -> (Vec<Row>, Vec<Row>) {
    let with_keys = |mut sim: Sim, from: u64| {
        for (at, key) in (from..).step_by(300).zip(keys) {
            sim = sim.keys(at, key);
        }
        sim.keys(from + 300 * keys.len() as u64 + 500, "q")
    };
    let grown = with_keys(
        sim(name, before).keys(100, "G").write(400, name, after),
        1500,
    );
    let grown = grown.kiri(&[name]).quiet();
    assert!(matches!(grown.result, Some(Ok(()))), "{:?}", grown.result);
    let fresh = with_keys(sim(name, after), 1500).kiri(&[name]).quiet();
    assert!(matches!(fresh.result, Some(Ok(()))), "{:?}", fresh.result);
    (last_screen(&grown), last_screen(&fresh))
}

#[test]
fn a_followed_log_text_and_csv_that_grow_show_what_they_show_opened_afresh() {
    set_colour_budget(Some(std::time::Duration::MAX));
    // The keys move the view to a place of their own: the end, the top, a
    // needle in the old part and one in the new.
    let cases = [
        (
            "app.log",
            log(0, 600),
            log(600, 150),
            &["/needle 582\r", "/needle 679\r"][..],
        ),
        ("notes.txt", log(0, 600), log(600, 1), &["/needle 582\r"]),
        (
            "table.csv",
            csv(1, 400),
            csv(401, 520),
            &["/name 450\r", "/name 150\r"],
        ),
    ];
    for (name, before, added, searches) in cases {
        let after = format!("{before}{added}");
        let keys = ["G", "g"].into_iter().chain(searches.iter().copied());
        for key in keys {
            let (grown, fresh) = grown_and_fresh(name, (&before, &after), &[key]);
            let texts = |rows: &[Row]| rows.iter().map(Row::text).collect::<Vec<_>>();
            assert_eq!(texts(&grown), texts(&fresh), "{name}, {key:?}");
            assert_eq!(grown, fresh, "{name}, {key:?}");
        }
    }
}

#[test]
fn a_followed_file_that_changed_before_its_end_is_laid_out_afresh() {
    set_colour_budget(Some(std::time::Duration::MAX));
    let before = log(0, 600);
    // The keys show the end, the top, and the place of the change.
    let changed = [
        (
            "a byte in the middle",
            before.replacen("request 300", "request 3o0", 1),
            "/request 3o0\r",
        ),
        (
            "a line removed at the top",
            before.split_once('\n').unwrap().1.to_owned(),
            "g",
        ),
        ("rotated", log(0, 20), "g"),
    ];
    for (what, after, at) in changed {
        for keys in [&["G"][..], &["g"], &[at]] {
            let (grown, fresh) = grown_and_fresh("app.log", (&before, &after), keys);
            assert_eq!(grown, fresh, "{what}, {keys:?}");
        }
    }
}

/// A CSV of a header and the rows `from` to `to`, of four-digit ids and
/// names, so that rows appended keep the widths of the columns.
fn csv_of_ids(from: usize, to: usize) -> String {
    let rows: String = (from..=to)
        .map(|n| format!("{n},name {n},\"a note\nof two lines\"\n"))
        .collect();
    format!("id,name,note\n{rows}")
}

/// The stretches laid out or measured by a pager that follows `name`,
/// holding `before`, while it is written as `after`, if it is, and then
/// gets `keys`.
fn stretches_of(name: &str, before: &str, after: Option<&str>, keys: &str) -> usize {
    let start = stretches_done();
    let mut sim = sim(name, before);
    if let Some(after) = after {
        sim = sim.write(400, name, after);
    }
    if !keys.is_empty() {
        sim = sim.keys(1500, keys);
    }
    let ran = sim.keys(2500, "q").kiri(&[name]).quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    stretches_done() - start
}

#[test]
fn a_reload_of_a_log_or_csv_grown_by_a_line_lays_out_its_last_stretch_alone() {
    // The same run without a change, with the file grown by a line or a
    // row, and with it written again changed before its end. 600 lines or
    // rows are 5 stretches, the last of 88.
    let log_before = log(0, 600);
    let csv_before = csv_of_ids(1000, 1599);
    let cases = [
        (
            "app.log",
            &log_before,
            format!("{log_before}{}", log(600, 1)),
            log_before.replacen("request 1", "request X", 1),
        ),
        (
            "table.csv",
            &csv_before,
            format!("{csv_before}1600,name 1600,\"a note\nof two lines\"\n"),
            csv_before.replacen("name 1300", "name 13X0", 1),
        ),
    ];
    for (name, before, grown, changed) in cases {
        let opened = stretches_of(name, before, None, "");
        let grown = stretches_of(name, before, Some(&grown), "");
        let changed = stretches_of(name, before, Some(&changed), "");
        // A reload of a file that changed before its end does it all again;
        // one that grew by a line or a row lays out its last stretch again.
        assert_eq!(grown - opened, 1, "{name}: opened {opened}, grown {grown}");
        assert!(
            changed - opened >= 5,
            "{name}: opened {opened}, changed {changed}"
        );
    }
}

#[test]
fn r_lays_a_followed_log_that_grew_out_afresh() {
    let before = log(0, 600);
    let after = format!("{before}{}", log(600, 1));
    let grown = stretches_of("app.log", &before, Some(&after), "");
    let then_r = stretches_of("app.log", &before, Some(&after), "r");
    // `r` lays the whole file out again, 5 stretches.
    assert!(then_r - grown >= 5, "grown {grown}, then r {then_r}");
}

#[test]
fn r_on_a_followed_csv_grown_by_wider_rows_shows_what_it_shows_opened_afresh() {
    // An automatic reload keeps the widths of the columns, and the wider
    // names wrap; `r` fits them to every row, as a fresh open does.
    let before = csv_of_ids(1000, 1699);
    let wider: String = (1700..1800)
        .map(|n| format!("{n},name {n} wider than any name before it,note\n"))
        .collect();
    let after = format!("{before}{wider}");
    for keys in [&["r", "G"][..], &["G", "r", "g"]] {
        let (grown, fresh) = grown_and_fresh("table.csv", (&before, &after), keys);
        assert_eq!(grown, fresh, "{keys:?}");
    }
}

#[test]
fn a_search_in_a_followed_log_counts_and_keeps_its_match_when_the_log_grows() {
    let before = log(0, 1000);
    let ran = sim("app.log", &before)
        .keys(100, "/needle\r")
        .keys(200, "n")
        .write(400, "app.log", &format!("{before}{}", log(1000, 200)))
        .keys(1500, "n")
        .keys(1600, "N")
        .keys(1800, "q")
        .kiri(&["app.log"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    let at = |shown: &str| {
        (texts.iter())
            .position(|text| text.contains(shown))
            .unwrap_or_else(|| panic!("{shown}: {texts:?}"))
    };
    // Eleven needles, then thirteen: 0, 97, …, 970, then 1067 and 1164.
    let (second, grown, back) = (at("2/11"), at("3/13"), at("2/13"));
    assert!(second < grown && grown < back, "{texts:?}");
    assert!(texts[grown].contains("needle 194"), "{}", texts[grown]);
}
