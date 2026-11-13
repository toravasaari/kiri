//! The deck of cards over the document: which is on top, where a key and
//! the pointer go, what Escape closes and what the status line shows.
//!
//! Whatever lies over the document and takes keys before it is a [`Card`]
//! in the [`Deck`], not a field of [`State`](crate::session::state::State).
//! The deck has no bottom card: the document the pager started with is what
//! an empty deck means.
//!
//! Today at most one card is open at a time: the help closes on any key,
//! and the prompt and the colon take every key, so none can be opened over
//! another. The rules agree because of that. [`Deck::press`],
//! [`Deck::dismiss`] and [`Deck::status_line`] read the top card;
//! [`Deck::lets_pointer_through`], [`Deck::lose_status_line`],
//! [`Deck::help`] and [`Deck::prompt`] read the whole deck. When cards
//! start to stack, each rule has to choose between the two on purpose.

use crate::present::keys::Key;
use crate::session::commands::{self, Action};
use crate::session::prompt::{Edit, Prompt};

/// Something over the document that takes keys before it.
#[derive(Debug)]
pub(crate) enum Card {
    /// The search prompt, with the term typed so far.
    Prompt(Prompt),
    /// A colon was typed: the next key gives a command.
    Colon,
    /// The help panel.
    Help,
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
        let (routed, stays) = match self.cards.last_mut() {
            None => return Routed::Document,
            Some(card) => answer(card, key),
        };
        if !stays {
            self.cards.pop();
        }
        routed
    }

    /// Whether a click or the wheel reaches the document: the help and
    /// the prompt hold it, and the colon does not.
    pub(crate) fn lets_pointer_through(&self) -> bool {
        self.cards.iter().all(|card| match card {
            Card::Help | Card::Prompt(_) => false,
            Card::Colon => true,
        })
    }

    /// Answers Escape: closes the card on top. `false` when there was
    /// none, and Escape is left to the document.
    pub(crate) fn dismiss(&mut self) -> bool {
        self.cards.pop().is_some()
    }

    /// What the status line shows: the prompt or the colon, when it is on
    /// top, and otherwise the document's own line.
    pub(crate) fn status_line(&self) -> StatusLine<'_> {
        match self.top() {
            Some(Card::Prompt(prompt)) => StatusLine::Prompt(prompt.term()),
            Some(Card::Colon) => StatusLine::Colon,
            Some(Card::Help) | None => StatusLine::Document,
        }
    }

    /// The window has no status line any more: the prompt, which lives on
    /// it, closes. The help, which has a panel of its own, and the colon,
    /// which waits unseen, stay.
    pub(crate) fn lose_status_line(&mut self) {
        self.cards.retain(|card| match card {
            Card::Prompt(_) => false,
            Card::Help | Card::Colon => true,
        });
    }

    /// Whether the help is open.
    pub(crate) fn help(&self) -> bool {
        self.cards.iter().any(|card| matches!(card, Card::Help))
    }

    /// The prompt, while it is open.
    pub(crate) fn prompt(&self) -> Option<&Prompt> {
        self.cards.iter().find_map(|card| match card {
            Card::Prompt(prompt) => Some(prompt),
            Card::Help | Card::Colon => None,
        })
    }
}

/// How `card` answers `key`, and whether it stays on the deck.
fn answer(card: &mut Card, key: Key) -> (Routed, bool) {
    match card {
        Card::Help => help_answer(key),
        Card::Prompt(prompt) => prompt_answer(prompt, key),
        Card::Colon => colon_answer(key),
    }
}

/// The help's own rule: any key closes it and does nothing else, but
/// Ctrl+C goes on to quit the document.
fn help_answer(key: Key) -> (Routed, bool) {
    match key {
        Key::Ctrl('c') => (Routed::Document, true),
        _ => (Routed::Taken, false),
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
mod tests {
    use super::*;

    fn deck_of(cards: impl IntoIterator<Item = Card>) -> Deck {
        let mut deck = Deck::default();
        for card in cards {
            deck.open(card);
        }
        deck
    }

    fn prompt_with(term: &str) -> Deck {
        let mut deck = deck_of([Card::Prompt(Prompt::default())]);
        for c in term.chars() {
            assert_eq!(deck.press(Key::Char(c)), Routed::Taken);
        }
        deck
    }

    #[test]
    fn an_empty_deck_has_no_top_and_sends_keys_to_the_document() {
        let mut deck = Deck::default();
        assert!(deck.top().is_none());
        assert_eq!(deck.press(Key::Char('j')), Routed::Document);
        assert_eq!(deck.press(Key::Escape), Routed::Document);
        assert_eq!(deck.press(Key::Ctrl('c')), Routed::Document);
    }

    #[test]
    fn the_card_opened_last_is_on_top() {
        let deck = deck_of([Card::Colon, Card::Help]);
        assert!(matches!(deck.top(), Some(Card::Help)));
        let deck = deck_of([Card::Help, Card::Colon]);
        assert!(matches!(deck.top(), Some(Card::Colon)));
    }

    // The help's own rule, which the help as a document replaces.
    #[test]
    fn any_key_but_ctrl_c_closes_the_help_and_does_nothing_else() {
        for key in [Key::Char('j'), Key::Char('q'), Key::Enter, Key::Mouse] {
            let mut deck = deck_of([Card::Help]);
            assert_eq!(deck.press(key), Routed::Taken, "{key:?}");
            assert!(deck.top().is_none(), "{key:?}");
        }
    }

    #[test]
    fn ctrl_c_goes_through_the_help_to_the_document() {
        let mut deck = deck_of([Card::Help]);
        assert_eq!(deck.press(Key::Ctrl('c')), Routed::Document);
        assert!(deck.help(), "the help stays open");
    }

    #[test]
    fn the_prompt_takes_keys_while_typing() {
        let mut deck = prompt_with("ab");
        assert_eq!(deck.press(Key::Backspace), Routed::Taken);
        assert_eq!(deck.press(Key::Ctrl('c')), Routed::Taken);
        assert_eq!(deck.prompt(), None, "Ctrl+C cancelled it");
        let deck = prompt_with("ab");
        assert_eq!(deck.prompt().map(Prompt::term), Some("ab"));
    }

    #[test]
    fn the_prompt_closes_on_a_cancel() {
        let mut deck = prompt_with("ab");
        assert_eq!(deck.press(Key::Escape), Routed::Taken);
        assert!(deck.top().is_none());
        let mut deck = prompt_with("");
        assert_eq!(deck.press(Key::Backspace), Routed::Taken);
        assert!(deck.top().is_none());
    }

    #[test]
    fn enter_in_the_prompt_finds_the_term() {
        let mut deck = prompt_with("ab");
        assert_eq!(deck.press(Key::Enter), Routed::Find("ab".to_owned()));
        assert!(deck.top().is_none());
    }

    #[test]
    fn enter_in_an_empty_prompt_asks_for_the_next_match() {
        let mut deck = prompt_with("");
        assert_eq!(deck.press(Key::Enter), Routed::Act(Some(Action::Next)));
        assert!(deck.top().is_none());
    }

    #[test]
    fn a_character_after_a_colon_is_a_command_and_closes_the_colon() {
        let mut deck = deck_of([Card::Colon]);
        let next = commands::action(Key::Command('n'));
        assert!(next.is_some());
        assert_eq!(deck.press(Key::Char('n')), Routed::Act(next));
        assert!(deck.top().is_none());
    }

    #[test]
    fn a_character_after_a_colon_that_is_no_command_only_closes_it() {
        let mut deck = deck_of([Card::Colon]);
        assert_eq!(deck.press(Key::Char('?')), Routed::Act(None));
        assert!(deck.top().is_none());
    }

    #[test]
    fn another_key_after_a_colon_only_closes_it() {
        for key in [Key::Ctrl('c'), Key::Enter, Key::Mouse] {
            let mut deck = deck_of([Card::Colon]);
            assert_eq!(deck.press(key), Routed::Taken, "{key:?}");
            assert!(deck.top().is_none(), "{key:?}");
        }
    }

    #[test]
    fn escape_closes_the_top_card_and_tells_whether_one_was_open() {
        let mut deck = deck_of([Card::Colon, Card::Help]);
        assert!(deck.dismiss());
        assert!(matches!(deck.top(), Some(Card::Colon)));
        assert!(deck.dismiss());
        assert!(!deck.dismiss());
    }

    #[test]
    fn the_help_and_the_prompt_hold_the_pointer_and_the_colon_does_not() {
        assert!(Deck::default().lets_pointer_through());
        assert!(deck_of([Card::Colon]).lets_pointer_through());
        assert!(!deck_of([Card::Help]).lets_pointer_through());
        assert!(!deck_of([Card::Prompt(Prompt::default())]).lets_pointer_through());
    }

    #[test]
    fn the_status_line_shows_the_prompt_or_the_colon_and_otherwise_the_document() {
        assert_eq!(Deck::default().status_line(), StatusLine::Document);
        assert_eq!(deck_of([Card::Help]).status_line(), StatusLine::Document);
        assert_eq!(deck_of([Card::Colon]).status_line(), StatusLine::Colon);
        assert_eq!(prompt_with("ab").status_line(), StatusLine::Prompt("ab"));
    }

    #[test]
    fn losing_the_status_line_closes_the_prompt_alone() {
        let mut deck = prompt_with("ab");
        deck.lose_status_line();
        assert!(deck.top().is_none());
        let mut deck = deck_of([Card::Help, Card::Colon]);
        deck.lose_status_line();
        assert!(deck.help());
        assert!(matches!(deck.top(), Some(Card::Colon)));
    }
}
