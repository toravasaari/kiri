//! The pictures behind the help darken where the terminal places images:
//! a shade is placed round the frame when the help opens, taken away when
//! it closes and placed anew when the window changes size.

use super::*;
use crate::present::kitty::graphics::SHADE_ID;

fn kitty() -> Capabilities {
    Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(60)
    }
}

fn frames_for(caps: Capabilities, keys: &[&[u8]]) -> Vec<String> {
    let mut terminal = Fake::new(60, 24, keys);
    let mut inputs = Inputs::new(caps);
    run_document(
        &numbers(30),
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    terminal.frames()
}

fn placements(frame: &str) -> usize {
    frame.matches(&format!("a=p,q=2,i={SHADE_ID},")).count()
}

#[test]
fn the_shade_is_placed_beside_the_frame_when_the_help_opens() {
    let frames = frames_for(kitty(), &[b"?"]);
    assert_eq!(placements(&frames[0]), 0, "{:?}", frames[0]);
    let open = &frames[1];
    assert_eq!(
        open.matches(&format!("a=t,q=2,f=32,o=z,s=128,v=128,i={SHADE_ID};"))
            .count(),
        1
    );
    // The help is as tall as the view: the shade is left and right of it.
    assert_eq!(placements(open), 2, "{open:?}");
    // The frame is 50 cells wide in 60 columns: 5 left and 5 right of it.
    assert!(
        open.contains(&format!(
            "\x1b[1;1H\x1b_Ga=p,q=2,i={SHADE_ID},p=1,c=5,r=23,"
        )),
        "{open:?}"
    );
    assert!(
        open.contains(&format!(
            "\x1b[1;56H\x1b_Ga=p,q=2,i={SHADE_ID},p=2,c=5,r=23,"
        )),
        "{open:?}"
    );
}

#[test]
fn the_shade_goes_when_the_help_closes() {
    let frames = frames_for(kitty(), &[b"?", b"?"]);
    let closed = &frames[2];
    assert!(
        closed.contains(&format!("a=d,d=I,i={SHADE_ID},q=2")),
        "{closed:?}"
    );
    assert_eq!(placements(closed), 0, "{closed:?}");
}

#[test]
fn a_click_outside_the_frame_takes_the_shade_away_too() {
    let frames = frames_for(kitty(), &[b"?", b"\x1b[<0;2;10M\x1b[<0;2;10m"]);
    assert!(
        frames[2].contains(&format!("a=d,d=I,i={SHADE_ID},q=2")),
        "{:?}",
        frames[2]
    );
}

#[test]
fn a_help_that_scrolls_keeps_its_shade() {
    let frames = frames_for(kitty(), &[b"?", b"j"]);
    assert_eq!(placements(&frames[2]), 0, "{:?}", frames[2]);
}

#[test]
fn the_shade_follows_a_window_that_changes() {
    let mut terminal = Fake::new(60, 24, &[b"?", b""]);
    terminal.sizes = [window(60, 24), window(70, 24)].into();
    let mut inputs = Inputs::new(kitty());
    run_document(
        &numbers(30),
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let frames = terminal.frames();
    let last = frames.last().unwrap();
    // 70 columns: 10 left and 10 right of the frame, placed again.
    assert!(
        last.contains(&format!(
            "\x1b[1;1H\x1b_Ga=p,q=2,i={SHADE_ID},p=1,c=10,r=23,"
        )),
        "{last:?}"
    );
    assert!(
        last.contains(&format!(
            "\x1b[1;61H\x1b_Ga=p,q=2,i={SHADE_ID},p=2,c=10,r=23,"
        )),
        "{last:?}"
    );
}

#[test]
fn no_shade_under_tmux_with_sixel_or_on_a_plain_terminal() {
    let tmux = Capabilities {
        tmux_passthrough: true,
        ..kitty()
    };
    for caps in [tmux, Capabilities::sixel(60), Capabilities::ansi(60)] {
        let frames = frames_for(caps, &[b"?", b"?"]);
        for frame in &frames {
            assert!(!frame.contains(&format!("i={SHADE_ID}")), "{frame:?}");
        }
    }
}
