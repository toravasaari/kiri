use super::*;
use crate::session::deck::Card;
use crate::session::prompt::Prompt;

/// A sidebar 30 columns wide and a document beside it, in a window of 100
/// columns and 20 rows, the last row being the status line's.
fn side_by_side() -> Areas {
    Areas::new(vec![
        Region {
            role: Role::Contents,
            area: Area {
                column: 1,
                row: 1,
                columns: 30,
                rows: 19,
            },
        },
        Region {
            role: Role::Document,
            area: Area {
                column: 31,
                row: 1,
                columns: 70,
                rows: 19,
            },
        },
    ])
}

#[test]
fn the_pointer_in_the_sidebar_is_answered_by_the_sidebar() {
    let map = side_by_side();
    assert_eq!(
        map.under(5, 3),
        Some(Under {
            role: Role::Contents,
            column: 5,
            row: 3
        })
    );
    assert_eq!(
        map.under(30, 19).map(|under| under.role),
        Some(Role::Contents)
    );
}

#[test]
fn the_pointer_in_the_document_gets_the_column_in_the_documents_area() {
    let map = side_by_side();
    assert_eq!(
        map.under(31, 1),
        Some(Under {
            role: Role::Document,
            column: 1,
            row: 1
        })
    );
    assert_eq!(
        map.under(45, 7),
        Some(Under {
            role: Role::Document,
            column: 15,
            row: 7
        })
    );
}

#[test]
fn the_pointer_outside_every_area_is_answered_by_none() {
    let map = side_by_side();
    // The status line's row, and the columns past the window.
    assert_eq!(map.under(5, 20), None);
    assert_eq!(map.under(101, 3), None);
    assert_eq!(map.under(0, 3), None);
}

#[test]
fn the_area_of_a_role_is_found() {
    let map = side_by_side();
    assert_eq!(map.area(Role::Contents).map(|area| area.columns), Some(30));
    assert_eq!(map.area(Role::Document).map(|area| area.column), Some(31));
}

#[test]
fn the_map_of_a_window_is_the_document_over_all_of_it() {
    let state = State::new(crate::session::settings::SearchCase::Smart);
    let map = Areas::of((80, 24), &state);
    assert_eq!(
        map,
        Areas::new(vec![Region {
            role: Role::Document,
            area: Area::window((80, 24)),
        }])
    );
    assert_eq!(
        map.under(80, 24),
        Some(Under {
            role: Role::Document,
            column: 80,
            row: 24
        })
    );
}

#[test]
fn a_key_goes_to_the_focused_area_when_no_card_is_open() {
    let mut deck = Deck::default();
    assert_eq!(
        route(&mut deck, Role::Contents, Key::Char('j')),
        Route::Area(Role::Contents)
    );
}

#[test]
fn a_key_goes_to_the_document_when_it_has_the_focus() {
    let mut deck = Deck::default();
    assert_eq!(
        route(&mut deck, Role::Document, Key::Char('j')),
        Route::Document
    );
}

#[test]
fn a_card_of_the_deck_takes_the_key_before_the_focused_area() {
    let mut deck = Deck::default();
    deck.open(Card::Prompt(Prompt::default()));
    assert_eq!(
        route(&mut deck, Role::Contents, Key::Char('j')),
        Route::Deck(Routed::Taken)
    );
}

#[test]
fn a_document_card_on_top_takes_the_key_whatever_area_has_the_focus() {
    let mut state = State::new(crate::session::settings::SearchCase::Smart);
    state.push_view(
        crate::session::sources::ViewId::BASE,
        Vec::new(),
        std::time::Instant::now(),
    );
    assert!(matches!(state.deck.top(), Some(Card::Document(_))));
    for focus in [Role::Contents, Role::Document] {
        assert_eq!(
            route(&mut state.deck, focus, Key::Char('j')),
            Route::Document
        );
    }
}
