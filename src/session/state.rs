//! What the reader does in the pager besides scrolling, which stays when
//! the window changes size: the state, and its answers to keys and to the
//! passing of time.

use std::borrow::Cow;
use std::ops::Range;
use std::time::{Duration, Instant, SystemTime};

use crate::present::keys::{Decoder, Key};
use crate::present::kitty::graphics::ImageId;
use crate::session::animation::{self, Animator, Pacing, Pause};
use crate::session::commands::{self, Action};
use crate::session::edition::{self, Edition, Held, Moves};
use crate::session::fade::{
    FADE, FADE_OUT, Fade, HIGHLIGHT_FADE, HIGHLIGHT_SHARE, HIGHLIGHT_STAY, STAY, WARNING_STAY,
};
use crate::session::files::Files;
use crate::session::frame::{Layout, Shown, map_of};
use crate::session::layers::{self, Highlights};
use crate::session::links::{Kind, Links, Placed};
use crate::session::prompt::{Edit, Prompt};
use crate::session::reveal::{Seen, Show, reveal};
use crate::session::search::Search;
use crate::session::settings;
use crate::session::settings::{FollowJump, SearchCase};
use crate::session::sources::Session;
use crate::session::step::{Effect, Run};
use crate::session::view::{Anchor, Motion, View};
use crate::session::{bands, changes, links, status};
use crate::typeset::align::Align;
use crate::typeset::block::Blocks;
use crate::typeset::color::{Color, Rgb};
use crate::typeset::compare::{self, Comparison};
use crate::typeset::diagnostic::Diagnostic;
use crate::typeset::layout::Content;
use crate::typeset::locate::Map;
use crate::typeset::model::{Document, Hashes};
use crate::typeset::position::Pos;
use crate::typeset::style::{Line, Style};
use crate::typeset::text::DocumentText;
use std::rc::Rc;

/// How long the pager waits for a key before it looks at the window size
/// again.
pub const POLL: Duration = Duration::from_millis(100);

/// How long the pager waits for a key while a fade is under way, so that
/// it draws a step of it each time.
pub const FRAME: Duration = Duration::from_millis(50);

/// What the status line says when there is no file to follow.
pub const NOTHING_TO_FOLLOW: &str = "nothing to follow";

/// Why `M` does not turn reading mode on.
pub const NEEDS_IMAGES: &str = "reading mode needs a terminal that draws images";

/// Why `B` does nothing: neither the theme nor the terminal gave a colour.
pub const NO_BACKGROUND: &str = "background colour not known";

/// Whether the document changed since it was last read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Unchanged,
    /// It changed, and looks as `Look` tells now.
    Changed(Look),
    /// It cannot be looked at, such as a file that is gone: why, as the
    /// status line says it.
    Gone(String),
}

impl Change {
    /// How a look at the document, `now`, differs from `since`, the look of
    /// the version that was read.
    pub fn between(since: Option<Look>, now: Result<Look, String>) -> Change {
        match now {
            Ok(look) if Some(look) == since => Change::Unchanged,
            Ok(look) => Change::Changed(look),
            Err(why) => Change::Gone(why),
        }
    }
}

/// What tells one version of a document from another without reading it:
/// for a file, when it was last modified and its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub modified: Option<SystemTime>,
    pub length: u64,
}

/// What the reader does in the pager besides scrolling, which stays when
/// the window changes size.
#[derive(Debug)]
pub(crate) struct State {
    /// How a search treats case.
    pub(crate) case: SearchCase,
    /// Whether the help is open.
    pub(crate) help: bool,
    /// The search prompt, while it is open.
    pub(crate) prompt: Option<Prompt>,
    /// The versions of the documents shown and what is worked out of them
    /// once for each version; `None` until the first layout.
    pub(crate) edition: Option<Edition>,
    /// The last search.
    pub(crate) search: Option<Search>,
    /// The selected link.
    pub(crate) links: Links,
    /// Whether a jump to a match scrolled or selected a band, which is
    /// shown anew once the key is done.
    pub(crate) bands_moved: bool,
    /// Whether the last search is shown; Escape hides it until `n` or
    /// `N`.
    pub(crate) shown: bool,
    /// Whether the status line tells that the last search found nothing,
    /// which it does until the next key.
    pub(crate) nowhere: bool,
    /// The last jump to a match, while its fade is under way.
    pub(crate) jumped: Option<Jump>,
    /// Whether a colon was typed, so that the next key gives a command.
    pub(crate) colon: bool,
    /// The last move to a file, while the view stays where it took it.
    pub(crate) went: Option<Went>,
    /// What the status line tells about the last key until the next one,
    /// such as that there is no next file, or until it has faded out.
    pub(crate) told: Option<Told>,
    /// Following the document as it changes.
    pub(crate) follow: Follow,
    /// The lines of the change auto-follow went to, while they are
    /// tinted.
    pub(crate) changed: Option<Changed>,
    /// The warnings about the document shown.
    pub(crate) warnings: Vec<Diagnostic>,
    /// What the last layout says of the pictures it leaves on their first
    /// frame, shown with the warnings and replaced at each layout.
    pub(crate) notes: Vec<Diagnostic>,
    /// When the warning line was last told, while it stays and fades.
    pub(crate) noticed: Option<Instant>,
    /// When the number of warnings last changed, while the number of the
    /// mark on the status line flashes.
    pub(crate) counted: Option<Instant>,
    /// Where the focus is, as the terminal last told.
    pub(crate) focus: Focus,
    /// The images sent through tmux that kitty has not yet answered for:
    /// the focus may go while they are being written, and tmux then drops
    /// the rest of them without a word.
    pub(crate) unconfirmed: Vec<ImageId>,
    /// The keys read, with what a read cut off waiting for the rest,
    /// also when a layout ends between the two reads.
    pub(crate) decoder: Decoder,
    /// How far reading mode's bands are scrolled, and which is selected.
    pub(crate) bands: bands::Bands,
    /// The cells the terminal last answered, until the step asks for them
    /// to be set (`Effect::SetCells`).
    pub(crate) cell: Option<(u16, u16)>,
    /// What the next layout keeps from this one.
    pub(crate) next: Next,
    /// The map of the lines of the layout the view was just made for, until
    /// `lay_out` hands it to that layout, which needs it too.
    map: Option<Map>,
    /// Which change following on auto moves the view to.
    pub(crate) jump: FollowJump,
    /// The page's background in use, and the one `B` turns to.
    pub(crate) background: Backgrounds,
    /// The animated images of the layout shown.
    pub(crate) animation: Animator,
    /// The pictures whose frames were to be built before the places they
    /// are shown at were drawn: built after the next draw, also when that
    /// draw is of the next layout.
    pub(crate) deferred: Vec<crate::typeset::images::ImageKey>,
}

/// The page's background as `B` turns it on and off. kitty shows its own
/// background, which may be translucent or an image, in a cell painted in
/// the very same colour, so the terminal's is painted one shade off.
#[derive(Debug, Default)]
pub(crate) struct Backgrounds {
    shown: Option<Color>,
    other: Option<Color>,
}

impl Backgrounds {
    /// For a theme whose background is `theme`, on a terminal whose
    /// background is `terminal`: `B` takes the theme's away, and gives a
    /// theme without one the terminal's, one shade off when `offset`.
    pub(crate) fn new(theme: Option<Color>, terminal: Option<Color>, offset: bool) -> Self {
        let shade = |color| match color {
            Color::Rgb(red, green, blue) if offset => {
                let off = |c: u8| if c < 128 { c + 1 } else { c - 1 };
                Color::Rgb(off(red), off(green), off(blue))
            }
            color => color,
        };
        let other = if theme.is_some() {
            None
        } else {
            terminal.map(shade)
        };
        Self {
            shown: theme,
            other,
        }
    }

    /// Whether there is a background to turn to.
    fn known(&self) -> bool {
        self.shown.is_some() || self.other.is_some()
    }

    /// Turns to the other background, and gives it.
    fn flip(&mut self) -> Option<Color> {
        std::mem::swap(&mut self.shown, &mut self.other);
        self.shown
    }
}

/// A layout as the view is made for it: the heights of its lines, the block
/// each line is of, the lines and the table of blocks, and the window's rows.
pub(crate) type Laid<'a> = (Vec<usize>, (&'a [Line], &'a Blocks), usize);

/// What the next layout keeps from the one before it.
#[derive(Debug, Default)]
pub(crate) struct Next {
    /// Whether the next layout keeps the view's top line, as when an image
    /// came in its placeholder's place.
    pub(crate) keep_top: bool,
    /// How the reloaded document differs from the one before it, until the
    /// view has found its place in the new lines.
    pub(crate) changes: Option<Comparison>,
    /// The hashes of the reloaded document, made when it was compared with
    /// the one before it, until the edition of the new version takes them.
    pub(crate) hashes: Option<Vec<Hashes>>,
    /// Where the view was when the document was read again, until the view
    /// of the new layout is put there.
    pub(crate) anchor: Option<Held<Anchor>>,
    /// The range of the search's current match then, until the search is
    /// made again.
    pub(crate) current: Option<Held<Range<Pos>>>,
}

/// Where the focus is, as the terminal tells with focus reporting: under
/// tmux, whether the pane is in the window shown.
#[derive(Debug)]
pub(crate) enum Focus {
    /// Not told yet, as when kiri started in a pane not shown.
    Unknown,
    In,
    /// Gone, with the images tmux may have dropped: those sent since, and
    /// those still unconfirmed when it went.
    Out(Vec<ImageId>),
}

/// The change auto-follow moved the view to.
#[derive(Debug, Clone)]
pub(crate) struct Changed {
    pub(crate) at: Instant,
    /// What it is of the document, which is the same in any layout of this
    /// version of it.
    pub(crate) range: Held<Range<Pos>>,
}

/// Following the document: reading it again whenever it changes.
#[derive(Debug, Default)]
pub(crate) struct Follow {
    pub(crate) mode: settings::Follow,
    /// How a change looked on the last poll, and when it was first seen
    /// looking so. It is read once a poll at least [`POLL`] later finds it
    /// looking the same, so that a file caught half written is not shown.
    pub(crate) seen: Option<(Look, Instant)>,
    /// Whether the document could not be looked at on the last poll, which
    /// the status line told.
    pub(crate) gone: bool,
}

/// What a poll of the document while it is followed calls for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Polled {
    Nothing,
    Reload,
    /// The status line is to tell why the document cannot be looked at.
    Gone(String),
}

impl Follow {
    /// Whether the document is followed, on or auto.
    pub(crate) fn on(&self) -> bool {
        self.mode != settings::Follow::Off
    }

    /// What `change`, found on a poll at `now`, calls for.
    pub(crate) fn poll(&mut self, change: Change, now: Instant) -> Polled {
        let gone = std::mem::replace(&mut self.gone, false);
        match change {
            Change::Unchanged => {
                self.seen = None;
                Polled::Nothing
            }
            Change::Changed(look) => match self.seen {
                Some((seen, at)) if seen == look => {
                    match now.saturating_duration_since(at) >= POLL {
                        true => Polled::Reload,
                        false => Polled::Nothing,
                    }
                }
                _ => {
                    self.seen = Some((look, now));
                    Polled::Nothing
                }
            },
            Change::Gone(why) => {
                self.seen = None;
                self.gone = true;
                match gone {
                    true => Polled::Nothing,
                    false => Polled::Gone(why),
                }
            }
        }
    }
}

/// A message on the status line.
#[derive(Debug)]
pub(crate) struct Told {
    pub(crate) message: String,
    /// When it was told, for a message that fades out.
    pub(crate) fades: Option<Instant>,
}

impl Told {
    /// `message`, which stays until the next key.
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            fades: None,
        }
    }

    /// `message`, told at `at`, which stays for [`STAY`] and then fades
    /// out over [`FADE_OUT`], unless a key clears it before.
    pub(crate) fn fading(message: impl Into<String>, at: Instant) -> Self {
        Self {
            fades: Some(at),
            ..Self::new(message)
        }
    }
}

/// A move to the start of a file with `:n` or `:p`. The view may stop
/// short of the start, at the last page, so the file the move went to is
/// kept until the view moves on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Went {
    pub(crate) file: usize,
    /// The top of the view after the move.
    pub(crate) top: usize,
}

/// A jump to a search match.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Jump {
    pub(crate) at: Instant,
    /// Whether the search went around an end of the document to it.
    pub(crate) around: bool,
}

impl State {
    /// Nothing done yet, with searches that treat case as `case` says.
    pub(crate) fn new(case: SearchCase) -> Self {
        Self {
            case,
            help: false,
            prompt: None,
            edition: None,
            search: None,
            links: Links::default(),
            bands_moved: false,
            shown: false,
            nowhere: false,
            jumped: None,
            colon: false,
            went: None,
            told: None,
            follow: Follow::default(),
            changed: None,
            warnings: Vec::new(),
            notes: Vec::new(),
            noticed: None,
            counted: None,
            focus: Focus::Unknown,
            unconfirmed: Vec::new(),
            decoder: Decoder::default(),
            bands: bands::Bands::default(),
            cell: None,
            next: Next::default(),
            map: None,
            jump: FollowJump::First,
            background: Backgrounds::default(),
            animation: Animator::new(Pacing::quick()),
            deferred: Vec::new(),
        }
    }

    /// The view of a new layout, of lines with these `heights` from these
    /// blocks, in a window of `rows`: `view` as it was, kept in its place,
    /// or a new one when there was none. After a reload the view stays on
    /// the block it was on, and following on auto moves on from there to a
    /// change, whose lines are tinted at `now` when the colours allow it.
    pub(crate) fn view_for(
        &mut self,
        view: Option<View>,
        (heights, (lines, blocks), rows): Laid,
        (tintable, now): (bool, Instant),
    ) -> View {
        let Some(old) = view else {
            return View::new(heights, rows);
        };
        // Lines made again for the same content keep a blank top line as it
        // is; otherwise it goes to the start of the next block.
        let exact = std::mem::take(&mut self.next.keep_top);
        let anchor = (self.edition.as_ref())
            .zip(self.next.anchor.take())
            .and_then(|(edition, held)| held.get(edition).cloned());
        // Without an anchor, as in a step test that did not lay the
        // document out through the pager, the top line stays the same line.
        let mut view = View::at(heights.clone(), rows, old.top());
        let map = &*self.map.insert(Map::new(lines, blocks));
        if let Some(anchor) = anchor {
            view.restore(&anchor, map, exact);
        }
        let Some(changes) = self.next.changes.take() else {
            return view;
        };
        // Following on auto moves on from the place kept to a change.
        let Some(edition) = &self.edition else {
            return view;
        };
        if self.follow.mode != settings::Follow::Auto {
            return view;
        }
        let align = Align::new(0, &changes, &edition.text);
        let ranges = changes::ranges(0, &changes, &align, &edition.text);
        let seen = Seen { map, lines, blocks };
        let rows = |range: &Range<Pos>| {
            let lines = seen.shown(range).map(|(_, lines)| lines);
            lines.map_or(0, |lines| heights[lines].iter().sum())
        };
        let Some(change) = changes::choose(&ranges, rows, self.jump) else {
            return view;
        };
        let bands = (&mut self.bands, edition);
        if reveal(change, Show::Change, seen, &mut view, bands).is_err() {
            return view;
        }
        // The tint is of what the change is of the document: a removal has
        // no lines to tint, and the colours may not allow a tint at all. Its
        // lines are found by `lay_out`, for this layout and each after it.
        let shown = seen.shown(change).ok().map(|(_, lines)| lines);
        if tintable && shown.is_some_and(|lines| !lines.is_empty()) {
            self.changed = Some(Changed {
                at: now,
                range: Held::new(edition, change.clone()),
            });
        }
        view
    }

    /// Notes where the view of `layout` is, and the search's current match,
    /// as places in the version shown, to be put in the layout of the next
    /// version of the document.
    pub(crate) fn keep_place(&mut self, layout: &Layout, reload: bool) {
        let Some(edition) = &self.edition else {
            return;
        };
        self.next.anchor = Some(Held::new(
            edition,
            layout.view.anchor(layout.map(), self.next.keep_top),
        ));
        // The search finds its matches again in a layout of the same
        // version, and only a new version needs its current match kept.
        if reload {
            let current = self.search.as_ref().and_then(Search::current);
            self.next.current = current.map(|found| Held::new(edition, found.range.clone()));
        }
    }

    /// Keeps the last cell size answer among the keys a read still has
    /// behind a key that ends the frame. The other keys behind it are
    /// dropped, but the answer says what the terminal is, so the next layout
    /// applies it.
    pub(crate) fn keep_answer(&mut self, rest: &[Key]) {
        let last = rest.iter().rev().find_map(|key| match key {
            Key::CellSize(width, height) => Some((*width, *height)),
            _ => None,
        });
        self.cell = last.or(self.cell);
    }

    /// Notes that the images `sent` went to the terminal, which tmux
    /// may drop while the focus is gone, and which kitty is to confirm.
    pub(crate) fn sent(&mut self, sent: Vec<ImageId>) {
        for id in sent {
            if !self.unconfirmed.contains(&id) {
                self.unconfirmed.push(id);
            }
            if let Focus::Out(away) = &mut self.focus
                && !away.contains(&id)
            {
                away.push(id);
            }
        }
    }

    /// Notes that kitty answered the check of the image `id`, that it holds
    /// it or that it does not: either way it is not for a focus-in to
    /// send again. One it does not hold is sent again at once, and noted
    /// as sent anew.
    pub(crate) fn answered(&mut self, id: ImageId) {
        self.unconfirmed.retain(|&unconfirmed| unconfirmed != id);
        if let Focus::Out(away) = &mut self.focus {
            away.retain(|&away| away != id);
        }
    }

    /// Notes that the focus went, which tmux does when it hides the pane.
    /// The images sent but not yet confirmed may have been cut by that,
    /// whether or not a frame was drawn after them. A second focus-out, as
    /// from kitty and then tmux, keeps what the first one had.
    pub(crate) fn focus_out(&mut self) {
        if !matches!(self.focus, Focus::Out(_)) {
            self.focus = Focus::Out(self.unconfirmed.clone());
        }
    }

    /// Takes `warnings`, about the document as read at `now`, for the
    /// ones before; the number of them flashes when it differs.
    pub(crate) fn warn(&mut self, warnings: Vec<Diagnostic>, now: Instant) {
        let before = self.warnings.len() + self.notes.len();
        self.warnings = warnings;
        self.warned(before, now);
    }

    /// Takes `notes`, which the last layout makes, in place of those of the
    /// layout before: what is told of the document stays as it is.
    pub(crate) fn note(&mut self, notes: Vec<Diagnostic>, now: Instant) {
        if notes == self.notes {
            return;
        }
        let before = self.warnings.len() + self.notes.len();
        self.notes = notes;
        self.warned(before, now);
    }

    /// The warnings and the notes together, as the status line tells them.
    pub(crate) fn all_warnings(&self) -> Vec<Diagnostic> {
        self.warnings.iter().chain(&self.notes).cloned().collect()
    }

    /// Flashes the number of warnings and notes when it is not `before`, and
    /// notices them when there are any.
    fn warned(&mut self, before: usize, now: Instant) {
        let count = self.warnings.len() + self.notes.len();
        if count != before {
            self.counted = Some(now);
        }
        self.noticed = (count > 0).then_some(now);
    }

    /// Does what `key`, pressed at `now`, asks in `layout` of a pager that
    /// is `run`, and tells what is left for the loop to do, if anything.
    pub(crate) fn press(
        &mut self,
        key: Key,
        now: Instant,
        layout: &mut Layout,
        run: &Run,
    ) -> Vec<Effect> {
        let mut action = commands::action(key);
        // A click and the sideways wheel act on the band under them, and
        // never reach the help, the prompt or the colon.
        if let Some(pointer) = action.filter(|action| action.is_pointer()) {
            match self.pointed(pointer, now, layout) {
                Ok(effect) => return effect,
                Err(fallback) => action = Some(fallback),
            }
        }
        // Any other mouse event does nothing at all.
        if key == Key::Mouse {
            return Vec::new();
        }
        self.nowhere = false;
        self.told = None;
        if action == Some(Action::Dismiss) {
            self.dismiss();
            return Vec::new();
        }
        // Any key closes the help and does nothing else, but Ctrl+C
        // still quits.
        if self.help && key != Key::Ctrl('c') {
            self.help = false;
            return Vec::new();
        }
        // While the prompt is open, keys edit it.
        if let Some(prompt) = &mut self.prompt {
            match prompt.edit(key) {
                Edit::Typing => {}
                Edit::Cancel => self.prompt = None,
                Edit::Search(term) => {
                    self.prompt = None;
                    self.find(&term, now, layout);
                }
                // The last search again goes on to its next match, as n
                // does.
                Edit::Repeat => {
                    self.prompt = None;
                    self.step(Search::forward, now, layout);
                }
            }
            return Vec::new();
        }
        // The key after a colon is a command's, and any other closes it.
        // The commands move between files, so a lone document has none
        // and its colon does nothing.
        if std::mem::take(&mut self.colon) {
            action = match key {
                Key::Char(c) => commands::action(Key::Command(c)),
                _ => return Vec::new(),
            };
        } else if key == commands::COMMAND && layout.status.files.several() {
            self.colon = true;
            return Vec::new();
        }
        match action {
            Some(Action::Quit) => return vec![Effect::Quit],
            // Enter copies the link selected on the screen, and otherwise
            // goes down a line.
            Some(Action::Enter) => match self.enter(layout, now) {
                Some(effects) => return effects,
                None => layout.view.scroll(Motion::Down(1)),
            },
            Some(Action::Move(motion)) => layout.view.scroll(motion),
            // The help and the prompt need a row besides the status line's.
            Some(Action::Help) => self.help = layout.status.bar,
            Some(Action::Search) => self.prompt = layout.status.bar.then(Prompt::default),
            Some(Action::Next) => self.step(Search::forward, now, layout),
            Some(Action::Previous) => self.step(Search::backward, now, layout),
            Some(Action::NextFile) => self.step_file(true, layout),
            Some(Action::PreviousFile) => self.step_file(false, layout),
            Some(Action::Reload) => return self.reload(layout, run, now),
            // As `less` repaints, also the images, which the terminal may
            // have lost.
            Some(Action::Redraw) => return vec![Effect::Lose],
            Some(Action::Follow) => self.cycle_follow(run),
            Some(Action::Reading) => return self.reading(run),
            Some(Action::Pause) => return self.pause(now, layout),
            Some(Action::Wide) => {
                return vec![Effect::FlipWide, Effect::Relayout { reuse: false }];
            }
            Some(Action::Background) if !self.background.known() => {
                self.told = Some(Told::new(NO_BACKGROUND));
            }
            Some(Action::Background) => {
                let background = self.background.flip();
                return vec![
                    Effect::SetBackground(background),
                    Effect::Relayout { reuse: false },
                ];
            }
            Some(Action::Sideways(step)) => {
                if let Some(edition) = &self.edition {
                    let shown = (layout.lines, layout.shown.blocks);
                    self.bands_moved |= self.bands.scroll(edition, shown, step);
                }
            }
            Some(Action::SelectLink { back }) => self.select_link(layout, back),
            // Answered above, or in `pointed`.
            Some(Action::Dismiss | Action::Click { .. } | Action::Wheel { .. }) | None => {}
        }
        Vec::new()
    }

    /// Answers Enter on the stop selected on the screen: an animation
    /// starts or stops, and a link's address is copied. `None` when no stop
    /// is selected on the screen.
    fn enter(&mut self, layout: &Layout, now: Instant) -> Option<Vec<Effect>> {
        let at = self.link_on_screen(layout)?;
        Some(match at.stop.kind {
            Kind::Picture { key, .. } => self.press_animation(&key, now),
            Kind::Link { url } => {
                self.told = Some(Told::fading("link copied", now));
                vec![Effect::Copy(url)]
            }
        })
    }

    /// Answers Tab, or Shift+Tab when `back`: selects the next stop, shows
    /// it whole and scrolls the band it is in to it.
    fn select_link(&mut self, layout: &mut Layout, back: bool) {
        let all = self.stops(layout);
        let Some(edition) = &self.edition else {
            return;
        };
        let visible = layout.view.visible();
        self.links.select_next(edition, &all, visible, back);
        let range = (self.links.selected(edition, &all)).map(|at| at.stop.range.clone());
        if let Some(range) = range {
            let seen = Seen {
                map: map_of(&layout.map, layout.lines, layout.shown.blocks),
                lines: layout.lines,
                blocks: layout.shown.blocks,
            };
            let bands = (&mut self.bands, edition);
            let shown = reveal(&range, Show::Whole, seen, layout.view, bands);
            self.bands_moved |= shown == Ok(true);
        }
    }

    /// Answers `r`: the reader sees that the pager is busy before it is,
    /// as the frame says it is reading.
    fn reload(&mut self, layout: &Layout, run: &Run, now: Instant) -> Vec<Effect> {
        if !run.can_reload {
            self.told = Some(Told::new("nothing to reload"));
            return Vec::new();
        }
        self.told = Some(Told::new("reloading…"));
        let frame = layout.frame(self, now);
        vec![Effect::Draw(Rc::new(frame), None), Effect::Reload]
    }

    /// Answers `p`: the animation selected, link or not, starts or stops.
    fn pause(&mut self, now: Instant, layout: &mut Layout) -> Vec<Effect> {
        let selected = self.link_on_screen(layout);
        let key = match selected {
            Some(found) => match found.stop.kind {
                Kind::Picture { key, .. } => Some(key),
                // A picture in a link: the link is the stop, and the
                // animation is on its lines.
                Kind::Link { .. } => layout.lines.get(found.lines).and_then(|lines| {
                    (animation::keys_in(lines, &self.animation).into_iter()).next()
                }),
            },
            // Nothing selected: the first animation on the screen, or else
            // the one the pill tells of, which the window then goes to.
            None => {
                let lines = layout.lines.get(layout.view.visible()).unwrap_or_default();
                match animation::keys_in(lines, &self.animation)
                    .into_iter()
                    .next()
                {
                    Some(key) => Some(key),
                    None => self.go_to_pill(layout),
                }
            }
        };
        match key {
            Some(key) => self.press_animation(&key, now),
            None => {
                self.told = Some(Told::fading("Tab selects an animation first", now));
                Vec::new()
            }
        }
    }

    /// Scrolls to the picture of the animation the pill tells of, and selects
    /// it: the animation, if there is one.
    fn go_to_pill(&mut self, layout: &mut Layout) -> Option<crate::typeset::images::ImageKey> {
        let key = self.animation.last()?.clone();
        let line = animation::first_line_of(layout.lines, &self.animation, &key)?;
        let all = self.stops(layout);
        let range = self.edition.as_ref().and_then(|edition| {
            let selected = self.links.select_picture(edition, &all, (line, &key));
            let at = self.links.selected(edition, &all).filter(|_| selected)?;
            Some(at.stop.range.clone())
        });
        match (range, &self.edition) {
            (Some(range), Some(edition)) => {
                let seen = Seen {
                    map: map_of(&layout.map, layout.lines, layout.shown.blocks),
                    lines: layout.lines,
                    blocks: layout.shown.blocks,
                };
                let bands = (&mut self.bands, edition);
                let shown = reveal(&range, Show::Line, seen, layout.view, bands);
                self.bands_moved |= shown == Ok(true);
            }
            _ => layout.view.reveal(line),
        }
        self.animation.view_moved_to_a_picture();
        Some(key)
    }

    /// Starts the animation `key`, or stops it when it plays.
    fn press_animation(
        &mut self,
        key: &crate::typeset::images::ImageKey,
        now: Instant,
    ) -> Vec<Effect> {
        let (mut commands, did) = self.animation.press(key, now);
        commands.extend(self.animation.pending());
        self.told = Some(Told::fading(
            match did {
                Pause::Stopped => "stopped",
                Pause::Played => "playing",
                Pause::Nothing => "no animation there",
            },
            now,
        ));
        match commands.is_empty() {
            true => Vec::new(),
            false => vec![Effect::Animate(commands)],
        }
    }

    /// Answers `M`: reading mode goes on or off. The loop loads what it
    /// shows that the text does not once it lays the document out again.
    fn reading(&mut self, run: &Run) -> Vec<Effect> {
        if !run.draws_images {
            self.told = Some(Told::new(NEEDS_IMAGES));
            return vec![Effect::Relayout { reuse: false }];
        }
        vec![Effect::SwapReading, Effect::Relayout { reuse: false }]
    }

    /// Answers `f`: the next way of following the document, or nothing to
    /// follow when there is no file to read again.
    fn cycle_follow(&mut self, run: &Run) {
        if !run.can_reload {
            self.told = Some(Told::new(NOTHING_TO_FOLLOW));
            return;
        }
        self.follow = Follow {
            mode: match self.follow.mode {
                settings::Follow::Off => settings::Follow::On,
                settings::Follow::On => settings::Follow::Auto,
                settings::Follow::Auto => settings::Follow::Off,
            },
            ..Follow::default()
        };
    }

    /// Answers Escape: closes what is open, the innermost first, and when
    /// nothing is open takes away the search's marks and the selected link.
    /// The jump mark, the colouring after a reload and `went` stay.
    pub(crate) fn dismiss(&mut self) {
        if self.help {
            self.help = false;
        } else if self.prompt.take().is_none() && !std::mem::take(&mut self.colon) {
            // The marks and the selection go first; with neither there to
            // go, the pill goes off the status line.
            let marked = std::mem::take(&mut self.shown);
            let selected = self.links.clear();
            if !marked && !selected {
                self.animation.hide_pill();
            }
        }
    }

    /// A click or a notch of the sideways wheel: the answer, or for the
    /// wheel with Shift away from a band the wheel's action. While the
    /// help or the prompt is open the mouse does nothing.
    fn pointed(
        &mut self,
        action: Action,
        now: Instant,
        layout: &mut Layout,
    ) -> Result<Vec<Effect>, Action> {
        // A click on the status line, where the pill is, goes to its
        // picture. Nothing tells the column, so any click on the line does.
        let on_status = layout.status.bar
            && matches!(action, Action::Click { row } if usize::from(row) == layout.view.rows() + 1);
        if on_status && !self.help && self.prompt.is_none() {
            self.go_to_pill(layout);
            return Ok(Vec::new());
        }
        let moved = match action {
            _ if self.help || self.prompt.is_some() => false,
            Action::Click { row } => {
                let line = Self::row_line(row, layout);
                // An animation under the click starts, or stops.
                if let Some(line) = line.filter(|line| layout.lines.len() > *line) {
                    let key = (self.stops(layout).into_iter()).find_map(|at| match at.stop.kind {
                        Kind::Picture { key, .. } if at.lines.contains(&line) => Some(key),
                        _ => None,
                    });
                    if let Some(key) = key {
                        let effects = self.press_animation(&key, now);
                        if !effects.is_empty() {
                            return Ok(effects);
                        }
                    }
                }
                let edition = self.edition.as_ref();
                let shown = (layout.lines, layout.shown.blocks);
                edition.is_some_and(|edition| self.bands.click(edition, shown, line))
            }
            Action::Wheel { right, shift, row } => {
                match (self.edition.as_ref()).and_then(|edition| {
                    let shown = (layout.lines, layout.shown.blocks);
                    self.bands
                        .wheel(edition, shown, Self::row_line(row, layout), right)
                }) {
                    Some(moved) => moved,
                    None if shift && right => {
                        return Err(Action::Move(Motion::Down(commands::WHEEL)));
                    }
                    None if shift => return Err(Action::Move(Motion::Up(commands::WHEEL))),
                    None => false,
                }
            }
            action => return Err(action),
        };
        self.bands_moved |= moved;
        Ok(Vec::new())
    }

    /// Moves to the start of the next file, or of the previous one.
    pub(crate) fn step_file(&mut self, next: bool, layout: &mut Layout) {
        let files = layout.status.files;
        let top = layout.view.top();
        let Some(file) = self.file(&files, top) else {
            return;
        };
        match next {
            true => self.go_to(files.next(file), "no next file", layout),
            false => self.go_to(files.previous(file, top), "no previous file", layout),
        }
    }

    /// The stops of the layout, in the order of the document: the links
    /// and the animations that no link has, which some line shows.
    pub(crate) fn stops(&self, layout: &Layout) -> Vec<Placed> {
        let Some(edition) = &self.edition else {
            return Vec::new();
        };
        let seen = layout.seen();
        let candidates = self.animation.keys();
        let pictures = |range: &Range<Pos>| {
            let key = seen.map.picture(seen.lines, range, &candidates)?;
            let name = self.animation.name_of(&key)?;
            Some((key, name.to_owned()))
        };
        let mut stops = links::stops(&edition.targets, pictures);
        stops.retain(|stop| layout.shown.links || !matches!(stop.kind, Kind::Link { .. }));
        links::placed(stops, seen)
    }

    /// The selected stop, when it is on the screen. A link without an
    /// address is never one, so a link has one to copy.
    pub(crate) fn link_on_screen(&self, layout: &Layout) -> Option<Placed> {
        let edition = self.edition.as_ref()?;
        self.links.selected.as_ref()?;
        let all = self.stops(layout);
        let found = self.links.selected(edition, &all)?;
        found.is_on(&layout.view.visible()).then(|| found.clone())
    }

    /// The line of the document on row `row` of the window, counted from 1.
    pub(crate) fn row_line(row: u16, layout: &Layout) -> Option<usize> {
        layout.view.line_at(usize::from(row).checked_sub(1)?)
    }

    /// The file the reader is in, of `files`, with the view's top on the
    /// line `top`: the one the last move to a file went to, or else the
    /// one at the top.
    pub(crate) fn file(&self, files: &Files, top: usize) -> Option<usize> {
        self.went.map(|went| went.file).or_else(|| files.at(top))
    }

    /// Moves the view to the start of `file`, when there is one to go to,
    /// and otherwise tells `none` on the status line.
    pub(crate) fn go_to(&mut self, file: Option<usize>, none: &'static str, layout: &mut Layout) {
        let Some(file) = file else {
            self.told = Some(Told::new(none));
            return;
        };
        layout.view.go_to(layout.status.files.start(file));
        self.went = Some(Went {
            file,
            top: layout.view.top(),
        });
    }

    /// Forgets the last move to a file once the view's top, now on the
    /// line `top`, has moved from where the move took it.
    pub(crate) fn moved(&mut self, top: usize) {
        self.went = self.went.filter(|went| went.top == top);
    }

    /// Searches for `term` from the top of the view at `now`, and shows
    /// the first match.
    pub(crate) fn find(&mut self, term: &str, now: Instant, layout: &mut Layout) {
        let top = layout.view.top();
        let text = self.text(&layout.shown);
        let shown = (layout.map(), layout.lines);
        let search = Search::new(shown, &text, term, self.case, top);
        self.jumped = None;
        let found = search.current().is_some();
        self.search = Some(search);
        match found {
            true => self.jump(now, false, layout),
            false => self.nowhere = true,
        }
        self.shown = true;
    }

    /// Shows the current match, jumped to at `now`, `around` an end of the
    /// document or not: a match in a band scrolls the band to it and selects
    /// it.
    pub(crate) fn jump(&mut self, now: Instant, around: bool, layout: &mut Layout) {
        self.jumped = Some(Jump { at: now, around });
        let Some(range) = (self.search.as_ref().and_then(Search::current)).map(|f| f.range.clone())
        else {
            return;
        };
        let Some(edition) = &self.edition else {
            return;
        };
        let seen = Seen {
            map: map_of(&layout.map, layout.lines, layout.shown.blocks),
            lines: layout.lines,
            blocks: layout.shown.blocks,
        };
        let bands = (&mut self.bands, edition);
        let shown = reveal(&range, Show::Line, seen, layout.view, bands);
        self.bands_moved |= shown == Ok(true);
    }

    /// Finds the last search again in `lines`, laid out anew, from the
    /// line `top` of the view, closes the prompt when the window has no
    /// status line, `bar`, to show it on, and moves the last move to a
    /// file along with the view: it is only the new layout that put the
    /// view's top on another line, not a key.
    pub(crate) fn lay_out(&mut self, layout: &Layout) {
        if !layout.status.bar {
            self.prompt = None;
        }
        let top = layout.view.top();
        if let Some(went) = &mut self.went {
            went.top = top;
        }
        // The change is a range of the document: its lines are those this
        // layout shows it on, if it shows it at all.
        if let Some(map) = self.map.take() {
            let _ = layout.map.set(map);
        }
        if let (Some(changed), Some(edition)) = (&self.changed, &self.edition) {
            let seen = layout.seen();
            let shown = changed.range.get(edition).and_then(|range| {
                let (_, lines) = seen.shown(range).ok()?;
                Some(lines).filter(|lines| !lines.is_empty())
            });
            if shown.is_none() {
                self.changed = None;
            }
        }
        if let Some(search) = self.search.take() {
            let kept = self.next.current.take();
            let current = (self.edition.as_ref())
                .zip(kept)
                .and_then(|(edition, held)| held.get(edition).cloned());
            let text = self.text(&layout.shown);
            let case = (self.case, top);
            let shown = (layout.map(), layout.lines);
            let again = search.again(shown, &text, case, current.as_ref());
            drop(text);
            self.search = Some(again);
        }
    }

    /// Takes the edition of `content` at the versions `session` has now,
    /// when the one held is of others: the hashes of a reloaded document
    /// are those that came with it, and the places kept move to the new
    /// version.
    pub(crate) fn show(&mut self, content: Content, session: &Session) {
        let versions = edition::versions(content, session);
        if (self.edition.as_ref()).is_some_and(|edition| edition.versions() == versions) {
            return;
        }
        let new = Edition::new(content, session, self.next.hashes.take());
        if let Some(old) = &self.edition {
            self.realign(&Moves::new(old, &new, self.next.changes.as_ref()), &new);
        }
        self.edition = Some(new);
    }

    /// Shows the bands of reading mode in `lines`, as scrolled and selected.
    pub(crate) fn apply_bands(&self, lines: &mut [Line], blocks: &Blocks) {
        if let Some(edition) = &self.edition {
            self.bands.apply(edition, lines, blocks);
        }
    }

    /// Selects the topmost band in `visible` when none that shows is
    /// selected; whether that changed the selection.
    pub(crate) fn refresh_bands(
        &mut self,
        lines: &[Line],
        blocks: &Blocks,
        visible: Range<usize>,
    ) -> bool {
        let edition = self.edition.as_ref();
        edition.is_some_and(|edition| self.bands.refresh(edition, (lines, blocks), visible))
    }

    /// Moves the places that are kept with their version, `State`'s
    /// aligned state, to `edition` by `moves`: those that cannot be are gone.
    fn realign(&mut self, moves: &Moves, edition: &Edition) {
        let Next {
            anchor,
            current,
            keep_top: _,
            changes: _,
            hashes: _,
        } = &mut self.next;
        *anchor = anchor.take().and_then(|held| held.align(moves, edition));
        let Links { selected } = &mut self.links;
        *selected = selected.take().and_then(|held| held.align(moves, edition));
        *current = current.take().and_then(|held| held.align(moves, edition));
        self.bands.align(moves, edition);
    }

    /// The text of what `shown` shows: the edition's, which is made once
    /// for each version, and when there is no edition yet, as in a test
    /// that does not lay a document out through the pager, made now.
    fn text(&self, shown: &Shown) -> Cow<'_, DocumentText> {
        match &self.edition {
            Some(edition) => Cow::Borrowed(&edition.text),
            None => Cow::Owned(DocumentText::new(edition::documents(shown.content))),
        }
    }

    /// How the document `new`, whose hashes are `hashes`, differs from `old`
    /// as shown, by hashes that were made once for each version.
    pub(crate) fn compared(&self, old: &Document, new: &Document, hashes: &Hashes) -> Comparison {
        match self.edition.as_ref().and_then(|edition| edition.hashes(0)) {
            Some(before) => compare::hashed(old, before, new, hashes),
            None => compare::compare(old, new),
        }
    }

    /// Moves the last search to another match by `step` at `now`, and
    /// shows it.
    pub(crate) fn step(
        &mut self,
        step: fn(&mut Search, Range<usize>) -> bool,
        now: Instant,
        layout: &mut Layout,
    ) {
        let Some(search) = &mut self.search else {
            return;
        };
        let around = step(search, layout.view.visible());
        match search.current().is_some() {
            true => self.jump(now, around, layout),
            false => self.nowhere = true,
        }
        self.shown = true;
    }

    /// Ends a fade that is over at `now`, and a message that has faded
    /// out.
    pub(crate) fn tick(&mut self, now: Instant) {
        self.jumped = self
            .jumped
            .filter(|jump| now.saturating_duration_since(jump.at) < FADE);
        self.changed = self.changed.take().filter(|changed| {
            now.saturating_duration_since(changed.at) < HIGHLIGHT_STAY + HIGHLIGHT_FADE
        });
        self.noticed = self
            .noticed
            .filter(|at| now.saturating_duration_since(*at) < WARNING_STAY + FADE);
        self.counted = self
            .counted
            .filter(|at| now.saturating_duration_since(*at) < FADE);
        self.told = self.told.take().filter(|told| {
            told.fades
                .is_none_or(|at| now.saturating_duration_since(at) < STAY + FADE_OUT)
        });
    }

    /// The fade out of the message under way at `now`, mixing colours
    /// with `rgb`.
    pub(crate) fn fade_out<'a>(&self, now: Instant, rgb: Option<&'a Rgb>) -> Option<Fade<'a>> {
        let at = self.told.as_ref()?.fades?;
        let elapsed = now.saturating_duration_since(at).checked_sub(STAY)?;
        Some(Fade { elapsed, rgb })
    }

    /// The tint at `now`: the range of the change and how much of the text
    /// colour to mix into its background: all of [`HIGHLIGHT_SHARE`] while
    /// the tint stays, then less as it fades out.
    pub(crate) fn tint(&self, now: Instant) -> Option<(Range<Pos>, f32)> {
        let changed = self.changed.as_ref()?;
        let range = changed.range.get(self.edition.as_ref()?)?;
        let elapsed = now.saturating_duration_since(changed.at);
        let left = match elapsed.checked_sub(HIGHLIGHT_STAY) {
            None => 1.0,
            Some(faded) => 1.0 - faded.as_secs_f32() / HIGHLIGHT_FADE.as_secs_f32(),
        };
        Some((range.clone(), HIGHLIGHT_SHARE * left.max(0.0)))
    }

    /// The flash of the warning count under way at `now`, mixing colours
    /// with `rgb`.
    pub(crate) fn count_fade<'a>(&self, now: Instant, rgb: Option<&'a Rgb>) -> Option<Fade<'a>> {
        let at = self.counted?;
        Some(Fade {
            elapsed: now.saturating_duration_since(at),
            rgb,
        })
    }

    /// The fade under way at `now`, mixing colours with `rgb`.
    pub(crate) fn fade<'a>(&self, now: Instant, rgb: Option<&'a Rgb>) -> Option<Fade<'a>> {
        let jump = self.jumped?;
        Some(Fade {
            elapsed: now.saturating_duration_since(jump.at),
            rgb,
        })
    }

    /// The lines `window` of `layout` with the highlights over them: the
    /// matches of the last search while it is shown, the current one faded
    /// by `fade`, the range of the change tinted at `now`, and the range of
    /// the `selected` link.
    pub(crate) fn marked(
        &self,
        layout: &Layout,
        window: Range<usize>,
        (now, fade): (Instant, Option<Fade>),
        selected: Option<Range<Pos>>,
    ) -> Vec<Line> {
        let highlights = Highlights {
            tint: layout.rgb.and_then(|_| self.tint(now)),
            search: self.search.as_ref().filter(|_| self.shown),
            fade,
            selected,
        };
        let map = layout.map();
        let set = layers::stack(&highlights, map, window.clone());
        let look = crate::typeset::layers::Look {
            marks: layout.marks,
            rgb: layout.rgb,
            columns: layout.columns,
        };
        crate::typeset::layers::paint(layout.lines, window, map, &set, &look)
    }

    /// What the status line tells about the last key, in `fading` when
    /// it is given, or else about the last search, with the match number
    /// in `flash` when it is given.
    pub(crate) fn found<'a>(
        &'a self,
        flash: Option<Style>,
        fading: Option<Style>,
    ) -> Option<status::Found<'a>> {
        if let Some(told) = &self.told {
            return Some(status::Found::Told {
                message: &told.message,
                fading,
            });
        }
        let search = self.search.as_ref()?;
        if self.nowhere {
            return Some(status::Found::Nowhere(search.term()));
        }
        if !self.shown {
            return None;
        }
        let (current, total) = search.count()?;
        Some(status::Found::At {
            term: search.term(),
            current,
            total,
            flash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::diagnostic::Code;

    fn diagnostic(text: &str) -> Diagnostic {
        Diagnostic::new(Code::Image, text)
    }

    #[test]
    fn a_reread_of_the_document_does_not_flash_the_count_of_warnings_the_layout_adds_to() {
        let (t0, t1) = (Instant::now(), Instant::now() + Duration::from_secs(5));
        let mut state = State::new(SearchCase::Smart);
        state.warn(vec![diagnostic("document")], t0);
        state.note(vec![diagnostic("note")], t0);
        assert_eq!(state.all_warnings().len(), 2);
        state.counted = None;
        // A reload reads the same document again: the same count, no flash,
        // and the layout's note is not touched.
        state.warn(vec![diagnostic("document")], t1);
        assert_eq!(state.counted, None);
        assert_eq!(state.all_warnings().len(), 2);
        // The same note from the next layout changes nothing.
        state.note(vec![diagnostic("note")], t1);
        assert_eq!(state.counted, None);
        // A layout that leaves no note takes it back, and the count flashes.
        state.note(Vec::new(), t1);
        assert_eq!(state.counted, Some(t1));
        assert_eq!(state.all_warnings(), [diagnostic("document")]);
    }

    #[test]
    fn b_gives_the_terminal_s_background_one_shade_off_when_asked() {
        let black = Some(Color::Rgb(0, 0, 0));
        assert_eq!(
            Backgrounds::new(None, black, true).flip(),
            Some(Color::Rgb(1, 1, 1))
        );
        assert_eq!(Backgrounds::new(None, black, false).flip(), black);
        // Towards the middle, so that white stays in range.
        let light = Some(Color::Rgb(255, 128, 127));
        let shifted = Some(Color::Rgb(254, 127, 128));
        assert_eq!(Backgrounds::new(None, light, true).flip(), shifted);
    }

    #[test]
    fn b_turns_back_and_forth_and_leaves_a_theme_s_background_as_it_is() {
        let mut theme =
            Backgrounds::new(Some(Color::Rgb(0, 0, 0)), Some(Color::Rgb(9, 9, 9)), true);
        assert_eq!(theme.flip(), None);
        assert_eq!(theme.flip(), Some(Color::Rgb(0, 0, 0)));
        let mut none = Backgrounds::new(None, Some(Color::Rgb(0, 0, 0)), true);
        assert_eq!(none.flip(), Some(Color::Rgb(1, 1, 1)));
        assert_eq!(none.flip(), None);
        assert_eq!(none.flip(), Some(Color::Rgb(1, 1, 1)));
    }

    fn looks(length: u64) -> Look {
        Look {
            modified: None,
            length,
        }
    }

    #[test]
    fn a_look_that_is_the_one_of_the_version_read_is_no_change() {
        let change = Change::between(Some(looks(3)), Ok(looks(3)));
        assert_eq!(change, Change::Unchanged);
    }

    #[test]
    fn another_look_is_a_change_with_that_look() {
        let change = Change::between(Some(looks(3)), Ok(looks(4)));
        assert_eq!(change, Change::Changed(looks(4)));
        let later = Look {
            modified: Some(SystemTime::UNIX_EPOCH),
            length: 3,
        };
        let change = Change::between(Some(looks(3)), Ok(later));
        assert_eq!(change, Change::Changed(later));
    }

    #[test]
    fn a_look_after_one_that_could_not_be_taken_is_a_change() {
        let change = Change::between(None, Ok(looks(3)));
        assert_eq!(change, Change::Changed(looks(3)));
    }

    #[test]
    fn a_file_that_cannot_be_looked_at_is_gone_whatever_it_looked_like() {
        let why = "No such file or directory".to_owned();
        for since in [None, Some(looks(3))] {
            let change = Change::between(since, Err(why.clone()));
            assert_eq!(change, Change::Gone(why.clone()));
        }
    }
}
