//! The state of one document, which each view has of its own: its edition,
//! its search, the selected link, its bands, its warnings and its
//! animations. What the views share, such as the message on the status
//! line, stays in the state engine's `State`.

use std::borrow::Cow;
use std::ops::Range;
use std::time::Instant;

use crate::session::animation::Animator;
use crate::session::bands;
use crate::session::edition::{self, Edition, Held};
use crate::session::fade::{Fade, HIGHLIGHT_FADE, HIGHLIGHT_SHARE, HIGHLIGHT_STAY};
use crate::session::files::Files;
use crate::session::frame::{Layout, Shown};
use crate::session::layers::{self, Highlights};
use crate::session::links::{self, Links, Placed};
use crate::session::search::Search;
use crate::session::sources::ViewId;
use crate::session::state::{Change, Look, POLL};
use crate::session::view::Anchor;
use crate::typeset::color::Rgb;
use crate::typeset::compare::{self, Comparison};
use crate::typeset::diagnostic::Diagnostic;
use crate::typeset::images::ImageKey;
use crate::typeset::locate::{Map, Part};
use crate::typeset::model::{Document, Hashes};
use crate::typeset::position::Pos;
use crate::typeset::style::Line;
use crate::typeset::text::DocumentText;

/// The state of one document, which each view has of its own: what the
/// reader searched for in it, the link selected, where its bands are
/// scrolled, its warnings and its animations. The state shared by the
/// views, such as the status line's message and the settings, is
/// [`State`](crate::session::state::State)'s.
#[derive(Debug)]
pub(crate) struct DocumentState {
    /// The view the document is of.
    pub(crate) view: ViewId,
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
    /// The last jump to a match, while its fade is under way.
    pub(crate) jumped: Option<Jump>,
    /// The last move to a file, while the view stays where it took it.
    pub(crate) went: Option<Went>,
    /// What the polls of the document's file have seen.
    pub(crate) polls: Polls,
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
    /// How far reading mode's bands are scrolled, and which is selected.
    pub(crate) bands: bands::Bands,
    /// What the next layout keeps from this one.
    pub(crate) next: Next,
    /// The animated images of the layout shown.
    pub(crate) animation: Animator,
    /// The pictures whose frames were to be built before the places they
    /// are shown at were drawn: built after the next draw, also when that
    /// draw is of the next layout.
    pub(crate) deferred: Vec<ImageKey>,
}

/// What the polls of a document's file have seen while it is followed.
#[derive(Debug, Default)]
pub(crate) struct Polls {
    /// How a change looked on the last poll, and when it was first seen
    /// looking so. It is read once a poll at least [`POLL`] later finds it
    /// looking the same, so that a file caught half written is not shown.
    pub(crate) seen: Option<(Look, Instant)>,
    /// Whether the document could not be looked at on the last poll, which
    /// the status line told.
    pub(crate) gone: bool,
}

impl Polls {
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

/// The change auto-follow moved the view to.
#[derive(Debug, Clone)]
pub(crate) struct Changed {
    pub(crate) at: Instant,
    /// What it is of the document, which is the same in any layout of this
    /// version of it.
    pub(crate) range: Held<Range<Pos>>,
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

/// What a poll of the document while it is followed calls for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Polled {
    Nothing,
    Reload,
    /// The status line is to tell why the document cannot be looked at.
    Gone(String),
}

impl DocumentState {
    /// The state of the document of `view`, with nothing done yet, as a
    /// document starts, whose images animate as `animation` does.
    pub(crate) fn new(view: ViewId, animation: Animator) -> Self {
        Self {
            view,
            edition: None,
            search: None,
            links: Links::default(),
            bands_moved: false,
            shown: false,
            jumped: None,
            went: None,
            polls: Polls::default(),
            changed: None,
            warnings: Vec::new(),
            notes: Vec::new(),
            noticed: None,
            counted: None,
            bands: bands::Bands::default(),
            next: Next::default(),
            animation,
            deferred: Vec::new(),
        }
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
            layout.view.anchor(layout.map, self.next.keep_top),
        ));
        // The search finds its matches again in a layout of the same
        // version, and only a new version needs its current match kept.
        if reload {
            let current = self.search.as_ref().and_then(Search::current);
            self.next.current = current.map(|found| Held::new(edition, found.range.clone()));
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

    /// Shows the bands of reading mode in `lines`, as scrolled and selected.
    pub(crate) fn apply_bands(&self, lines: &mut [Line], map: &Map) {
        if let Some(edition) = &self.edition {
            self.bands.apply(edition, lines, map);
        }
    }

    /// Selects the topmost band in `visible` when none that shows is
    /// selected; whether that changed the selection.
    pub(crate) fn refresh_bands(&mut self, map: &Map, visible: Range<usize>) -> bool {
        let edition = self.edition.as_ref();
        edition.is_some_and(|edition| self.bands.refresh(edition, map, visible))
    }

    /// The text of what `shown` shows: the edition's, which is made once
    /// for each version, and when there is no edition yet, as in a test
    /// that does not lay a document out through the pager, made now.
    pub(crate) fn text(&self, shown: &Shown) -> Cow<'_, DocumentText> {
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
        let map = layout.map;
        let set = layers::stack(&highlights, map, window.clone());
        let look = crate::typeset::layers::Look {
            marks: layout.marks,
            rgb: layout.rgb,
            columns: layout.columns,
        };
        crate::typeset::layers::paint(layout.lines, window, map, &set, &look)
    }

    /// The file the reader is in, of `files`, with the view's top on the
    /// line `top`: the one the last move to a file went to, or else the
    /// one at the top.
    pub(crate) fn file(&self, files: &Files, top: usize) -> Option<usize> {
        self.went.map(|went| went.file).or_else(|| files.at(top))
    }

    /// Forgets the last move to a file once the view's top, now on the
    /// line `top`, has moved from where the move took it.
    pub(crate) fn moved(&mut self, top: usize) {
        self.went = self.went.filter(|went| went.top == top);
    }

    /// The stops of the layout, in the order of the document: the links
    /// and the animations that no link has, which some line shows.
    pub(crate) fn stops(&self, layout: &Layout) -> Vec<Placed> {
        let Some(edition) = &self.edition else {
            return Vec::new();
        };
        links::on_layout(edition, &self.animation, layout.seen(), layout.shown.links)
    }

    /// The selected stop, when it is on the screen. A link without an
    /// address is never one, so a link has one to copy.
    pub(crate) fn link_on_screen(&self, layout: &Layout) -> Option<Placed> {
        let edition = self.edition.as_ref()?;
        self.links.selected.as_ref()?;
        let all = self.stops(layout);
        let found = self.links.selected(edition, &all)?;
        if !found.is_on(&layout.view.visible()) {
            return None;
        }
        // In a band that is scrolled clear of it, a stop has nothing to show.
        let seen = layout.seen();
        let (parts, _) = seen.shown(&found.stop.range).ok()?;
        let shown = |part: &Part| {
            let (Some(across), Some(start)) = (&part.band, seen.map.start(&part.piece)) else {
                return true;
            };
            let line = start + part.rows.start;
            let at = layout.map;
            self.bands.shows(edition, at, line, &across.pixels)
        };
        parts.iter().any(shown).then(|| found.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::settings::SearchCase;
    use crate::session::state::State;
    use crate::typeset::diagnostic::Code;
    use std::time::Duration;

    fn diagnostic(text: &str) -> Diagnostic {
        Diagnostic::new(Code::Image, text)
    }

    #[test]
    fn a_reread_of_the_document_does_not_flash_the_count_of_warnings_the_layout_adds_to() {
        let (t0, t1) = (Instant::now(), Instant::now() + Duration::from_secs(5));
        let mut state = State::new(SearchCase::Smart);
        let document = state.document_mut();
        document.warn(vec![diagnostic("document")], t0);
        document.note(vec![diagnostic("note")], t0);
        assert_eq!(document.all_warnings().len(), 2);
        document.counted = None;
        // A reload reads the same document again: the same count, no flash,
        // and the layout's note is not touched.
        document.warn(vec![diagnostic("document")], t1);
        assert_eq!(document.counted, None);
        assert_eq!(document.all_warnings().len(), 2);
        // The same note from the next layout changes nothing.
        document.note(vec![diagnostic("note")], t1);
        assert_eq!(document.counted, None);
        // A layout that leaves no note takes it back, and the count flashes.
        document.note(Vec::new(), t1);
        assert_eq!(document.counted, Some(t1));
        assert_eq!(document.all_warnings(), [diagnostic("document")]);
    }

    #[test]
    fn every_field_of_the_document_state_starts_as_the_state_did() {
        let document = DocumentState::new(ViewId::BASE, State::new(SearchCase::Smart).animator());
        assert_eq!(document.view, ViewId::BASE);
        assert!(document.edition.is_none());
        assert!(document.search.is_none());
        assert!(document.links.selected.is_none());
        assert!(!document.bands_moved && !document.shown);
        assert!(document.jumped.is_none() && document.went.is_none());
        assert!(document.polls.seen.is_none() && !document.polls.gone);
        assert!(document.changed.is_none());
        assert!(document.warnings.is_empty() && document.notes.is_empty());
        assert!(document.noticed.is_none() && document.counted.is_none());
        let next = &document.next;
        assert!(!next.keep_top && next.changes.is_none() && next.hashes.is_none());
        assert!(next.anchor.is_none() && next.current.is_none());
        assert!(document.deferred.is_empty());
        // What `State::new` started these with on the base branch: nothing
        // set, and the animator of the quick pacing.
        let debug = |value: &dyn std::fmt::Debug| format!("{value:?}");
        assert_eq!(debug(&document.bands), debug(&bands::Bands::default()));
        assert_eq!(debug(&document.links), debug(&Links::default()));
        let quick = Animator::new(crate::session::animation::Pacing::quick());
        assert_eq!(debug(&document.animation), debug(&quick));
        let state = State::new(SearchCase::Smart);
        assert_eq!(debug(&state.base.animation), debug(&quick));
    }
}
