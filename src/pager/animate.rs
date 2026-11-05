//! The pager's side of animated images: what the loop does with the
//! animations the state engine decided on (`src/session/animation.rs`),
//! which is to tell the encoder who plays them and to carry out the
//! commands it gives for a terminal that plays frames itself.

use std::io::{self, Write};

use super::Driver;
use crate::present::encoder::Command;
use crate::present::output::Writer;
use crate::session::animation::{self, Animator};
use crate::session::frame::Frame;
use crate::session::state::State;
use crate::session::step::Pager;
use crate::typeset::caps::{Capabilities, Playback};
use crate::typeset::style::Line;

/// Has `state` animate on a terminal with `caps`, each animation playing at
/// most `plays` times, 0 for as often as its file says.
pub(super) fn begin(state: &mut State, plays: u32, caps: &Capabilities) {
    state.animation = Animator::new(plays);
    state.animation.redraw(caps.playback() == Playback::Redraw);
}

/// Takes the animations among the images of the new `lines`, and has the
/// encoder send them as `state` says they play.
pub(super) fn plan<T: super::Terminal>(driver: &mut Driver<'_, '_, '_, T>, lines: &[Line]) {
    let (inputs, state) = (&*driver.tools.inputs, &mut driver.state);
    let playback = inputs.caps.playback();
    driver.tools.encoder.play(playback);
    let clips = match playback {
        Playback::None => Default::default(),
        _ => animation::clips(lines, &inputs.images),
    };
    state.animation.know(clips);
    state.links.know(state.animation.stops());
    // What the last layout left on its first frame, told like any image
    // warning, in place of what the layout before said.
    let notes = inputs.images.notes().into_iter().map(|(_, note)| note);
    state.note(notes.collect(), driver.tools.terminal.now());
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

/// Draws `frame` with the animations on the frames they are on. After it,
/// an animation that was sent anew stands on its first frame, whatever it
/// was doing, and is told to go on.
pub(super) fn draw(
    frame: &Frame,
    pager: &mut Pager,
    writer: &mut Writer,
    out: &mut impl Write,
) -> io::Result<()> {
    let now = pager.now();
    writer.show_frames(pager.state.animation.frames(now));
    super::draw_frame(frame, pager.layout.view.rows(), writer, out)?;
    for key in writer.take_fresh() {
        for command in pager.state.animation.sent(&key, now) {
            writer.control(&command, out)?;
        }
    }
    Ok(())
}
