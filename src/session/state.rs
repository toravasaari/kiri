//! What the reader does in the pager besides scrolling, which stays when
//! the window changes size: the state, and its answers to keys and to the
//! passing of time.

use std::ops::Range;
use std::time::{Duration, Instant, SystemTime};

use crate::present::encoder::Command;
use crate::present::keys::{Decoder, Key};
use crate::present::kitty::graphics::ImageId;
use crate::session::animation::{self, Animator, Pacing, Pause};
use crate::session::commands::Topic;
use crate::session::commands::{self, Action};
use crate::session::deck::{Card, Deck, Routed, Sheet};
use crate::session::document::{Changed, DocumentState, Jump, Next, Polls, Went};
use crate::session::edition::{self, Edition, Held, Moves};
use crate::session::fade::{
    FADE, FADE_OUT, Fade, HIGHLIGHT_FADE, HIGHLIGHT_STAY, STAY, WARNING_STAY,
};
use crate::session::follow::{self, Left, Opening, Return, Turn};
use crate::session::frame::Layout;
use crate::session::history::{Ahead, Back, History};
use crate::session::links::{Kind, Links};
use crate::session::panels::{Help, Pressed};
use crate::session::prompt::Prompt;
use crate::session::reveal::{Seen, Show, reveal};
use crate::session::search::Search;
use crate::session::settings;
use crate::session::settings::{FollowJump, SearchCase};
use crate::session::sources::{Session, ViewId};
use crate::session::step::{Effect, Run};
use crate::session::view::{Anchor, Motion, View};
use crate::session::{changes, status};
use crate::typeset::align::Align;
use crate::typeset::color::{Color, Rgb};
use crate::typeset::diagnostic::Diagnostic;
use crate::typeset::driver::Content;
use crate::typeset::locate::{Hidden, Map};
use crate::typeset::position::Pos;
use crate::typeset::style::{Line, Style};
use std::path::PathBuf;
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
    /// The files going back left, which going forward opens again, the
    /// last to leave last.
    pub(crate) ahead: Vec<Ahead>,
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
    /// Whether reading mode starts headings on whole rows, as `:s` turns it.
    pub(crate) snap: bool,
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

/// The state of the document on top, borrowing the deck and the base
/// alone, so that the other fields of the state stay free to borrow.
macro_rules! doc {
    ($state:ident) => {
        top(&mut $state.deck, &mut $state.base)
    };
}

/// The state of the document on top of `deck`, or `base` when the deck
/// has none; the other fields of the `State` stay free to borrow.
fn top<'a>(deck: &'a mut Deck, base: &'a mut DocumentState) -> &'a mut DocumentState {
    deck.document_mut().unwrap_or(base)
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

/// Why a place that a link names is not reached.
enum Missing {
    /// The file has no such place.
    Named,
    /// The place is not shown, so the view cannot go to it.
    Shown,
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
            ahead: Vec::new(),
            told: None,
            follow: Follow::default(),
            focus: Focus::Unknown,
            unconfirmed: Vec::new(),
            decoder: Decoder::default(),
            cell: None,
            jump: FollowJump::First,
            background: Backgrounds::default(),
            snap: false,
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

    /// The state of the document on top: the base until the deck has
    /// documents.
    pub(crate) fn document(&self) -> &DocumentState {
        self.deck.document().unwrap_or(&self.base)
    }

    /// The state of the document on top, to change.
    pub(crate) fn document_mut(&mut self) -> &mut DocumentState {
        top(&mut self.deck, &mut self.base)
    }

    /// A view is opened over the document on top: the animations under it
    /// stop, and the builds of frames it waits for are given up, and its
    /// state, with `warnings` told at `now`, goes on the deck. Gives the
    /// commands that stop the animations, for the loop to send.
    pub(crate) fn push_view(
        &mut self,
        view: ViewId,
        warnings: Vec<Diagnostic>,
        now: Instant,
    ) -> Vec<Command> {
        let below = doc!(self);
        let mut commands = below.animation.stop();
        commands.extend(below.animation.pending());
        below.deferred.clear();
        let mut document = DocumentState::new(view, self.animator());
        document.warn(warnings, now);
        self.deck.push_document(Box::new(document));
        commands
    }

    /// The reader goes back to `view`, or to the base when it is the base:
    /// the documents above it leave the deck. Gives their views, the one
    /// that was on top first.
    pub(crate) fn back_to(&mut self, view: ViewId) -> Vec<ViewId> {
        let target = (view != self.base.view).then_some(view);
        let gone = self.deck.back_to(target);
        gone.into_iter().map(|document| document.view).collect()
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
        let doc = doc!(self);
        let Some(old) = view else {
            return View::new(heights, rows);
        };
        // Lines made again for the same content keep a blank top line as it
        // is; otherwise it goes to the start of the next block.
        let exact = std::mem::take(&mut doc.next.keep_top);
        let anchor = (doc.edition.as_ref())
            .zip(doc.next.anchor.take())
            .and_then(|(edition, held)| held.get(edition).cloned());
        // Without an anchor, as in a step test that did not lay the
        // document out through the pager, the top line stays the same line.
        let mut view = View::at(heights.clone(), rows, old.top());
        if let Some(anchor) = anchor {
            view.restore(&anchor, map, exact);
        }
        let Some(changes) = doc.next.changes.take() else {
            return view;
        };
        // Following on auto moves on from the place kept to a change.
        let Some(edition) = &doc.edition else {
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
        let bands = (&mut doc.bands, edition);
        // A change the layout has not laid out has no lines to go to yet, and
        // is kept without them, to be tinted when it has some.
        if let Err(why) = reveal(change, Show::Change, seen, &mut view, bands)
            && why != Hidden::NotLaidOut
        {
            return view;
        }
        // The tint is of what the change is of the document: a removal has
        // no lines to tint, and the colours may not allow a tint at all. Its
        // lines are found by `lay_out`, for this layout and each after it.
        if tintable && doc.shows(seen, change) {
            doc.changed = Some(Changed {
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
        if self.panel_pointer(key, layout) {
            return Vec::new();
        }
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
        if matches!(key, Key::Mouse | Key::Drag { .. } | Key::Release { .. }) {
            return Vec::new();
        }
        self.nowhere = false;
        self.told = None;
        if action == Some(Action::Dismiss) {
            self.dismiss();
            return Vec::new();
        }
        // A card on the deck answers the key first. The key after a colon
        // is a command's. A lone document has no files to move between, so
        // after its colon only the commands that are not about files act,
        // and any other key acts as it would alone: `:n` is `n`, and
        // Ctrl+C quits.
        let several = layout.status.files.several();
        let colon = matches!(self.deck.top(), Some(Card::Colon));
        match self.deck.press(key) {
            Routed::Taken if colon && !several => {}
            Routed::Taken => return Vec::new(),
            Routed::Find(term) => {
                self.find(&term, now, layout);
                return Vec::new();
            }
            Routed::Act(card_action) if colon && !several => {
                action = card_action
                    .filter(|action| !action.is_file_move())
                    .or(action);
            }
            Routed::Act(card_action) => action = card_action,
            Routed::Scroll(motion) => {
                if let (Some(help), Some(panel)) = (self.deck.panel_mut(), layout.panel) {
                    help.scroll(motion, panel.len(), layout.view.rows());
                }
                return Vec::new();
            }
            // The colon, like the help and the prompt, needs the status line.
            Routed::Document if key == commands::COMMAND && layout.status.bar => {
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
                doc!(self).next.keep_top = true;
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
            Some(Action::Snap) => return self.flip_snap(now),
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
                let doc = doc!(self);
                if let Some(edition) = &doc.edition {
                    let shown = layout.map;
                    doc.bands_moved |= doc.bands.scroll(edition, shown, step);
                }
            }
            Some(Action::SelectLink { back }) => self.select_link(layout, back),
            Some(Action::Back) => return self.back(layout),
            Some(Action::Forward) => return self.forward(layout, now),
            // Answered above, or in `pointed`.
            Some(Action::Dismiss | Action::Click { .. } | Action::Wheel { .. }) | None => {}
        }
        Vec::new()
    }

    /// Answers Enter on the stop selected on the screen: an animation
    /// starts or stops, a link to a place in a file shown goes there, a
    /// link to a Markdown file that is not shown opens it as a view, and
    /// the address of any other link is copied. `None` when no stop is
    /// selected on the screen.
    fn enter(&mut self, layout: &mut Layout, now: Instant) -> Option<Vec<Effect>> {
        let doc = doc!(self);
        let at = doc.link_on_screen(layout)?;
        Some(match at.stop.kind {
            Kind::Picture { key, .. } => self.press_animation(&key, now),
            Kind::Link { url, goes, .. } => {
                let from = at.stop.range.start.file;
                let paths = doc
                    .edition
                    .as_ref()
                    .map_or(&[][..], |edition| edition.paths());
                match follow::follow((&url, &goes), from, paths) {
                    follow::Follow::Copy(address) => {
                        self.told = Some(Told::fading("link copied", now));
                        vec![Effect::Copy(address)]
                    }
                    follow::Follow::Jump { file, id } => {
                        self.go_to_target((file, id.as_deref()), from, layout);
                        Vec::new()
                    }
                    follow::Follow::Open { path, id } => self.open((path, id), from, layout, now),
                }
            }
        })
    }

    /// Follows the link at column `column`, counted from 1, of line `line`,
    /// as Enter follows a link selected: the link is selected and the
    /// answer is Enter's. `None` when no link is there.
    fn click_link(
        &mut self,
        line: usize,
        column: u16,
        layout: &mut Layout,
        now: Instant,
    ) -> Option<Vec<Effect>> {
        let doc = doc!(self);
        let edition = doc.edition.as_ref()?;
        let seen = layout.seen();
        let place = seen
            .map
            .hit(seen.lines, line, usize::from(column).checked_sub(1)?)?;
        let at = (doc.stops(layout).into_iter()).find(|at| {
            matches!(at.stop.kind, Kind::Link { .. }) && at.stop.range.contains(&place)
        })?;
        doc.links.selected = Some(Held::new(edition, at.stop.range));
        self.enter(layout, now)
    }

    /// Moves the view to the place named `id` in file `file`, as a browser
    /// goes to an anchor: the place is on the top line. Without an `id`
    /// it goes to the start of the file. The place the reader left is
    /// added to the history once the view got there; where it cannot go,
    /// the status line tells why and the view stays.
    fn go_to_target(
        &mut self,
        (file, id): (usize, Option<&str>),
        from: usize,
        layout: &mut Layout,
    ) {
        let doc = doc!(self);
        let Some(edition) = &doc.edition else {
            return;
        };
        let several = layout.status.files.several();
        let back = Back {
            view: doc.view,
            file: from,
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
            Some(id) => match self.reach(file, id, layout) {
                Ok(()) => {}
                Err(Missing::Named) => {
                    let place = match several {
                        true => format!(" in {}", layout.status.files.name(file)),
                        false => " here".to_owned(),
                    };
                    self.told = Some(Told::new(format!("no #{id}{place}")));
                    return;
                }
                Err(Missing::Shown) => {
                    self.told = Some(Told::new(format!("#{id} is not shown")));
                    return;
                }
            },
        }
        // As in a browser, Tab goes on from the target, not from the link
        // that was followed: the first stop on the screen is the next.
        let doc = doc!(self);
        doc.links.selected = None;
        if several {
            doc.went = Some(Went {
                file,
                top: layout.view.top(),
            });
        }
        self.history.push(back);
    }

    /// Brings the view to the place named `id` in file `file`, on its top
    /// line, and says why it cannot when it cannot.
    fn reach(&mut self, file: usize, id: &str, layout: &mut Layout) -> Result<(), Missing> {
        let doc = doc!(self);
        let edition = doc.edition.as_ref().ok_or(Missing::Named)?;
        let named = edition.targets.named(file, id).ok_or(Missing::Named)?;
        let bands = (&mut doc.bands, edition);
        let seen = Seen {
            map: layout.map,
            lines: layout.lines,
        };
        match reveal(&named.range, Show::Top, seen, layout.view, bands) {
            Ok(scrolled) => {
                doc.bands_moved |= scrolled;
                Ok(())
            }
            // No kind of `Hidden` reaches this today: a named range is
            // a heading or an HTML block, whole, or an empty place in a
            // text, and the lines show those also when a heading is
            // drawn as an image (`a_heading_drawn_as_an_image_is_a_target_too`)
            // or an `<a id>` is in the alt text of a picture. The branch
            // keeps the view and the history as they are if one does.
            Err(_) => Err(Missing::Shown),
        }
    }

    /// Answers Enter on a link to a Markdown file that is not shown: the
    /// reader sees that the pager is busy, as with `r`, and the loop opens
    /// the file between layouts. The place the reader leaves comes with
    /// it, and is added to the history once the view is open.
    fn open(
        &mut self,
        (path, id): (PathBuf, Option<String>),
        from: usize,
        layout: &Layout,
        now: Instant,
    ) -> Vec<Effect> {
        let Some(back) = self.here(from, layout) else {
            return Vec::new();
        };
        let opening = Opening {
            path,
            id,
            at: None,
            forward: false,
            back,
        };
        self.opening(opening, layout, now)
    }

    /// Answers going forward: opens again the file that going back left
    /// last, at the place it was left at, as following a link to it does.
    /// Nothing happens when no file is left to go forward to.
    fn forward(&mut self, layout: &mut Layout, now: Instant) -> Vec<Effect> {
        // A view of several files has the file that is at its top.
        let from = layout.status.files.at(layout.view.top()).unwrap_or(0);
        let Some(back) = self.here(from, layout) else {
            return Vec::new();
        };
        let Some(Ahead { path, place }) = self.ahead.pop() else {
            return Vec::new();
        };
        let opening = Opening {
            path,
            id: None,
            at: place,
            forward: true,
            back,
        };
        self.opening(opening, layout, now)
    }

    /// Where the reader is in file `from` of the view on top, as a place to
    /// come back to.
    fn here(&self, from: usize, layout: &Layout) -> Option<Back> {
        let doc = self.deck.document().unwrap_or(&self.base);
        let edition = doc.edition.as_ref()?;
        Some(Back {
            view: doc.view,
            file: from,
            place: Held::new(edition, layout.view.anchor(layout.map, false)),
        })
    }

    /// Tells that the pager is busy and asks the loop to open `opening`.
    fn opening(&mut self, opening: Opening, layout: &Layout, now: Instant) -> Vec<Effect> {
        self.told = Some(Told::new(format!("opening {}…", opening.path.display())));
        let frame = layout.frame(self, now);
        vec![
            Effect::Draw(Rc::new(frame), None),
            Effect::Turn(Turn::Open(Box::new(opening))),
        ]
    }

    /// The loop tried to open the view of `opening`: the view is on the
    /// deck and the place to come back to is in the history, or the status
    /// line tells why not, with nothing added.
    pub(crate) fn opened(&mut self, opening: Opening, result: Result<ViewId, String>) {
        match result {
            Ok(_) => {
                self.told = None;
                self.history.push(opening.back);
                // A link goes on from here: what was ahead is no more.
                if !opening.forward {
                    self.ahead.clear();
                }
                let doc = doc!(self);
                doc.arrive = opening.id;
                doc.arrive_at = opening.at;
            }
            Err(why) => {
                // What is left to go forward to was ahead of the file that
                // would not open: the reader has not come to it in order.
                if opening.forward {
                    self.ahead.clear();
                }
                let path = opening.path.display();
                self.told = Some(Told::new(format!("cannot open {path}: {why}")));
            }
        }
    }

    /// The loop went back to a view, where `place` says when the history
    /// had it: the next layout puts the view there, on the line it was
    /// on, also when that is a blank one between blocks.
    pub(crate) fn returned(&mut self, place: Option<Held<Anchor>>) {
        if place.is_some() {
            let next = &mut doc!(self).next;
            next.anchor = place;
            next.keep_top = true;
        }
    }

    /// The first layout of a view that a link opened at `id`: the place is
    /// on the top line. What is not there is told, and the view stays at
    /// the start. The place the reader left was added when the view opened.
    pub(crate) fn arrive(&mut self, id: &str, layout: &mut Layout) {
        let name = layout.status.name;
        match self.reach(0, id, layout) {
            Ok(()) => {}
            Err(Missing::Named) => self.told = Some(Told::new(format!("no #{id} in {name}"))),
            Err(Missing::Shown) => self.told = Some(Told::new(format!("#{id} is not shown"))),
        }
    }

    /// Answers Backspace: goes back to the place the last link was followed
    /// from, in this view or in one below, and tells when there is none. A
    /// place in another view is the loop's to go to.
    fn back(&mut self, layout: &mut Layout) -> Vec<Effect> {
        let top = doc!(self).view;
        let mut passed = Vec::new();
        // A place of a view that is no more on the deck is passed over.
        while let Some(back) = self.history.pop() {
            if back.view == top {
                return self.back_here(back.place, layout);
            }
            if back.view == self.base.view || self.deck.holds(back.view) {
                let left = self.leaving(back.view, &passed, layout);
                let place = Some(back.place);
                let view = back.view;
                return vec![Effect::Turn(Turn::Back(Return { view, place, left }))];
            }
            passed.push(back);
        }
        // The history has run out, as it does when it keeps the last
        // hundred: an opened view goes back to the view below.
        if self.deck.document().is_some() {
            let view = self.deck.view_below().unwrap_or(self.base.view);
            let left = self.leaving(view, &passed, layout);
            let back = Return {
                view,
                place: None,
                left,
            };
            return vec![Effect::Turn(Turn::Back(back))];
        }
        self.told = Some(Told::new("nothing to go back to"));
        Vec::new()
    }

    /// The views that going back to `target` closes, the one on top first,
    /// and where each was left: the view on top where it is now, a view
    /// under it at the place the link to the next view was followed from,
    /// which is the last of `passed`, the places taken off the history, that
    /// is of it.
    fn leaving(&self, target: ViewId, passed: &[Back], layout: &Layout) -> Vec<Left> {
        let base = self.base.view;
        let views: Vec<ViewId> = (self.deck.sheets())
            .filter_map(|sheet| match sheet {
                Sheet::Base => Some(base),
                Sheet::View(view) => Some(view),
                Sheet::Help => None,
            })
            .collect();
        let Some(at) = views.iter().position(|&view| view == target) else {
            return Vec::new();
        };
        let top = views.len() - 1;
        (at + 1..views.len())
            .rev()
            .map(|index| {
                let view = views[index];
                let place = match index == top {
                    true => Some(layout.view.anchor(layout.map, false)),
                    false => passed
                        .iter()
                        .find(|back| back.view == view)
                        .map(|back| back.place.value().clone()),
                };
                Left { view, place }
            })
            .collect()
    }

    /// The loop closed the views of `left`: their files are where going
    /// forward goes, the one nearest the view the reader came back to
    /// last.
    pub(crate) fn went_back(&mut self, left: impl IntoIterator<Item = Ahead>) {
        self.ahead.extend(left);
    }

    /// Goes back to `place` in the view on top.
    fn back_here(&mut self, place: Held<Anchor>, layout: &mut Layout) -> Vec<Effect> {
        let doc = doc!(self);
        match doc.edition.as_ref().and_then(|edition| place.get(edition)) {
            Some(anchor) => {
                layout.view.restore(anchor, layout.map, false);
                doc.went = None;
            }
            None => self.told = Some(Told::new("nothing to go back to")),
        }
        Vec::new()
    }

    /// Answers Tab, or Shift+Tab when `back`: selects the next stop, shows
    /// it whole and scrolls the band it is in to it.
    fn select_link(&mut self, layout: &mut Layout, back: bool) {
        let doc = doc!(self);
        let all = doc.stops(layout);
        let Some(edition) = &doc.edition else {
            return;
        };
        let visible = layout.view.visible();
        doc.links.select_next(edition, &all, visible, back);
        let range = (doc.links.selected(edition, &all)).map(|at| at.stop.range.clone());
        if let Some(range) = range {
            let seen = Seen {
                map: layout.map,
                lines: layout.lines,
            };
            let bands = (&mut doc.bands, edition);
            let shown = reveal(&range, Show::Whole, seen, layout.view, bands);
            doc.bands_moved |= shown == Ok(true);
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
        let doc = doc!(self);
        let selected = doc.link_on_screen(layout);
        let key = match selected {
            Some(found) => match found.stop.kind {
                Kind::Picture { key, .. } => Some(key),
                // A picture in a link: the link is the stop, and the
                // animation is on its lines.
                Kind::Link { .. } => (found.lines.and_then(|lines| layout.lines.get(lines)))
                    .and_then(|lines| {
                        (animation::keys_in(lines, &doc.animation).into_iter()).next()
                    }),
            },
            // Nothing selected: the first animation on the screen, or else
            // the one the pill tells of, which the window then goes to.
            None => {
                let lines = layout.lines.get(layout.view.visible()).unwrap_or_default();
                match animation::keys_in(lines, &doc.animation).into_iter().next() {
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

    /// The file of the base view that the link was followed in, which
    /// opened the views over it: the file kept with the last place of the
    /// base in the history. `None` when there is none.
    pub(crate) fn base_file(&self) -> Option<usize> {
        Some(self.history.last_of(self.base.view)?.file)
    }

    /// Answers a click at column `column`, counted from 1, of the status
    /// line when it tells a path of views: a click on a name below the last
    /// goes back to that view, as Backspace pressed again and again would;
    /// one on the last name does nothing. `None` for a click that is on no
    /// name.
    fn click_path(
        &mut self,
        column: u16,
        layout: &mut Layout,
        now: Instant,
    ) -> Option<Vec<Effect>> {
        let base = self.base.view;
        let views: Vec<ViewId> = (self.deck.sheets())
            .filter_map(|sheet| match sheet {
                Sheet::Base => Some(base),
                Sheet::View(view) => Some(view),
                Sheet::Help => None,
            })
            .collect();
        // What is drawn tells the names, whatever it left out for room.
        let names = layout.frame(self, now).path;
        let column = usize::from(column);
        let at = names.iter().find(|name| name.cells.contains(&column))?.view;
        let target = *views.get(at)?;
        if at + 1 == views.len() {
            return Some(Vec::new());
        }
        // The place is the one Backspace would come to: the history is
        // taken off down to the first place of that view, and what the views
        // above it had is passed over.
        let mut place = None;
        let mut passed = Vec::new();
        while let Some(back) = self.history.pop() {
            if back.view == target {
                place = Some(back.place);
                break;
            }
            passed.push(back);
        }
        let left = self.leaving(target, &passed, layout);
        let back = Return {
            view: target,
            place,
            left,
        };
        Some(vec![Effect::Turn(Turn::Back(back))])
    }

    /// Scrolls to the picture of the animation the pill tells of, and selects
    /// it: the animation, if there is one.
    fn go_to_pill(&mut self, layout: &mut Layout) -> Option<crate::typeset::images::ImageKey> {
        let doc = doc!(self);
        let key = doc.animation.last()?.clone();
        let line = animation::first_line_of(layout.lines, &doc.animation, &key)?;
        let all = doc.stops(layout);
        let range = doc.edition.as_ref().and_then(|edition| {
            let selected = doc.links.select_picture(edition, &all, (line, &key));
            let at = doc.links.selected(edition, &all).filter(|_| selected)?;
            Some(at.stop.range.clone())
        });
        match (range, &doc.edition) {
            (Some(range), Some(edition)) => {
                let seen = Seen {
                    map: layout.map,
                    lines: layout.lines,
                };
                let bands = (&mut doc.bands, edition);
                let shown = reveal(&range, Show::Line, seen, layout.view, bands);
                doc.bands_moved |= shown == Ok(true);
            }
            _ => layout.view.reveal(line),
        }
        doc.animation.view_moved_to_a_picture();
        Some(key)
    }

    /// Starts the animation `key`, or stops it when it plays.
    fn press_animation(
        &mut self,
        key: &crate::typeset::images::ImageKey,
        now: Instant,
    ) -> Vec<Effect> {
        let (mut commands, did) = doc!(self).animation.press(key, now);
        commands.extend(doc!(self).animation.pending());
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

    /// Answers `:s`: headings on whole rows in reading mode, or off them,
    /// and a word on the status line for a while about which it is now.
    fn flip_snap(&mut self, now: Instant) -> Vec<Effect> {
        self.snap = !self.snap;
        let said = if self.snap { "on" } else { "off" };
        self.told = Some(Told::fading(format!("headings on rows: {said}"), now));
        vec![
            Effect::SetSnap(self.snap),
            Effect::Relayout { reuse: false },
        ]
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
        doc!(self).polls = Polls::default();
    }

    /// The topic the help opens at: the one for what is on the screen.
    fn help_topic(&self, layout: &Layout) -> Topic {
        if self.document().shown {
            Topic::Searching
        } else if self.document().links.selected.is_some() {
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
            let marked = std::mem::take(&mut doc!(self).shown);
            let selected = doc!(self).links.clear();
            if !marked && !selected {
                doc!(self).animation.hide_pill();
            }
        }
    }

    /// The left button's press, motion and release while a panel is open:
    /// the panel's bar and frame answer them, and a press outside the
    /// frame closes the panel and does nothing else. `false` for any
    /// other key, and with no panel open.
    fn panel_pointer(&mut self, key: Key, layout: &Layout) -> bool {
        let Some(shape) = layout.panel_shape().filter(|_| self.deck.panel().is_some()) else {
            return false;
        };
        let Some(help) = self.deck.panel_mut() else {
            return false;
        };
        match key {
            Key::Click { column, row } => {
                let at = (usize::from(column), usize::from(row));
                if help.press(&shape, at) == Pressed::Outside {
                    self.deck.dismiss();
                }
            }
            Key::Drag { row, .. } => help.drag(&shape, usize::from(row)),
            Key::Release { .. } => help.release(),
            _ => return false,
        }
        self.nowhere = false;
        self.told = None;
        true
    }

    /// A click or a notch of the sideways wheel: the answer, or for the
    /// wheel with Shift away from a band the wheel's action. While the
    /// help or the prompt is open these do nothing here; the help's frame
    /// answers the left button first (`panel_pointer`).
    fn pointed(
        &mut self,
        action: Action,
        now: Instant,
        layout: &mut Layout,
    ) -> Result<Vec<Effect>, Action> {
        let through = self.deck.lets_pointer_through();
        // A click on a name of the path of views goes back to that view;
        // any other click on the status line, where the pill is, goes to
        // its picture.
        let on_status = layout.status.bar
            && matches!(action, Action::Click { row, .. } if usize::from(row) == layout.view.rows() + 1);
        if on_status && through {
            if let Action::Click { column, .. } = action
                && let Some(effects) = self.click_path(column, layout, now)
            {
                return Ok(effects);
            }
            self.go_to_pill(layout);
            return Ok(Vec::new());
        }
        let moved = match action {
            _ if !through => false,
            Action::Click { column, row } => {
                let raw = Self::row_line(row, layout);
                let line = raw.filter(|line| layout.lines.len() > *line);
                // A link under the click is followed, as Enter follows it:
                // the column decides, also on the row of an animation.
                if let Some(line) = line
                    && let Some(effects) = self.click_link(line, column, layout, now)
                {
                    return Ok(effects);
                }
                // An animation on the row starts, or stops.
                if let Some(line) = line {
                    let key =
                        (doc!(self).stops(layout).into_iter()).find_map(|at| match at.stop.kind {
                            Kind::Picture { key, .. }
                                if (at.lines.as_ref())
                                    .is_some_and(|lines| lines.contains(&line)) =>
                            {
                                Some(key)
                            }
                            _ => None,
                        });
                    if let Some(key) = key {
                        let effects = self.press_animation(&key, now);
                        if !effects.is_empty() {
                            return Ok(effects);
                        }
                    }
                }
                let doc = doc!(self);
                let edition = doc.edition.as_ref();
                let shown = layout.map;
                edition.is_some_and(|edition| doc.bands.click(edition, shown, raw))
            }
            Action::Wheel { right, shift, row } => {
                let doc = doc!(self);
                match (doc.edition.as_ref()).and_then(|edition| {
                    let shown = layout.map;
                    doc.bands
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
        doc!(self).bands_moved |= moved;
        Ok(Vec::new())
    }

    /// Moves to the start of the next file, or of the previous one.
    pub(crate) fn step_file(&mut self, next: bool, layout: &mut Layout) {
        let doc = doc!(self);
        let files = layout.status.files;
        let top = layout.view.top();
        let Some(file) = doc.file(&files, top) else {
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
        doc!(self).went = Some(Went {
            file,
            top: layout.view.top(),
        });
    }

    /// Searches for `term` from the top of the view at `now`, and shows
    /// the first match.
    pub(crate) fn find(&mut self, term: &str, now: Instant, layout: &mut Layout) {
        let top = layout.view.top();
        let text = doc!(self).text(&layout.shown);
        let shown = (layout.map, layout.lines);
        let search = Search::new(shown, &text, term, self.case, top);
        doc!(self).jumped = None;
        let found = search.current().is_some();
        doc!(self).search = Some(search);
        match found {
            true => self.jump(now, false, layout),
            false => self.nowhere = true,
        }
        doc!(self).shown = true;
    }

    /// Shows the current match, jumped to at `now`, `around` an end of the
    /// document or not: a match in a band scrolls the band to it and selects
    /// it.
    pub(crate) fn jump(&mut self, now: Instant, around: bool, layout: &mut Layout) {
        let doc = doc!(self);
        doc.jumped = Some(Jump { at: now, around });
        let Some(range) = (doc.search.as_ref().and_then(Search::current)).map(|f| f.range.clone())
        else {
            return;
        };
        let Some(edition) = &doc.edition else {
            return;
        };
        let seen = Seen {
            map: layout.map,
            lines: layout.lines,
        };
        let bands = (&mut doc.bands, edition);
        let shown = reveal(&range, Show::Line, seen, layout.view, bands);
        doc.bands_moved |= shown == Ok(true);
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
        if let (Some(help), Some(panel)) = (self.deck.panel_mut(), layout.panel) {
            help.resolve(panel, layout.view.rows());
        }
        let doc = doc!(self);
        let top = layout.view.top();
        if let Some(went) = &mut doc.went {
            went.top = top;
        }
        // The change is a range of the document: its lines are those this
        // layout shows it on, if it shows it at all.
        doc.keep_change(layout.seen());
        if let Some(search) = doc.search.take() {
            let kept = doc.next.current.take();
            let current = (doc.edition.as_ref())
                .zip(kept)
                .and_then(|(edition, held)| held.get(edition).cloned());
            let text = doc.text(&layout.shown);
            let case = (self.case, top);
            let shown = (layout.map, layout.lines);
            let again = search.again(shown, &text, case, current.as_ref());
            drop(text);
            doc.search = Some(again);
        }
    }

    /// Goes on after the layout `shown` was laid out further than it was when
    /// the search and the change were last found in it: the matches that had
    /// no line get theirs, and a change that no line shows is gone. The text
    /// is not searched again.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the pager lays out in steps from G1.11.7 on")
    )]
    pub(crate) fn extended(&mut self, shown: (&Map, &[Line])) {
        let doc = doc!(self);
        doc.keep_change(Seen {
            map: shown.0,
            lines: shown.1,
        });
        if let Some(search) = &mut doc.search {
            search.extend(shown);
        }
    }

    /// Takes the edition of `content` at the versions `session` has now,
    /// when the one held is of others: the hashes of a reloaded document
    /// are those that came with it, and the places kept move to the new
    /// version.
    pub(crate) fn show(&mut self, content: Content, session: &Session) {
        let doc = doc!(self);
        let view = doc.view;
        let versions = edition::versions(content, session, view);
        if (doc.edition.as_ref()).is_some_and(|edition| edition.versions() == versions) {
            return;
        }
        let new = Edition::new(content, session, view, doc.next.hashes.take());
        let moves =
            (doc.edition.as_ref()).map(|old| Moves::new(old, &new, doc.next.changes.as_ref()));
        if let Some(moves) = moves {
            self.realign(&moves, &new);
        }
        doc!(self).edition = Some(new);
    }

    /// Moves the places that are kept with their version, `State`'s
    /// aligned state, to `edition` by `moves`: those that cannot be are gone.
    fn realign(&mut self, moves: &Moves, edition: &Edition) {
        let doc = doc!(self);
        let Next {
            anchor,
            current,
            keep_top: _,
            changes: _,
            hashes: _,
        } = &mut doc.next;
        *anchor = anchor.take().and_then(|held| held.align(moves, edition));
        let Links { selected } = &mut doc.links;
        *selected = selected.take().and_then(|held| held.align(moves, edition));
        self.history.align(edition.view(), moves, edition);
        *current = current.take().and_then(|held| held.align(moves, edition));
        doc.bands.align(moves, edition);
    }

    /// Moves the last search to another match by `step` at `now`, and
    /// shows it.
    pub(crate) fn step(
        &mut self,
        step: fn(&mut Search, Range<usize>) -> bool,
        now: Instant,
        layout: &mut Layout,
    ) {
        let Some(search) = &mut doc!(self).search else {
            return;
        };
        let around = step(search, layout.view.visible());
        match search.current().is_some() {
            true => self.jump(now, around, layout),
            false => self.nowhere = true,
        }
        doc!(self).shown = true;
    }

    /// Ends a fade that is over at `now`, and a message that has faded
    /// out.
    pub(crate) fn tick(&mut self, now: Instant) {
        let doc = doc!(self);
        doc.jumped = doc
            .jumped
            .filter(|jump| now.saturating_duration_since(jump.at) < FADE);
        doc.changed = doc.changed.take().filter(|changed| {
            now.saturating_duration_since(changed.at) < HIGHLIGHT_STAY + HIGHLIGHT_FADE
        });
        doc.noticed = doc
            .noticed
            .filter(|at| now.saturating_duration_since(*at) < WARNING_STAY + FADE);
        doc.counted = doc
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
        let search = self.document().search.as_ref()?;
        if self.nowhere {
            return Some(status::Found::Nowhere(search.term()));
        }
        if !self.document().shown {
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

    fn state_over_a_base() -> (State, Vec<ViewId>) {
        let mut session = Session::new(["a.md"]);
        let views = (1..=2)
            .map(|at| {
                let file = crate::session::sources::SourceFile {
                    path: format!("{at}.md").into(),
                    input: crate::typeset::input::Input::Markdown,
                    look: None,
                };
                session.open(format!("{at}.md"), file)
            })
            .collect();
        (State::new(SearchCase::Smart), views)
    }

    fn warning(text: &str) -> Diagnostic {
        Diagnostic::new(crate::typeset::diagnostic::Code::Image, text)
    }

    #[test]
    fn a_pushed_view_has_its_own_search_and_the_base_keeps_its_own() {
        let (mut state, views) = state_over_a_base();
        let now = Instant::now();
        state.base.shown = true;
        state.push_view(views[0], Vec::new(), now);
        assert_eq!(state.document().view, views[0]);
        assert!(!state.document().shown, "the view starts with none");
        // Escape on the view takes away what it shows, and not the base's.
        state.document_mut().shown = true;
        state.dismiss();
        assert!(!state.document().shown);
        assert!(state.base.shown, "the base keeps its search shown");
        state.back_to(ViewId::BASE);
        assert_eq!(state.document().view, ViewId::BASE);
        assert!(state.document().shown);
    }

    #[test]
    fn pushing_stops_the_animations_below_and_drops_their_deferred_builds() {
        use crate::session::animation::Clip;
        use crate::typeset::images::ImageKey;
        use crate::typeset::images::animation::Plays;
        use crate::typeset::images::marks::State as Mark;
        let (mut state, views) = state_over_a_base();
        let (key, now) = (ImageKey::from("a.gif"), Instant::now());
        let clip = Clip {
            gaps: vec![Duration::from_millis(100); 3],
            plays: Plays::Forever,
            name: "a".to_owned(),
        };
        let below = &mut state.base;
        below.animation.know(vec![(key.clone(), clip)]);
        below.animation.start(&key, now);
        below.deferred.push(key.clone());
        let commands = state.push_view(views[0], Vec::new(), now);
        assert!(!commands.is_empty(), "the stop is for the loop to send");
        assert!(state.base.deferred.is_empty());
        let marks = state.base.animation.marks(now);
        assert_eq!(marks[&key], Mark::Stopped);
        assert!(state.document().animation.keys().is_empty());
    }

    #[test]
    fn the_shared_state_stays_across_a_push_and_back() {
        let (mut state, views) = state_over_a_base();
        let now = Instant::now();
        state.told = Some(Told::new("told"));
        state.nowhere = true;
        state.case = SearchCase::Literal;
        state.focus = Focus::Out(Vec::new());
        state.follow.mode = settings::Follow::Auto;
        state.background = Backgrounds::new(None, Some(Color::Rgb(1, 2, 3)), true);
        // One place to go back to, which the views share.
        let document = crate::typeset::parse::parse("a\n\nb\n");
        let session = Session::new(["a.md"]);
        let content = Content::Document(&document);
        let edition = Edition::new(content, &session, ViewId::BASE, None);
        let anchor = crate::session::view::Anchor::At {
            pos: Pos::block(0, &[1]),
            lines: 0,
        };
        let back = Back {
            view: ViewId::BASE,
            file: 0,
            place: Held::new(&edition, anchor),
        };
        state.history.push(back.clone());
        let before = format!(
            "{:?} {:?} {:?} {:?} {:?} {:?} {:?}",
            state.told,
            state.nowhere,
            state.case,
            state.focus,
            state.follow,
            state.background,
            state.history
        );
        state.push_view(views[0], Vec::new(), now);
        state.back_to(ViewId::BASE);
        let after = format!(
            "{:?} {:?} {:?} {:?} {:?} {:?} {:?}",
            state.told,
            state.nowhere,
            state.case,
            state.focus,
            state.follow,
            state.background,
            state.history
        );
        assert_eq!(after, before);
        assert_eq!(state.history.pop(), Some(back), "the history is whole");
    }

    #[test]
    fn the_warnings_of_an_opened_view_are_its_own() {
        let (mut state, views) = state_over_a_base();
        let now = Instant::now();
        state.document_mut().warn(vec![warning("base")], now);
        state.push_view(views[0], vec![warning("view"), warning("two")], now);
        let top = state.document();
        assert_eq!(top.all_warnings().len(), 2);
        assert_eq!(top.warnings[0].message, "view");
        assert_eq!(top.noticed, Some(now));
        assert_eq!(top.counted, Some(now));
        assert_eq!(state.base.warnings.len(), 1);
        assert_eq!(state.base.warnings[0].message, "base");
        let gone = state.back_to(ViewId::BASE);
        assert_eq!(gone, [views[0]]);
        assert_eq!(state.document().warnings[0].message, "base");
    }
}
