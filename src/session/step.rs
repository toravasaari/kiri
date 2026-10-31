//! The pager's step: one message in, the effects it asks for out.
//!
//! The loop in `src/pager/` waits for something to come in, such as keys,
//! the answer of a poll or the result of a reload, steps the [`Pager`] with
//! it as a [`Msg`], and carries out the [`Effect`]s the step returns, in
//! order. The step does no I/O and reads no clock: the time comes in with
//! the message, and what an effect finds out comes back as the next
//! message. So a test can drive it without a terminal.

use std::time::Instant;

use crate::present::keys::Key;
use crate::present::kitty::graphics::ImageId;
use crate::session::frame::{Frame, Layout};
use crate::session::state::{Focus, State};

/// What the pager knows of the run that does not change while it shows a
/// document.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Run {
    /// Whether there is a source to read the document again from, for `r`
    /// and for following.
    pub(crate) can_reload: bool,
    /// Whether the terminal is reached through tmux, which drops images
    /// while its pane is not shown.
    pub(crate) tmux: bool,
}

/// A pager showing one layout of the document: the state that stays from
/// one layout to the next, and the layout.
pub(crate) struct Pager<'a, 'b> {
    pub(crate) state: &'a mut State,
    pub(crate) layout: &'a mut Layout<'b>,
    pub(crate) run: Run,
    /// What the keys of the last read asked, until the loop has done it.
    pub(crate) pending: Pending,
}

/// What the keys of a read leave for the rest of its turn.
#[derive(Debug, Default)]
pub(crate) struct Pending {
    /// Whether the frame is drawn again even if it has not changed.
    pub(crate) again: bool,
    /// Whether images were sent again, which goes out with a check of
    /// what was sent.
    pub(crate) written: bool,
    /// Whether a key scrolled a band or selected another.
    pub(crate) bands: bool,
}

/// What comes in.
pub(crate) enum Msg {
    /// These keys were read at `now`.
    Keys(Vec<Key>, Instant),
}

/// What goes out.
pub(crate) enum Effect {
    /// Quit: the loop stops the loads and leaves the screen.
    Quit,
    /// The document is to be read again.
    Reload,
    /// The screen and its images are to be sent again, as `less` repaints.
    Redraw,
    /// Reading mode is to be turned on or off.
    Reading,
    /// Wide blocks are to switch between wrapping and scrolling.
    Wide,
    /// Put this on the clipboard.
    Copy(String),
    /// Draw the frame, then check the images sent and flush; the frame
    /// shows what a key just told, before something slow starts.
    Present(Frame),
    /// The terminal dropped this image, whose placeholders are on the
    /// screen: send it again.
    Resend(ImageId),
    /// The terminal lost the images it was sent.
    Lose,
    /// The terminal lost these images, sent while its pane was away.
    LoseSome(Vec<ImageId>),
}

/// Takes `msg` in `pager`, and says what is to be done.
pub(crate) fn step(pager: &mut Pager, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Keys(keys, now) => keys_in(pager, keys, now),
    }
}

/// The effects of `keys`, read at `now`, in the order of the keys. A key
/// that ends the layout ends them: the keys after it are dropped, but the
/// last cell size answer among them is kept.
fn keys_in(pager: &mut Pager, keys: Vec<Key>, now: Instant) -> Vec<Effect> {
    let mut effects = Vec::new();
    let mut keys = keys.into_iter();
    while let Some(key) = keys.next() {
        if report(pager, key, &mut effects) {
            continue;
        }
        let pressed = (pager.state).press(key, now, pager.layout, &pager.run);
        if std::mem::take(&mut pager.state.bands_moved) {
            pager.pending.bands = true;
        }
        match pressed {
            None => {}
            Some(Effect::Redraw) => {
                effects.push(Effect::Lose);
                pager.pending.again = true;
            }
            Some(effect @ Effect::Copy(_)) => effects.push(effect),
            Some(effect) => {
                // The keys after it in the same read are dropped.
                pager.state.keep_answer(keys.as_slice());
                if matches!(effect, Effect::Reload) {
                    // The reader sees that the pager is busy before it is.
                    let frame = pager.layout.frame(pager.state, now);
                    effects.push(Effect::Present(frame));
                }
                effects.push(effect);
                return effects;
            }
        }
        pager.state.moved(pager.layout.view.top());
    }
    effects
}

/// Takes in what the terminal reports in `key`, if it is that and not a
/// key pressed: whether it was.
fn report(pager: &mut Pager, key: Key, effects: &mut Vec<Effect>) -> bool {
    match key {
        // tmux passed no images while the pane was not shown: those sent
        // since the focus went, or any when it is not known when that was.
        Key::FocusIn if pager.run.tmux => {
            match std::mem::replace(&mut pager.state.focus, Focus::In) {
                Focus::Unknown => effects.push(Effect::Lose),
                Focus::Out(sent) if !sent.is_empty() => effects.push(Effect::LoseSome(sent)),
                Focus::Out(_) | Focus::In => return true,
            }
            pager.pending.again = true;
        }
        // A second one, as from kitty and then tmux, keeps the images sent
        // while away.
        Key::FocusOut => {
            if !matches!(pager.state.focus, Focus::Out(_)) {
                pager.state.focus = Focus::Out(Vec::new());
            }
        }
        Key::FocusIn | Key::Checked(_, true) => {}
        // The last answer of the read is the cells now; the keys after it
        // still count.
        Key::CellSize(width, height) => pager.state.cell = Some((width, height)),
        // tmux dropped it: its placeholders are on the screen.
        Key::Checked(id, false) => {
            effects.push(Effect::Resend(id));
            pager.pending.written = true;
        }
        _ => return false,
    }
    true
}
