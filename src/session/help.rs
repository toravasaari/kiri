//! The help panel: a box over the middle of the document that lists the
//! pager's keys.

use unicode_width::UnicodeWidthStr;

use crate::pager::keys::{BINDINGS, Binding};
use crate::typeset::style::{Span, Style};
use crate::typeset::wrap;

/// Spans to write at a place in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// The row and the column, from 1 at the top left.
    pub row: usize,
    pub column: usize,
    pub spans: Vec<Span>,
}

/// The help panel in `style`, with notes in `muted`, in the middle of the
/// top `rows` rows of a window `columns` wide. A window too small for it
/// shows its top left. The keys that move between files are listed only
/// when there are `several` files.
pub fn panel(
    columns: usize,
    rows: usize,
    style: &Style,
    muted: &Style,
    several: bool,
) -> Vec<Placed> {
    let span = |text: &str, style: &Style| Span::new(text, style.clone());
    let bold = Style {
        bold: true,
        ..style.clone()
    };
    let listed: Vec<&Binding> = BINDINGS
        .iter()
        .filter(|binding| several || !binding.action.between_files())
        .collect();
    let names = listed.iter().map(|binding| binding.names.width());
    let names = names.max().unwrap_or(0);
    let whats = listed.iter().map(|binding| binding.what.width());
    let whats = whats.max().unwrap_or(0);
    // The keys, a gap of three and what they do.
    let inner = (names + 3 + whats).max(FOOTER.width());
    let line = |spans: Vec<Span>| {
        let used = wrap::width(&spans);
        let mut row = vec![span("│ ", style)];
        row.extend(spans);
        row.push(span(&" ".repeat(inner - used), style));
        row.push(span(" │", style));
        row
    };

    let mut lines = vec![vec![
        span("┌─", style),
        span(TITLE, &bold),
        span(&"─".repeat(inner + 1 - TITLE.width()), style),
        span("┐", style),
    ]];
    for binding in listed {
        let gap = names - binding.names.width() + 3;
        lines.push(line(vec![
            span(binding.names, &bold),
            span(&format!("{}{}", " ".repeat(gap), binding.what), style),
        ]));
    }
    // A blank line above the footer, where the window has room for it.
    if lines.len() + 3 <= rows {
        lines.push(line(Vec::new()));
    }
    lines.push(line(vec![span(FOOTER, muted)]));
    lines.push(vec![span(&format!("└{}┘", "─".repeat(inner + 2)), style)]);

    let width = inner + 4;
    let top = rows.saturating_sub(lines.len()) / 2 + 1;
    let left = columns.saturating_sub(width) / 2 + 1;
    lines
        .into_iter()
        .take(rows)
        .enumerate()
        .map(|(i, spans)| Placed {
            row: top + i,
            column: left,
            spans: wrap::cut(spans, columns),
        })
        .collect()
}

const TITLE: &str = " Keys ";
const FOOTER: &str = "Any key closes this.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::style;
    use crate::typeset::wrap::width;

    fn text(placed: &Placed) -> String {
        style::text(&placed.spans)
    }

    #[test]
    fn lists_every_binding_in_a_box() {
        let panel = panel(80, 40, &Style::default(), &Style::default(), true);
        let rows: Vec<String> = panel.iter().map(text).collect();
        assert_eq!(rows.len(), BINDINGS.len() + 4, "{rows:#?}");
        assert!(rows[0].starts_with("┌─ Keys ─"), "{rows:#?}");
        assert!(rows[0].ends_with('┐'), "{rows:#?}");
        assert!(rows[1].starts_with("│ j ↓ Enter "), "{rows:#?}");
        assert!(rows[1].contains("   down a line "), "{rows:#?}");
        assert!(rows[1].ends_with(" │"), "{rows:#?}");
        assert!(rows.iter().any(|row| row.contains("? h ")), "{rows:#?}");
        assert!(
            rows[rows.len() - 2].contains("Any key closes this"),
            "{rows:#?}"
        );
        assert!(rows[rows.len() - 1].starts_with('└'), "{rows:#?}");
        let first = width(&panel[0].spans);
        assert!(panel.iter().all(|placed| width(&placed.spans) == first));
    }

    /// The rows of the panel as plain text, for a window of 80 × 40.
    fn rows_for(several: bool) -> Vec<String> {
        let panel = panel(80, 40, &Style::default(), &Style::default(), several);
        panel.iter().map(text).collect()
    }

    #[test]
    fn the_panel_for_several_files_reads_as_it_always_has() {
        let expected = [
            "┌─ Keys ─────────────────────────────────────────┐",
            "│ j ↓ Enter       down a line                    │",
            "│ k ↑             up a line                      │",
            "│ Space f PgDn    down a page                    │",
            "│ b PgUp          up a page                      │",
            "│ d               down half a page               │",
            "│ u               up half a page                 │",
            "│ g Home          top                            │",
            "│ G End           bottom                         │",
            "│ :n Shift+PgDn   next file                      │",
            "│ :p Shift+PgUp   previous file                  │",
            "│ /               search                         │",
            "│ n               next match                     │",
            "│ N               previous match                 │",
            "│ Esc             hide the search                │",
            "│ r               reload the file                │",
            "│ Ctrl+L          redraw the screen              │",
            "│ F               follow: on, auto, off          │",
            "│ M               reading mode: on, off          │",
            "│ ← →             scroll a wide block            │",
            "│ H L             scroll it by its width         │",
            "│ Tab Shift+Tab   select a link, Enter copies it │",
            "│ W               wide blocks: wrap, scroll      │",
            "│ ? h             this help                      │",
            "│ q               quit                           │",
            "│                                                │",
            "│ Any key closes this.                           │",
            "└────────────────────────────────────────────────┘",
        ];
        assert_eq!(rows_for(true), expected);
    }

    #[test]
    fn the_panel_for_one_file_reads_as_it_always_has() {
        let expected = [
            "┌─ Keys ─────────────────────────────────────────┐",
            "│ j ↓ Enter       down a line                    │",
            "│ k ↑             up a line                      │",
            "│ Space f PgDn    down a page                    │",
            "│ b PgUp          up a page                      │",
            "│ d               down half a page               │",
            "│ u               up half a page                 │",
            "│ g Home          top                            │",
            "│ G End           bottom                         │",
            "│ /               search                         │",
            "│ n               next match                     │",
            "│ N               previous match                 │",
            "│ Esc             hide the search                │",
            "│ r               reload the file                │",
            "│ Ctrl+L          redraw the screen              │",
            "│ F               follow: on, auto, off          │",
            "│ M               reading mode: on, off          │",
            "│ ← →             scroll a wide block            │",
            "│ H L             scroll it by its width         │",
            "│ Tab Shift+Tab   select a link, Enter copies it │",
            "│ W               wide blocks: wrap, scroll      │",
            "│ ? h             this help                      │",
            "│ q               quit                           │",
            "│                                                │",
            "│ Any key closes this.                           │",
            "└────────────────────────────────────────────────┘",
        ];
        assert_eq!(rows_for(false), expected);
    }

    #[test]
    fn lists_the_reload_key() {
        let panel = panel(80, 24, &Style::default(), &Style::default(), false);
        let rows: Vec<String> = panel.iter().map(text).collect();
        assert!(
            rows.iter()
                .any(|row| row.contains("r ") && row.contains("reload")),
            "{rows:#?}"
        );
    }

    #[test]
    fn lists_the_redraw_key() {
        let panel = panel(80, 24, &Style::default(), &Style::default(), false);
        let rows: Vec<String> = panel.iter().map(text).collect();
        assert!(
            rows.iter()
                .any(|row| row.starts_with("│ Ctrl+L ") && row.contains("   redraw the screen ")),
            "{rows:#?}"
        );
    }

    #[test]
    fn lists_the_follow_key() {
        let panel = panel(80, 24, &Style::default(), &Style::default(), false);
        let rows: Vec<String> = panel.iter().map(text).collect();
        assert!(
            rows.iter()
                .any(|row| row.starts_with("│ F ") && row.contains("follow: on, auto, off")),
            "{rows:#?}"
        );
    }

    #[test]
    fn lists_the_search_keys() {
        let panel = panel(80, 23, &Style::default(), &Style::default(), true);
        let rows: Vec<String> = panel.iter().map(text).collect();
        for (names, what) in [
            ("/", "search"),
            ("n", "next match"),
            ("N", "previous match"),
        ] {
            let listed = |row: &String| {
                let row = row.trim_start_matches("│ ");
                row.starts_with(&format!("{names} ")) && row.contains(&format!("   {what} "))
            };
            assert!(rows.iter().any(listed), "{names}: {rows:#?}");
        }
    }

    #[test]
    fn lists_the_keys_between_files_only_for_several() {
        let listed = |several| {
            let panel = panel(80, 40, &Style::default(), &Style::default(), several);
            panel.iter().map(text).collect::<Vec<_>>()
        };
        for (names, what) in [
            (":n Shift+PgDn", "next file"),
            (":p Shift+PgUp", "previous file"),
        ] {
            let found = |rows: &[String]| {
                rows.iter().any(|row| {
                    let row = row.trim_start_matches("│ ");
                    row.starts_with(&format!("{names} ")) && row.contains(&format!("   {what} "))
                })
            };
            assert!(found(&listed(true)), "{names}");
            assert!(!found(&listed(false)), "{names}");
        }
        assert_eq!(listed(false).len(), listed(true).len() - 2);
    }

    #[test]
    fn a_window_too_short_cuts_the_panel_s_last_rows() {
        // The status line takes one of the 24 rows.
        let rows = 23;
        let panel = panel(80, rows, &Style::default(), &Style::default(), true);
        assert_eq!(panel.len(), rows);
        assert!(panel[0].row >= 1 && panel[panel.len() - 1].row <= rows);
        let tall = super::panel(80, 40, &Style::default(), &Style::default(), true);
        assert_eq!(tall.len(), BINDINGS.len() + 4);
    }

    #[test]
    fn sits_in_the_middle_of_the_window() {
        let panel = panel(80, 24, &Style::default(), &Style::default(), true);
        let box_width = width(&panel[0].spans);
        assert_eq!(panel[0].column, (80 - box_width) / 2 + 1);
        assert_eq!(panel[0].row, (24 - panel.len()) / 2 + 1);
        for (i, placed) in panel.iter().enumerate() {
            assert_eq!(placed.row, panel[0].row + i);
            assert_eq!(placed.column, panel[0].column);
        }
    }

    #[test]
    fn a_small_window_shows_what_fits() {
        let panel = panel(20, 5, &Style::default(), &Style::default(), true);
        assert_eq!(panel.len(), 5);
        for placed in &panel {
            assert_eq!(placed.column, 1);
            assert!(width(&placed.spans) <= 20, "{placed:?}");
        }
        assert_eq!(panel[0].row, 1);
    }
}
