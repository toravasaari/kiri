//! The pager's side of animated images: what the loop does with the
//! animations the state engine decided on (`src/session/animation.rs`),
//! which is to tell the encoder who plays them, to carry out the commands
//! it gives for a terminal that plays frames itself, and to have the frames
//! of a picture that starts built.

use std::io::{self, Write};
use std::ops::Range;

use super::{Terminal, Tools};
use crate::present::encoder::{Command, Near};
use crate::present::output::Writer;
use crate::present::presenter::Presenter;
use crate::session::animation::{self, Pacing};
use crate::session::frame::Frame;
use crate::session::state::State;
use crate::session::step::{Msg, Pager};
use crate::typeset::caps::{Capabilities, Playback};
use crate::typeset::images::ImageKey;
use crate::typeset::style::Line;

/// How many screens above the view the budget of image memory keeps images.
const KEPT_BEHIND: usize = 1;

/// How many screens below the view it keeps them: readers scroll towards
/// the end more often than back. Two screens are 2 × the window's rows in
/// lines; `layouts::NEAR` is how far, in lines, the pager lays out round
/// the view, 256 each way, and these stay within it up to a window of 128
/// rows. Further out, lines that are only measured place no images.
const KEPT_AHEAD: usize = 2;

/// The lines of a document of `total` lines that are near a view of `rows`
/// rows, where `visible` are on the screen: [`KEPT_BEHIND`] screens above
/// it to [`KEPT_AHEAD`] below it, clamped to the document.
fn near_span(visible: &Range<usize>, rows: usize, total: usize) -> Range<usize> {
    let start = visible.start.saturating_sub(KEPT_BEHIND * rows);
    start..visible.end.saturating_add(KEPT_AHEAD * rows).min(total)
}

/// Has `state` animate on a terminal with `caps`, as `pacing` says.
pub(super) fn begin(state: &mut State, pacing: Pacing, caps: &Capabilities) {
    state.pacing = (pacing, caps.playback() == Playback::Redraw);
    state.document_mut().animation = state.animator();
}

/// Takes the animations among the images of the new `lines`, and has the
/// encoder send them as `state` says they play; what the layout says of the
/// pictures it leaves on their first frame is taken for the documents
/// numbered `numbers`, which are the view on top's.
pub(super) fn plan<T: super::Terminal>(
    tools: &mut Tools<'_, '_, '_, T>,
    state: &mut State,
    (lines, numbers): (&[Line], std::ops::Range<usize>),
) {
    let inputs = &*tools.inputs;
    let playback = inputs.caps.playback();
    tools.encoder.play(playback);
    let clips = match playback {
        Playback::None => Default::default(),
        _ => animation::clips(lines, &inputs.images),
    };
    state.document_mut().animation.know(clips);
    // What the last layout left on its first frame, told like any image
    // warning, in place of what the layout before said.
    let notes = inputs.images.notes_in(numbers);
    let notes = notes.into_iter().map(|(_, note)| note);
    state
        .document_mut()
        .note(notes.collect(), tools.terminal.now());
}

/// Carries out `commands`: tells the terminal what they say, and flushes;
/// has the frames of a picture built in `source`'s loads, or the build
/// stopped. The frames are built with the pictures' `marks` as they are
/// now, after the press that gave the commands. A picture that is shown at no
/// place drawn yet, as after a scroll the draw has not followed, is put in
/// the state's `deferred` to be built after the draw.
pub(super) fn carry(
    commands: &[Command],
    pager: &mut Pager,
    writer: &mut Writer,
    source: &mut super::Origin<'_, '_>,
    out: &mut impl Write,
) -> io::Result<()> {
    writer.show_marks(pager.state.document().animation.marks(pager.now()));
    let deferred = &mut pager.state.document_mut().deferred;
    for command in commands {
        match command {
            Command::Build(picture) => {
                deferred.retain(|key| key != picture);
                let jobs = writer.frame_jobs(picture);
                match jobs.is_empty() {
                    true => deferred.push(picture.clone()),
                    false => source.loads.build(picture.clone(), jobs),
                }
            }
            Command::Cancel(picture) => {
                deferred.retain(|key| key != picture);
                source.loads.cancel_build(picture);
            }
            command => writer.control(command, out)?,
        }
    }
    out.flush()
}

/// Stops what the animations of a view that was covered were doing:
/// tells the terminal what `commands` say, and stops the builds they
/// cancel. It is for a writer made after the view was covered.
pub(super) fn stop_below(
    commands: Vec<Command>,
    writer: &mut Writer,
    source: &mut super::Origin<'_, '_>,
    out: &mut impl Write,
) -> io::Result<()> {
    if commands.is_empty() {
        return Ok(());
    }
    for command in commands {
        match command {
            Command::Build(_) => {}
            Command::Cancel(picture) => source.loads.cancel_build(&picture),
            command => writer.control(&command, out)?,
        }
    }
    out.flush()
}

/// Takes what loads have come, and the frames that were built: the
/// frames go to the encoder, and the message tells which pictures they are
/// of, with whether they could be built.
pub(super) fn poll(source: &mut super::Origin<'_, '_>, writer: &mut Writer) -> Msg {
    source.arrived.extend(source.loads.poll());
    let came = !source.arrived.is_empty();
    let mut built = Vec::new();
    for finished in source.loads.poll_builds() {
        for (placement, frames) in finished.frames {
            writer.install_frames(placement, finished.picture.clone(), frames);
        }
        built.push((finished.picture, finished.ok));
    }
    Msg::Loads { came, built }
}

/// Draws `frame` with the animations on the frames they are on. After it,
/// an animation that was sent anew stands on its first frame, whatever it
/// was doing, and is told to go on. The frames of the pictures that waited
/// for this draw are built in `source`'s loads.
pub(super) fn draw(
    frame: &Frame,
    pager: &mut Pager,
    (writer, presenter): (&mut Writer, &mut Presenter),
    source: &mut super::Origin<'_, '_>,
    out: &mut impl Terminal,
) -> io::Result<()> {
    let now = pager.now();
    writer.show_marks(pager.state.document().animation.marks(now));
    writer.show_frames(pager.state.document().animation.frames(now));
    let scene = frame.scene(pager.layout.view.rows(), pager.layout.window());
    // The document's area is the one with images today.
    let (lines, view) = (pager.layout.lines, &pager.layout.view);
    let visible = view.visible();
    let span = near_span(&visible, view.rows(), lines.len());
    let shifted = visible.start.saturating_sub(span.start)..visible.end.saturating_sub(span.start);
    writer.near(&[Near {
        lines: &lines[span],
        visible: shifted,
        drawn: &frame.lines,
    }]);
    out.drew(&scene, &|line| writer.rows(line));
    presenter.view_at(pager.layout.view.top(), pager.layout.lines, |line| {
        writer.rows(line)
    });
    presenter.present(&scene, writer, out)?;
    for key in writer.take_fresh() {
        for command in pager.state.document_mut().animation.sent(&key, now) {
            writer.control(&command, out)?;
        }
    }
    build_deferred(&mut pager.state.document_mut().deferred, writer, source);
    Ok(())
}

/// Builds the frames of the pictures that waited for a draw, now that
/// `writer` has the places drawn: with no place still, the build comes to
/// nothing, as one that was never deferred does.
fn build_deferred(
    deferred: &mut Vec<ImageKey>,
    writer: &mut Writer,
    source: &mut super::Origin<'_, '_>,
) {
    for picture in std::mem::take(deferred) {
        let jobs = writer.frame_jobs(&picture);
        source.loads.build(picture, jobs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_is_one_screen_back_and_two_ahead_clamped_to_the_document() {
        // A window of 10 rows with lines 50 to 59 on it.
        assert_eq!(near_span(&(50..60), 10, 500), 40..80);
        assert_eq!(near_span(&(3..13), 10, 500), 0..33);
        assert_eq!(near_span(&(50..60), 10, 70), 40..70);
        assert_eq!(near_span(&(0..0), 10, 0), 0..0);
    }
}
