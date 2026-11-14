//! What the reader does in the pager besides scrolling, which stays when
//! the window changes size: the state, and its answers to keys and to the
//! passing of time.

use std::ops::Range;
use std::time::{Duration, Instant, SystemTime};

use crate::present::keys::{Decoder, Key};
use crate::present::kitty::graphics::ImageId;
use crate::session::animation::{self, Animator, Pacing, Pause};
use crate::session::commands::Topic;
use crate::session::commands::{self, Action};
use crate::session::deck::{Card, Deck, Routed};
use crate::session::document::{Changed, DocumentState, Jump, Next, Polls, Went};
use crate::session::edition::{self, Edition, Held, Moves};
use crate::session::fade::{
    FADE, FADE_OUT, Fade, HIGHLIGHT_FADE, HIGHLIGHT_STAY, STAY, WARNING_STAY,
};
use crate::session::follow;
use crate::session::frame::Layout;
use crate::session::history::{Back, History};
use crate::session::links::{Kind, Links};
use crate::session::panels::Help;
use crate::session::prompt::Prompt;
use crate::session::reveal::{Seen, Show, reveal};
use crate::session::search::Search;
use crate::session::settings;
use crate::session::settings::{FollowJump, SearchCase};
use crate::session::sources::{Session, ViewId};
use crate::session::step::{Effect, Run};
use crate::session::view::{Motion, View};
use crate::session::{changes, status};
use crate::typeset::align::Align;
use crate::typeset::color::{Color, Rgb};
use crate::typeset::driver::Content;
use crate::typeset::locate::Map;
use crate::typeset::position::Pos;
use crate::typeset::style::{Line, Style};
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
    /// The help, the search prompt and the colon, while they are open.
    pub(crate) deck: Deck,
    /// The state of the first document, which the deck's cards of
    /// documents are on top of.
    pub(crate) base: DocumentState,
    /// Whether the status line tells that the last search found nothing,
    /// which it does until the next key.
    pub(crate) nowhere: bool,
    /// The places the reader followed links from, which Backspace goes
    /// back to.
    pub(crate) history: History,
    /// What the status line tells about the last key until the next one,
    /// such as that there is no next file, or until it has faded out.
    pub(crate) told: Option<Told>,
    /// Following the documents as they change.
    pub(crate) follow: Follow,
    /// Where the focus is, as the terminal last told.
    pub(crate) focus: Focus,
    /// The images sent through tmux that kitty has not yet answered for:
    /// the focus may go while they are being written, and tmux then drops
    /// the rest of them without a word.
    pub(crate) unconfirmed: Vec<ImageId>,
    /// The keys read, with what a read cut off waiting for the rest,
    /// also when a layout ends between the two reads.
    pub(crate) decoder: Decoder,
    /// The cells the terminal last answered, until the step asks for them
    /// to be set (`Effect::SetCells`).
    pub(crate) cell: Option<(u16, u16)>,
    /// Which change following on auto moves the view to.
    pub(crate) jump: FollowJump,
    /// The page's background in use, and the one `B` turns to.
    pub(crate) background: Backgrounds,
    /// How the animations play, and whether a terminal that plays them
    /// by redrawing is on: what the animator of each document is made
    /// with.
    pub(crate) pacing: (Pacing, bool),
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

/// A layout as the view is made for it: the heights of its lines, the lines
/// and their map, and the window's rows.
pub(crate) type Laid<'a> = (Vec<usize>, (&'a [Line], &'a Map), usize);

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

/// Following the documents: reading one again whenever it changes. The
/// mode is the reader's, one for all views.
#[derive(Debug, Default)]
pub(crate) struct Follow {
    pub(crate) mode: settings::Follow,
}

impl Follow {
    /// Whether the documents are followed, on or auto.
    pub(crate) fn on(&self) -> bool {
        self.mode != settings::Follow::Off
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

impl State {
    /// Nothing done yet, with searches that treat case as `case` says.
    pub(crate) fn new(case: SearchCase) -> Self {
        let pacing = (Pacing::quick(), false);
        Self {
            case,
            deck: Deck::default(),
            base: DocumentState::new(ViewId::BASE, Animator::new(pacing.0)),
            nowhere: false,
            history: History::default(),
            told: None,
            follow: Follow::default(),
            focus: Focus::Unknown,
            unconfirmed: Vec::new(),
            decoder: Decoder::default(),
            cell: None,
            jump: FollowJump::First,
            background: Backgrounds::default(),
            pacing,
        }
    }

    /// The animator of a document: the way animations play, as the
    /// pager began.
    pub(crate) fn animator(&self) -> Animator {
        let mut animator = Animator::new(self.pacing.0);
        animator.redraw(self.pacing.1);
        animator
    }

    /// The state of the document at the bottom of the deck.
    pub(crate) fn document(&self) -> &DocumentState {
        &self.base
    }

    /// The state of the document at the bottom of the deck, to change.
    pub(crate) fn document_mut(&mut self) -> &mut DocumentState {
        &mut self.base
    }

    /// The view of a new layout, of lines with these `heights` and this
    /// map, in a window of `rows`: `view` as it was, kept in its place,
    /// or a new one when there was none. After a reload the view stays on
    /// the block it was on, and following on auto moves on from there to a
    /// change, whose lines are tinted at `now` when the colours allow it.
    pub(crate) fn view_for(
        &mut self,
        view: Option<View>,
        (heights, (lines, map), rows): Laid,
        (tintable, now): (bool, Instant),
    ) -> View {
        let Some(old) = view else {
            return View::new(heights, rows);
        };
        // Lines made again for the same content keep a blank top line as it
        // is; otherwise it goes to the start of the next block.
        let exact = std::mem::take(&mut self.base.next.keep_top);
        let anchor = (self.base.edition.as_ref())
            .zip(self.base.next.anchor.take())
            .and_then(|(edition, held)| held.get(edition).cloned());
        // Without an anchor, as in a step test that did not lay the
        // document out through the pager, the top line stays the same line.
        let mut view = View::at(heights.clone(), rows, old.top());
        if let Some(anchor) = anchor {
            view.restore(&anchor, map, exact);
        }
        let Some(changes) = self.base.next.changes.take() else {
            return view;
        };
        // Following on auto moves on from the place kept to a change.
        let Some(edition) = &self.base.edition else {
            return view;
        };
        if self.follow.mode != settings::Follow::Auto {
            return view;
        }
        let align = Align::new(0, &changes, &edition.text);
        let ranges = changes::ranges(0, &changes, &align, &edition.text);
        let seen = Seen { map, lines };
        let rows = |range: &Range<Pos>| {
            let lines = seen.shown(range).map(|(_, lines)| lines);
            lines.map_or(0, |lines| heights[lines].iter().sum())
        };
        let Some(change) = changes::choose(&ranges, rows, self.jump) else {
            return view;
        };
        let bands = (&mut self.base.bands, edition);
        if reveal(change, Show::Change, seen, &mut view, bands).is_err() {
            return view;
        }
        // The tint is of what the change is of the document: a removal has
        // no lines to tint, and the colours may not allow a tint at all. Its
        // lines are found by `lay_out`, for this layout and each after it.
        let shown = seen.shown(change).ok().map(|(_, lines)| lines);
        if tintable && shown.is_some_and(|lines| !lines.is_empty()) {
            self.base.changed = Some(Changed {
                at: now,
                range: Held::new(edition, change.clone()),
            });
        }
        view
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
        // A click and the sideways wheel act on the band under them; only
        // the wheel with Shift away from a band falls back to a move, which
        // only the colon answers as a key: the help and the prompt hold the
        // pointer, so it does nothing there.
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
        // A card on the deck answers the key first. The key after a colon
        // is a command's; the commands move between files, so a lone
        // document has none and its colon does nothing.
        match self.deck.press(key) {
            Routed::Taken => return Vec::new(),
            Routed::Find(term) => {
                self.find(&term, now, layout);
                return Vec::new();
            }
            Routed::Act(card_action) => action = card_action,
            Routed::Scroll(motion) => {
                if let (Some(help), Some(panel)) = (self.deck.help_mut(), layout.panel) {
                    help.scroll(motion, panel.len(), layout.view.rows());
                }
                return Vec::new();
            }
            Routed::Document if key == commands::COMMAND && layout.status.files.several() => {
                self.deck.open(Card::Colon);
                return Vec::new();
            }
            Routed::Document => {}
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
            Some(Action::Help | Action::Search) if !layout.status.bar => {}
            // The document stays where it is while the help is laid out.
            Some(Action::Help) => {
                self.deck
                    .open(Card::Help(Help::at(self.help_topic(layout))));
                self.base.next.keep_top = true;
                return vec![Effect::Relayout { reuse: true }];
            }
            Some(Action::Search) => self.deck.open(Card::Prompt(Prompt::default())),
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
                if let Some(edition) = &self.base.edition {
                    let shown = layout.map;
                    self.base.bands_moved |= self.base.bands.scroll(edition, shown, step);
                }
            }
            Some(Action::SelectLink { back }) => self.select_link(layout, back),
            Some(Action::Back) => self.back(layout),
            // Answered above, or in `pointed`.
            Some(Action::Dismiss | Action::Click { .. } | Action::Wheel { .. }) | None => {}
        }
        Vec::new()
    }

    /// Answers Enter on the stop selected on the screen: an animation
    /// starts or stops, a link to a place in a file shown goes there, and
    /// the address of another link is copied. `None` when no stop is
    /// selected on the screen.
    fn enter(&mut self, layout: &mut Layout, now: Instant) -> Option<Vec<Effect>> {
        let at = self.base.link_on_screen(layout)?;
        Some(match at.stop.kind {
            Kind::Picture { key, .. } => self.press_animation(&key, now),
            Kind::Link { url, goes, .. } => {
                let from = at.stop.range.start.file;
                let paths = self
                    .base
                    .edition
                    .as_ref()
                    .map_or(&[][..], |edition| edition.paths());
                match follow::follow((&url, &goes), from, paths) {
                    follow::Follow::Copy(address) => {
                        self.told = Some(Told::fading("link copied", now));
                        vec![Effect::Copy(address)]
                    }
                    follow::Follow::Jump { file, id } => {
                        self.go_to_target(file, id.as_deref(), layout);
                        Vec::new()
                    }
                }
            }
        })
    }

    /// Moves the view to the place named `id` in file `file`, as a browser
    /// goes to an anchor: the place is on the top line. Without an `id`
    /// it goes to the start of the file. The place the reader left is
    /// added to the history once the view got there; where it cannot go,
    /// the status line tells why and the view stays.
    fn go_to_target(&mut self, file: usize, id: Option<&str>, layout: &mut Layout) {
        let Some(edition) = &self.base.edition else {
            return;
        };
        let several = layout.status.files.several();
        let back = Back {
            view: edition.view(),
            place: Held::new(edition, layout.view.anchor(layout.map, false)),
        };
        match id {
            None => {
                let line = if several {
                    layout.status.files.start(file)
                } else {
                    0
                };
                layout.view.go_to(line);
            }
            Some(id) => {
                let Some(named) = edition.targets.named(file, id) else {
                    let place = match several {
                        true => format!(" in {}", layout.status.files.name(file)),
                        false => " here".to_owned(),
                    };
                    self.told = Some(Told::new(format!("no #{id}{place}")));
                    return;
                };
                let bands = (&mut self.base.bands, edition);
                let seen = Seen {
                    map: layout.map,
                    lines: layout.lines,
                };
                match reveal(&named.range, Show::Top, seen, layout.view, bands) {
                    Ok(scrolled) => self.base.bands_moved |= scrolled,
                    // No kind of `Hidden` reaches this today: a named range is
                    // a heading or an HTML block, whole, or an empty place in a
                    // text, and the lines show those also when a heading is
                    // drawn as an image (`a_heading_drawn_as_an_image_is_a_target_too`)
                    // or an `<a id>` is in the alt text of a picture. The branch
                    // keeps the view and the history as they are if one does.
                    Err(_) => {
                        self.told = Some(Told::new(format!("#{id} is not shown")));
                        return;
                    }
                }
            }
        }
        // As in a browser, Tab goes on from the target, not from the link
        // that was followed: the first stop on the screen is the next.
        self.base.links.selected = None;
        if several {
            self.base.went = Some(Went {
                file,
                top: layout.view.top(),
            });
        }
        self.history.push(back);
    }

    /// Answers Backspace: goes back to the place the last link was followed
    /// from, and tells when there is none.
    fn back(&mut self, layout: &mut Layout) {
        let Some(edition) = &self.base.edition else {
            return;
        };
        let place = self.history.pop().and_then(|back| {
            // Until a link opens another view, a place is in this one.
            debug_assert_eq!(back.view, edition.view());
            (back.view == edition.view()).then_some(back.place)
        });
        match place.as_ref().and_then(|place| place.get(edition)) {
            Some(anchor) => {
                layout.view.restore(anchor, layout.map, false);
                self.base.went = None;
            }
            None => self.told = Some(Told::new("nothing to go back to")),
        }
    }

    /// Answers Tab, or Shift+Tab when `back`: selects the next stop, shows
    /// it whole and scrolls the band it is in to it.
    fn select_link(&mut self, layout: &mut Layout, back: bool) {
        let all = self.base.stops(layout);
        let Some(edition) = &self.base.edition else {
            return;
        };
        let visible = layout.view.visible();
        self.base.links.select_next(edition, &all, visible, back);
        let range = (self.base.links.selected(edition, &all)).map(|at| at.stop.range.clone());
        if let Some(range) = range {
            let seen = Seen {
                map: layout.map,
                lines: layout.lines,
            };
            let bands = (&mut self.base.bands, edition);
            let shown = reveal(&range, Show::Whole, seen, layout.view, bands);
            self.base.bands_moved |= shown == Ok(true);
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
        let selected = self.base.link_on_screen(layout);
        let key = match selected {
            Some(found) => match found.stop.kind {
                Kind::Picture { key, .. } => Some(key),
                // A picture in a link: the link is the stop, and the
                // animation is on its lines.
                Kind::Link { .. } => layout.lines.get(found.lines).and_then(|lines| {
                    (animation::keys_in(lines, &self.base.animation).into_iter()).next()
                }),
            },
            // Nothing selected: the first animation on the screen, or else
            // the one the pill tells of, which the window then goes to.
            None => {
                let lines = layout.lines.get(layout.view.visible()).unwrap_or_default();
                match animation::keys_in(lines, &self.base.animation)
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
        let key = self.base.animation.last()?.clone();
        let line = animation::first_line_of(layout.lines, &self.base.animation, &key)?;
        let all = self.base.stops(layout);
        let range = self.base.edition.as_ref().and_then(|edition| {
            let selected = self.base.links.select_picture(edition, &all, (line, &key));
            let at = self
                .base
                .links
                .selected(edition, &all)
                .filter(|_| selected)?;
            Some(at.stop.range.clone())
        });
        match (range, &self.base.edition) {
            (Some(range), Some(edition)) => {
                let seen = Seen {
                    map: layout.map,
                    lines: layout.lines,
                };
                let bands = (&mut self.base.bands, edition);
                let shown = reveal(&range, Show::Line, seen, layout.view, bands);
                self.base.bands_moved |= shown == Ok(true);
            }
            _ => layout.view.reveal(line),
        }
        self.base.animation.view_moved_to_a_picture();
        Some(key)
    }

    /// Starts the animation `key`, or stops it when it plays.
    fn press_animation(
        &mut self,
        key: &crate::typeset::images::ImageKey,
        now: Instant,
    ) -> Vec<Effect> {
        let (mut commands, did) = self.base.animation.press(key, now);
        commands.extend(self.base.animation.pending());
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
        self.follow.mode = match self.follow.mode {
            settings::Follow::Off => settings::Follow::On,
            settings::Follow::On => settings::Follow::Auto,
            settings::Follow::Auto => settings::Follow::Off,
        };
        self.base.polls = Polls::default();
    }

    /// The topic the help opens at: the one for what is on the screen.
    fn help_topic(&self, layout: &Layout) -> Topic {
        if self.base.shown {
            Topic::Searching
        } else if self.base.links.selected.is_some() {
            Topic::Links
        } else if layout.status.files.several() {
            Topic::Files
        } else {
            Topic::Moving
        }
    }

    /// Answers Escape: closes what is open, the innermost first, and when
    /// nothing is open takes away the search's marks and the selected link.
    /// The jump mark, the colouring after a reload and `went` stay.
    pub(crate) fn dismiss(&mut self) {
        if !self.deck.dismiss() {
            // The marks and the selection go first; with neither there to
            // go, the pill goes off the status line.
            let marked = std::mem::take(&mut self.base.shown);
            let selected = self.base.links.clear();
            if !marked && !selected {
                self.base.animation.hide_pill();
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
        if on_status && self.deck.lets_pointer_through() {
            self.go_to_pill(layout);
            return Ok(Vec::new());
        }
        let moved = match action {
            _ if !self.deck.lets_pointer_through() => false,
            Action::Click { row } => {
                let line = Self::row_line(row, layout);
                // An animation under the click starts, or stops.
                if let Some(line) = line.filter(|line| layout.lines.len() > *line) {
                    let key =
                        (self.base.stops(layout).into_iter()).find_map(|at| match at.stop.kind {
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
                let edition = self.base.edition.as_ref();
                let shown = layout.map;
                edition.is_some_and(|edition| self.base.bands.click(edition, shown, line))
            }
            Action::Wheel { right, shift, row } => {
                match (self.base.edition.as_ref()).and_then(|edition| {
                    let shown = layout.map;
                    self.base
                        .bands
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
        self.base.bands_moved |= moved;
        Ok(Vec::new())
    }

    /// Moves to the start of the next file, or of the previous one.
    pub(crate) fn step_file(&mut self, next: bool, layout: &mut Layout) {
        let files = layout.status.files;
        let top = layout.view.top();
        let Some(file) = self.base.file(&files, top) else {
            return;
        };
        match next {
            true => self.go_to(files.next(file), "no next file", layout),
            false => self.go_to(files.previous(file, top), "no previous file", layout),
        }
    }

    /// The line of the document on row `row` of the window, counted from 1.
    pub(crate) fn row_line(row: u16, layout: &Layout) -> Option<usize> {
        layout.view.line_at(usize::from(row).checked_sub(1)?)
    }

    /// Moves the view to the start of `file`, when there is one to go to,
    /// and otherwise tells `none` on the status line.
    pub(crate) fn go_to(&mut self, file: Option<usize>, none: &'static str, layout: &mut Layout) {
        let Some(file) = file else {
            self.told = Some(Told::new(none));
            return;
        };
        layout.view.go_to(layout.status.files.start(file));
        self.base.went = Some(Went {
            file,
            top: layout.view.top(),
        });
    }

    /// Searches for `term` from the top of the view at `now`, and shows
    /// the first match.
    pub(crate) fn find(&mut self, term: &str, now: Instant, layout: &mut Layout) {
        let top = layout.view.top();
        let text = self.base.text(&layout.shown);
        let shown = (layout.map, layout.lines);
        let search = Search::new(shown, &text, term, self.case, top);
        self.base.jumped = None;
        let found = search.current().is_some();
        self.base.search = Some(search);
        match found {
            true => self.jump(now, false, layout),
            false => self.nowhere = true,
        }
        self.base.shown = true;
    }

    /// Shows the current match, jumped to at `now`, `around` an end of the
    /// document or not: a match in a band scrolls the band to it and selects
    /// it.
    pub(crate) fn jump(&mut self, now: Instant, around: bool, layout: &mut Layout) {
        self.base.jumped = Some(Jump { at: now, around });
        let Some(range) =
            (self.base.search.as_ref().and_then(Search::current)).map(|f| f.range.clone())
        else {
            return;
        };
        let Some(edition) = &self.base.edition else {
            return;
        };
        let seen = Seen {
            map: layout.map,
            lines: layout.lines,
        };
        let bands = (&mut self.base.bands, edition);
        let shown = reveal(&range, Show::Line, seen, layout.view, bands);
        self.base.bands_moved |= shown == Ok(true);
    }

    /// Finds the last search again in `lines`, laid out anew, from the
    /// line `top` of the view, closes the prompt when the window has no
    /// status line, `bar`, to show it on, and moves the last move to a
    /// file along with the view: it is only the new layout that put the
    /// view's top on another line, not a key.
    pub(crate) fn lay_out(&mut self, layout: &Layout) {
        if !layout.status.bar {
            self.deck.lose_status_line();
        }
        if let (Some(help), Some(panel)) = (self.deck.help_mut(), layout.panel) {
            help.resolve(panel, layout.view.rows());
        }
        let top = layout.view.top();
        if let Some(went) = &mut self.base.went {
            went.top = top;
        }
        // The change is a range of the document: its lines are those this
        // layout shows it on, if it shows it at all.
        if let (Some(changed), Some(edition)) = (&self.base.changed, &self.base.edition) {
            let seen = layout.seen();
            let shown = changed.range.get(edition).and_then(|range| {
                let (_, lines) = seen.shown(range).ok()?;
                Some(lines).filter(|lines| !lines.is_empty())
            });
            if shown.is_none() {
                self.base.changed = None;
            }
        }
        if let Some(search) = self.base.search.take() {
            let kept = self.base.next.current.take();
            let current = (self.base.edition.as_ref())
                .zip(kept)
                .and_then(|(edition, held)| held.get(edition).cloned());
            let text = self.base.text(&layout.shown);
            let case = (self.case, top);
            let shown = (layout.map, layout.lines);
            let again = search.again(shown, &text, case, current.as_ref());
            drop(text);
            self.base.search = Some(again);
        }
    }

    /// Takes the edition of `content` at the versions `session` has now,
    /// when the one held is of others: the hashes of a reloaded document
    /// are those that came with it, and the places kept move to the new
    /// version.
    pub(crate) fn show(&mut self, content: Content, session: &Session) {
        let view = self.base.view;
        let versions = edition::versions(content, session, view);
        if (self.base.edition.as_ref()).is_some_and(|edition| edition.versions() == versions) {
            return;
        }
        let new = Edition::new(content, session, view, self.base.next.hashes.take());
        if let Some(old) = &self.base.edition {
            self.realign(
                &Moves::new(old, &new, self.base.next.changes.as_ref()),
                &new,
            );
        }
        self.base.edition = Some(new);
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
        } = &mut self.base.next;
        *anchor = anchor.take().and_then(|held| held.align(moves, edition));
        let Links { selected } = &mut self.base.links;
        *selected = selected.take().and_then(|held| held.align(moves, edition));
        self.history.align(edition.view(), moves, edition);
        *current = current.take().and_then(|held| held.align(moves, edition));
        self.base.bands.align(moves, edition);
    }

    /// Moves the last search to another match by `step` at `now`, and
    /// shows it.
    pub(crate) fn step(
        &mut self,
        step: fn(&mut Search, Range<usize>) -> bool,
        now: Instant,
        layout: &mut Layout,
    ) {
        let Some(search) = &mut self.base.search else {
            return;
        };
        let around = step(search, layout.view.visible());
        match search.current().is_some() {
            true => self.jump(now, around, layout),
            false => self.nowhere = true,
        }
        self.base.shown = true;
    }

    /// Ends a fade that is over at `now`, and a message that has faded
    /// out.
    pub(crate) fn tick(&mut self, now: Instant) {
        self.base.jumped = self
            .base
            .jumped
            .filter(|jump| now.saturating_duration_since(jump.at) < FADE);
        self.base.changed = self.base.changed.take().filter(|changed| {
            now.saturating_duration_since(changed.at) < HIGHLIGHT_STAY + HIGHLIGHT_FADE
        });
        self.base.noticed = self
            .base
            .noticed
            .filter(|at| now.saturating_duration_since(*at) < WARNING_STAY + FADE);
        self.base.counted = self
            .base
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
        let search = self.base.search.as_ref()?;
        if self.nowhere {
            return Some(status::Found::Nowhere(search.term()));
        }
        if !self.base.shown {
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
