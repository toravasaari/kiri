//! The hook before a wait for keys: what `before_wait` makes of a layout
//! that has work left, and of one that has none.

use super::*;
use crate::pager::idle::{Before, before_wait};
use crate::typeset::driver::{Content, laid_measured};
use crate::typeset::parse::file;

const ROWS: usize = 20;

/// A layout of a large file that has work left near its top, and the
/// layout of a small one that has none.
fn layouts() -> (Laid, Laid) {
    let inputs = Inputs::new(Capabilities::kitty(80));
    let code: String = (0..3000).map(|n| format!("fn row_{n}() {{}}\n")).collect();
    let (large, small) = (file(&code, Some("rust")), file("fn a() {}\n", Some("rust")));
    let large = laid_measured(Content::Document(&large), 0, &inputs);
    let small = laid_measured(Content::Document(&small), 0, &inputs);
    (large, small)
}

#[test]
fn a_wait_for_keys_with_work_left_and_no_key_is_idle() {
    let (large, _) = layouts();
    assert!(large.pending(layouts::near(0, ROWS)), "the layout has work");
    let mut terminal = Fake::new(80, 24, &[b""]);
    let before = before_wait(&Effect::Read, &large, (0, ROWS), &mut terminal).unwrap();
    assert!(matches!(before, Before::Idle));
    // The read did not wait.
    assert_eq!(terminal.waits, [Duration::ZERO]);
}

#[test]
fn a_wait_for_keys_with_work_left_and_a_key_gives_the_key() {
    let (large, _) = layouts();
    let mut terminal = Fake::new(80, 24, &[b"j"]);
    let before = before_wait(&Effect::Read, &large, (0, ROWS), &mut terminal).unwrap();
    match before {
        Before::Keys(Msg::Input(bytes, _)) => assert_eq!(bytes, b"j"),
        _ => panic!("the key that was waiting is the next message"),
    }
    assert_eq!(terminal.waits, [Duration::ZERO]);
}

#[test]
fn a_wait_with_no_work_left_and_an_effect_that_is_no_wait_are_carried_out() {
    let (large, small) = layouts();
    assert!(
        !small.pending(layouts::near(0, ROWS)),
        "the layout has none"
    );
    let mut terminal = Fake::new(80, 24, &[b"j"]);
    let done = before_wait(&Effect::Read, &small, (0, ROWS), &mut terminal).unwrap();
    assert!(matches!(done, Before::Carry));
    let other = before_wait(&Effect::Check, &large, (0, ROWS), &mut terminal).unwrap();
    assert!(matches!(other, Before::Carry));
    // Neither read the terminal, so the key is still there.
    assert!(terminal.waits.is_empty());
}
