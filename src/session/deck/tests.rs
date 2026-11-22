use std::path::PathBuf;

use super::*;
use crate::session::animation::{Animator, Pacing};
use crate::session::commands::Topic;
use crate::session::sources::{Session, SourceFile};
use crate::typeset::input::Input;

fn deck_of(cards: impl IntoIterator<Item = Card>) -> Deck {
    let mut deck = Deck::default();
    for card in cards {
        deck.open(card);
    }
    deck
}

fn help() -> Card {
    Card::Help(Help::at(Topic::Moving))
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
    let deck = deck_of([Card::Colon, help()]);
    assert!(matches!(deck.top(), Some(Card::Help(_))));
    let deck = deck_of([help(), Card::Colon]);
    assert!(matches!(deck.top(), Some(Card::Colon)));
}

#[test]
fn question_mark_h_and_q_close_the_help() {
    for key in [
        Key::Char('?'),
        Key::Char('h'),
        Key::Char('q'),
        Key::Char('Q'),
    ] {
        let mut deck = deck_of([help()]);
        assert_eq!(deck.press(key), Routed::Taken, "{key:?}");
        assert!(deck.top().is_none(), "{key:?}");
    }
}

#[test]
fn backspace_leaves_the_help_open() {
    let mut deck = deck_of([help()]);
    assert_eq!(deck.press(Key::Backspace), Routed::Taken);
    assert!(deck.panel().is_some());
}

#[test]
fn ctrl_c_goes_through_the_help_to_the_document() {
    let mut deck = deck_of([help()]);
    assert_eq!(deck.press(Key::Ctrl('c')), Routed::Document);
    assert!(deck.panel().is_some(), "the help stays open");
}

#[test]
fn moving_keys_and_the_wheel_scroll_the_help() {
    let keys = [
        (Key::Char('j'), Motion::Down(1)),
        (Key::Down, Motion::Down(1)),
        (Key::Char('k'), Motion::Up(1)),
        (Key::Char(' '), Motion::PageDown),
        (Key::PageUp, Motion::PageUp),
        (Key::Char('d'), Motion::HalfPageDown),
        (Key::Char('g'), Motion::Start),
        (Key::Char('G'), Motion::End),
        (Key::WheelDown, Motion::Down(commands::WHEEL)),
        (Key::WheelUp, Motion::Up(commands::WHEEL)),
    ];
    for (key, motion) in keys {
        let mut deck = deck_of([help()]);
        assert_eq!(deck.press(key), Routed::Scroll(motion), "{key:?}");
        assert!(deck.panel().is_some(), "{key:?}");
    }
}

#[test]
fn enter_scrolls_the_help_down_a_line() {
    let mut deck = deck_of([help()]);
    assert_eq!(deck.press(Key::Enter), Routed::Scroll(Motion::Down(1)));
    assert!(deck.panel().is_some());
}

#[test]
fn other_keys_do_nothing_in_the_help() {
    for key in [Key::Char('x'), Key::Char('/'), Key::Char(':'), Key::Mouse] {
        let mut deck = deck_of([help()]);
        assert_eq!(deck.press(key), Routed::Taken, "{key:?}");
        assert!(deck.panel().is_some(), "{key:?}");
    }
}

#[test]
fn the_panel_is_the_one_open_with_its_sheet_and_title() {
    assert!(Deck::default().panel().is_none());
    assert!(deck_of([Card::Colon]).panel().is_none());
    let deck = deck_of([help(), Card::Colon]);
    let open = deck.panel().expect("the help is a panel");
    assert_eq!((open.sheet, open.title), (Sheet::Help, "Help"));
}

#[test]
fn the_sheets_are_the_base_and_the_help_from_the_bottom_up() {
    let sheets = |deck: &Deck| deck.sheets().collect::<Vec<_>>();
    assert_eq!(sheets(&Deck::default()), [Sheet::Base]);
    assert_eq!(sheets(&deck_of([help()])), [Sheet::Base, Sheet::Help]);
    let prompt = Card::Prompt(Prompt::default());
    let deck = deck_of([help(), prompt]);
    assert_eq!(sheets(&deck), [Sheet::Base, Sheet::Help]);
    assert_eq!(sheets(&deck_of([Card::Colon])), [Sheet::Base]);
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
    let mut deck = deck_of([Card::Colon, help()]);
    assert!(deck.dismiss());
    assert!(matches!(deck.top(), Some(Card::Colon)));
    assert!(deck.dismiss());
    assert!(!deck.dismiss());
}

#[test]
fn the_help_and_the_prompt_hold_the_pointer_and_the_colon_does_not() {
    assert!(Deck::default().lets_pointer_through());
    assert!(deck_of([Card::Colon]).lets_pointer_through());
    assert!(!deck_of([help()]).lets_pointer_through());
    assert!(!deck_of([Card::Prompt(Prompt::default())]).lets_pointer_through());
}

#[test]
fn the_status_line_shows_the_prompt_or_the_colon_and_otherwise_the_document() {
    assert_eq!(Deck::default().status_line(), StatusLine::Document);
    assert_eq!(deck_of([help()]).status_line(), StatusLine::Document);
    assert_eq!(deck_of([Card::Colon]).status_line(), StatusLine::Colon);
    assert_eq!(prompt_with("ab").status_line(), StatusLine::Prompt("ab"));
}

#[test]
fn losing_the_status_line_closes_the_prompt_alone() {
    let mut deck = prompt_with("ab");
    deck.lose_status_line();
    assert!(deck.top().is_none());
    let mut deck = deck_of([help(), Card::Colon]);
    deck.lose_status_line();
    assert!(deck.panel().is_some());
    assert!(matches!(deck.top(), Some(Card::Colon)));
}

/// Views numbered after the base: a session that opened `n` files.
fn views(n: usize) -> Vec<ViewId> {
    let mut session = Session::new(["a.md"]);
    let open = |session: &mut Session, at: usize| {
        let file = SourceFile {
            path: PathBuf::from(format!("{at}.md")),
            input: Input::Markdown,
            look: None,
        };
        session.open(format!("{at}.md"), file)
    };
    (0..n).map(|at| open(&mut session, at)).collect()
}

fn document(view: ViewId) -> Card {
    let animator = Animator::new(Pacing::quick());
    Card::Document(Box::new(DocumentState::new(view, animator)))
}

fn views_of(documents: &[DocumentState]) -> Vec<ViewId> {
    documents.iter().map(|document| document.view).collect()
}

#[test]
fn escape_does_not_close_a_document_card() {
    let [view] = views(1)[..] else { panic!() };
    let mut deck = deck_of([document(view)]);
    assert!(!deck.dismiss());
    assert!(matches!(deck.top(), Some(Card::Document(_))));
    // A card over it closes first, and then Escape is the document's.
    deck.open(help());
    assert!(deck.dismiss());
    assert!(!deck.dismiss());
    assert!(deck.document().is_some());
}

#[test]
fn the_pointer_reaches_a_document_card() {
    let [view] = views(1)[..] else { panic!() };
    assert!(deck_of([document(view)]).lets_pointer_through());
    let mut deck = deck_of([document(view)]);
    deck.open(help());
    assert!(!deck.lets_pointer_through(), "the help over it holds it");
}

#[test]
fn keys_go_to_the_top_document() {
    let [view] = views(1)[..] else { panic!() };
    let mut deck = deck_of([document(view)]);
    for key in [Key::Char('j'), Key::Escape, Key::Ctrl('c'), Key::Enter] {
        assert_eq!(deck.press(key), Routed::Document, "{key:?}");
        assert!(matches!(deck.top(), Some(Card::Document(_))), "{key:?}");
    }
    assert_eq!(deck.status_line(), StatusLine::Document);
    deck.lose_status_line();
    assert!(deck.document().is_some());
    // A help over it takes the keys, and is closed by `?`.
    deck.open(help());
    assert_eq!(deck.press(Key::Char('x')), Routed::Taken);
    assert_eq!(deck.press(Key::Char('?')), Routed::Taken);
    assert_eq!(deck.press(Key::Char('j')), Routed::Document);
}

#[test]
fn pushing_a_document_closes_the_cards_above_the_top_document() {
    let [first, second] = views(2)[..] else {
        panic!()
    };
    let mut deck = deck_of([help(), document(first), help(), Card::Colon]);
    let Card::Document(state) = document(second) else {
        panic!()
    };
    deck.push_document(state);
    let sheets: Vec<Sheet> = deck.sheets().collect();
    let views = [Sheet::Help, Sheet::View(first), Sheet::View(second)];
    assert_eq!(sheets, [[Sheet::Base].as_slice(), &views].concat());
    assert!(matches!(deck.top(), Some(Card::Document(_))));
    // The help under the first document is not over it, and stays.
    assert!(deck.panel().is_some());
}

#[test]
fn back_to_a_view_takes_off_the_cards_above_it_and_gives_their_documents() {
    let [a, b, c] = views(3)[..] else { panic!() };
    let mut deck = deck_of([document(a), document(b), help(), document(c)]);
    let gone = deck.back_to(Some(a));
    assert_eq!(views_of(&gone), [c, b]);
    assert_eq!(deck.document().map(|document| document.view), Some(a));
    assert!(
        deck.panel().is_none(),
        "the help over the documents went too"
    );
    let mut deck = deck_of([document(a), document(b)]);
    let gone = deck.back_to(None);
    assert_eq!(views_of(&gone), [b, a]);
    assert!(deck.top().is_none());
    assert_eq!(deck.document_sheet(), Sheet::Base);
}

#[test]
fn back_to_a_view_not_on_the_deck_does_nothing() {
    let [a, b] = views(2)[..] else { panic!() };
    let mut deck = deck_of([document(a), help()]);
    assert!(deck.back_to(Some(b)).is_empty());
    assert_eq!(deck.sheets().count(), 3, "the deck is as it was");
    // The top document is the view asked for: only what is over it goes.
    assert!(deck.back_to(Some(a)).is_empty());
    assert!(deck.panel().is_none());
    assert!(deck.document().is_some());
}

#[test]
fn the_sheets_are_the_base_the_views_and_the_help_from_the_bottom_up() {
    let [view] = views(1)[..] else { panic!() };
    let deck = deck_of([document(view), help()]);
    let sheets: Vec<Sheet> = deck.sheets().collect();
    assert_eq!(sheets, [Sheet::Base, Sheet::View(view), Sheet::Help]);
}

#[test]
fn the_document_sheet_is_the_top_view_or_the_base() {
    let [a, b] = views(2)[..] else { panic!() };
    assert_eq!(Deck::default().document_sheet(), Sheet::Base);
    assert_eq!(deck_of([help()]).document_sheet(), Sheet::Base);
    let deck = deck_of([document(a), document(b), help()]);
    assert_eq!(deck.document_sheet(), Sheet::View(b));
    assert_eq!(deck.document().map(|document| document.view), Some(b));
}

#[test]
fn the_view_below_is_the_next_document_down_or_none_for_the_base() {
    let [a, b] = views(2)[..] else { panic!() };
    assert_eq!(Deck::default().view_below(), None);
    assert_eq!(deck_of([document(a)]).view_below(), None);
    let deck = deck_of([document(a), help(), document(b), help()]);
    assert_eq!(deck.view_below(), Some(a));
}

#[test]
fn a_view_is_held_while_it_is_on_the_deck() {
    let [a, b] = views(2)[..] else { panic!() };
    let mut deck = deck_of([document(a), help()]);
    assert!(deck.holds(a));
    assert!(!deck.holds(b));
    deck.back_to(None);
    assert!(!deck.holds(a));
}
