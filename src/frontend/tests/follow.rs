//! Following a file in the simulated front end: a followed file is paged
//! even when short, the pager reads it again when it changes without a key,
//! and standard input and several files have nothing to follow. Which flag
//! or variable sets which setting is `config`'s tests.

use crate::frontend::sim::{Sim, plain};
use crate::session::settings::{Follow, FollowJump, Paging};

/// `count` paragraphs of one row each, `Line 1` to `Line count`.
fn paragraphs(count: usize) -> String {
    (1..=count).map(|n| format!("Line {n}\n\n")).collect()
}

/// A document longer than a window of 5 rows.
fn long() -> String {
    paragraphs(10)
}

/// The text of `long.md` written anew: ten paragraphs of `Fresh`.
fn fresh() -> String {
    (1..=10).map(|n| format!("Fresh {n}\n\n")).collect()
}

#[test]
fn following_pages_a_short_file() {
    // `-f` and `--follow on`, and `--fa` and `--follow auto`, set the same
    // settings; the configuration file's `follow` is the setting alone.
    for (follow, word) in [(Follow::On, " follow "), (Follow::Auto, " auto ")] {
        let ran = Sim::new(40, 24)
            .file("document.md", &paragraphs(3))
            .follow(follow, FollowJump::First)
            .keys(10, "q")
            .kiri(&["document.md"]);
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
        .kiri(&["document.md"]);
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
        .kiri(&["document.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "{:?}", ran.out);
    assert!(plain(&ran.out).contains("Line 3"), "{:?}", ran.out);

    let ran = Sim::new(40, 24)
        .file("document.md", &paragraphs(3))
        .settings(|settings| {
            settings.paging = Paging::Never;
            settings.follow = Follow::On;
        })
        .kiri(&["document.md"]);
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
        .kiri(&["document.md"]);
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
        .kiri(&["-"]);
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
        .kiri(&["long.md"]);
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
        .kiri(&["long.md"]);
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
        .kiri(&["-"]);
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
        .kiri(&["long.md", "document.md"]);
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
        .kiri(&["-"]);
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
        .kiri(&["-"]);
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
        .kiri(&["long.md", "document.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert!(texts[0].contains("long.md 1/2"), "{texts:?}");
    assert!(
        !texts.iter().any(|text| text.contains("nothing to follow")),
        "{texts:?}"
    );
}
