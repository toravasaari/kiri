//! Following a file in the simulated frontend: a followed file is paged
//! even when short, the pager reads it again when it changes without a key,
//! and standard input and several files have nothing to follow. Which flag
//! or variable sets which setting is `config`'s tests.

use super::{fresh, long, paragraphs};
use crate::frontend::sim::{Sim, plain};
use crate::session::settings::{Follow, FollowJump, Paging};

#[test]
fn following_pages_a_short_file() {
    // `-f` and `--follow on`, and `--fa` and `--follow auto`, set the same
    // settings; the configuration file's `follow` is the setting alone.
    for (follow, word) in [(Follow::On, " follow "), (Follow::Auto, " auto ")] {
        let ran = Sim::new(40, 24)
            .file("document.md", &paragraphs(3))
            .follow(follow, FollowJump::First)
            .keys(10, "q")
            .kiri(&["document.md"])
            .quiet();
        assert!(
            matches!(ran.result, Some(Ok(()))),
            "{follow:?}: {:?}",
            ran.result
        );
        assert!(!ran.frames().is_empty(), "{follow:?}: {:?}", ran.out);
        let first = &ran.texts()[0];
        assert!(first.contains(word), "{follow:?}: {first}");
    }
    let ran = Sim::new(40, 24)
        .file("document.md", &paragraphs(3))
        .settings(|settings| settings.follow = Follow::On)
        .keys(10, "q")
        .kiri(&["document.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(!ran.frames().is_empty(), "{:?}", ran.out);
    assert!(ran.texts()[0].contains(" follow "), "{:?}", ran.texts());
}

#[test]
fn following_still_prints_with_paging_never() {
    // `-f -P` on the command line, and the same two in the file.
    let ran = Sim::new(40, 24)
        .file("document.md", &paragraphs(3))
        .follow(Follow::On, FollowJump::First)
        .settings(|settings| settings.paging = Paging::Never)
        .kiri(&["document.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "{:?}", ran.out);
    assert!(plain(&ran.out).contains("Line 3"), "{:?}", ran.out);

    let ran = Sim::new(40, 24)
        .file("document.md", &paragraphs(3))
        .settings(|settings| {
            settings.paging = Paging::Never;
            settings.follow = Follow::On;
        })
        .kiri(&["document.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "{:?}", ran.out);
    assert!(plain(&ran.out).contains("Line 3"), "{:?}", ran.out);
}

#[test]
fn kiri_follow_pages_a_short_file_and_leaves_standard_input_quiet() {
    // `KIRI_FOLLOW=on` is the setting `follow` on.
    let ran = Sim::new(40, 24)
        .file("document.md", &paragraphs(3))
        .settings(|settings| settings.follow = Follow::On)
        .keys(10, "q")
        .kiri(&["document.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(!ran.frames().is_empty(), "{:?}", ran.out);
    assert!(ran.texts()[0].contains(" follow "), "{:?}", ran.texts());

    let ran = Sim::new(40, 5)
        .stdin(&long())
        .settings(|settings| {
            settings.follow = Follow::On;
            settings.paging = Paging::Always;
        })
        .keys(10, "q")
        .kiri(&["-"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert!(texts[0].contains("Line 1"), "{texts:?}");
    assert!(
        !texts.iter().any(|text| text.contains("follow")),
        "{texts:?}"
    );
}

#[test]
fn follow_reads_the_file_again_when_it_changes_without_a_key() {
    // `--follow on` and `-f` set the same.
    let ran = Sim::new(40, 5)
        .file("long.md", &long())
        .follow(Follow::On, FollowJump::First)
        .write(50, "long.md", &fresh())
        .keys(3000, "q")
        .kiri(&["long.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert!(texts[0].contains("Line 1"), "{texts:?}");
    assert!(texts[0].contains(" follow "), "{texts:?}");
    let last = texts.last().expect("a frame");
    assert!(last.contains("Fresh 1"), "{texts:?}");
    assert!(last.contains("reloaded"), "{texts:?}");
    // Without a key, the pager does not say it is reloading.
    assert!(
        !texts.iter().any(|text| text.contains("reloading")),
        "{texts:?}"
    );
}

#[test]
fn the_fa_flag_follows_on_auto_and_moves_the_view_to_a_change_in_the_file() {
    // The view moving with `Follow::Auto` is pinned by the pager's tests;
    // this is that the flag means it.
    let changed = long().replace("Line 9\n", "Line 9 changed\n");
    let ran = Sim::new(40, 5)
        .file("long.md", &long())
        .follow(Follow::Auto, FollowJump::First)
        .write(50, "long.md", &changed)
        .keys(3000, "q")
        .kiri(&["long.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    let last = texts.last().expect("a frame");
    assert!(last.contains("Line 9 changed"), "{texts:?}");
    assert!(!last.contains("Line 1 "), "{texts:?}");
    assert!(last.contains(" auto "), "{texts:?}");
}

#[test]
fn standard_input_and_several_files_have_nothing_to_follow() {
    let ran = Sim::new(40, 5)
        .stdin(&long())
        .follow(Follow::On, FollowJump::First)
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "q")
        .kiri(&["-"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(
        ran.texts()
            .iter()
            .any(|text| text.contains("nothing to follow")),
        "{:?}",
        ran.texts()
    );

    let ran = Sim::new(40, 5)
        .file("long.md", &long())
        .file("document.md", &paragraphs(3))
        .follow(Follow::On, FollowJump::First)
        .keys(10, "q")
        .kiri(&["long.md", "document.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(
        ran.texts()
            .iter()
            .any(|text| text.contains("nothing to follow")),
        "{:?}",
        ran.texts()
    );

    let ran = Sim::new(40, 5)
        .stdin(&long())
        .follow(Follow::Auto, FollowJump::First)
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "q")
        .kiri(&["-"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(
        ran.texts()
            .iter()
            .any(|text| text.contains("nothing to follow")),
        "{:?}",
        ran.texts()
    );
}

#[test]
fn following_from_the_configuration_file_is_quiet_where_there_is_nothing_to_follow() {
    let ran = Sim::new(40, 5)
        .stdin(&long())
        .settings(|settings| {
            settings.paging = Paging::Always;
            settings.follow = Follow::On;
        })
        .keys(10, "q")
        .kiri(&["-"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert!(texts[0].contains("Line 1"), "{texts:?}");
    assert!(
        !texts.iter().any(|text| text.contains("nothing to follow")),
        "{texts:?}"
    );

    let ran = Sim::new(40, 5)
        .file("long.md", &long())
        .file("document.md", &paragraphs(3))
        .settings(|settings| settings.follow = Follow::On)
        .keys(10, "q")
        .kiri(&["long.md", "document.md"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert!(texts[0].contains("long.md 1/2"), "{texts:?}");
    assert!(
        !texts.iter().any(|text| text.contains("nothing to follow")),
        "{texts:?}"
    );
}

/// A comma-separated file whose header row has more cells than kiri shows.
fn too_wide() -> String {
    format!("{}\n1,2\n", ",".repeat(100_001))
}

#[test]
fn a_followed_file_that_cannot_be_read_is_told_once_and_not_read_again_until_it_changes() {
    let ran = Sim::new(60, 5)
        .file("table.csv", "a,b\n1,2\n")
        .follow(Follow::On, FollowJump::First)
        .write(50, "table.csv", &too_wide())
        .keys(500, "j")
        .keys(1500, "q")
        .kiri(&["table.csv"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    let told = texts
        .iter()
        .position(|text| text.contains("too wide"))
        .unwrap_or_else(|| panic!("{texts:?}"));
    let key = texts
        .iter()
        .rposition(|text| !text.contains("too wide"))
        .filter(|&at| at > told)
        .unwrap_or_else(|| panic!("a key clears it: {texts:?}"));
    // Told once: the file looks as it did when it failed, so it is not read
    // again, and nothing says `reloaded` either.
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.contains("too wide"))
            .count(),
        texts[told..key].len(),
        "{texts:?}"
    );
    assert!(
        texts[key..]
            .iter()
            .all(|text| !text.contains("too wide") && !text.contains("reloaded")),
        "{texts:?}"
    );
}

#[test]
fn a_followed_file_that_failed_is_read_when_it_is_written_again() {
    let ran = Sim::new(60, 5)
        .file("table.csv", "a,b\n1,2\n")
        .follow(Follow::On, FollowJump::First)
        .write(50, "table.csv", &too_wide())
        .keys(500, "j")
        .write(800, "table.csv", "c,d\n3,4\n")
        .keys(2000, "q")
        .kiri(&["table.csv"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert!(
        texts.iter().any(|text| text.contains("too wide")),
        "{texts:?}"
    );
    let last = texts.last().expect("a frame");
    assert!(last.contains("reloaded"), "{texts:?}");
    assert!(last.contains('c') && last.contains('d'), "{texts:?}");
}
