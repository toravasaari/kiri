//! The deck of cards over the document: which is on top, where a key and
//! the pointer go, what Escape closes and what the status line shows.
//!
//! Whatever lies over the document and takes keys before it is a [`Card`]
//! in the [`Deck`], not a field of [`State`](crate::session::state::State).
//! The deck has no bottom card: the document the pager started with is what
//! an empty deck means. A document opened over it is a card too, with the
//! state it has of its own; Escape does not close it, and keys and the
//! pointer go to it as they go to the document.
//!
//! Today at most one card is open at a time: the help takes every key
//! but the few that scroll or close it, and the prompt and the colon take
//! every key, so none can be opened over another. Whether cards stack is
//! decided rule by rule, and each rule reads the deck as follows.
//!
//! - The top card alone: [`Deck::press`] (a key goes to the card on top
//!   and to no other), [`Deck::dismiss`] (Escape closes one card, the
//!   top) and [`Deck::status_line`] (the card on top, if it has a line).
//! - The whole deck: [`Deck::lets_pointer_through`] (any card that holds
//!   the pointer holds it, however far down), [`Deck::lose_status_line`]
//!   (every card that lives on the status line closes), [`Deck::sheets`]
//!   (every card that is laid out, from the bottom up), and
//!   [`Deck::panel`] and [`Deck::prompt`] (the card of that kind, the
//!   panel nearest the top, wherever it is), [`Deck::document`] (the document on top,
//!   with a panel over it or not), [`Deck::push_document`] and
//!   [`Deck::back_to`] (a document opened, and the way back to one).
//!
//! The help is meant to stack: a card opened over it takes the keys, and
//! Escape closes that card first. Nothing opens over the help today, since
//! it takes `:` and `/` as keys of its own that do nothing.
//!
//! `Sheet` names what the loop lays out for the deck, from the bottom up: the
//! document at the bottom, and each card that is a document of its own, the
//! help and a document opened over the first. A document card is
//! `Card::Document`, which carries its `DocumentState`, so the order of the
//! views is the deck's. Escape does not close a document card; keys and the
//! pointer go to the one on top. `push_document` and `back_to` open one and go
//! back to one, and `view_below` and `holds` say which view is under the one
//! on top and whether a view is on the deck.

use crate::present::keys::Key;
use crate::session::commands::{self, Action};
use crate::session::document::DocumentState;
use crate::session::panels::{Help, help};
use crate::session::prompt::{Edit, Prompt};
use crate::session::sources::ViewId;
use crate::session::view::Motion;

/// Something over the document that takes keys before it.
#[derive(Debug)]
pub(crate) enum Card {
    /// The search prompt, with the term typed so far.
    Prompt(Prompt),
    /// A colon was typed: the next key gives a command.
    Colon,
    /// The help panel, and where it is scrolled to.
    Help(Help),
    /// A document opened over the one below, with the state it has of its
    /// own. Its view is in the state, so the order of the views is that
    /// of the cards.
    Document(Box<DocumentState>),
}

impl Card {
    /// What the loop lays out for the card, if it is a document of its
    /// own.
    fn sheet(&self) -> Option<Sheet> {
        match self {
            Card::Help(_) => Some(Sheet::Help),
            Card::Document(document) => Some(Sheet::View(document.view)),
            Card::Prompt(_) | Card::Colon => None,
        }
    }
}

/// What the loop keeps a layout of: the document at the bottom, or a
/// card laid out as a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Sheet {
    /// The document shown at the start: what an empty deck means.
    Base,
    /// The help panel.
    Help,
    /// A document opened over the others, by its view.
    View(ViewId),
}

/// A panel that is open: what the frame and the loop need to draw it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Open<'a> {
    /// What the loop laid its text out as.
    pub(crate) sheet: Sheet,
    /// The title in the top edge of its frame.
    pub(crate) title: &'static str,
    /// Where it is scrolled to.
    pub(crate) scroll: &'a Help,
}

/// The cards over the document, from the bottom up.
#[derive(Debug, Default)]
pub(crate) struct Deck {
    cards: Vec<Card>,
}

/// Where a key went, and what the document is left to do.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Routed {
    /// A card took the key; the document does nothing.
    Taken,
    /// No card took it: the key goes to the document as it is.
    Document,
    /// A card took it and the document does this action instead: a
    /// command after a colon, or `None` for a key after a colon that is
    /// no command; the last search again (`Action::Next`) from an empty
    /// prompt.
    Act(Option<Action>),
    /// The prompt closed on this term, and the document searches for it.
    Find(String),
    /// The help took the key as a move of its own view.
    Scroll(Motion),
}

/// What the status line shows.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StatusLine<'a> {
    /// The document's own status line.
    Document,
    /// The search prompt, with this term.
    Prompt(&'a str),
    /// The colon.
    Colon,
}

impl Deck {
    /// The card on top, if any.
    pub(crate) fn top(&self) -> Option<&Card> {
        self.cards.last()
    }

    /// Puts `card` on top.
    pub(crate) fn open(&mut self, card: Card) {
        self.cards.push(card);
    }

    /// Gives `key` to the card on top, which answers it, and closes the
    /// card when its answer says so. With no card, the key is the
    /// document's.
    pub(crate) fn press(&mut self, key: Key) -> Routed {
        let Some(card) = self.cards.last_mut() else {
            return Routed::Document;
        };
        let (routed, stays) = answer(card, key);
        if !stays {
            self.cards.pop();
        }
        routed
    }

    /// Whether a click or the wheel reaches the document: the help and
    /// the prompt hold it, and the colon does not.
    pub(crate) fn lets_pointer_through(&self) -> bool {
        self.cards.iter().all(|card| match card {
            Card::Help(_) | Card::Prompt(_) => false,
            Card::Colon | Card::Document(_) => true,
        })
    }

    /// Answers Escape: closes the card on top. `false` when there was
    /// none, or it is a document, which Escape does not close: Escape is
    /// then left to the document.
    pub(crate) fn dismiss(&mut self) -> bool {
        if matches!(self.top(), Some(Card::Document(_)) | None) {
            return false;
        }
        self.cards.pop().is_some()
    }

    /// What the status line shows: the prompt or the colon, when it is on
    /// top, and otherwise the document's own line.
    pub(crate) fn status_line(&self) -> StatusLine<'_> {
        match self.top() {
            Some(Card::Prompt(prompt)) => StatusLine::Prompt(prompt.term()),
            Some(Card::Colon) => StatusLine::Colon,
            Some(Card::Help(_) | Card::Document(_)) | None => StatusLine::Document,
        }
    }

    /// The window has no status line any more: the prompt, which lives on
    /// it, closes. The help, which has a panel of its own, and the colon,
    /// which waits unseen, stay.
    pub(crate) fn lose_status_line(&mut self) {
        self.cards.retain(|card| match card {
            Card::Prompt(_) => false,
            Card::Help(_) | Card::Colon | Card::Document(_) => true,
        });
    }

    /// The panel on top of the deck, with the sheet the loop laid it out
    /// as and the title of its frame, while one is open.
    pub(crate) fn panel(&self) -> Option<Open<'_>> {
        self.cards.iter().rev().find_map(|card| match card {
            Card::Help(scroll) => Some(Open {
                sheet: Sheet::Help,
                title: help::TITLE,
                scroll,
            }),
            Card::Prompt(_) | Card::Colon | Card::Document(_) => None,
        })
    }

    /// The scroll of the panel on top, to move.
    pub(crate) fn panel_mut(&mut self) -> Option<&mut Help> {
        self.cards.iter_mut().rev().find_map(|card| match card {
            Card::Help(scroll) => Some(scroll),
            Card::Prompt(_) | Card::Colon | Card::Document(_) => None,
        })
    }

    /// What the loop lays out, from the bottom up: the document, and then
    /// each card that is a document of its own.
    pub(crate) fn sheets(&self) -> impl Iterator<Item = Sheet> {
        let cards = self.cards.iter().filter_map(Card::sheet);
        std::iter::once(Sheet::Base).chain(cards)
    }

    /// The state of the document on top of the deck, even when a panel is
    /// over it.
    pub(crate) fn document(&self) -> Option<&DocumentState> {
        self.cards.iter().rev().find_map(|card| match card {
            Card::Document(document) => Some(&**document),
            Card::Prompt(_) | Card::Colon | Card::Help(_) => None,
        })
    }

    /// The state of the document on top of the deck, to change.
    pub(crate) fn document_mut(&mut self) -> Option<&mut DocumentState> {
        self.cards.iter_mut().rev().find_map(|card| match card {
            Card::Document(document) => Some(&mut **document),
            Card::Prompt(_) | Card::Colon | Card::Help(_) => None,
        })
    }

    /// The sheet of the document on top: its view, or the base when the
    /// deck has no document.
    pub(crate) fn document_sheet(&self) -> Sheet {
        self.document()
            .map_or(Sheet::Base, |document| Sheet::View(document.view))
    }

    /// Puts a document on top. What lay over the document that was on top
    /// closes first: the prompt, the colon and the help.
    pub(crate) fn push_document(&mut self, document: Box<DocumentState>) {
        let at = (self.cards.iter())
            .rposition(|card| matches!(card, Card::Document(_)))
            .map_or(0, |at| at + 1);
        self.cards.truncate(at);
        self.cards.push(Card::Document(document));
    }

    /// The view of the document card under the one on top, or `None` when
    /// what is under it is the base, or no document is on top.
    pub(crate) fn view_below(&self) -> Option<ViewId> {
        let mut views = self.cards.iter().rev().filter_map(|card| match card {
            Card::Document(document) => Some(document.view),
            Card::Prompt(_) | Card::Colon | Card::Help(_) => None,
        });
        views.nth(1)
    }

    /// Whether the document of `view` is a card of the deck.
    pub(crate) fn holds(&self, view: ViewId) -> bool {
        let is = |card: &Card| matches!(card, Card::Document(document) if document.view == view);
        self.cards.iter().any(is)
    }

    /// Takes off the cards until the document on top is the one of
    /// `view`, with `None` for the base, and gives the states of the
    /// documents taken off, the one that was on top first. A view that is
    /// not on the deck takes off nothing.
    pub(crate) fn back_to(&mut self, view: Option<ViewId>) -> Vec<DocumentState> {
        let keep = match view {
            None => 0,
            Some(view) => {
                let is = |card: &Card| matches!(card, Card::Document(d) if d.view == view);
                match self.cards.iter().rposition(is) {
                    Some(at) => at + 1,
                    None => return Vec::new(),
                }
            }
        };
        let mut gone = Vec::new();
        for card in self.cards.drain(keep..).rev() {
            if let Card::Document(document) = card {
                gone.push(*document);
            }
        }
        gone
    }

    /// The prompt, while it is open.
    pub(crate) fn prompt(&self) -> Option<&Prompt> {
        self.cards.iter().find_map(|card| match card {
            Card::Prompt(prompt) => Some(prompt),
            Card::Help(_) | Card::Colon | Card::Document(_) => None,
        })
    }
}

/// How `card` answers `key`, and whether it stays on the deck.
fn answer(card: &mut Card, key: Key) -> (Routed, bool) {
    match card {
        Card::Help(_) => help_answer(key),
        Card::Prompt(prompt) => prompt_answer(prompt, key),
        Card::Colon => colon_answer(key),
        Card::Document(_) => (Routed::Document, true),
    }
}

/// The help's own rule: `?`, `h`, `q` and `Q` close it, and the keys that
/// move scroll it, the wheel too, and Enter a line down. Every other key,
/// Backspace among them, does nothing. Ctrl+C goes on to quit the
/// document. Escape closes the card before the deck is asked.
fn help_answer(key: Key) -> (Routed, bool) {
    if key == Key::Ctrl('c') {
        return (Routed::Document, true);
    }
    match commands::action(key) {
        Some(Action::Help) => (Routed::Taken, false),
        Some(Action::Quit) => (Routed::Taken, false),
        Some(Action::Move(motion)) => (Routed::Scroll(motion), true),
        Some(Action::Enter) => (Routed::Scroll(Motion::Down(1)), true),
        _ => (Routed::Taken, true),
    }
}

/// The prompt edits its term; Enter and the cancels close it.
fn prompt_answer(prompt: &mut Prompt, key: Key) -> (Routed, bool) {
    match prompt.edit(key) {
        Edit::Typing => (Routed::Taken, true),
        Edit::Cancel => (Routed::Taken, false),
        Edit::Search(term) => (Routed::Find(term), false),
        // The last search again goes on to its next match, as `n` does.
        Edit::Repeat => (Routed::Act(Some(Action::Next)), false),
    }
}

/// The key after a colon is a command's, and any other closes the colon.
fn colon_answer(key: Key) -> (Routed, bool) {
    match key {
        Key::Char(c) => (Routed::Act(commands::action(Key::Command(c))), false),
        _ => (Routed::Taken, false),
    }
}

#[cfg(test)]
mod tests;
