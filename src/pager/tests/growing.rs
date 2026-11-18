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
            log(0, 1500),
            log(1500, 300),
            &["/needle 1455\r", "/needle 1746\r"][..],
        ),
        ("notes.txt", log(0, 1500), log(1500, 1), &["/needle 1455\r"]),
        (
            "table.csv",
            csv(1, 700),
            csv(701, 900),
            &["/name 850\r", "/name 350\r"],
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
    let before = log(0, 1500);
    let changed = [
        (
            "a byte in the middle",
            before.replacen("request 700", "request 7o0", 1),
        ),
        (
            "a line removed at the top",
            before.split_once('\n').unwrap().1.to_owned(),
        ),
        ("rotated", log(0, 20)),
    ];
    for (what, after) in changed {
        for keys in [&["G"][..], &["g"]] {
            let (grown, fresh) = grown_and_fresh("app.log", (&before, &after), keys);
            assert_eq!(grown, fresh, "{what}, {keys:?}");
        }
    }
}

#[test]
fn a_reload_of_a_log_grown_by_a_line_lays_out_its_last_stretch_and_the_view_s() {
    // The same run without a change, with the file grown by a line, and
    // with it written again changed at the top.
    let before = log(0, 3000);
    let run = |after: Option<&str>| {
        let start = stretches_done();
        let mut sim = sim("app.log", &before);
        if let Some(after) = after {
            sim = sim.write(400, "app.log", after);
        }
        let ran = sim.keys(2000, "q").kiri(&["app.log"]).quiet();
        assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
        stretches_done() - start
    };
    let opened = run(None);
    let grown = run(Some(&format!("{before}{}", log(3000, 1))));
    let changed = run(Some(&before.replacen("request 1", "request X", 1)));
    // Opened, the 24 stretches are measured or laid out, and those near the
    // view laid out; a reload of a file that changed at its top does it all
    // again; one that grew by a line lays out its last stretch again.
    assert_eq!(grown - opened, 1, "opened {opened}, grown {grown}");
    assert!(changed - opened >= 24, "opened {opened}, changed {changed}");
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
