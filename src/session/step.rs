//! The pager's step: one message in, the effects it asks for out.
//!
//! The loop in `src/pager/` waits for something to come in, such as keys,
//! the answer of a poll or the result of a reload, steps the [`Pager`] with
//! it as a [`Msg`], and carries out the [`Effect`]s the step returns, in
//! order. The step does no I/O and reads no clock: the time comes in with
//! the message, and what an effect finds out comes back as the next
//! message. So a test can drive it without a terminal.

use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::present::keys::Key;
use crate::present::kitty::graphics::ImageId;
use crate::session::frame::{Frame, Layout};
use crate::session::state::{Change, Focus, Polled, State, Told};

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
    /// The lines in view when the loads were last told to start nearest
    /// to them.
    preferred: Range<usize>,
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
            preferred: 0..0,
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
    /// What the poll of the document found, which [`Effect::PollChange`]
    /// asked for.
    Changed(Change),
    /// Whether loads came, which [`Effect::PollLoads`] asked for.
    Loads { came: bool },
    /// Whether the window changed size, which [`Effect::PollWindow`] asked
    /// for, and whether the frame has bands, which move with each draw.
    Window { changed: bool, bands: bool },
}

/// What goes out. The loop carries the effects out in order; the last
/// one either asks for the next message or ends the layout.
pub(crate) enum Effect {
    /// Wait for keys, at most [`FRAME`](crate::session::state::FRAME) when
    /// `fading` or the frame has bands, and otherwise
    /// [`POLL`](crate::session::state::POLL). The keys come back as
    /// [`Msg::Input`].
    Read { fading: bool },
    /// Look at the document, which comes back as [`Msg::Changed`].
    PollChange,
    /// Take what loads have come, which comes back as [`Msg::Loads`].
    PollLoads,
    /// Look at the window's size, which comes back as [`Msg::Window`].
    PollWindow,
    /// Start the loads of the images nearest to these lines first.
    Prefer(Range<usize>),
    /// Draw the frame, at this time since the start unless that is not
    /// asked, then check the images sent and flush.
    Draw(Rc<Frame>, Option<Duration>),
    /// Check the images sent, and flush.
    Check,
    /// The terminal dropped this image, whose placeholders are on the
    /// screen: send it again.
    Resend(ImageId),
    /// The terminal lost the images it was sent.
    Lose,
    /// The terminal lost these images, sent while its pane was away.
    LoseSome(Vec<ImageId>),
    /// Put this on the clipboard.
    Copy(String),
    /// The screen and its images are to be sent again, as `less` repaints.
    Redraw,
    /// Quit: the loop stops the loads and leaves the screen.
    Quit,
    /// The document is to be read again.
    Reload,
    /// Reading mode is to be turned on or off.
    Reading,
    /// Wide blocks are to switch between wrapping and scrolling.
    Wide,
    /// The terminal answered that its cells are of this size.
    Cells((u16, u16)),
    /// A band is scrolled or selected otherwise: the same lines show it
    /// anew.
    Bands,
    /// Loads came.
    Loaded,
    /// The window is of another size.
    Resized,
}

impl Effect {
    /// Whether the effect ends the layout.
    pub(crate) fn ends(&self) -> bool {
        matches!(
            self,
            Self::Quit
                | Self::Reload
                | Self::Reading
                | Self::Wide
                | Self::Cells(_)
                | Self::Bands
                | Self::Loaded
                | Self::Resized
        )
    }
}

/// Takes `msg` in `pager`, and says what is to be done.
pub(crate) fn step(pager: &mut Pager, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Shown(now) => shown(pager, now),
        Msg::Input(bytes, now) => input(pager, &bytes, now),
        Msg::Changed(change) => changed(pager, change),
        Msg::Loads { came } => match came {
            true => vec![Effect::Loaded],
            false => vec![Effect::PollWindow],
        },
        Msg::Window { changed, bands } => window(pager, changed, bands),
    }
}

/// The first turn of a layout: the images in view load first, and the
/// frame is drawn.
fn shown(pager: &mut Pager, now: Instant) -> Vec<Effect> {
    pager.now = now;
    pager.preferred = pager.layout.view.visible();
    let frame = Rc::new(pager.layout.frame(pager.state, now));
    pager.shown = Some(Rc::clone(&frame));
    let at = now.saturating_duration_since(pager.run.started);
    vec![
        Effect::Prefer(pager.preferred.clone()),
        Effect::Draw(frame, Some(at)),
        wait(pager),
    ]
}

/// Waits for keys.
fn wait(pager: &Pager) -> Effect {
    Effect::Read {
        fading: pager.layout.fades(pager.state, pager.now),
    }
}

/// A read's turn: its keys, then the cells the terminal answered, and on
/// to the poll of the document.
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
        effects.push(Effect::Cells(new));
    } else if pager.state.follow.on() && pager.run.can_reload {
        effects.push(Effect::PollChange);
    } else {
        settle(pager, &mut effects);
    }
    effects
}

/// The poll of the document found `change`.
fn changed(pager: &mut Pager, change: Change) -> Vec<Effect> {
    let mut effects = Vec::new();
    match pager.state.follow.poll(change, pager.now) {
        Polled::Nothing => settle(pager, &mut effects),
        Polled::Reload => effects.push(Effect::Reload),
        Polled::Gone(why) => {
            pager.state.told = Some(Told::new(why));
            settle(pager, &mut effects);
        }
    }
    effects
}

/// Goes on from the poll of the document: the loads of the images in
/// view, the bands, and then the loads that came.
fn settle(pager: &mut Pager, effects: &mut Vec<Effect>) {
    let visible = pager.layout.view.visible();
    if visible != pager.preferred {
        pager.preferred = visible.clone();
        effects.push(Effect::Prefer(visible));
    }
    // The band selected scrolled away: the topmost on the screen takes its
    // place.
    let layout = &*pager.layout;
    let refreshed = (pager.state.bands).refresh(layout.lines, layout.places, layout.view.visible());
    match pager.pending.bands || refreshed {
        true => effects.push(Effect::Bands),
        false => effects.push(Effect::PollLoads),
    }
}

/// The last turn: if the window changed size the layout ends, and
/// otherwise the frame is drawn, if it differs from the one shown or has
/// bands.
fn window(pager: &mut Pager, changed: bool, bands: bool) -> Vec<Effect> {
    if changed {
        return vec![Effect::Resized];
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
    effects.push(wait(pager));
    effects
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
                    effects.push(Effect::Draw(Rc::new(frame), None));
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
