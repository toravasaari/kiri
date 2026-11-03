//! The pager's side of animated images: what the loop does with the
//! animations the state engine decided on (`src/session/animation.rs`),
//! which is to tell the encoder who plays them and to carry out the
//! commands it gives for a terminal that plays frames itself.

use std::io::{self, Write};

use super::Driver;
use crate::present::encoder::Command;
use crate::present::output::Writer;
use crate::session::animation::{self, Animator};
use crate::session::settings::Animations;
use crate::session::state::State;
use crate::session::step::Pager;
use crate::typeset::caps::{Capabilities, Playback};
use crate::typeset::style::Line;

/// Has `state` animate as `animations` says on a terminal with `caps`.
pub(super) fn begin(state: &mut State, animations: Animations, caps: &Capabilities) {
    state.animation = Animator::new(animations);
    state.animation.redraw(caps.playback() == Playback::Redraw);
}

/// Takes the animations among the images of the new `lines`, and has the
/// encoder send them as `state` says they play.
pub(super) fn plan<T>(driver: &mut Driver<'_, '_, '_, T>, lines: &[Line]) {
    let (inputs, state) = (&*driver.tools.inputs, &mut driver.state);
    let playback = match state.animation.setting() {
        Animations::Never => Playback::None,
        _ => inputs.caps.playback(),
    };
    driver.tools.encoder.play(playback);
    let clips = match playback {
        Playback::None => Vec::new(),
        _ => animation::clips(lines, &inputs.images),
    };
    state.animation.know(clips);
}

/// Tells the terminal what `commands` say, and flushes.
pub(super) fn carry(
    commands: &[Command],
    writer: &mut Writer,
    out: &mut impl Write,
) -> io::Result<()> {
    for command in commands {
        writer.control(command, out)?;
    }
    out.flush()
}

/// After a frame was drawn: an animation that was sent anew stands on its
/// first frame, whatever it was doing, and is told to go on.
pub(super) fn sent(pager: &mut Pager, writer: &mut Writer, out: &mut impl Write) -> io::Result<()> {
    let now = pager.now();
    for key in writer.take_fresh() {
        for command in pager.state.animation.sent(&key, now) {
            writer.control(&command, out)?;
        }
    }
    Ok(())
}
