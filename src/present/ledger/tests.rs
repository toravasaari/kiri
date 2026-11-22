use super::*;
use crate::typeset::images::size::{Shows, Size};

fn placement(name: &str) -> Placement {
    Placement::block(
        ImageKey::from(name),
        Size {
            width: 10,
            height: 20,
            columns: 1,
            rows: 1,
        },
        Shows::Image,
    )
}

fn cells() -> Cells {
    Cells {
        columns: 1,
        rows: 1,
    }
}

fn id(n: u8) -> ImageId {
    ImageId::new(&[n], 1, 1, cells(), None)
}

fn held(n: u8) -> Held {
    Held {
        id: id(n),
        cell: (10, 20),
        checked: true,
        sent: true,
        states: 0,
    }
}

fn keep(ledger: &mut Ledger, placed: &[&Placement], cell: (u16, u16)) {
    let placed: HashSet<&Placement> = placed.iter().copied().collect();
    ledger.keep(&placed, &HashSet::new(), cell);
}

#[test]
fn an_id_two_placements_share_goes_only_when_neither_keeps_it() {
    let (a, b) = (placement("a"), placement("b"));
    let mut ledger = Ledger::default();
    ledger.put_still(a.clone(), held(1));
    ledger.put_still(b.clone(), held(1));
    keep(&mut ledger, &[&b], (10, 20));
    assert!(ledger.take_unused().is_empty());
    keep(&mut ledger, &[], (10, 20));
    assert_eq!(ledger.take_unused(), vec![id(1)]);
}

#[test]
fn ids_put_back_are_unused_again_unless_held_or_already_there() {
    let a = placement("a");
    let mut ledger = Ledger::default();
    ledger.put_still(a, held(1));
    ledger.unuse(&[id(1), id(2), id(2), id(3)]);
    assert_eq!(ledger.take_unused(), vec![id(2), id(3)]);
}

#[test]
fn a_new_cell_size_lets_every_image_go() {
    let a = placement("a");
    let mut ledger = Ledger::default();
    ledger.put_still(a.clone(), held(1));
    keep(&mut ledger, &[&a], (12, 24));
    assert_eq!(ledger.take_unused(), vec![id(1)]);
}

#[test]
fn lost_some_resets_the_tries_of_those_images_alone() {
    let mut ledger = Ledger::default();
    for _ in 0..AGAIN {
        assert!(ledger.try_again(id(1)));
        assert!(ledger.try_again(id(2)));
    }
    assert!(!ledger.try_again(id(1)));
    assert!(!ledger.try_again(id(2)));
    ledger.lost_some(&[id(1)]);
    assert!(ledger.try_again(id(1)));
    assert!(!ledger.try_again(id(2)));
    ledger.lost();
    assert!(ledger.try_again(id(1)));
    assert!(ledger.try_again(id(2)));
}

#[test]
fn pixels_held_and_sent_under_another_slot_are_not_sent_again() {
    let mut ledger = Ledger::default();
    ledger.put_still(placement("a"), held(5));
    assert!(ledger.holds_sent(id(5)));
    ledger.lost_some(&[id(5)]);
    assert!(!ledger.holds_sent(id(5)));
    assert!(ledger.holds(id(5)));
}

#[test]
fn sending_an_image_marks_the_stills_and_frames_that_share_its_id_sent() {
    let mut ledger = Ledger::default();
    let lost = |n| Held {
        sent: false,
        ..held(n)
    };
    ledger.put_still(placement("a"), lost(1));
    let frame = (placement("b"), ImageKey::from("b"), 1);
    ledger.put_frame(frame.clone(), lost(1));
    let driven = (placement("c"), ImageKey::from("c"), DRIVEN);
    ledger.put_frame(driven.clone(), lost(1));
    ledger.put_still(placement("d"), lost(2));
    ledger.mark_sent(id(1));
    assert!(ledger.still(&placement("a")).unwrap().sent);
    assert!(ledger.frame(&frame).unwrap().sent);
    assert!(!ledger.frame(&driven).unwrap().sent);
    assert!(!ledger.still(&placement("d")).unwrap().sent);
}

#[test]
fn ids_sent_through_tmux_wait_for_one_check_each() {
    let mut ledger = Ledger::default();
    ledger.sending(id(1), cells(), true);
    ledger.sending(id(1), cells(), true);
    ledger.sending(id(2), cells(), false);
    assert_eq!(ledger.take_sent(), vec![(id(1), cells())]);
    assert!(ledger.take_sent().is_empty());
}

#[test]
fn an_id_several_placements_share_counts_its_bytes_once() {
    let mut ledger = Ledger::default();
    ledger.put_still(placement("a"), held(1));
    ledger.weigh(id(1), 400);
    ledger.put_still(placement("b"), held(1));
    ledger.weigh(id(1), 400);
    ledger.put_still(placement("c"), held(2));
    ledger.weigh(id(2), 100);
    assert_eq!(ledger.bytes(), 500);
    keep(&mut ledger, &[&placement("c")], (10, 20));
    // Let go but not yet deleted: the terminal still holds it.
    assert_eq!(ledger.bytes(), 500);
    for id in ledger.take_unused() {
        ledger.forget(id);
    }
    assert_eq!(ledger.bytes(), 100);
}

#[test]
fn an_entry_marked_unsent_still_counts() {
    let mut ledger = Ledger::default();
    ledger.put_still(placement("a"), held(1));
    ledger.weigh(id(1), 400);
    ledger.lost();
    assert!(!ledger.holds_sent(id(1)));
    assert_eq!(ledger.bytes(), 400);
    let (a, b) = (placement("a"), placement("b"));
    ledger.put_still(
        b,
        Held {
            sent: false,
            ..held(2)
        },
    );
    ledger.weigh(id(2), 50);
    assert_eq!(ledger.bytes(), 450);
    // What nothing holds any more is not counted, nor are the bands.
    ledger.put_band(4, (10, 20), id(3));
    keep(&mut ledger, &[&a], (10, 20));
    for id in ledger.take_unused() {
        ledger.forget(id);
    }
    assert_eq!(ledger.bytes(), 400);
}

#[test]
fn an_id_let_go_counts_until_its_delete_went_out_and_again_when_it_failed() {
    let mut ledger = Ledger::default();
    ledger.put_still(placement("a"), held(1));
    ledger.weigh(id(1), 400);
    keep(&mut ledger, &[], (10, 20));
    assert_eq!(ledger.bytes(), 400);
    // Taken to be deleted: it counts until the delete is done.
    let ids = ledger.take_unused();
    assert_eq!(ids, vec![id(1)]);
    assert_eq!(ledger.bytes(), 400);
    // The delete failed and the id was put back.
    ledger.unuse(&ids);
    assert_eq!(ledger.bytes(), 400);
    ledger.take_unused();
    ledger.forget(id(1));
    assert_eq!(ledger.bytes(), 0);
}

#[test]
fn an_id_replaced_through_tmux_counts_until_it_is_deleted() {
    let mut ledger = Ledger::default();
    let a = placement("a");
    ledger.put_still(a.clone(), held(1));
    ledger.weigh(id(1), 400);
    ledger.put_still(a, held(2));
    ledger.weigh(id(2), 100);
    ledger.replace(id(1), id(2));
    assert_eq!(ledger.bytes(), 500);
    let due = ledger.answered(id(2));
    assert_eq!(due, vec![id(1)]);
    assert_eq!(ledger.bytes(), 500);
    ledger.forget(id(1));
    assert_eq!(ledger.bytes(), 100);
}

#[test]
fn an_id_something_holds_or_waits_to_delete_is_not_forgotten() {
    let mut ledger = Ledger::default();
    ledger.put_still(placement("a"), held(1));
    ledger.weigh(id(1), 400);
    ledger.forget(id(1));
    assert_eq!(ledger.bytes(), 400);
    keep(&mut ledger, &[], (10, 20));
    ledger.forget(id(1));
    assert_eq!(ledger.bytes(), 400);
}

#[test]
fn draining_the_ledger_forgets_every_size() {
    let mut ledger = Ledger::default();
    ledger.put_still(placement("a"), held(1));
    ledger.weigh(id(1), 400);
    ledger.put_band(4, (10, 20), id(2));
    ledger.drain();
    assert_eq!(ledger.bytes(), 0);
    assert_eq!(ledger.used(id(1)), None);
}

#[test]
fn the_age_of_an_id_is_the_use_counter_when_it_was_last_asked_for() {
    let mut ledger = Ledger::default();
    ledger.put_still(placement("a"), held(1));
    ledger.weigh(id(1), 4);
    ledger.put_still(placement("b"), held(2));
    ledger.weigh(id(2), 4);
    let first = ledger.used(id(1)).expect("an age");
    assert_eq!(ledger.used(id(2)), Some(first));
    ledger.tick();
    ledger.touch(id(2));
    assert!(ledger.used(id(2)) > ledger.used(id(1)));
    assert_eq!(ledger.used(id(1)), Some(first));
    assert_eq!(ledger.used(id(3)), None);
}

#[test]
fn delete_all_gives_every_id_held_once_bands_included() {
    let mut ledger = Ledger::default();
    ledger.put_still(placement("a"), held(1));
    ledger.put_still(placement("b"), held(1));
    let at = (placement("c"), ImageKey::from("c"), 1);
    ledger.put_frame(at, held(2));
    ledger.put_band(4, (10, 20), id(3));
    let mut ids = ledger.drain();
    ids.sort_by_key(|id| id.value());
    let mut want = vec![id(1), id(2), id(3)];
    want.sort_by_key(|id| id.value());
    assert_eq!(ids, want);
}

/// Five images of 100 bytes each, asked for in turn, and a frame since
/// the last: the fifth is the one the last frame asked for.
fn five() -> Ledger {
    let mut ledger = Ledger::default();
    for n in 1..=5 {
        ledger.put_still(placement(&n.to_string()), held(n));
        ledger.weigh(id(n), 100);
        ledger.tick();
    }
    ledger.touch(id(5));
    ledger
}

fn kept(view: &[u8], near: &[u8]) -> Kept {
    Kept {
        view: view.iter().map(|&n| id(n)).collect(),
        near: near.iter().map(|&n| id(n)).collect(),
    }
}

#[test]
fn past_the_budget_what_is_not_near_goes_before_older_near_images_the_oldest_first() {
    let ledger = five();
    // Image 4 is in view and 5 was asked for by the last frame; 1 is
    // near and the oldest of all.
    let marks = kept(&[4], &[1]);
    assert_eq!(ledger.over(450, 0, &marks), vec![id(2)]);
    assert_eq!(ledger.over(250, 0, &marks), vec![id(2), id(3), id(1)]);
    // Never one in view or one the last frame asked for.
    assert_eq!(ledger.over(0, 0, &marks), vec![id(2), id(3), id(1)]);
    // What is about to be added counts.
    assert_eq!(ledger.over(500, 100, &marks), vec![id(2)]);
    assert!(ledger.over(500, 0, &marks).is_empty());
}

#[test]
fn what_waits_to_be_deleted_is_not_counted_and_not_named_again() {
    let mut ledger = five();
    let marks = kept(&[], &[]);
    ledger.let_go(&[id(1), id(2)]);
    assert_eq!(ledger.over(250, 0, &marks), vec![id(3)]);
}

#[test]
fn letting_go_of_a_shared_id_drops_every_placement_that_holds_it() {
    let mut ledger = Ledger::default();
    ledger.put_still(placement("a"), held(1));
    ledger.put_still(placement("b"), held(1));
    ledger.weigh(id(1), 400);
    ledger.tick();
    ledger.tick();
    let ids = ledger.over(0, 0, &Kept::default());
    assert_eq!(ids, vec![id(1)]);
    ledger.let_go(&ids);
    assert!(ledger.still(&placement("a")).is_none());
    assert!(ledger.still(&placement("b")).is_none());
    assert_eq!(ledger.take_unused(), vec![id(1)]);
}
