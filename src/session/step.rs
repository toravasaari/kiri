//! The pager's step: one message in, the effects it asks for out.
//!
//! The loop in `src/pager/` waits for something to come in, such as keys,
//! the answer of a poll or the result of a reload, steps the [`Pager`] with
//! it as a [`Msg`], and carries out the [`Effect`]s the step returns, in
//! order. The step does no I/O and reads no clock: the time comes in with
//! the message, and what an effect finds out comes back as the next
//! message. So a test can drive it without a terminal.

use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::present::keys::Key;
use crate::present::kitty::graphics::ImageId;
use crate::session::frame::{Frame, Layout};
use crate::session::sources::Looked;
use crate::session::state::{FRAME, Focus, POLL, Polled, State, Told};
use crate::typeset::color::Color;
use crate::typeset::compare::Comparison;
use crate::typeset::diagnostic::Diagnostic;

/// What the pager knows of the run that does not change while it shows a
/// document.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Run {
    /// Whether there is a source to read the document again from, for `r`
    /// and for following.
    pub(crate) can_reload: bool,
    /// Whether the terminal is reached through tmux, which drops images
    /// while its pane is not shown.
    pub(crate) tmux: bool,
    /// When the pager started, from when the bands below the images that
    /// load move.
    pub(crate) started: Instant,
    /// Whether the terminal draws images, which reading mode needs.
    pub(crate) draws_images: bool,
}

/// A pager showing one layout of the document: the state that stays from
/// one layout to the next, and the layout.
pub(crate) struct Pager<'a, 'b> {
    pub(crate) state: &'a mut State,
    pub(crate) layout: &'a mut Layout<'b>,
    pub(crate) run: Run,
    /// The cells, width and height in pixels, that this layout was made
    /// for.
    pub(crate) cells: Option<(u16, u16)>,
    /// The time of the last message that has one.
    now: Instant,
    /// The frame last drawn.
    shown: Option<Rc<Frame>>,
    /// What the keys of the last read asked, until the turn is over.
    pending: Pending,
}

impl<'a, 'b> Pager<'a, 'b> {
    /// A pager that shows `layout` in `state`, for cells of `cells`
    /// pixels, as the run `run` is, at `now`.
    pub(crate) fn new(
        state: &'a mut State,
        layout: &'a mut Layout<'b>,
        run: Run,
        cells: Option<(u16, u16)>,
        now: Instant,
    ) -> Self {
        Self {
            state,
            layout,
            run,
            cells,
            now,
            shown: None,
            pending: Pending::default(),
        }
    }
}

/// What the keys of a read leave for the rest of its turn.
#[derive(Debug, Default)]
struct Pending {
    /// Whether the frame is drawn again even if it has not changed.
    again: bool,
    /// Whether images were sent again, which goes out with a check of
    /// what was sent.
    written: bool,
    /// Whether a key scrolled a band or selected another.
    bands: bool,
}

/// What comes in.
pub(crate) enum Msg {
    /// A layout was just made, and is to be shown at `now`.
    Shown(Instant),
    /// The bytes of the keys that a read gave, none when the wait ran out,
    /// at `now`.
    Input(Vec<u8>, Instant),
    /// What the looks at the watches that were due found, none when nothing
    /// was due, which [`Effect::PollWatches`] asked for.
    Changed(Vec<Looked>),
    /// Whether loads came, which [`Effect::PollLoads`] asked for.
    Loads { came: bool },
    /// Whether the window changed size, which [`Effect::PollWindow`] asked
    /// for, and whether the frame has bands, which move with each draw.
    Window {
        resized: Option<Resized>,
        bands: bool,
    },
    /// What reading the document again came to, which [`Effect::Reload`]
    /// asked for, at `now`.
    Reloaded(Reply, Instant),
}

/// The window is of another size.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Resized {
    /// Whether the window has a size at all.
    pub(crate) known: bool,
    /// Whether it tells the pixels of its cells.
    pub(crate) tells_cells: bool,
}

/// What reading the document again came to.
pub(crate) enum Reply {
    /// The document could not be read again: why, as the status line says
    /// it.
    Failed(String),
    /// It was read, with these warnings about it, and how it differs from
    /// the one before when that is known. The loop keeps the document.
    Read {
        warnings: Vec<Diagnostic>,
        changes: Option<Comparison>,
    },
}

/// What goes out. The loop carries the effects out in order. The last one
/// either asks for the next message, or ends the layout, which the loop
/// then makes again: the ones that end it are those [`Effect::ends`] tells.
pub(crate) enum Effect {
    /// Wait for keys, at most as long as [`Pager::wait`] says. The keys
    /// come back as [`Msg::Input`].
    Read,
    /// Look at what the session watches, those watches that are due, which
    /// comes back as [`Msg::Changed`].
    PollWatches,
    /// Take what loads have come, which comes back as [`Msg::Loads`].
    PollLoads,
    /// Look at the window's size, which comes back as [`Msg::Window`].
    PollWindow,
    /// Draw the frame, at this time since the start unless that is not
    /// asked, then check the images sent and flush.
    Draw(Rc<Frame>, Option<Duration>),
    /// Check the images sent, and flush.
    Check,
    /// The terminal dropped this image, whose placeholders are on the
    /// screen: send it again.
    Resend(ImageId),
    /// The terminal lost the images it was sent, as when the reader asked
    /// for them again, as `less` repaints.
    Lose,
    /// The terminal lost these images, sent while its pane was away.
    LoseSome(Vec<ImageId>),
    /// Put this on the clipboard.
    Copy(String),
    /// Stop the loads, take the images off the screen, and quit.
    Quit,
    /// The terminal answered that its cells are of this size: use it.
    SetCells((u16, u16)),
    /// Ask the terminal for the size of its cells, as the window changed.
    AskCells,
    /// Turn reading mode on or off.
    SwapReading,
    /// Switch wide blocks between wrapping and scrolling, in reading mode
    /// as it is and as `M` would turn it on.
    FlipWide,
    /// Set the page's background colour, or none for the terminal's own.
    SetBackground(Option<Color>),
    /// Read the document again; what it came to is [`Msg::Reloaded`].
    Reload,
    /// Make the layout again, with the same lines when `reuse`.
    Relayout { reuse: bool },
}

impl Effect {
    /// Whether the effect ends the layout: it is carried out when nothing
    /// is drawn any more.
    pub(crate) fn ends(&self) -> bool {
        matches!(
            self,
            Self::Quit
                | Self::SetCells(_)
                | Self::AskCells
                | Self::SwapReading
                | Self::FlipWide
                | Self::SetBackground(_)
                | Self::Reload
                | Self::Relayout { .. }
        )
    }
}

/// Takes `msg` in `pager`, and says what is to be done.
pub(crate) fn step(pager: &mut Pager, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Shown(now) => shown(pager, now),
        Msg::Input(bytes, now) => input(pager, &bytes, now),
        Msg::Changed(looks) => changed(pager, looks),
        Msg::Loads { came } => match came {
            true => loaded(pager),
            false => vec![Effect::PollWindow],
        },
        Msg::Window { resized, bands } => window(pager, resized, bands),
        Msg::Reloaded(reply, now) => reloaded(pager, reply, now),
    }
}

/// The first turn of a layout: the frame is drawn.
fn shown(pager: &mut Pager, now: Instant) -> Vec<Effect> {
    pager.now = now;
    let frame = Rc::new(pager.layout.frame(pager.state, now));
    pager.shown = Some(Rc::clone(&frame));
    let at = now.saturating_duration_since(pager.run.started);
    vec![Effect::Draw(frame, Some(at)), Effect::Read]
}

impl Pager<'_, '_> {
    /// The time of the turn: when the layout was shown, or when the last
    /// read of keys ended.
    pub(crate) fn now(&self) -> Instant {
        self.now
    }

    /// How long the pager waits for keys from now, which is when it next
    /// has something to do without any: a [`FRAME`] while something fades
    /// or `bands` move, as each frame draws a step of it, and otherwise a
    /// [`POLL`]. The loop waits this long, and a simulated one moves its
    /// clock on by it.
    pub(crate) fn wait(&self, bands: bool) -> Duration {
        match bands || self.layout.fades(self.state, self.now) {
            true => FRAME,
            false => POLL,
        }
    }
}

/// Ends the layout with `effects`: what the pending cell size answer asks
/// goes first among the ones that end it, so that they use the new cells,
/// and a reload starts from a view that shows no tint.
fn finish(pager: &mut Pager, mut effects: Vec<Effect>) -> Vec<Effect> {
    if effects
        .iter()
        .any(|effect| matches!(effect, Effect::Reload))
    {
        pager.state.changed = None;
        pager.state.follow.seen = None;
    }
    if let Some(cell) = pager.state.cell.take() {
        pager.state.changed = None;
        let at = effects
            .iter()
            .position(Effect::ends)
            .unwrap_or(effects.len());
        effects.insert(at, Effect::SetCells(cell));
    }
    effects
}

/// A read's turn: its keys, then the cells the terminal answered, and on
/// to the look at what is watched.
fn input(pager: &mut Pager, bytes: &[u8], now: Instant) -> Vec<Effect> {
    pager.now = now;
    // Nothing more came for a key the last read cut off.
    let keys = match bytes.is_empty() {
        true => pager.state.decoder.flush(),
        false => pager.state.decoder.keys(bytes),
    };
    pager.state.tick(now);
    pager.pending = Pending::default();
    let mut effects = keys_in(pager, keys, now);
    if effects.last().is_some_and(Effect::ends) {
        return effects;
    }
    if let Some(new) = pager.state.cell.take()
        && Some(new) != pager.cells
    {
        pager.state.changed = None;
        effects.push(Effect::SetCells(new));
        effects.push(Effect::Relayout { reuse: false });
    } else if pager.state.follow.on() && pager.run.can_reload {
        effects.push(Effect::PollWatches);
    } else {
        settle(pager, &mut effects);
    }
    effects
}

/// The looks at the watches that were due found `looks`. With none, as
/// the watch was looked at a moment ago, the debounce is left as it is.
fn changed(pager: &mut Pager, looks: Vec<Looked>) -> Vec<Effect> {
    let mut effects = Vec::new();
    let Some(Looked { change, .. }) = looks.into_iter().next() else {
        settle(pager, &mut effects);
        return effects;
    };
    match pager.state.follow.poll(change, pager.now) {
        Polled::Nothing => settle(pager, &mut effects),
        Polled::Reload => return finish(pager, vec![Effect::Reload]),
        Polled::Gone(why) => {
            pager.state.told = Some(Told::new(why));
            settle(pager, &mut effects);
        }
    }
    effects
}

/// Goes on from the look at the watches: the bands, and then the loads
/// that came.
fn settle(pager: &mut Pager, effects: &mut Vec<Effect>) {
    // The band selected scrolled away: the topmost on the screen takes its
    // place.
    let layout = &*pager.layout;
    let refreshed = (pager.state.bands).refresh(layout.lines, layout.places, layout.view.visible());
    if pager.pending.bands || refreshed {
        // The same lines, with the band shown anew.
        pager.state.next.keep_top = true;
        effects.extend(finish(pager, vec![Effect::Relayout { reuse: true }]));
    } else {
        effects.push(Effect::PollLoads);
    }
}

/// Images that were loading came: the tinted lines move with them, and the
/// next layout, which takes them in first, keeps its top line.
fn loaded(pager: &mut Pager) -> Vec<Effect> {
    if let Some(changed) = pager.state.changed.take() {
        let places = pager.layout.places.get(changed.lines).unwrap_or_default();
        let blocks = places.iter().flatten().cloned().collect();
        pager.state.next.tinted = Some((changed.at, blocks));
    }
    pager.state.next.keep_top = true;
    finish(pager, vec![Effect::Relayout { reuse: false }])
}

/// The last turn: if the window changed size the layout ends, and
/// otherwise the frame is drawn, if it differs from the one shown or has
/// bands.
fn window(pager: &mut Pager, resized: Option<Resized>, bands: bool) -> Vec<Effect> {
    if let Some(resized) = resized {
        // The lines laid out for the new size are others.
        pager.state.changed = None;
        let mut effects = Vec::new();
        // A new font size changes the cells, which the window does not
        // always tell: it tells none, or inside tmux it may tell tmux's
        // guess. The terminal's answer comes among the keys; until then
        // the cells known are used.
        if resized.known && (pager.run.tmux || !resized.tells_cells) {
            effects.push(Effect::AskCells);
        }
        effects.push(Effect::Relayout { reuse: false });
        return finish(pager, effects);
    }
    let now = pager.now;
    let frame = pager.layout.frame(pager.state, now);
    let mut effects = Vec::new();
    // A band on the screen moves on with each frame drawn.
    if pager.shown.as_deref() != Some(&frame) || bands || pager.pending.again {
        let at = now.saturating_duration_since(pager.run.started);
        let frame = Rc::new(frame);
        pager.shown = Some(Rc::clone(&frame));
        effects.push(Effect::Draw(frame, Some(at)));
    } else if pager.pending.written {
        effects.push(Effect::Check);
    }
    effects.push(Effect::Read);
    effects
}

/// The document was read again, or could not be, at `now`.
fn reloaded(pager: &mut Pager, reply: Reply, now: Instant) -> Vec<Effect> {
    match reply {
        Reply::Failed(why) => pager.state.told = Some(Told::new(why)),
        Reply::Read { warnings, changes } => {
            pager.state.next.changes = changes;
            pager.state.warn(warnings, now);
            pager.state.told = Some(Told::fading("reloaded", now));
        }
    }
    vec![Effect::Relayout { reuse: false }]
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
        pager.pending.again |= pressed.iter().any(|effect| matches!(effect, Effect::Lose));
        effects.extend(pressed);
        if effects.last().is_some_and(Effect::ends) {
            // The keys after it in the same read are dropped.
            pager.state.keep_answer(keys.as_slice());
            let quit = matches!(effects.last(), Some(Effect::Quit));
            return if quit {
                effects
            } else {
                finish(pager, effects)
            };
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

#[cfg(test)]
mod tests;
