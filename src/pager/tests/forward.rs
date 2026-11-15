//! Going forward again after going back: the file that going back left
//! opens again, as following a link to it does, at the place it was left.
//! The files are those a `Then` answers with, so no file system is read.

use super::opening::{BACKSPACE, ansi, click, keyed, opens, status_of, with_rows};
use super::*;
use crate::session::deck::Sheet;

const ALT_LEFT: &[u8] = b"\x1b[1;3D";
const ALT_RIGHT: &[u8] = b"\x1b[1;3C";
const BUTTON_BACK: &[u8] = b"\x1b[<128;5;5M\x1b[<128;5;5m";
const BUTTON_FORWARD: &[u8] = b"\x1b[<129;5;5M\x1b[<129;5;5m";

/// What a run of `keys` over a base that links to `docs/other.md` leaves:
/// the files that were asked for, the sheets of the deck, and the screens.
/// The other file links to `third.md` at its top; the files that open, in
/// order, are `other` and `third` as many times as `rounds` says.
fn run(keys: &[&[u8]], rounds: usize) -> (Vec<PathBuf>, Vec<Sheet>, Vec<Vec<String>>) {
    let base = with_rows("[go](docs/other.md) at the start.", 20);
    let files = (0..rounds).flat_map(|_| {
        [
            with_rows("[next](third.md) to the third.", 30),
            with_rows("Third", 30),
        ]
    });
    let mut then = opens(files);
    let mut sheets = Vec::new();
    let mut keys = keys.to_vec();
    keys.push(b"");
    let terminal = keyed(&base, (&mut ansi(), 8), &mut then, &keys, |rig| {
        sheets = rig.state.deck.sheets().collect();
    });
    let paths = then.opened.iter().map(|(path, _)| path.clone()).collect();
    (paths, sheets, terminal.screens())
}

fn bar(screen: &[String]) -> String {
    status_of(screen, 8)
}

#[test]
fn forward_after_back_opens_the_file_again_at_the_place_it_was_left() {
    let open = click(4, 1);
    let (_, _, before) = run(&[&open, b"d", b"d"], 2);
    for (back, forward) in [
        (BACKSPACE, ALT_RIGHT),
        (ALT_LEFT, BUTTON_FORWARD),
        (BUTTON_BACK, ALT_RIGHT),
    ] {
        let (paths, sheets, after) = run(&[&open, b"d", b"d", back, forward], 2);
        assert_eq!(
            paths,
            [
                PathBuf::from("docs/other.md"),
                PathBuf::from("docs/other.md")
            ]
        );
        assert_eq!(sheets.len(), 2, "{sheets:?}");
        assert_eq!(after.last(), before.last(), "{back:?} {forward:?}");
        assert!(bar(after.last().unwrap()).contains(" notes.md › other.md "));
    }
}

#[test]
fn forward_opens_a_view_that_backspace_comes_back_from() {
    let open = click(4, 1);
    let (_, sheets, _) = run(&[&open, b"d", BACKSPACE, ALT_RIGHT, BACKSPACE], 2);
    assert_eq!(sheets, [Sheet::Base]);
}

/// The keys that open the third file over the other over the base.
fn two_deep() -> Vec<Vec<u8>> {
    vec![click(4, 1), click(4, 1), b"d".to_vec()]
}

#[test]
fn forward_after_a_click_on_the_first_name_goes_forward_one_view_at_a_time() {
    let deep = two_deep();
    let mut keys: Vec<&[u8]> = deep.iter().map(Vec::as_slice).collect();
    let (_, _, whole) = run(&keys, 2);
    // `notes.md` is on columns 8 to 15 of the status line, on row 8.
    let name = click(10, 8);
    keys.push(&name);
    let (_, sheets, back) = run(&keys, 2);
    assert_eq!(sheets, [Sheet::Base]);
    assert!(!bar(back.last().unwrap()).contains('›'));
    keys.push(ALT_RIGHT);
    let (_, sheets, one) = run(&keys, 2);
    assert_eq!(sheets.len(), 2, "{sheets:?}");
    let last = bar(one.last().unwrap());
    assert!(last.contains(" notes.md › other.md "), "{last:?}");
    assert!(!last.contains("third.md"), "{last:?}");
    keys.push(BUTTON_FORWARD);
    let (paths, sheets, two) = run(&keys, 2);
    assert_eq!(sheets.len(), 3, "{sheets:?}");
    assert_eq!(
        paths.last(),
        Some(&PathBuf::from("docs/third.md")),
        "{paths:?}"
    );
    assert_eq!(two.last(), whole.last());
    // Nothing is left to go forward to.
    keys.push(ALT_RIGHT);
    let (paths_after, sheets, three) = run(&keys, 2);
    assert_eq!(paths_after, paths);
    assert_eq!(sheets.len(), 3);
    assert_eq!(three.last(), two.last());
}

#[test]
fn a_new_link_forgets_what_was_to_go_forward_to() {
    let deep = two_deep();
    let mut keys: Vec<&[u8]> = deep.iter().map(Vec::as_slice).collect();
    // Back to the base by its name leaves two files to go forward to.
    let (name, open) = (click(10, 8), click(4, 1));
    keys.extend([name.as_slice(), &open]);
    let (paths, sheets, _) = run(&keys, 2);
    assert_eq!(paths.len(), 3, "{paths:?}");
    assert_eq!(sheets.len(), 2, "{sheets:?}");
    // The link opened another view, so forward has nothing.
    keys.push(ALT_RIGHT);
    let (paths, sheets, _) = run(&keys, 2);
    assert_eq!(paths.len(), 3, "forward had nothing: {paths:?}");
    assert_eq!(sheets.len(), 2, "{sheets:?}");
}

#[test]
fn forward_with_nothing_to_go_to_does_nothing() {
    let (paths, sheets, screens) = run(&[ALT_RIGHT, BUTTON_FORWARD], 1);
    assert!(paths.is_empty(), "{paths:?}");
    assert_eq!(sheets, [Sheet::Base]);
    assert_eq!(screens.last(), screens.first());
}
