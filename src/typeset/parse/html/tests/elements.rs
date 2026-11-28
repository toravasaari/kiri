//! The table of HTML elements, tested: each row has a name of its own, and
//! the rows say what they hold, what they are and what reads them.

use crate::typeset::parse::html::elements::*;

#[test]
fn a_row_has_a_lower_case_name_of_its_own() {
    for (at, row) in TABLE.iter().enumerate() {
        assert_eq!(row.name, row.name.to_ascii_lowercase());
        assert!(
            TABLE[..at].iter().all(|other| other.name != row.name),
            "{} twice",
            row.name
        );
    }
}

#[test]
fn the_kind_of_a_row_says_what_it_holds() {
    for row in &TABLE {
        assert_eq!(
            matches!(row.kind, Kind::Void | Kind::VoidBlock),
            row.holds == Holds::Nothing,
            "{}",
            row.name
        );
    }
}

#[test]
fn only_the_rows_that_say_so_are_headings_phrases_wrappers_lists_and_source() {
    let names = |keep: fn(&Element) -> bool| -> Vec<&str> {
        TABLE
            .iter()
            .filter(|row| keep(row))
            .map(|row| row.name)
            .collect()
    };
    assert_eq!(
        names(|row| matches!(row.treatment, Treatment::Heading(_))),
        ["h1", "h2", "h3", "h4", "h5", "h6"]
    );
    assert_eq!(
        names(|row| matches!(row.treatment, Treatment::Wrapper(_) | Treatment::Details)),
        ["p", "div", "center", "details"]
    );
    assert_eq!(
        names(|row| matches!(row.treatment, Treatment::Phrase(_))),
        [
            "kbd", "mark", "sub", "sup", "b", "strong", "i", "em", "s", "del", "code", "u", "span",
            "ins", "strike", "a", "picture"
        ]
    );
    // The block elements left as source for now.
    assert_eq!(
        names(|row| row.treatment == Treatment::Source
            && matches!(row.kind, Kind::Block | Kind::VoidBlock)),
        [
            "blockquote",
            "dl",
            "dt",
            "dd",
            "figure",
            "figcaption",
            "hr",
            "table"
        ]
    );
    assert_eq!(
        names(|row| matches!(row.treatment, Treatment::List | Treatment::Item)),
        ["ol", "ul", "li"]
    );
    assert_eq!(names(|row| row.treatment == Treatment::Input), ["input"]);
    assert_eq!(
        names(|row| row.kind == Kind::RawText),
        ["pre", "script", "style", "textarea"]
    );
    assert_eq!(heading_level("h3"), Some(3));
    assert_eq!(heading_level("h7"), None);
    assert_eq!(heading_level("hr"), None);
}

#[test]
fn u_and_center_are_only_at_the_level_all() {
    let level = |name| get(name).map(|row| row.level);
    assert_eq!(level("u"), Some(Level::All));
    assert_eq!(level("center"), Some(Level::All));
    assert_eq!(level("div"), Some(Level::Github));
    assert_eq!(level("script"), Some(Level::Neither));
}
