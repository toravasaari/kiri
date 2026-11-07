use super::*;
use crate::session::state::Look;

/// The look of a version of a document, told apart by `length`.
fn look(length: u64) -> Change {
    Change::Changed(Look {
        modified: None,
        length,
    })
}

/// `document` followed with `then` to reload it, in a window of
/// 60 × 6, with `reads` and then `empty` polls without keys.
fn following(document: &Document, then: &mut Then, reads: &[&[u8]], empty: usize) -> Vec<String> {
    let how = (settings::Follow::On, FollowJump::First);
    following_in(6, how, document, then, reads, empty)
}

#[test]
fn following_reloads_a_change_without_a_key_and_keeps_the_block_at_the_top() {
    // The read of the keys, then a change seen on two polls in a row.
    let changes = [Change::Unchanged, look(1), look(1)];
    let mut then = Then::changing([Reloaded::Document(rows(10))], changes);
    let frames = following(&numbers(10), &mut then, &[b"jjjjjj"], 5);
    assert_eq!(then.asked, 1, "{frames:#?}");
    assert_eq!(frames.len(), 3, "{frames:#?}");
    assert!(at_top(&frames[1], "Line 4"), "{frames:#?}");
    assert!(at_top(&frames[2], "Row 4"), "{frames:#?}");
    assert!(
        status_row(&frames[2], 6).contains("reloaded"),
        "{frames:#?}"
    );
}

/// `document` followed as `follow` says, jumping to the change `jump`
/// chooses, with `then` to reload it, in a window of 60 × `rows`, with
/// `reads` and then `empty` polls without keys.
fn following_in(
    rows: u16,
    how: (settings::Follow, FollowJump),
    document: &Document,
    then: &mut Then,
    reads: &[&[u8]],
    empty: usize,
) -> Vec<String> {
    let caps = Capabilities::ansi(80);
    following_on(caps, rows, how, document, then, reads, empty).frames()
}

/// As [`following_in`], on a terminal with `caps`, which is returned
/// for what it was asked and what it drew.
fn following_on(
    caps: Capabilities,
    rows: u16,
    (follow, jump): (settings::Follow, FollowJump),
    document: &Document,
    then: &mut Then,
    reads: &[&[u8]],
    empty: usize,
) -> Fake {
    let mut reads = reads.to_vec();
    reads.extend(std::iter::repeat_n(b"".as_slice(), empty));
    let mut terminal = Fake::new(60, rows, &reads);
    let mut inputs = Inputs::new(caps);
    run_document(
        document,
        &mut inputs,
        Options {
            files: Some(then),
            follow,
            jump,
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    terminal
}

/// Following on auto, jumping to the first change.
const AUTO: (settings::Follow, FollowJump) = (settings::Follow::Auto, FollowJump::First);

/// `numbers(count)` with each line of `changed` written anew.
pub(super) fn numbers_changed(count: usize, changed: &[usize]) -> Document {
    let source: String = (1..=count)
        .map(|n| match changed.contains(&n) {
            true => format!("Line {n} changed\n\n"),
            false => format!("Line {n}\n\n"),
        })
        .collect();
    crate::typeset::parse::parse(&source)
}

#[test]
fn auto_following_moves_the_view_to_a_change_below_it() {
    // A change seen on two polls, to Line 30, far below the view.
    let changes = [look(1), look(1)];
    let new = numbers_changed(40, &[30]);
    let mut then = Then::changing([Reloaded::Document(new)], changes);
    let frames = following_in(21, AUTO, &numbers(40), &mut then, &[], 3);
    let screens = replay(&frames);
    assert_eq!(then.asked, 1, "{screens:#?}");
    assert!(at_top_of(&screens[0], "Line 1"), "{screens:#?}");
    let last = screens.last().unwrap();
    // Three rows of what comes before it above it.
    assert_eq!(
        screen_row_of(last, 21, "Line 30 changed"),
        Some(4),
        "{screens:#?}"
    );
    assert_eq!(screen_row_of(last, 21, "Line 29"), Some(2), "{screens:#?}");
}

#[test]
fn auto_following_tints_the_change_it_went_to_and_fades_the_tint_out() {
    let new = numbers_changed(40, &[30]);
    let mut then = Then::new([Reloaded::Document(new)]);
    let terminal = following_on(
        known_colours(),
        21,
        AUTO,
        &numbers(40),
        &mut then,
        &[b"r"],
        90,
    );
    let screens = terminal.screens();
    let tint = |screen: &[String], text: &str| background(&screen_row_style(screen, text));
    // Grey text on a black page: 15 % of 200 is 30; a line on the page has
    // no background of its own.
    let first = &screens[2];
    assert_eq!(
        tint(first, "Line 30 changed"),
        Some("30;30;30".to_owned()),
        "{screens:#?}"
    );
    // Only the change: the line above is on the page.
    assert_eq!(tint(first, "Line 29"), None, "{screens:#?}");
    // The tint reaches the end of the row.
    let row = unstyled(screen_row(first, 4));
    assert_eq!(row.chars().count(), 60, "{row:?}");
    // A second later it begins to fade, and is gone after two more.
    let tints: Vec<_> = screens[2..]
        .iter()
        .map(|screen| tint(screen, "Line 30 changed"))
        .collect();
    let levels: Vec<u8> = tints
        .iter()
        .map(|tint| {
            tint.as_ref()
                .map_or(0, |tint| tint.split(';').next().unwrap().parse().unwrap())
        })
        .collect();
    assert!(
        levels.windows(2).all(|pair| pair[0] >= pair[1]),
        "{levels:?}"
    );
    assert_eq!(levels[0], 30);
    assert_eq!(*levels.last().unwrap(), 0);
    // A step of the fade every frame for the three seconds from the
    // reload's read, and polls again after them.
    let waits = &terminal.waits[1..];
    let frames = waits.iter().take_while(|&&wait| wait == FRAME).count();
    assert_eq!(frames, 60, "{waits:?}");
    assert_eq!(waits[frames], POLL, "{waits:?}");
}

/// The frames drawn while following `old` as `how` says, reloaded to
/// `new` by `r`, on a terminal with `caps`, with `empty` polls after it.
fn reloaded_to(
    caps: Capabilities,
    how: (settings::Follow, FollowJump),
    (old, new): (&Document, Document),
    reads: &[&[u8]],
    empty: usize,
) -> Fake {
    let mut then = Then::new([Reloaded::Document(new)]);
    following_on(caps, 21, how, old, &mut then, reads, empty)
}

#[test]
fn a_change_already_in_view_is_tinted_too() {
    let new = numbers_changed(40, &[8]);
    let terminal = reloaded_to(known_colours(), AUTO, (&numbers(40), new), &[b"r"], 3);
    let screens = terminal.screens();
    assert!(at_top_of(&screens[2], "Line 1"), "{screens:#?}");
    let tint = background(&screen_row_style(&screens[2], "Line 8 changed"));
    assert_eq!(tint, Some("30;30;30".to_owned()), "{screens:#?}");
}

#[test]
fn a_removal_is_not_tinted_and_nothing_fades() {
    let terminal = reloaded_to(
        known_colours(),
        AUTO,
        (&numbers(40), numbers(39)),
        &[b"r"],
        90,
    );
    let frames = terminal.frames();
    for frame in &frames {
        assert!(!document_part(frame, 21).contains("48;2"), "{frame:?}");
    }
    assert!(
        !terminal.waits[1..5].contains(&FRAME),
        "{:?}",
        terminal.waits
    );
}

#[test]
fn only_auto_following_tints_a_change() {
    let on = (settings::Follow::On, FollowJump::First);
    let new = numbers_changed(40, &[8]);
    let terminal = reloaded_to(known_colours(), on, (&numbers(40), new), &[b"r"], 3);
    for frame in terminal.frames() {
        assert!(!document_part(&frame, 21).contains("48;2"), "{frame:?}");
    }
}

#[test]
fn without_colours_known_as_rgb_nothing_is_tinted() {
    let new = numbers_changed(40, &[8]);
    let terminal = reloaded_to(
        Capabilities::ansi(80),
        AUTO,
        (&numbers(40), new),
        &[b"r"],
        3,
    );
    for frame in terminal.frames() {
        assert!(!document_part(&frame, 21).contains("48;2"), "{frame:?}");
    }
    // Nor does the pager wait a frame for it.
    assert!(
        !terminal.waits[1..5].contains(&FRAME),
        "{:?}",
        terminal.waits
    );
}

#[test]
fn the_tint_stays_on_the_same_lines_when_the_view_scrolls() {
    let new = numbers_changed(40, &[30]);
    let terminal = reloaded_to(
        known_colours(),
        AUTO,
        (&numbers(40), new),
        &[b"r", b"kk"],
        3,
    );
    let screens = terminal.screens();
    let last = screens.last().unwrap();
    // Two rows up, the change is two rows lower than it was.
    assert_eq!(
        screen_row_of(last, 21, "Line 30 changed"),
        Some(6),
        "{screens:#?}"
    );
    assert_eq!(
        background(&screen_row_style(last, "Line 30 changed")),
        Some("30;30;30".to_owned()),
        "{screens:#?}"
    );
    assert_eq!(
        background(&screen_row_style(last, "Line 31")),
        None,
        "{screens:#?}"
    );
}

#[test]
fn a_new_reload_replaces_the_tint_still_fading() {
    let first = numbers_changed(40, &[5]);
    let second = numbers_changed(40, &[5, 7]);
    let mut then = Then::new([Reloaded::Document(first), Reloaded::Document(second)]);
    // The second reload comes half a second after the first.
    let waits = [b"".as_slice(); 9];
    let reads = [&[b"r".as_slice()][..], &waits, &[b"r"]].concat();
    let terminal = following_on(
        known_colours(),
        21,
        AUTO,
        &numbers(40),
        &mut then,
        &reads,
        3,
    );
    let screens = terminal.screens();
    let last = screens.last().unwrap();
    // Line 5 is the same as it was, and not the change of this reload.
    assert_eq!(
        background(&screen_row_style(last, "Line 7 changed")),
        Some("30;30;30".to_owned()),
        "{screens:#?}"
    );
    assert_eq!(
        background(&screen_row_style(last, "Line 5 changed")),
        None,
        "{screens:#?}"
    );
}

#[test]
fn a_tint_is_mixed_over_the_background_a_line_has() {
    let old = crate::typeset::parse::parse("Text.\n\n```\ncode\n```\n");
    let new = crate::typeset::parse::parse("Text.\n\n```\ncode changed\n```\n");
    let terminal = reloaded_to(known_colours(), AUTO, (&old, new), &[b"r"], 3);
    let screens = terminal.screens();
    let before = background(&screen_row_style(&screens[0], "code"));
    let after = background(&screen_row_style(&screens[2], "code changed"));
    let level = |bg: &Option<String>| -> u8 {
        bg.as_ref()
            .map_or(0, |bg| bg.split(';').next().unwrap().parse().unwrap())
    };
    // The code block's own background, lighter by the tint.
    assert!(before.is_some(), "{screens:#?}");
    assert!(level(&after) > level(&before), "{before:?} {after:?}");
}

#[test]
fn r_moves_to_a_change_while_following_is_auto() {
    let new = numbers_changed(40, &[30]);
    let mut then = Then::new([Reloaded::Document(new)]);
    let frames = following_in(21, AUTO, &numbers(40), &mut then, &[b"r"], 1);
    let screens = replay(&frames);
    assert_eq!(then.asked, 1, "{screens:#?}");
    let last = screens.last().unwrap();
    assert_eq!(
        screen_row_of(last, 21, "Line 30 changed"),
        Some(4),
        "{screens:#?}"
    );
}

#[test]
fn r_keeps_the_place_while_following_is_on() {
    let new = numbers_changed(40, &[30]);
    let mut then = Then::new([Reloaded::Document(new)]);
    let on = (settings::Follow::On, FollowJump::First);
    let frames = following_in(21, on, &numbers(40), &mut then, &[b"r"], 1);
    let screens = replay(&frames);
    assert_eq!(then.asked, 1, "{screens:#?}");
    assert!(at_top_of(screens.last().unwrap(), "Line 1"), "{screens:#?}");
}

#[test]
fn auto_following_does_not_move_to_a_change_in_view() {
    let new = numbers_changed(40, &[8]);
    let mut then = Then::new([Reloaded::Document(new)]);
    let frames = following_in(21, AUTO, &numbers(40), &mut then, &[b"jj", b"r"], 1);
    let screens = replay(&frames);
    let last = screens.last().unwrap();
    assert!(at_top_of(last, "Line 2"), "{screens:#?}");
    assert!(
        screen_row_of(last, 21, "Line 8 changed").is_some(),
        "{screens:#?}"
    );
}

#[test]
fn auto_following_does_not_move_from_inside_a_change_taller_than_the_window() {
    // Lines 10 to 25 changed: 31 lines in a view of 20 rows. The
    // reader is at Line 15, inside them.
    let changed: Vec<usize> = (10..=25).collect();
    let new = numbers_changed(40, &changed);
    let mut then = Then::new([Reloaded::Document(new)]);
    let down = [b"j".as_slice(); 28].concat();
    let frames = following_in(21, AUTO, &numbers(40), &mut then, &[&down, b"r"], 1);
    assert!(at_top(&frames[1], "Line 15"), "{frames:#?}");
    assert!(
        at_top(frames.last().unwrap(), "Line 15 changed"),
        "{frames:#?}"
    );
}

#[test]
fn auto_following_does_not_move_when_the_start_of_a_change_is_shown() {
    // Lines 8 to 30 changed; Line 8 is on the first page.
    let changed: Vec<usize> = (8..=30).collect();
    let new = numbers_changed(40, &changed);
    let mut then = Then::new([Reloaded::Document(new)]);
    let frames = following_in(21, AUTO, &numbers(40), &mut then, &[b"r"], 1);
    let screens = replay(&frames);
    assert!(at_top_of(screens.last().unwrap(), "Line 1"), "{screens:#?}");
}

#[test]
fn auto_following_does_not_move_after_a_reload_that_changed_nothing() {
    let mut then = Then::new([Reloaded::Document(numbers(40))]);
    let frames = following_in(21, AUTO, &numbers(40), &mut then, &[b"jjjj", b"r"], 1);
    let screens = replay(&frames);
    assert_eq!(then.asked, 1, "{screens:#?}");
    assert!(at_top_of(screens.last().unwrap(), "Line 3"), "{screens:#?}");
}

#[test]
fn auto_following_moves_to_the_change_the_setting_chooses() {
    let long = "Line 30 changed into a paragraph long enough to take two \
                    lines or more of the window";
    let source: String = (1..=40)
        .map(|n| match n {
            15 => "Line 15 changed\n\n".to_owned(),
            30 => format!("{long}\n\n"),
            n => format!("Line {n}\n\n"),
        })
        .collect();
    for (jump, shown) in [
        (FollowJump::First, "Line 15 changed"),
        (FollowJump::Last, "Line 30 changed"),
        (FollowJump::Most, "Line 30 changed"),
    ] {
        let new = crate::typeset::parse::parse(&source);
        let mut then = Then::new([Reloaded::Document(new)]);
        let auto = (settings::Follow::Auto, jump);
        let frames = following_in(21, auto, &numbers(40), &mut then, &[b"r"], 1);
        let screens = replay(&frames);
        let last = screens.last().unwrap();
        assert_eq!(
            screen_row_of(last, 21, shown),
            Some(4),
            "{jump:?}: {screens:#?}"
        );
    }
    // Most is the first of changes that take as many rows.
    let new = numbers_changed(40, &[15, 30]);
    let mut then = Then::new([Reloaded::Document(new)]);
    let most = (settings::Follow::Auto, FollowJump::Most);
    let frames = following_in(21, most, &numbers(40), &mut then, &[b"r"], 1);
    let screens = replay(&frames);
    let last = screens.last().unwrap();
    assert_eq!(
        screen_row_of(last, 21, "Line 15 changed"),
        Some(4),
        "{screens:#?}"
    );
}

#[test]
fn auto_following_moves_to_a_removal_as_the_setting_chooses() {
    // Line 15 changed and Line 30 removed.
    let source: String = (1..=40)
        .filter(|&n| n != 30)
        .map(|n| match n {
            15 => "Line 15 changed\n\n".to_owned(),
            n => format!("Line {n}\n\n"),
        })
        .collect();
    for (jump, shown) in [
        // The removal is the last change, at Line 31 now after it.
        (FollowJump::Last, "Line 31"),
        // A removal counts as no rows.
        (FollowJump::Most, "Line 15 changed"),
    ] {
        let new = crate::typeset::parse::parse(&source);
        let mut then = Then::new([Reloaded::Document(new)]);
        let auto = (settings::Follow::Auto, jump);
        let frames = following_in(21, auto, &numbers(40), &mut then, &[b"r"], 1);
        let screens = replay(&frames);
        let last = screens.last().unwrap();
        assert_eq!(
            screen_row_of(last, 21, shown),
            Some(4),
            "{jump:?}: {screens:#?}"
        );
    }
    // A removal alone is the change of the most rows.
    let source: String = (1..=40)
        .filter(|&n| n != 30)
        .map(|n| format!("Line {n}\n\n"))
        .collect();
    let mut then = Then::new([Reloaded::Document(crate::typeset::parse::parse(&source))]);
    let most = (settings::Follow::Auto, FollowJump::Most);
    let frames = following_in(21, most, &numbers(40), &mut then, &[b"r"], 1);
    let screens = replay(&frames);
    let last = screens.last().unwrap();
    assert_eq!(screen_row_of(last, 21, "Line 31"), Some(4), "{screens:#?}");
}

#[test]
fn auto_following_shows_a_change_at_the_end_on_the_last_page() {
    // The last line of the last page, above the two rows of the margin.
    let new = numbers_changed(40, &[40]);
    let mut then = Then::new([Reloaded::Document(new)]);
    let frames = following_in(21, AUTO, &numbers(40), &mut then, &[b"r"], 1);
    let screens = replay(&frames);
    let last = screens.last().unwrap();
    assert_eq!(
        screen_row_of(last, 21, "Line 40 changed"),
        Some(18),
        "{screens:#?}"
    );
    // A removal at the end, too.
    let mut then = Then::new([Reloaded::Document(numbers(39))]);
    let frames = following_in(21, AUTO, &numbers(40), &mut then, &[b"r"], 1);
    let screens = replay(&frames);
    let last = screens.last().unwrap();
    assert_eq!(screen_row_of(last, 21, "Line 39"), Some(18), "{screens:#?}");
}

/// Whether the status line of `frame`, in a window of 6 rows, tells
/// that the pager follows the document.
fn follows(frame: &str) -> bool {
    status_row(frame, 6).contains(" follow   ")
}

/// Whether the status line of `frame`, in a window of 6 rows, tells
/// that the pager follows the document and moves to a change.
fn follows_auto(frame: &str) -> bool {
    status_row(frame, 6).contains(" auto   ")
}

#[test]
fn capital_f_goes_from_off_to_on_to_auto_and_back_to_off() {
    // F, a change seen on two polls, F to auto, where the next change
    // is seen once, and F to off, where the last one is not asked
    // about.
    let changes = [look(1), look(1), look(2), look(3)];
    let mut then = Then::changing([Reloaded::Document(rows(10))], changes);
    let reads: &[&[u8]] = &[b"F", b"", b"F", b"F", b""];
    let frames = reloading_with(Capabilities::ansi(80), &numbers(10), &mut then, reads, 0);
    assert_eq!(then.asked, 1, "{frames:#?}");
    assert_eq!(then.changes.len(), 1, "{frames:#?}");
    assert_eq!(frames.len(), 5, "{frames:#?}");
    assert!(!follows(&frames[0]), "{frames:#?}");
    assert!(!follows_auto(&frames[0]), "{frames:#?}");
    assert!(follows(&frames[1]), "{frames:#?}");
    assert!(at_top(&frames[2], "Row 1"), "{frames:#?}");
    assert!(follows(&frames[2]), "{frames:#?}");
    assert!(follows_auto(&frames[3]), "{frames:#?}");
    assert!(!follows(&frames[3]), "{frames:#?}");
    assert!(!follows(&frames[4]), "{frames:#?}");
    assert!(!follows_auto(&frames[4]), "{frames:#?}");
}

#[test]
fn the_status_line_tells_following_from_the_start() {
    let mut then = Then::new([]);
    let frames = following(&numbers(10), &mut then, &[], 0);
    assert!(follows(&frames[0]), "{frames:#?}");
}

#[test]
fn without_a_file_to_follow_capital_f_says_so_until_the_next_key() {
    let frames = shown(&numbers(10), 60, 6, &[b"F", b"j"]);
    assert_eq!(frames.len(), 3, "{frames:#?}");
    assert!(status_row(&frames[1], 6).contains("nothing to follow"));
    assert!(!status_row(&frames[2], 6).contains("follow"), "{frames:#?}");
}

#[test]
fn following_from_the_start_without_a_file_says_there_is_nothing_to_follow() {
    let mut terminal = Fake::new(60, 6, &[b"j"]);
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    let options = Options {
        follow: settings::Follow::On,
        ..Options::new(SearchCase::Smart)
    };
    let document = numbers(10);
    run_document(&document, &mut inputs, options, &mut terminal);
    let frames = terminal.frames();
    assert_eq!(frames.len(), 2, "{frames:#?}");
    assert!(status_row(&frames[0], 6).contains("nothing to follow"));
    assert!(!status_row(&frames[1], 6).contains("follow"), "{frames:#?}");
}

#[test]
fn following_waits_a_poll_s_time_for_a_change_to_look_the_same_while_keys_come_fast() {
    // A change seen on the reads of two keys 5 ms apart, and then on
    // a poll without keys.
    let changes = [look(1), look(1), look(1)];
    let mut then = Then::changing([Reloaded::Document(rows(10))], changes);
    let mut terminal = Fake::new(60, 6, &[b"j", b"j", b""]);
    terminal.typing = Some(Duration::from_millis(5));
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    let document = numbers(10);
    let options = Options {
        files: Some(&mut then),
        follow: settings::Follow::On,
        ..Options::new(SearchCase::Smart)
    };
    run_document(&document, &mut inputs, options, &mut terminal);
    let frames = terminal.frames();
    assert_eq!(then.asked, 1, "{frames:#?}");
    assert_eq!(frames.len(), 4, "{frames:#?}");
    assert!(at_top(&frames[2], "Line 2"), "{frames:#?}");
    assert!(at_top(&frames[3], "Row 2"), "{frames:#?}");
}

#[test]
fn a_followed_file_is_looked_at_once_a_poll_however_many_keys_come() {
    // Keys 5 ms apart for four reads, and then a poll without keys: the
    // file is looked at on the first turn and on the turn a poll's time
    // later, two of the five looks the reads' turns could take.
    let mut then = Then::changing([], std::iter::repeat_n(Change::Unchanged, 5));
    let mut terminal = Fake::new(60, 6, &[b"j", b"j", b"j", b"j", b""]);
    terminal.typing = Some(Duration::from_millis(5));
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    let document = numbers(10);
    let options = Options {
        files: Some(&mut then),
        follow: settings::Follow::On,
        ..Options::new(SearchCase::Smart)
    };
    run_document(&document, &mut inputs, options, &mut terminal);
    let frames = terminal.frames();
    assert_eq!(then.changes.len(), 3, "{frames:#?}");
    assert_eq!(then.asked, 0, "{frames:#?}");
}

#[test]
fn following_waits_until_a_change_looks_the_same_on_two_polls() {
    // Still being written on the second poll, then done.
    let changes = [look(1), look(2), look(2)];
    let mut then = Then::changing([Reloaded::Document(rows(10))], changes);
    let frames = following(&numbers(10), &mut then, &[], 5);
    assert_eq!(then.asked, 1, "{frames:#?}");
    assert_eq!(frames.len(), 2, "{frames:#?}");
    assert!(at_top(&frames[1], "Row 1"), "{frames:#?}");
    // A change that is not seen again is not read.
    let changes = [look(1), Change::Unchanged, look(2)];
    let mut then = Then::changing([], changes);
    let frames = following(&numbers(10), &mut then, &[], 5);
    assert_eq!(then.asked, 0, "{frames:#?}");
}

#[test]
fn without_following_a_change_is_not_read() {
    let changes = [look(1), look(1), look(1)];
    let mut then = Then::changing([], changes);
    let frames = reloading_with(Capabilities::ansi(80), &numbers(10), &mut then, &[], 5);
    assert_eq!(then.asked, 0, "{frames:#?}");
    assert_eq!(then.changes.len(), 3, "{frames:#?}");
}

#[test]
fn following_a_document_that_is_gone_keeps_it_and_tells_why_once() {
    let gone = || Change::Gone("No such file or directory".to_owned());
    let mut then = Then::changing([], [gone(), gone(), gone(), gone()]);
    let frames = following(&numbers(10), &mut then, &[b"", b"", b"j"], 3);
    let screens = replay(&frames);
    assert_eq!(then.asked, 0, "{screens:#?}");
    assert_eq!(screens.len(), 3, "{screens:#?}");
    assert!(at_top_of(&screens[1], "Line 1"), "{screens:#?}");
    assert!(
        screen_row(&screens[1], 6).contains("No such file"),
        "{screens:#?}"
    );
    // A key clears it, and it is not told again while still gone.
    assert!(!screen_row(&screens[2], 6).contains("No such file"));
}

#[test]
fn following_reads_a_document_back_once_it_is_there_again() {
    let gone = Change::Gone("No such file or directory".to_owned());
    let changes = [gone, look(1), look(1)];
    let mut then = Then::changing([Reloaded::Document(rows(10))], changes);
    let frames = following(&numbers(10), &mut then, &[], 5);
    assert_eq!(then.asked, 1, "{frames:#?}");
    let last = frames.last().unwrap();
    assert!(at_top(last, "Row 1"), "{frames:#?}");
    assert!(status_row(last, 6).contains("reloaded"), "{frames:#?}");
}

#[test]
fn following_tells_why_a_change_could_not_be_read() {
    let why = Reloaded::Failed("not UTF-8 text".to_owned());
    let mut then = Then::changing([why], [look(1), look(1)]);
    let frames = following(&numbers(10), &mut then, &[], 5);
    let screens = replay(&frames);
    assert_eq!(then.asked, 1, "{screens:#?}");
    let last = screens.last().unwrap();
    assert!(at_top_of(last, "Line 1"), "{screens:#?}");
    assert!(
        screen_row(last, 6).contains("not UTF-8 text"),
        "{screens:#?}"
    );
}
