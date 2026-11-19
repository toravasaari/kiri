//! The contents bar at the left of the window: whether it is shown, how
//! wide it is, which of its entries are folded, and what it draws.
//!
//! Whether the bar shows is kept for the session in three values ([`Visibility`]),
//! not undone by the window changing size. The entries are the contents of
//! the documents shown ([`Contents`]), which this does not make; it only says
//! which of them are folded and which rows the bar has.

use std::collections::BTreeSet;

use unicode_width::UnicodeWidthStr;

use crate::present::keys::Key;
use crate::session::status;
use crate::typeset::contents::{Contents, Entry};
use crate::typeset::style::{Line, Span, Style};

/// The bar's width in cells, in a window wide enough for it.
pub(crate) const WIDTH: u16 = 30;

/// The columns the document keeps at least for the bar to show itself.
const AUTO_LEAVES: u16 = 80;

/// The columns the document keeps at least when the reader opened the bar.
const OPEN_LEAVES: u16 = 40;

/// Whether the bar shows, as the reader's choices left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    /// The start: the bar shows when the window leaves the document room
    /// for 80 columns.
    #[default]
    Auto,
    /// The reader opened it: it shows when the document keeps 40 columns.
    Open,
    /// The reader closed it.
    Closed,
}

/// The key of an entry among the folded ones: its file's name and its id,
/// which a file has none of.
type FoldKey = (String, Option<String>);

/// What the session keeps of the bar: whether it shows, and the entry the
/// reader selected in it, as an index of the contents' entries.
#[derive(Debug, Default)]
pub(crate) struct Sidebar {
    pub(crate) visibility: Visibility,
    pub(crate) selected: Option<usize>,
}

impl Sidebar {
    /// A bar that is `visibility` at the start.
    pub(crate) fn new(visibility: Visibility) -> Self {
        Self {
            visibility,
            selected: None,
        }
    }

    /// The columns the bar takes in a window of `columns`, none when it is
    /// not shown: when it is closed, when there is nothing to list
    /// (`entries` is false) and when the document would be left too narrow.
    pub(crate) fn width(&self, columns: u16, entries: bool) -> u16 {
        let leaves = match self.visibility {
            _ if !entries => return 0,
            Visibility::Closed => return 0,
            Visibility::Auto => AUTO_LEAVES,
            Visibility::Open => OPEN_LEAVES,
        };
        let width = WIDTH.min(columns / 3);
        if columns - width >= leaves { width } else { 0 }
    }
}

/// The entries of the contents that are folded: their children are not
/// listed. They belong to the session, by the name of the file (as the
/// session's sources have it) and the id of the heading, so a file opened
/// again, or come back to, has its folds as it left them.
#[derive(Debug, Default)]
pub(crate) struct Folds {
    folded: BTreeSet<FoldKey>,
}

impl Folds {
    /// Whether the entry, which is of the file called `name`, is folded.
    pub(crate) fn is_folded(&self, name: &str, entry: &Entry) -> bool {
        self.folded.contains(&(name.to_owned(), entry.id.clone()))
    }

    /// Folds the entry, which is of the file called `name`, or opens it.
    pub(crate) fn fold(&mut self, name: &str, entry: &Entry, folded: bool) {
        let key = (name.to_owned(), entry.id.clone());
        match folded {
            true => self.folded.insert(key),
            false => self.folded.remove(&key),
        };
    }

    /// Lets go of the folds of the headings that the files called `names`
    /// (by file number) no longer have in `contents`, as after a file was
    /// read again. The folds of files that are not among `names` are not
    /// touched: they are of documents that are not shown now.
    pub(crate) fn keep(&mut self, names: &[String], contents: &Contents) {
        self.folded.retain(|(name, id)| {
            let files: Vec<usize> = (names.iter().enumerate())
                .filter(|(_, other)| *other == name)
                .map(|(file, _)| file)
                .collect();
            files.is_empty()
                || (contents.entries().iter())
                    .any(|entry| files.contains(&entry.file) && entry.id.as_ref() == id.as_ref())
        });
    }
}

/// The styles the bar's rows are drawn in, from the theme.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Looks {
    /// An entry.
    pub(crate) page: Style,
    /// The fold glyph.
    pub(crate) muted: Style,
    /// The entry the reader is in.
    pub(crate) mark: Style,
    /// The entry the reader is in while another is selected: a quiet look,
    /// for the selection has the mark's.
    pub(crate) quiet: Style,
}

impl Looks {
    /// The style of the fold glyph: the muted one, except in the marked
    /// row, where it is the mark's own.
    fn glyph(&self, marked: bool) -> &Style {
        match marked {
            true => &self.mark,
            false => &self.muted,
        }
    }
}

/// What a key does in the bar when it has the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BarKey {
    Up,
    Down,
    /// Folds the entry, or goes to its parent when it is folded or has no
    /// entries under it.
    Left,
    /// Opens a folded entry.
    Right,
    First,
    Last,
    /// Goes to the heading selected.
    Enter,
}

impl BarKey {
    /// The bar's key for `key`, none for a key that is the document's.
    pub(crate) fn of(key: Key) -> Option<Self> {
        Some(match key {
            Key::Char('j') | Key::Down => Self::Down,
            Key::Char('k') | Key::Up => Self::Up,
            Key::Char('h') | Key::Left => Self::Left,
            Key::Char('l') | Key::Right => Self::Right,
            Key::Char('g') | Key::Home => Self::First,
            Key::Char('G') | Key::End => Self::Last,
            Key::Enter => Self::Enter,
            _ => return None,
        })
    }
}

/// What a key in the bar came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chosen {
    /// The entry selected, which may be the same, as an index of the entries.
    Selected(usize),
    /// The reader asked to go to the entry.
    Jump(usize),
}

/// Answers `key` with the entry `selected` of `contents` selected: moves
/// the selection among the entries that are listed, or folds or opens an
/// entry in `folds`.
pub(crate) fn press(
    key: BarKey,
    selected: usize,
    contents: &Contents,
    (folds, names): (&mut Folds, &[String]),
) -> Chosen {
    let entries = contents.entries();
    let listed = listed(contents, (folds, names));
    // The entry that holds the selection, which a fold may have hidden.
    let Some(here) = listed.iter().rposition(|&at| at <= selected) else {
        return Chosen::Selected(selected);
    };
    let at = listed[here];
    let entry = &entries[at];
    let name = names.get(entry.file).map_or("", String::as_str);
    let parent = || {
        let above = entries[..at].iter().enumerate().rev();
        let mut same_file = above.take_while(|(_, other)| other.file == entry.file);
        same_file.find(|(_, other)| other.level < entry.level)
    };
    let folded = has_children(entries, at) && folds.is_folded(name, entry);
    Chosen::Selected(match key {
        BarKey::Down => listed[(here + 1).min(listed.len() - 1)],
        BarKey::Up => listed[here.saturating_sub(1)],
        BarKey::First => listed[0],
        BarKey::Last => listed[listed.len() - 1],
        BarKey::Enter => return Chosen::Jump(at),
        BarKey::Left if has_children(entries, at) && !folded => {
            folds.fold(name, entry, true);
            at
        }
        BarKey::Left => parent().map_or(at, |(parent, _)| parent),
        BarKey::Right => {
            if folded {
                folds.fold(name, entry, false);
            }
            at
        }
    })
}

/// The rows of a bar `columns` wide and `rows` tall: one line for each
/// entry that is listed, from the first that keeps in view the entry
/// selected, or when none is, `current`, the one the reader is in, and
/// blank lines after the last. Every line is as wide as the bar. With an
/// entry `selected` the bar has the focus: the selection has the mark's look
/// and the entry the reader is in the quiet one.
pub(crate) fn rows(
    contents: &Contents,
    (folds, names): (&Folds, &[String]),
    (current, selected): (Option<usize>, Option<usize>),
    (columns, rows): (usize, usize),
    looks: &Looks,
) -> Vec<Line> {
    let entries = contents.entries();
    let is_folded = |entry: &Entry| {
        let name = names.get(entry.file).map_or("", String::as_str);
        folds.is_folded(name, entry)
    };
    let listed = listed(contents, (folds, names));
    // The listed entry that holds the reader: the last one at or before
    // `current`, which is the one that hides it when it is folded away.
    let holding =
        |entry: Option<usize>| entry.and_then(|entry| listed.iter().rposition(|&at| at <= entry));
    let marked = holding(current);
    // The row the bar keeps in view: the selection when there is one,
    // else the reader's.
    let kept = holding(selected).or(marked);
    let first = match kept {
        Some(kept) if listed.len() > rows => {
            (kept.saturating_sub(rows / 2)).min(listed.len() - rows)
        }
        _ => 0,
    };
    // Headings go one level in when files are listed above them.
    let shift = usize::from(entries.iter().any(|entry| entry.level == 0));
    let blank = || Line::new(vec![Span::new(&" ".repeat(columns), looks.page.clone())]);
    let shown = listed.iter().enumerate().skip(first).take(rows);
    let mut lines: Vec<Line> = shown
        .map(|(position, &at)| {
            // The bar has the focus when something is selected: the
            // selection then has the mark's look and the entry the reader
            // is in a quieter one, as it is the other's place that counts.
            let is_selected = Some(position) == holding(selected);
            let is_marked = match selected {
                Some(_) => is_selected,
                None => Some(position) == marked,
            };
            let style = match (is_marked, Some(position) == marked) {
                (true, _) => &looks.mark,
                (false, true) => &looks.quiet,
                (false, false) => &looks.page,
            };
            let entry = &entries[at];
            let indent = match entry.level {
                0 => 0,
                level => 2 * (usize::from(level) - 1 + shift),
            };
            let glyph = match (has_children(entries, at), is_folded(entry)) {
                (false, _) => ' ',
                (true, false) => '▾',
                (true, true) => '▸',
            };
            row(
                (&" ".repeat(1 + indent), glyph, &entry.text),
                columns,
                (style, looks.glyph(is_marked)),
            )
        })
        .collect();
    lines.resize_with(rows, blank);
    lines
}

/// The entries that are listed, as indexes of the contents' entries: those
/// that no folded entry above them hides.
pub(crate) fn listed(contents: &Contents, (folds, names): (&Folds, &[String])) -> Vec<usize> {
    let entries = contents.entries();
    let mut listed = Vec::new();
    let mut hidden_by: Option<(usize, u8)> = None;
    for (at, entry) in entries.iter().enumerate() {
        if let Some((file, level)) = hidden_by {
            if entry.file == file && entry.level > level {
                continue;
            }
            hidden_by = None;
        }
        listed.push(at);
        let name = names.get(entry.file).map_or("", String::as_str);
        if folds.is_folded(name, entry) && has_children(entries, at) {
            hidden_by = Some((entry.file, entry.level));
        }
    }
    listed
}

/// Whether the entry `at` has entries under it: the next one is of its file
/// and deeper.
fn has_children(entries: &[Entry], at: usize) -> bool {
    let entry = &entries[at];
    (entries.get(at + 1)).is_some_and(|next| next.file == entry.file && next.level > entry.level)
}

/// A row of `columns` cells: the indent, the glyph, a space and the text,
/// cut with an ellipsis where it does not fit, and filled to the width.
fn row(
    (indent, glyph, text): (&str, char, &str),
    columns: usize,
    (style, glyph_style): (&Style, &Style),
) -> Line {
    let room = columns.saturating_sub(indent.width() + 2);
    let text = status::head(text, room);
    let used = indent.width() + 2 + text.width();
    let mut spans = vec![
        Span::new(indent, style.clone()),
        Span::new(&glyph.to_string(), glyph_style.clone()),
        Span::new(&format!(" {text}"), style.clone()),
    ];
    if used < columns {
        spans.push(Span::new(&" ".repeat(columns - used), style.clone()));
    }
    // An indent deeper than the bar leaves no room for the glyph: cut.
    Line::new(crate::typeset::wrap::cut(spans, columns))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::parse::parse;
    use crate::typeset::targets::targets;

    /// The names of the files of a lone document.
    fn a_md() -> Vec<String> {
        vec!["a.md".to_owned()]
    }

    fn contents_of(source: &str) -> Contents {
        let document = parse(source);
        let found = targets([&document]);
        crate::typeset::contents::contents([(None, &document)], &found)
    }

    #[test]
    fn auto_shows_the_bar_when_the_document_keeps_80_columns() {
        let bar = Sidebar::default();
        assert_eq!(bar.width(109, true), 0);
        assert_eq!(bar.width(110, true), WIDTH);
        assert_eq!(bar.width(300, true), WIDTH);
    }

    #[test]
    fn a_document_without_headings_has_no_bar() {
        for visibility in [Visibility::Auto, Visibility::Open] {
            assert_eq!(Sidebar::new(visibility).width(200, false), 0);
        }
    }

    #[test]
    fn closed_has_no_bar_in_any_window() {
        assert_eq!(Sidebar::new(Visibility::Closed).width(300, true), 0);
    }

    #[test]
    fn the_bar_is_at_most_a_third_of_the_window_and_open_needs_40_columns_left() {
        let open = Sidebar::new(Visibility::Open);
        assert_eq!(open.width(90, true), 30);
        assert_eq!(open.width(75, true), 25);
        assert_eq!(open.width(60, true), 20);
        // A bar of 19 leaves 40 of 59 columns, and 39 of 58.
        assert_eq!(open.width(59, true), 19);
        assert_eq!(open.width(58, true), 0);
    }

    #[test]
    fn a_fold_is_kept_by_the_file_and_the_id() {
        let contents = contents_of("# One\n\n## Two\n");
        let mut folds = Folds::default();
        let entry = &contents.entries()[0];
        assert!(!folds.is_folded("a.md", entry));
        folds.fold("a.md", entry, true);
        assert!(folds.is_folded("a.md", entry));
        assert!(!folds.is_folded("a.md", &contents.entries()[1]));
        folds.fold("a.md", entry, false);
        assert!(!folds.is_folded("a.md", entry));
    }

    #[test]
    fn the_folds_of_headings_that_are_gone_are_dropped() {
        let before = contents_of("# One\n\n## Two\n\n## Three\n");
        let mut folds = Folds::default();
        for entry in before.entries() {
            folds.fold("a.md", entry, true);
        }
        let after = contents_of("# One\n\n## Three\n\n## Four\n");
        folds.keep(&["a.md".to_owned()], &after);
        let folded: Vec<bool> = after
            .entries()
            .iter()
            .map(|e| folds.is_folded("a.md", e))
            .collect();
        assert_eq!(folded, [true, true, false]);
        // The dropped one is not folded if its heading comes back.
        assert!(!folds.is_folded("a.md", &before.entries()[1]));
    }

    fn looks() -> Looks {
        let mark = Style {
            bold: true,
            ..Style::default()
        };
        let muted = Style {
            italic: true,
            ..Style::default()
        };
        Looks {
            page: Style::default(),
            muted,
            mark,
            quiet: Style {
                underline: crate::typeset::color::Underline::Single,
                ..Style::default()
            },
        }
    }

    fn text_of(line: &Line) -> String {
        line.spans.iter().map(|span| span.text.as_str()).collect()
    }

    /// The text of each row of the bar of `source`, `columns` wide and
    /// `rows` tall, with `current` the entry the reader is in.
    fn drawn(source: &str, current: Option<usize>, size: (usize, usize)) -> Vec<String> {
        let contents = contents_of(source);
        let lines = rows(
            &contents,
            (&Folds::default(), &a_md()),
            (current, None),
            size,
            &looks(),
        );
        lines.iter().map(text_of).collect()
    }

    const NESTED: &str = "# One\n\n## Two\n\n### Three\n\n## Four\n";

    #[test]
    fn a_row_for_each_heading_indented_by_its_level_with_a_glyph_for_a_parent() {
        let rows = drawn(NESTED, None, (16, 6));
        assert_eq!(rows.len(), 6);
        let trimmed: Vec<&str> = rows.iter().map(|row| row.trim_end()).collect();
        assert_eq!(
            trimmed,
            [" ▾ One", "   ▾ Two", "       Three", "     Four", "", ""]
        );
    }

    #[test]
    fn every_row_is_as_wide_as_the_bar_and_those_after_the_last_entry_are_blank() {
        let contents = contents_of(NESTED);
        let lines = rows(
            &contents,
            (&Folds::default(), &a_md()),
            (None, None),
            (16, 6),
            &looks(),
        );
        assert_eq!(lines.len(), 6);
        for line in &lines {
            let width: usize = line.spans.iter().map(|span| span.text.width()).sum();
            assert_eq!(width, 16, "{:?}", text_of(line));
        }
        assert!(text_of(&lines[5]).trim().is_empty());
    }

    #[test]
    fn a_folded_entry_hides_what_is_under_it_and_shows_a_closed_glyph() {
        let contents = contents_of(NESTED);
        let mut folds = Folds::default();
        folds.fold("a.md", &contents.entries()[1], true);
        let lines = rows(
            &contents,
            (&folds, &a_md()),
            (None, None),
            (16, 5),
            &looks(),
        );
        let rows: Vec<String> = lines
            .iter()
            .map(|line| text_of(line).trim_end().to_owned())
            .collect();
        assert_eq!(rows[..3], [" ▾ One", "   ▸ Two", "     Four"]);
        // Folding the first hides all of what follows it.
        folds.fold("a.md", &contents.entries()[0], true);
        let lines = rows_text(&contents, &folds, None, (16, 3));
        assert_eq!(lines, [" ▸ One", "", ""]);
    }

    fn rows_text(
        contents: &Contents,
        folds: &Folds,
        current: Option<usize>,
        size: (usize, usize),
    ) -> Vec<String> {
        let lines = rows(contents, (folds, &a_md()), (current, None), size, &looks());
        lines
            .iter()
            .map(|line| text_of(line).trim_end().to_owned())
            .collect()
    }

    #[test]
    fn a_long_text_is_cut_with_an_ellipsis_and_never_wraps() {
        let rows = drawn(
            "# A heading that is much too long for the bar\n",
            None,
            (14, 3),
        );
        assert_eq!(rows.len(), 3);
        assert!(rows[0].trim_end().ends_with('…'), "{:?}", rows[0]);
        assert_eq!(rows[0].width(), 14);
        assert!(rows[1].trim().is_empty());
    }

    #[test]
    fn an_empty_heading_is_an_empty_row() {
        let rows = drawn("# One\n\n#\n\n# Three\n", None, (12, 4));
        let trimmed: Vec<&str> = rows.iter().map(|row| row.trim_end()).collect();
        assert_eq!(trimmed, ["   One", "", "   Three", ""]);
    }

    #[test]
    fn the_entry_the_reader_is_in_is_marked_and_the_others_are_not() {
        let contents = contents_of(NESTED);
        let lines = rows(
            &contents,
            (&Folds::default(), &a_md()),
            (Some(2), None),
            (16, 4),
            &looks(),
        );
        let marked: Vec<bool> = lines
            .iter()
            .map(|line| line.spans.iter().all(|span| span.style == looks().mark))
            .collect();
        assert_eq!(marked, [false, false, true, false]);
        let glyph = &lines[0]
            .spans
            .iter()
            .find(|span| span.text.contains('▾'))
            .unwrap();
        assert_eq!(glyph.style, looks().muted);
    }

    #[test]
    fn the_reader_in_a_folded_part_is_marked_on_the_entry_that_hides_it() {
        let contents = contents_of(NESTED);
        let mut folds = Folds::default();
        folds.fold("a.md", &contents.entries()[1], true);
        // In `Three`, which `Two` hides: `Two` is marked.
        let lines = rows(
            &contents,
            (&folds, &a_md()),
            (Some(2), None),
            (16, 3),
            &looks(),
        );
        let marked: Vec<bool> = lines
            .iter()
            .map(|line| line.spans.iter().all(|span| span.style == looks().mark))
            .collect();
        assert_eq!(marked, [false, true, false]);
    }

    #[test]
    fn with_more_entries_than_rows_the_entry_the_reader_is_in_is_in_view() {
        let source: String = (0..30).map(|n| format!("# H{n:02}\n\n")).collect();
        for current in [0, 15, 29] {
            let rows = drawn(&source, Some(current), (10, 5));
            assert_eq!(rows.len(), 5);
            let want = format!("H{current:02}");
            assert!(
                rows.iter().any(|row| row.contains(&want)),
                "{current}: {rows:?}"
            );
        }
        // Without one, from the top.
        assert!(drawn(&source, None, (10, 5))[0].contains("H00"));
    }

    #[test]
    fn a_file_is_listed_above_its_headings_which_are_one_level_in() {
        let documents = [parse("# A\n\n## B\n"), parse("# C\n")];
        let found = targets([&documents[0], &documents[1]]);
        let contents = crate::typeset::contents::contents(
            [Some("one.md"), Some("two.md")]
                .into_iter()
                .zip(documents.iter()),
            &found,
        );
        let lines = rows(
            &contents,
            (
                &Folds::default(),
                &["one.md".to_owned(), "two.md".to_owned()],
            ),
            (None, None),
            (16, 6),
            &looks(),
        );
        let rows: Vec<String> = lines
            .iter()
            .map(|line| text_of(line).trim_end().to_owned())
            .collect();
        assert_eq!(
            rows[..5],
            [" ▾ one.md", "   ▾ A", "       B", " ▾ two.md", "     C"].map(str::to_owned)
        );
    }

    /// A state that shows `document` as the base, in `session`.
    fn showing(
        session: &crate::session::sources::Session,
        document: &crate::typeset::model::Document,
    ) -> crate::session::state::State {
        let mut state =
            crate::session::state::State::new(crate::session::settings::SearchCase::Smart);
        state.show(crate::typeset::driver::Content::Document(document), session);
        state
    }

    /// The file `name` opened as a view over the top one, shown as `document`.
    fn followed(
        state: &mut crate::session::state::State,
        session: &mut crate::session::sources::Session,
        name: &str,
        document: &crate::typeset::model::Document,
    ) {
        let file = crate::session::sources::SourceFile {
            path: name.into(),
            input: crate::typeset::input::Input::Markdown,
            look: None,
        };
        let view = session.open(name.to_owned(), file);
        state.push_view(view, Vec::new(), std::time::Instant::now());
        state.show(crate::typeset::driver::Content::Document(document), session);
    }

    fn first_entry(document: &crate::typeset::model::Document) -> Entry {
        let found = targets([document]);
        crate::typeset::contents::contents([(None, document)], &found).entries()[0].clone()
    }

    #[test]
    fn a_fold_made_in_the_base_is_there_after_a_visit_to_a_linked_file() {
        let mut session = crate::session::sources::Session::new(["a.md"]);
        let (base, other) = (parse("# One\n\n## Two\n"), parse("# One\n\n## Else\n"));
        let mut state = showing(&session, &base);
        state.folds.fold("a.md", &first_entry(&base), true);
        followed(&mut state, &mut session, "b.md", &other);
        // The heading with the same id, in another file, is not folded.
        assert!(!state.folds.is_folded("b.md", &first_entry(&other)));
        state.folds.fold("b.md", &first_entry(&other), true);
        state.back_to(crate::session::sources::ViewId::BASE);
        state.show(crate::typeset::driver::Content::Document(&base), &session);
        assert!(state.folds.is_folded("a.md", &first_entry(&base)));
        assert!(state.folds.is_folded("b.md", &first_entry(&other)));
    }

    #[test]
    fn a_fold_is_there_when_a_link_opens_the_same_file_again() {
        let mut session = crate::session::sources::Session::new(["a.md"]);
        let (base, other) = (parse("# One\n"), parse("# Elsewhere\n\n## Else\n"));
        let mut state = showing(&session, &base);
        followed(&mut state, &mut session, "b.md", &other);
        state.folds.fold("b.md", &first_entry(&other), true);
        state.back_to(crate::session::sources::ViewId::BASE);
        followed(&mut state, &mut session, "b.md", &other);
        assert!(state.folds.is_folded("b.md", &first_entry(&other)));
        // The folds the view showed are still the ones the bar draws: the
        // edition's names say which file's they are.
        let names = &state.document().edition.as_ref().unwrap().names;
        assert_eq!(names, &["b.md".to_owned()]);
    }

    #[test]
    fn a_reload_of_one_file_drops_only_the_lost_ids_of_that_file() {
        use crate::typeset::driver::Content;
        use crate::typeset::model::File;
        let mut session = crate::session::sources::Session::new(["a.md", "b.md"]);
        let file = |name: &str, source: &str| File {
            name: name.to_owned(),
            document: parse(source),
        };
        let before = [
            file("a.md", "# One\n\n## Two\n"),
            file("b.md", "# One\n\n## Two\n"),
        ];
        let mut state =
            crate::session::state::State::new(crate::session::settings::SearchCase::Smart);
        state.show(Content::Files(&before), &session);
        let entries = state
            .document()
            .edition
            .as_ref()
            .unwrap()
            .contents
            .entries()
            .to_vec();
        let of = |file: usize, id: &str| {
            let found = (entries.iter()).find(|e| e.file == file && e.id.as_deref() == Some(id));
            found.expect("an entry").clone()
        };
        for (name, file) in [("a.md", 0), ("b.md", 1)] {
            for id in ["one", "two"] {
                state.folds.fold(name, &of(file, id), true);
            }
        }
        // `a.md` is read again without `Two`; `b.md` is as it was.
        let id = session.sources()[0].id();
        session.reload(id, None);
        let after = [file("a.md", "# One\n"), file("b.md", "# One\n\n## Two\n")];
        state.show(Content::Files(&after), &session);
        let folded = |name: &str, file: usize, id: &str| state.folds.is_folded(name, &of(file, id));
        assert!(folded("a.md", 0, "one"));
        assert!(!folded("a.md", 0, "two"), "a.md lost Two");
        assert!(folded("b.md", 1, "one"));
        assert!(folded("b.md", 1, "two"), "b.md did not change");
    }

    #[test]
    fn two_files_with_the_same_heading_id_fold_independently() {
        use crate::typeset::driver::Content;
        use crate::typeset::model::File;
        let session = crate::session::sources::Session::new(["a.md", "b.md"]);
        let files = [
            File {
                name: "a.md".to_owned(),
                document: parse("# Intro\n\n## Deep\n"),
            },
            File {
                name: "b.md".to_owned(),
                document: parse("# Intro\n\n## Deep\n"),
            },
        ];
        let mut state =
            crate::session::state::State::new(crate::session::settings::SearchCase::Smart);
        state.show(Content::Files(&files), &session);
        let edition = state.document().edition.as_ref().unwrap();
        let intro = |file: usize| {
            let found =
                (edition.contents.entries().iter()).find(|e| e.file == file && e.level == 1);
            found.expect("an entry").clone()
        };
        let (a, b) = (intro(0), intro(1));
        assert_eq!(a.id, b.id);
        state.folds.fold("a.md", &a, true);
        assert!(state.folds.is_folded("a.md", &a));
        assert!(!state.folds.is_folded("b.md", &b));
    }

    #[test]
    fn a_row_in_a_bar_of_no_cells_to_four_is_cut_to_it_at_a_deep_level() {
        // Six levels; the deepest row's indent is 11 cells. Each row is as
        // wide as the bar, with the glyph and the text cut where they
        // do not fit, and nothing at all in a bar of no cells.
        let deep = "# a\n\n## b\n\n### c\n\n#### d\n\n##### e\n\n###### ff\n";
        let expected: [&[&str]; 5] = [
            &["", "", "", "", "", ""],
            &[" ", " ", " ", " ", " ", " "],
            &[" ▾", "  ", "  ", "  ", "  ", "  "],
            &[" ▾ ", "   ", "   ", "   ", "   ", "   "],
            &[" ▾ a", "   ▾", "    ", "    ", "    ", "    "],
        ];
        for (columns, want) in expected.iter().enumerate() {
            assert_eq!(drawn(deep, None, (columns, 6)), *want, "{columns} columns");
            assert!(want.iter().all(|row| row.width() == columns));
        }
    }

    #[test]
    fn a_tab_and_a_bell_in_a_heading_are_a_space_and_a_replacement_in_the_row() {
        // The contents makes one line of the text: white space is a space
        // and another control character is U+FFFD, before the row is cut.
        let source = "# tab\tinside\n\n# a\u{7}bcdefgh\n";
        for (columns, want) in [
            (6, ["   ta…", "   a�…"]),
            (8, ["   tab …", "   a�bc…"]),
            (12, ["   tab insi…", "   a�bcdefgh"]),
        ] {
            assert_eq!(drawn(source, None, (columns, 2)), want, "{columns} columns");
        }
    }

    /// The selection after `keys`, from entry `from`, with `folds` as they are.
    fn pressed(source: &str, folds: &mut Folds, from: usize, keys: &[BarKey]) -> Chosen {
        let contents = contents_of(source);
        let mut at = Chosen::Selected(from);
        for &key in keys {
            let Chosen::Selected(selected) = at else {
                break;
            };
            at = press(key, selected, &contents, (folds, &a_md()));
        }
        at
    }

    #[test]
    fn up_and_down_move_between_the_listed_entries_and_stop_at_the_ends() {
        use BarKey::{Down, Up};
        let mut folds = Folds::default();
        let go = |folds: &mut Folds, from, keys: &[BarKey]| pressed(NESTED, folds, from, keys);
        assert_eq!(go(&mut folds, 0, &[Down]), Chosen::Selected(1));
        assert_eq!(go(&mut folds, 0, &[Down, Down, Down]), Chosen::Selected(3));
        assert_eq!(go(&mut folds, 3, &[Down]), Chosen::Selected(3));
        assert_eq!(go(&mut folds, 3, &[Up, Up]), Chosen::Selected(1));
        assert_eq!(go(&mut folds, 0, &[Up]), Chosen::Selected(0));
        // `Three` is hidden by a folded `Two`.
        let contents = contents_of(NESTED);
        folds.fold("a.md", &contents.entries()[1], true);
        assert_eq!(go(&mut folds, 1, &[Down]), Chosen::Selected(3));
        assert_eq!(go(&mut folds, 3, &[Up]), Chosen::Selected(1));
    }

    #[test]
    fn first_and_last_select_the_first_and_the_last_listed_entry() {
        let mut folds = Folds::default();
        assert_eq!(
            pressed(NESTED, &mut folds, 2, &[BarKey::Last]),
            Chosen::Selected(3)
        );
        assert_eq!(
            pressed(NESTED, &mut folds, 2, &[BarKey::First]),
            Chosen::Selected(0)
        );
        // The last entry listed is not one that a fold hides.
        let source = "# One\n\n## Two\n\n### Three\n";
        let contents = contents_of(source);
        folds.fold("a.md", &contents.entries()[1], true);
        assert_eq!(
            pressed(source, &mut folds, 0, &[BarKey::Last]),
            Chosen::Selected(1)
        );
    }

    #[test]
    fn left_folds_an_open_entry_and_goes_to_the_parent_from_a_folded_one_or_a_leaf() {
        let contents = contents_of(NESTED);
        let mut folds = Folds::default();
        // `Two` is open and has `Three`: folded, selection stays.
        let at = pressed(NESTED, &mut folds, 1, &[BarKey::Left]);
        assert_eq!(at, Chosen::Selected(1));
        assert!(folds.is_folded("a.md", &contents.entries()[1]));
        // Folded now: to the parent, `One`.
        assert_eq!(
            pressed(NESTED, &mut folds, 1, &[BarKey::Left]),
            Chosen::Selected(0)
        );
        // A leaf: to its parent, which is not folded by it.
        assert_eq!(
            pressed(NESTED, &mut folds, 3, &[BarKey::Left]),
            Chosen::Selected(0)
        );
        assert!(!folds.is_folded("a.md", &contents.entries()[0]));
        // The top has no parent: it folds, and then stays.
        assert_eq!(
            pressed(NESTED, &mut folds, 0, &[BarKey::Left, BarKey::Left]),
            Chosen::Selected(0)
        );
        assert!(folds.is_folded("a.md", &contents.entries()[0]));
    }

    #[test]
    fn right_opens_a_folded_entry_and_does_nothing_on_an_open_one_or_a_leaf() {
        let contents = contents_of(NESTED);
        let mut folds = Folds::default();
        folds.fold("a.md", &contents.entries()[1], true);
        assert_eq!(
            pressed(NESTED, &mut folds, 1, &[BarKey::Right]),
            Chosen::Selected(1)
        );
        assert!(!folds.is_folded("a.md", &contents.entries()[1]));
        for from in [1, 2, 3] {
            assert_eq!(
                pressed(NESTED, &mut folds, from, &[BarKey::Right]),
                Chosen::Selected(from)
            );
        }
        assert!(!folds.is_folded("a.md", &contents.entries()[1]));
    }

    #[test]
    fn a_leaf_with_no_parent_stays_on_left() {
        let mut folds = Folds::default();
        assert_eq!(
            pressed("# One\n\n# Two\n", &mut folds, 1, &[BarKey::Left]),
            Chosen::Selected(1)
        );
    }

    #[test]
    fn enter_asks_for_the_entry_selected() {
        let mut folds = Folds::default();
        assert_eq!(
            pressed(NESTED, &mut folds, 2, &[BarKey::Enter]),
            Chosen::Jump(2)
        );
    }

    #[test]
    fn the_keys_of_the_bar_are_those_of_the_plan() {
        let of = |key| BarKey::of(key);
        for key in [Key::Char('j'), Key::Down] {
            assert_eq!(of(key), Some(BarKey::Down));
        }
        for key in [Key::Char('k'), Key::Up] {
            assert_eq!(of(key), Some(BarKey::Up));
        }
        for key in [Key::Char('h'), Key::Left] {
            assert_eq!(of(key), Some(BarKey::Left));
        }
        for key in [Key::Char('l'), Key::Right] {
            assert_eq!(of(key), Some(BarKey::Right));
        }
        assert_eq!(of(Key::Char('g')), Some(BarKey::First));
        assert_eq!(of(Key::Char('G')), Some(BarKey::Last));
        assert_eq!(of(Key::Enter), Some(BarKey::Enter));
        for key in [Key::Char('q'), Key::Char('/'), Key::Char(' '), Key::Escape] {
            assert_eq!(of(key), None, "{key:?}");
        }
    }

    #[test]
    fn the_bar_scrolls_to_keep_the_selection_in_view_and_the_selection_wins() {
        let source: String = (0..30).map(|n| format!("# H{n:02}\n\n")).collect();
        let contents = contents_of(&source);
        let drawn = |current, selected| -> Vec<String> {
            let lines = rows(
                &contents,
                (&Folds::default(), &a_md()),
                (current, selected),
                (10, 5),
                &looks(),
            );
            lines.iter().map(text_of).collect()
        };
        // The reader is at the top and the selection is at the end.
        let seen = drawn(Some(0), Some(29));
        assert!(seen.iter().any(|row| row.contains("H29")), "{seen:?}");
        assert!(!seen.iter().any(|row| row.contains("H00")), "{seen:?}");
        // With no selection, the reader's entry.
        assert!(drawn(Some(0), None)[0].contains("H00"));
    }

    /// The look of each row of the bar of `NESTED` drawn with `current` and
    /// `selected`: `mark`, `quiet` or `page`.
    fn looks_of(current: Option<usize>, selected: Option<usize>) -> Vec<&'static str> {
        let contents = contents_of(NESTED);
        let all = looks();
        let lines = rows(
            &contents,
            (&Folds::default(), &a_md()),
            (current, selected),
            (16, 4),
            &all,
        );
        lines
            .iter()
            .map(|line| match &line.spans[0].style {
                style if *style == all.mark => "mark",
                style if *style == all.quiet => "quiet",
                _ => "page",
            })
            .collect()
    }

    #[test]
    fn without_the_focus_the_entry_the_reader_is_in_has_the_mark() {
        assert_eq!(looks_of(Some(2), None), ["page", "page", "mark", "page"]);
    }

    #[test]
    fn with_the_focus_the_selection_has_the_mark_and_the_reader_s_entry_a_quiet_look() {
        assert_eq!(
            looks_of(Some(2), Some(0)),
            ["mark", "page", "quiet", "page"]
        );
        assert_eq!(
            looks_of(Some(2), Some(3)),
            ["page", "page", "quiet", "mark"]
        );
    }

    #[test]
    fn the_selection_on_the_reader_s_entry_is_the_mark_alone() {
        assert_eq!(looks_of(Some(2), Some(2)), ["page", "page", "mark", "page"]);
        // Before the first heading nothing is quiet.
        assert_eq!(looks_of(None, Some(1)), ["page", "mark", "page", "page"]);
    }

    #[test]
    fn the_help_lists_the_keys_of_the_bar_and_each_is_a_key_of_it() {
        use crate::session::commands::{Action, COMMANDS, Topic};
        let key_of = |name: &str| match name {
            "↓" => Key::Down,
            "↑" => Key::Up,
            "←" => Key::Left,
            "→" => Key::Right,
            "Enter" => Key::Enter,
            name => Key::Char(name.chars().next().unwrap()),
        };
        let listed: Vec<_> = (COMMANDS.iter())
            .filter(|command| command.action == Action::Bar)
            .collect();
        assert!(!listed.is_empty(), "the help has rows for the bar's keys");
        let mut seen = Vec::new();
        for command in listed {
            assert_eq!(command.topic, Topic::Contents);
            assert!(command.keys.is_empty(), "the bar answers them");
            for name in command.names.split(' ') {
                let bar = BarKey::of(key_of(name));
                assert!(bar.is_some(), "{name} is not a key of the bar");
                seen.extend(bar);
            }
        }
        for key in [
            BarKey::Up,
            BarKey::Down,
            BarKey::Left,
            BarKey::Right,
            BarKey::First,
            BarKey::Last,
            BarKey::Enter,
        ] {
            assert!(seen.contains(&key), "{key:?} is in the help");
        }
    }
}
