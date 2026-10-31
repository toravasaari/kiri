//! What the reader does in the pager besides scrolling, which stays when
//! the window changes size: the state, and its answers to keys and to the
//! passing of time.

use std::ops::Range;
use std::time::{Duration, Instant, SystemTime};

use crate::present::keys::{Decoder, Key};
use crate::present::kitty::graphics::ImageId;
use crate::session::commands::{self, Action};
use crate::session::fade::{
    FADE, FADE_OUT, Fade, HIGHLIGHT_FADE, HIGHLIGHT_SHARE, HIGHLIGHT_STAY, Rgb, STAY, WARNING_STAY,
};
use crate::session::files::Files;
use crate::session::frame::Layout;
use crate::session::layers::{self, Highlights, Tint};
use crate::session::prompt::{Edit, Prompt};
use crate::session::search::{Marks, Search};
use crate::session::settings;
use crate::session::settings::SearchCase;
use crate::session::step::{Effect, Run};
use crate::session::view::Motion;
use crate::session::{bands, links, status};
use crate::typeset::model::FrontMatterError;
use crate::typeset::style::{Line, Style};

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

/// What tells one version of a document from another without reading it:
/// for a file, when it was last modified and its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub modified: Option<SystemTime>,
    pub length: u64,
}

/// Something kiri found wrong with a document while it read it, which
/// the pager tells about without stopping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// The line of the file the trouble is at, counting from 1, when
    /// known.
    pub line: Option<usize>,
    /// What is wrong, as one line of text.
    pub message: String,
}

impl From<&FrontMatterError> for Warning {
    fn from(error: &FrontMatterError) -> Self {
        Self {
            line: error.line,
            message: format!("front matter: {}", error.message),
        }
    }
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
    /// The last search.
    pub(crate) search: Option<Search>,
    /// The selected link.
    pub(crate) links: links::Links,
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
    pub(crate) warnings: Vec<Warning>,
    /// When the warning line was last told, while it stays and fades.
    pub(crate) noticed: Option<Instant>,
    /// When the number of warnings last changed, while the number of the
    /// mark on the status line flashes.
    pub(crate) counted: Option<Instant>,
    /// Where the focus is, as the terminal last told.
    pub(crate) focus: Focus,
    /// The keys read, with what a read cut off waiting for the rest,
    /// also when a layout ends between the two reads.
    pub(crate) decoder: Decoder,
    /// How far reading mode's bands are scrolled, and which is selected.
    pub(crate) bands: bands::Bands,
    /// The cells the terminal last answered, until `page_through` applies
    /// them when `show` returns.
    pub(crate) cell: Option<(u16, u16)>,
}

/// Where the focus is, as the terminal tells with focus reporting: under
/// tmux, whether the pane is in the window shown.
#[derive(Debug)]
pub(crate) enum Focus {
    /// Not told yet, as when kiri started in a pane not shown.
    Unknown,
    In,
    /// Gone, with the images sent since, which tmux may have dropped.
    Out(Vec<ImageId>),
}

/// The change auto-follow moved the view to.
#[derive(Debug, Clone)]
pub(crate) struct Changed {
    pub(crate) at: Instant,
    /// Its lines in the laid-out document.
    pub(crate) lines: Range<usize>,
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
            search: None,
            links: links::Links::default(),
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
            noticed: None,
            counted: None,
            focus: Focus::Unknown,
            decoder: Decoder::default(),
            bands: bands::Bands::default(),
            cell: None,
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
    /// may drop while the focus is gone.
    pub(crate) fn sent(&mut self, sent: Vec<ImageId>) {
        if let Focus::Out(away) = &mut self.focus {
            away.extend(sent);
        }
    }

    /// Takes `warnings`, about the document as read at `now`, for the
    /// ones before; the number of them flashes when it differs.
    pub(crate) fn warn(&mut self, warnings: Vec<Warning>, now: Instant) {
        if warnings.len() != self.warnings.len() {
            self.counted = Some(now);
        }
        self.noticed = (!warnings.is_empty()).then_some(now);
        self.warnings = warnings;
    }

    /// Does what `key`, pressed at `now`, asks in `layout` of a pager that
    /// is `run`, and tells what is left for the loop to do, if anything.
    pub(crate) fn press(
        &mut self,
        key: Key,
        now: Instant,
        layout: &mut Layout,
        run: &Run,
    ) -> Option<Effect> {
        self.pressed(key, now, layout, run)
    }

    fn pressed(
        &mut self,
        key: Key,
        now: Instant,
        layout: &mut Layout,
        run: &Run,
    ) -> Option<Effect> {
        let mut action = commands::action(key);
        // A click and the sideways wheel act on the band under them, and
        // never reach the help, the prompt or the colon.
        if let Some(pointer) = action.filter(|action| action.is_pointer()) {
            match self.pointed(pointer, layout) {
                Ok(effect) => return effect,
                Err(fallback) => action = Some(fallback),
            }
        }
        // Any other mouse event does nothing at all.
        if key == Key::Mouse {
            return None;
        }
        self.nowhere = false;
        self.told = None;
        if action == Some(Action::Dismiss) {
            self.dismiss();
            return None;
        }
        // Any key closes the help and does nothing else, but Ctrl+C
        // still quits.
        if self.help && key != Key::Ctrl('c') {
            self.help = false;
            return None;
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
            return None;
        }
        // The key after a colon is a command's, and any other closes it.
        // The commands move between files, so a lone document has none
        // and its colon does nothing.
        if std::mem::take(&mut self.colon) {
            action = match key {
                Key::Char(c) => commands::action(Key::Command(c)),
                _ => return None,
            };
        } else if key == commands::COMMAND && layout.status.files.several() {
            self.colon = true;
            return None;
        }
        match action {
            Some(Action::Quit) => return Some(Effect::Quit),
            // Enter copies the link selected on the screen, and otherwise
            // goes down a line.
            Some(Action::Enter) => match self.link_on_screen(layout) {
                Some(link) => {
                    self.told = Some(Told::fading("link copied", now));
                    return Some(Effect::Copy(link.url));
                }
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
            Some(Action::Reload) => return self.reload(run),
            // As `less` repaints, also the images, which the terminal may
            // have lost.
            Some(Action::Redraw) => return Some(Effect::Redraw),
            Some(Action::Follow) => self.cycle_follow(run),
            Some(Action::Reading) => return Some(Effect::Reading),
            Some(Action::Wide) => return Some(Effect::Wide),
            Some(Action::Sideways(step)) => {
                self.bands_moved |= self.bands.scroll(layout.lines, layout.places, step);
            }
            Some(Action::SelectLink { back }) => {
                let visible = layout.view.visible();
                let (lines, places) = (layout.lines, layout.places);
                let scrolled =
                    (self.links).select_scrolling(&mut self.bands, lines, places, visible, back);
                self.bands_moved |= scrolled;
            }
            // Answered above, or in `pointed`.
            Some(Action::Dismiss | Action::Click { .. } | Action::Wheel { .. }) | None => {}
        }
        None
    }

    /// Answers `r`: the reader sees that the pager is busy before it is.
    fn reload(&mut self, run: &Run) -> Option<Effect> {
        if !run.can_reload {
            self.told = Some(Told::new("nothing to reload"));
            return None;
        }
        self.told = Some(Told::new("reloading…"));
        Some(Effect::Reload)
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
            self.shown = false;
            self.links.clear();
        }
    }

    /// A click or a notch of the sideways wheel: the answer, or for the
    /// wheel with Shift away from a band the wheel's action. While the
    /// help or the prompt is open the mouse does nothing.
    fn pointed(&mut self, action: Action, layout: &Layout) -> Result<Option<Effect>, Action> {
        let moved = match action {
            _ if self.help || self.prompt.is_some() => false,
            Action::Click { row } => {
                let line = Self::row_line(row, layout);
                self.bands.click(layout.lines, layout.places, line)
            }
            Action::Wheel { right, shift, row } => match self.bands.wheel(
                layout.lines,
                layout.places,
                Self::row_line(row, layout),
                right,
            ) {
                Some(moved) => moved,
                None if shift && right => return Err(Action::Move(Motion::Down(commands::WHEEL))),
                None if shift => return Err(Action::Move(Motion::Up(commands::WHEEL))),
                None => false,
            },
            action => return Err(action),
        };
        self.bands_moved |= moved;
        Ok(None)
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

    /// The selected link, when it is on the screen and has an address to
    /// copy.
    pub(crate) fn link_on_screen(&self, layout: &Layout) -> Option<links::Found> {
        let visible = layout.view.visible();
        (self.links.selected_on(layout.lines, layout.places, visible))
            .filter(|link| !link.url.is_empty())
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
        let search = Search::new(layout.lines, term, self.case, layout.view.top());
        self.jumped = None;
        let line = search.current().map(|found| found.line);
        self.search = Some(search);
        match line {
            Some(line) => self.jump(line, now, false, layout),
            None => self.nowhere = true,
        }
        self.shown = true;
    }

    /// Shows the match on line `line`, jumped to at `now`, `around` an
    /// end of the document or not.
    pub(crate) fn jump(&mut self, line: usize, now: Instant, around: bool, layout: &mut Layout) {
        layout.view.reveal(line);
        self.jumped = Some(Jump { at: now, around });
        // A match in a band scrolls the band to it, and selects it.
        let band = (self.search.as_ref().and_then(Search::current))
            .and_then(|found| found.place.as_ref()?.band.clone());
        if let Some((_, x)) = band
            && self.bands.reveal(layout.lines, layout.places, line, x) == Some(true)
        {
            self.bands_moved = true;
        }
    }

    /// Finds the last search again in `lines`, laid out anew, from the
    /// line `top` of the view, closes the prompt when the window has no
    /// status line, `bar`, to show it on, and moves the last move to a
    /// file along with the view: it is only the new layout that put the
    /// view's top on another line, not a key.
    pub(crate) fn lay_out(&mut self, lines: &[Line], top: usize, bar: bool) {
        if !bar {
            self.prompt = None;
        }
        if let Some(went) = &mut self.went {
            went.top = top;
        }
        if let Some(search) = &mut self.search {
            *search = search.again(lines, self.case, top);
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
        match search.current().map(|found| found.line) {
            Some(line) => self.jump(line, now, around, layout),
            None => self.nowhere = true,
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

    /// The tint at `now`, in `rgb`'s colours over `columns` cells: the
    /// lines of the change, and how much of the text colour to mix into
    /// their background: all of [`HIGHLIGHT_SHARE`]
    /// while the tint stays, then less as it fades out.
    pub(crate) fn tint<'a>(&self, now: Instant, rgb: &'a Rgb, columns: usize) -> Option<Tint<'a>> {
        let changed = self.changed.as_ref()?;
        let elapsed = now.saturating_duration_since(changed.at);
        let left = match elapsed.checked_sub(HIGHLIGHT_STAY) {
            None => 1.0,
            Some(faded) => 1.0 - faded.as_secs_f32() / HIGHLIGHT_FADE.as_secs_f32(),
        };
        Some(Tint {
            lines: changed.lines.clone(),
            amount: HIGHLIGHT_SHARE * left.max(0.0),
            rgb,
            columns,
        })
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

    /// `lines`, from line `first` of the document on, with the highlights
    /// over them: the matches of the last search marked with `marks` while
    /// it is shown, the current one faded by `fade`, the lines of `tint`
    /// tinted and the selected `link`.
    pub(crate) fn marked(
        &self,
        lines: &[Line],
        first: usize,
        marks: &Marks,
        fade: Option<Fade>,
        tint: Option<Tint>,
        link: Option<&links::Found>,
    ) -> Vec<Line> {
        let highlights = Highlights {
            tint,
            search: self.search.as_ref().filter(|_| self.shown),
            marks,
            fade,
            link,
        };
        layers::paint(lines, first, &layers::stack(&highlights, lines, first))
    }

    /// What the status line tells about the last key, in `fading` when
    /// it is given, or else about the last search, with the match number
    /// in `flash` when it is given.
    pub(crate) fn found<'a>(
        &'a self,
        flash: Option<Style>,
        fading: Option<Style>,
        link: Option<&'a str>,
    ) -> Option<status::Found<'a>> {
        if let Some(told) = &self.told {
            return Some(status::Found::Told {
                message: &told.message,
                fading,
            });
        }
        // Where the selected link goes.
        if let Some(url) = link {
            return Some(status::Found::Link(url));
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
