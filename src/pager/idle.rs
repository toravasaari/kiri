//! The idle work of a layout: what the loop does while no key waits.
//!
//! A layout that measures has stretches that are not laid out yet, and it
//! colours the stretches near its view in the background. Before the loop
//! waits for keys, the layout does a piece of that work, and it does so
//! only while no key is waiting: a key that is there comes first. When the
//! piece changes a line, the layout is drawn again, and when it does not,
//! the wait goes on where it was. `before_wait` is the hook that `show`
//! calls before each wait: it decides whether the layout has work left and
//! no key waits (`Before`). The loop then does a piece of the work with
//! `layouts::went_on`, which stays in `layouts.rs`, and `turn_after` says
//! how the layout is shown again (`Again`).

use std::io;
use std::time::{Duration, Instant};

use super::{Terminal, layouts};
use crate::session::step::{Effect, Msg};
use crate::typeset::laid::Laid;

/// How a layout is shown again after its lines were laid out or a piece of
/// its work was done.
#[derive(Clone, Copy)]
pub(super) enum Again {
    /// Its lines changed: it is drawn.
    Drawn,
    /// No line changed, and less than a poll has passed since the pager
    /// last took a turn: it goes on waiting for keys.
    Wait,
    /// No line changed, and a poll has passed: the pager takes a turn, as
    /// after a wait that ran out.
    Tick,
}

/// What the layout's work comes to before a wait for keys.
pub(super) enum Before {
    /// The effect is carried out as it is: it is no wait for keys, or the
    /// layout has no work left.
    Carry,
    /// No key waits and the layout has work left: the layout ends, and the
    /// loop does a piece of the work once it is shown again.
    Idle,
    /// A key waits: it is the next message, and the effect is not carried
    /// out.
    Keys(Msg),
}

/// Before the wait for keys that `effect` asks for, the layout does what it
/// has left to do, a piece at a time, as long as no key waits. `laid` is the
/// layout of the view whose top line is `top`, in a window of `rows` rows.
pub(super) fn before_wait(
    effect: &Effect,
    laid: &Laid,
    (top, rows): (usize, usize),
    terminal: &mut impl Terminal,
) -> io::Result<Before> {
    let near = layouts::near(top, rows);
    if !(matches!(effect, Effect::Read) && laid.pending(near)) {
        return Ok(Before::Carry);
    }
    let bytes = terminal.read(Duration::ZERO)?;
    if bytes.is_empty() {
        return Ok(Before::Idle);
    }
    Ok(Before::Keys(Msg::Input(bytes, terminal.now())))
}

/// How a layout is shown again at `now`, after its lines `changed` or did
/// not: `ticked` is when the pager last took a turn, and is moved on when
/// it takes one now.
pub(super) fn turn_after(changed: bool, ticked: &mut Instant, now: Instant) -> Again {
    let due = now.saturating_duration_since(*ticked) >= crate::session::state::POLL;
    if changed || due {
        *ticked = now;
    }
    match (changed, due) {
        (true, _) => Again::Drawn,
        (false, true) => Again::Tick,
        (false, false) => Again::Wait,
    }
}
