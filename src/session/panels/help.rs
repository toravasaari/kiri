//! The help as rows of keys and what they do, made from the command table
//! by topic.

use unicode_width::UnicodeWidthStr;

use crate::session::commands::{COMMANDS, Topic};
use crate::typeset::caps::Capabilities;
use crate::typeset::color::Rgb;
use crate::typeset::style::{Line, Span, Style};
use crate::typeset::theme::Theme;

/// The topics in the order the help shows them, each with its heading.
pub(crate) const TOPICS: [(Topic, &str); 6] = [
    (Topic::Moving, "Moving"),
    (Topic::Searching, "Searching"),
    (Topic::Links, "Links"),
    (Topic::Files, "Files"),
    (Topic::View, "View"),
    (Topic::Help, "Help"),
];

/// The title of the help's frame.
pub(crate) const TITLE: &str = "Help";

/// The cells between the keys and what they do.
const GAP: usize = 3;

/// The room what the keys do needs at least to stay beside them: with less
/// it goes below them.
const LEAST: usize = 12;

/// How much of the way from the text colour to the background the keys
/// are: bold, but greyer than what they do.
const GREYER: f32 = 0.25;

/// The styles of the help's rows.
#[derive(Debug, Clone)]
pub(crate) struct Look {
    pub(crate) heading: Style,
    pub(crate) keys: Style,
    pub(crate) text: Style,
}

impl Look {
    /// The look the theme gives the help: the text on the page's colours,
    /// the headings as a second level heading is, and the keys bold in
    /// the text's colour (the theme's, or else the terminal's) moved towards the page's, so they stand out less
    /// than a heading and more than a note.
    pub(crate) fn new(theme: &Theme, caps: &Capabilities) -> Self {
        let text = Style {
            fg: theme.foreground,
            bg: theme.background,
            ..Style::default()
        };
        let level = theme.heading(2);
        let heading = Style {
            fg: level.color.or(theme.foreground),
            bold: true,
            ..text.clone()
        };
        // The page as the fades take it: the theme's colours, then the ones
        // the terminal told.
        let page = Rgb::new(theme, caps).and_then(|rgb| rgb.page());
        let greyer = (page.and_then(|(fg, bg)| fg.mix(bg, GREYER))).or(theme.muted);
        let keys = Style {
            fg: greyer.or(theme.foreground),
            bold: true,
            ..text.clone()
        };
        Self {
            heading,
            keys,
            text,
        }
    }
}

/// The cells of the longest row with the keys and what they do side by
/// side: the text width at which nothing wraps.
pub(crate) fn fit() -> usize {
    let listed = || COMMANDS.iter().filter(|command| command.listed());
    let whats = listed().map(|command| command.what.width()).max();
    keys_width() + GAP + whats.unwrap_or(0)
}

/// The width of the column of keys.
fn keys_width() -> usize {
    let names = COMMANDS.iter().filter(|command| command.listed());
    names
        .map(|command| command.names.width())
        .max()
        .unwrap_or(0)
}

/// `text` broken at spaces into rows of at most `width` cells; a word
/// longer than that stays whole.
fn wrapped(text: &str, width: usize) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    for word in text.split(' ') {
        match rows.last_mut() {
            Some(row) if row.width() + 1 + word.width() <= width => {
                row.push(' ');
                row.push_str(word);
            }
            _ => rows.push(word.to_owned()),
        }
    }
    rows
}

/// The help laid out in `width` cells: a blank row, then for each topic
/// its heading, a row for each of its commands with the keys in a column
/// and what they do beside them, and a blank row. What they do wraps under
/// its own column; where `width` is too narrow for two columns it goes
/// under the keys instead.
pub(crate) fn lines(width: usize, look: &Look) -> Vec<Line> {
    let span = |text: &str, style: &Style| Span::new(text, style.clone());
    let row = |spans: Vec<Span>| Line::new(spans);
    let names = keys_width();
    let room = width.saturating_sub(names + GAP);
    let mut rows = vec![row(Vec::new())];
    for (topic, heading) in TOPICS {
        rows.push(row(vec![span(heading, &look.heading)]));
        let listed = COMMANDS.iter().filter(|command| command.topic == topic);
        for command in listed.filter(|command| command.listed()) {
            if room < LEAST {
                rows.push(row(vec![span(command.names, &look.keys)]));
                let below = width.saturating_sub(2).max(1);
                for part in wrapped(command.what, below) {
                    rows.push(row(vec![span(&format!("  {part}"), &look.text)]));
                }
                continue;
            }
            let gap = " ".repeat(names - command.names.width() + GAP);
            let mut parts = wrapped(command.what, room).into_iter();
            let first = parts.next().unwrap_or_default();
            rows.push(row(vec![
                span(command.names, &look.keys),
                span(&format!("{gap}{first}"), &look.text),
            ]));
            for part in parts {
                let indent = " ".repeat(names + GAP);
                rows.push(row(vec![span(&format!("{indent}{part}"), &look.text)]));
            }
        }
        rows.push(row(Vec::new()));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::color::Color;
    use crate::typeset::style::text;

    fn look() -> Look {
        let theme = Theme {
            foreground: Some(Color::Rgb(220, 220, 220)),
            background: Some(Color::Rgb(20, 20, 20)),
            ..Theme::default()
        };
        Look::new(&theme, &Capabilities::ansi(80))
    }

    fn rows(width: usize) -> Vec<String> {
        lines(width, &look())
            .iter()
            .map(|line| text(&line.spans))
            .collect()
    }

    fn listed() -> impl Iterator<Item = &'static crate::session::commands::Command> {
        COMMANDS.iter().filter(|command| command.listed())
    }

    #[test]
    fn the_keys_are_a_column_and_what_they_do_follows_after_three_cells() {
        let rows = rows(fit());
        let names = keys_width();
        for command in listed() {
            let row = rows
                .iter()
                .find(|row| row.starts_with(command.names) && row.contains(command.what))
                .unwrap_or_else(|| panic!("{}: {rows:#?}", command.names));
            let after = &row[command.names.len()..];
            assert_eq!(after.trim_start(), command.what);
            let at = row.find(command.what).unwrap();
            assert_eq!(row[..at].width(), names + GAP, "{row}");
        }
    }

    #[test]
    fn each_topic_lists_exactly_its_commands_in_table_order() {
        let rows = rows(fit());
        for (topic, heading) in TOPICS {
            let at = rows.iter().position(|row| row == heading).unwrap();
            let shown: Vec<&String> = rows[at + 1..]
                .iter()
                .take_while(|row| !row.is_empty())
                .collect();
            let expected: Vec<&str> = (COMMANDS.iter())
                .filter(|command| command.topic == topic && command.listed())
                .map(|command| command.names)
                .collect();
            assert_eq!(shown.len(), expected.len(), "{heading}: {shown:#?}");
            for (row, names) in shown.iter().zip(&expected) {
                // The keys, then at least the gap, then what they do.
                let rest = row.strip_prefix(names);
                assert!(
                    rest.is_some_and(|rest| rest.starts_with(&" ".repeat(GAP))),
                    "{heading}: {row} for {names}"
                );
            }
        }
    }

    #[test]
    fn the_rows_have_no_bullet_and_no_dash() {
        for row in rows(fit()) {
            assert!(!row.contains('•') && !row.contains('–'), "{row}");
        }
    }

    #[test]
    fn a_heading_has_no_blank_row_under_it_and_a_topic_ends_with_one() {
        let rows = rows(fit());
        assert_eq!(rows[0], "", "a blank row opens the help");
        let at = |heading: &str| rows.iter().position(|row| row == heading).unwrap();
        for (_, heading) in TOPICS {
            assert!(!rows[at(heading) + 1].is_empty(), "{heading}");
            assert_eq!(rows[at(heading) - 1], "", "{heading}");
        }
        assert_eq!(rows.last().unwrap(), "");
        // One blank row between topics, never two.
        assert!(
            rows.windows(2)
                .all(|pair| !(pair[0].is_empty() && pair[1].is_empty()))
        );
    }

    #[test]
    fn at_the_fit_width_nothing_wraps() {
        let rows = rows(fit());
        assert!(rows.iter().all(|row| row.width() <= fit()), "{rows:#?}");
        let expected = listed().count() + 2 * TOPICS.len() + 1;
        assert_eq!(rows.len(), expected);
    }

    #[test]
    fn a_long_description_wraps_under_its_own_column() {
        let width = fit() - 8;
        let rows = rows(width);
        assert!(rows.iter().all(|row| row.width() <= width), "{rows:#?}");
        let at = rows.iter().position(|row| row.starts_with("Esc ")).unwrap();
        assert!(rows[at].ends_with("clear selection,"), "{}", rows[at]);
        let indent = " ".repeat(keys_width() + GAP);
        assert!(
            rows[at + 1].starts_with(&format!("{indent}search, pill")),
            "{}",
            rows[at + 1]
        );
    }

    #[test]
    fn in_a_frame_too_narrow_for_two_columns_what_they_do_goes_below_the_keys() {
        let rows = rows(keys_width() + GAP + LEAST - 1);
        let at = rows.iter().position(|row| row == "Backspace").unwrap();
        assert_eq!(rows[at + 1], "  go back");
        let at = rows.iter().position(|row| row == "Tab Shift+Tab").unwrap();
        assert_eq!(rows[at + 1], "  select a link or");
        assert_eq!(rows[at + 2], "  animation");
    }

    #[test]
    fn the_keys_are_bold_and_greyer_than_the_text_and_the_headings_are_the_themes() {
        let look = look();
        assert!(look.keys.bold);
        assert_ne!(look.keys.fg, look.text.fg);
        assert_eq!(look.keys.fg, Some(Color::Rgb(170, 170, 170)));
        assert!(!look.text.bold);
        let theme = Theme::default();
        assert_eq!(look.heading.fg, theme.heading(2).color);
        let lines = lines(fit(), &look);
        let heading = &lines[1].spans[0];
        assert_eq!(
            (heading.text.as_str(), heading.style.bold),
            ("Moving", true)
        );
        let row = &lines[2].spans;
        assert_eq!(row[0].style.fg, look.keys.fg);
        assert_eq!(row[1].style.fg, look.text.fg);
    }

    #[test]
    fn a_theme_without_colours_keeps_the_keys_bold() {
        let look = Look::new(
            &Theme {
                foreground: None,
                background: None,
                muted: None,
                ..Theme::default()
            },
            &Capabilities::ansi(80),
        );
        assert!(look.keys.bold);
    }

    fn probed(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> Capabilities {
        Capabilities {
            foreground: Some(Color::Rgb(fg.0, fg.1, fg.2)),
            background: Some(Color::Rgb(bg.0, bg.1, bg.2)),
            ..Capabilities::ansi(80)
        }
    }

    #[test]
    fn the_built_in_theme_gets_greyer_keys_from_the_colours_the_terminal_told() {
        // The built-in theme has no colours of its own.
        let theme = Theme::default();
        let dark = Look::new(&theme, &probed((220, 220, 220), (20, 20, 20)));
        assert_eq!(dark.keys.fg, Some(Color::Rgb(170, 170, 170)));
        assert!(dark.keys.bold);
        let light = Look::new(&theme, &probed((30, 30, 30), (250, 250, 250)));
        assert_eq!(light.keys.fg, Some(Color::Rgb(85, 85, 85)));
        // Greyer: nearer the page than the text is, on either page.
        assert!(matches!(light.keys.fg, Some(Color::Rgb(85, ..))));
    }

    #[test]
    fn a_terminal_that_told_no_colours_keeps_the_keys_bold_in_the_text_colour() {
        let look = Look::new(&Theme::default(), &Capabilities::ansi(80));
        assert!(look.keys.bold);
        assert_eq!(look.keys.fg, look.text.fg);
    }
}
