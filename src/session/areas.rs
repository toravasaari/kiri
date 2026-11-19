//! The map of the window's areas: the one place that drawing and the
//! mouse both ask which part of the window is what, and the order a key
//! goes in.
//!
//! The window is divided into areas, each with a [`Role`]. Today there is
//! one, the document, over the whole window. The contents bar and the
//! panels beside the document will be areas of the same map, so that
//! drawing them and answering the pointer over them need no second list.
//!
//! Which area has the focus is kept apart from the [`Deck`]. The deck is
//! the stack of modals over the window; the focus is which docked area
//! takes keys when no card does. A key goes to the deck first, then to the
//! focused area, and then to the document ([`route`]).

use crate::present::keys::Key;
use crate::present::scene::Area;
use crate::session::deck::{Card, Deck, Routed};

/// What an area of the window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Role {
    /// The document: the lines of the view.
    #[default]
    Document,
    /// The bar of the document's headings beside it.
    Contents,
}

/// An area of the window and what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Region {
    pub(crate) role: Role,
    pub(crate) area: Area,
}

/// The areas of a window, from the bottom up: where two overlap, the later
/// one is on top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Areas {
    regions: Vec<Region>,
}

/// A cell of the window as the area under it sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Under {
    pub(crate) role: Role,
    /// The cell's column and row counted from the area's own top left
    /// corner, from 1.
    pub(crate) column: usize,
    pub(crate) row: usize,
}

impl Areas {
    /// A map of these areas.
    pub(crate) fn new(regions: Vec<Region>) -> Self {
        Self { regions }
    }

    /// The areas of a window of `window` columns and rows with a contents
    /// bar `contents` columns wide at its left, none when it has no bar:
    /// the bar, and the document in the rest.
    pub(crate) fn of(window: (u16, u16), contents: u16) -> Self {
        let whole = Area::window(window);
        let bar = usize::from(contents.min(window.0));
        if bar == 0 {
            return Self::new(vec![Region {
                role: Role::Document,
                area: whole,
            }]);
        }
        Self::new(vec![
            Region {
                role: Role::Contents,
                area: Area {
                    columns: bar,
                    ..whole
                },
            },
            Region {
                role: Role::Document,
                area: Area {
                    column: bar + 1,
                    columns: whole.columns - bar,
                    ..whole
                },
            },
        ])
    }

    /// The area of `role`, if the window has one.
    pub(crate) fn area(&self, role: Role) -> Option<Area> {
        (self.regions.iter())
            .find(|region| region.role == role)
            .map(|region| region.area)
    }

    /// The area at `column` and `row` of the window, from 1, with the cell
    /// in the area's own count; none where no area is.
    pub(crate) fn under(&self, column: usize, row: usize) -> Option<Under> {
        let region = (self.regions.iter().rev()).find(|region| region.area.holds(row, column))?;
        Some(Under {
            role: region.role,
            column: column - region.area.column + 1,
            row: row - region.area.row + 1,
        })
    }
}

/// Where a key goes first.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// A card of the deck is on top, and this is what it answered.
    Deck(Routed),
    /// No card took the key and no document card is on top; this docked
    /// area takes keys and is offered it before the document.
    Area(Role),
    /// The document takes the key: a document card is on top, or no card
    /// took it and the document has the focus.
    Document,
}

/// Gives `key` to the deck, and when the card on top, if any, leaves it to
/// the document, to the area in `focus`. A document card on top takes the
/// key whatever the focus: a view covers the window, so its keys are its
/// own.
pub(crate) fn route(deck: &mut Deck, focus: Role, key: Key) -> Route {
    let covered = matches!(deck.top(), Some(Card::Document(_)));
    match deck.press(key) {
        Routed::Document if covered || focus == Role::Document => Route::Document,
        Routed::Document => Route::Area(focus),
        routed => Route::Deck(routed),
    }
}

#[cfg(test)]
mod tests;
