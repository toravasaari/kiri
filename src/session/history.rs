//! The places to go back to, as in a browser: where the reader was when
//! they followed a link, so that Backspace brings them back.
//!
//! A place is the view's own [`Anchor`], the one that keeps the view where
//! it was over a reload, so that going back after a reload comes to the same
//! block as `r` would leave the view at. It names the view it is a place
//! of, which is the one the edition is of until a link opens another.

use crate::session::edition::{Edition, Held, Moves};
use crate::session::sources::ViewId;
use crate::session::view::Anchor;

/// A place to go back to: the view, and where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Back {
    pub(crate) view: ViewId,
    pub(crate) place: Held<Anchor>,
}

/// The places to go back to, the last first, of every view, as in a
/// browser.
#[derive(Debug, Default)]
pub(crate) struct History {
    backs: Vec<Back>,
}

impl History {
    /// At most this many places are kept; the oldest goes first.
    pub(crate) const MOST: usize = 100;

    /// Adds `back` as the last place to go back to.
    pub(crate) fn push(&mut self, back: Back) {
        if self.backs.len() == Self::MOST {
            self.backs.remove(0);
        }
        self.backs.push(back);
    }

    /// The last place of `view` that is kept, which stays.
    pub(crate) fn last_of(&self, view: ViewId) -> Option<&Back> {
        self.backs.iter().rev().find(|back| back.view == view)
    }

    /// The last place to go back to, taken off.
    pub(crate) fn pop(&mut self) -> Option<Back> {
        self.backs.pop()
    }

    /// Moves the places of `view` to `edition` by `moves`; a place that is
    /// no more is dropped. The places of other views stay as they are.
    pub(crate) fn align(&mut self, view: ViewId, moves: &Moves, edition: &Edition) {
        self.backs = std::mem::take(&mut self.backs)
            .into_iter()
            .filter_map(|back| match back.view == view {
                false => Some(back),
                true => Some(Back {
                    place: back.place.align(moves, edition)?,
                    ..back
                }),
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::sources::Session;
    use crate::typeset::driver::Content;
    use crate::typeset::model::Document;
    use crate::typeset::parse::parse;
    use crate::typeset::position::Pos;

    fn edition(document: &Document, session: &Session) -> Edition {
        Edition::new(
            Content::Document(document),
            session,
            session.base().id(),
            None,
        )
    }

    fn at(edition: &Edition, block: usize) -> Back {
        Back {
            view: edition.view(),
            place: Held::new(
                edition,
                Anchor::At {
                    pos: Pos::block(0, &[block]),
                    lines: 0,
                },
            ),
        }
    }

    #[test]
    fn the_last_place_comes_back_first() {
        let document = parse("a\n\nb\n\nc\n");
        let edition = edition(&document, &Session::new(["a.md"]));
        let mut history = History::default();
        assert_eq!(history.pop(), None);
        history.push(at(&edition, 0));
        history.push(at(&edition, 2));
        assert_eq!(history.pop(), Some(at(&edition, 2)));
        assert_eq!(history.pop(), Some(at(&edition, 0)));
        assert_eq!(history.pop(), None);
    }

    #[test]
    fn at_most_a_hundred_places_are_kept_and_the_oldest_goes() {
        let document = parse("a\n");
        let edition = edition(&document, &Session::new(["a.md"]));
        let mut history = History::default();
        for block in 0..History::MOST + 5 {
            history.push(at(&edition, block));
        }
        assert_eq!(history.backs.len(), History::MOST);
        assert_eq!(history.backs.first(), Some(&at(&edition, 5)));
        assert_eq!(history.pop(), Some(at(&edition, History::MOST + 4)));
    }

    #[test]
    fn a_place_moves_with_its_view_s_new_version_and_one_that_is_gone_is_dropped() {
        let (old, new) = (parse("a\n\nb\n\nc\n"), parse("new\n\na\n\nc\n"));
        let mut session = Session::new(["a.md"]);
        let before = edition(&old, &session);
        let id = session.sources()[0].id();
        session.reload(id, None);
        let after = edition(&new, &session);
        let comparison = crate::typeset::compare::compare(&old, &new);
        let mut history = History::default();
        // Block 0 ("a") is block 1 in the new version.
        history.push(at(&before, 0));
        history.align(
            before.view(),
            &Moves::new(&before, &after, Some(&comparison)),
            &after,
        );
        let back = history.pop().expect("the place moved");
        assert_eq!(back.view, after.view());
        assert_eq!(
            back.place.get(&after),
            Some(&Anchor::At {
                pos: Pos::block(0, &[1]),
                lines: 0
            })
        );
        // A file that changed with nothing to align it by loses its places.
        history.push(at(&before, 0));
        history.align(before.view(), &Moves::new(&before, &after, None), &after);
        assert_eq!(history.pop(), None);
    }

    #[test]
    fn the_places_of_another_view_stay_as_they_are() {
        let (old, new) = (parse("a\n\nb\n"), parse("new\n\na\n\nb\n"));
        let mut session = Session::new(["a.md"]);
        let other = session.another_view();
        let before = edition(&old, &session);
        let id = session.sources()[0].id();
        session.reload(id, None);
        let after = edition(&new, &session);
        let comparison = crate::typeset::compare::compare(&old, &new);
        let moves = Moves::new(&before, &after, Some(&comparison));
        let mut history = History::default();
        let elsewhere = Back {
            view: other,
            ..at(&before, 1)
        };
        history.push(elsewhere.clone());
        history.align(before.view(), &moves, &after);
        assert_eq!(history.pop(), Some(elsewhere));
    }
}
