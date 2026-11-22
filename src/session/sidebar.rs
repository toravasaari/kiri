//! The contents bar at the left of the window: whether it is shown, how
//! wide it is, which of its entries are folded, and what it draws.
//!
//! Whether the bar shows is kept for the session in three values ([`Visibility`]),
//! not undone by the window changing size. The entries are the contents of
//! the documents shown ([`Contents`]), which this does not make; it only says
//! which of them are folded and which rows the bar has.

use std::collections::{BTreeMap, BTreeSet};

use unicode_width::UnicodeWidthStr;

use crate::present::keys::Key;
use crate::session::panels;
use crate::session::status;
use crate::typeset::caps::Capabilities;
use crate::typeset::color::{Color, Underline};
use crate::typeset::contents::{Contents, Entry};
use crate::typeset::style::{Line, Span, Style};
use crate::typeset::theme::Theme;

pub(crate) mod colors;

use colors::BarColors;

/// The bar's width in cells, in a window wide enough for it.
pub(crate) const WIDTH: u16 = 30;

/// The columns the document keeps at least for the bar to show itself.
const AUTO_LEAVES: u16 = 80;

/// The columns the document keeps at least when the reader opened the bar.
const OPEN_LEAVES: u16 = 40;

/// The least the reader can drag the bar's width to, in cells.
pub(crate) const MIN_WIDTH: u16 = 16;

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

/// What the session keeps of the bar: whether it shows, and the entry the
/// reader selected in it, as an index of the contents' entries.
#[derive(Debug, Default)]
pub(crate) struct Sidebar {
    pub(crate) visibility: Visibility,
    pub(crate) selected: Option<usize>,
    /// Whether moving the selection moves the document to the heading.
    pub(crate) preview: bool,
    /// The width in cells the reader dragged the border to, as the pointer
    /// was, not yet held to the limits of the window: those apply when the
    /// bar is laid out ([`Sidebar::width`]). For the session.
    pub(crate) dragged: Option<u16>,
    /// Whether the reader holds the left button down on the border.
    pub(crate) dragging: bool,
    /// Where on the scroll bar's thumb the reader holds it, in half rows
    /// from its top, while the left button is down on it.
    pub(crate) grip: Option<isize>,
    /// The bar's row, from 1, where the pointer was last while the thumb
    /// was held: the list moves only when the pointer changes it.
    pub(crate) grip_row: usize,
}

impl Sidebar {
    /// A bar that is `visibility` at the start.
    pub(crate) fn new(visibility: Visibility) -> Self {
        Self {
            visibility,
            selected: None,
            preview: false,
            dragged: None,
            dragging: false,
            grip: None,
            grip_row: 0,
        }
    }

    /// Notes that the reader dragged the border to `column` in a window of
    /// `columns`, held to the window's limits: what the reader never saw
    /// does not come back in a wider window. Each motion is an absolute
    /// column, so a drag past a limit and back still works.
    pub(crate) fn drag(&mut self, column: u16, columns: u16) {
        let (least, most) = Self::limits(columns);
        self.dragged = Some(column.clamp(least, most));
    }

    /// The least and the most a dragged bar can be in a window of `columns`:
    /// 16 cells, and half the window with 40 columns left for the document.
    fn limits(columns: u16) -> (u16, u16) {
        let room = (columns / 2).min(columns.saturating_sub(OPEN_LEAVES));
        (MIN_WIDTH.min(room), room)
    }

    /// The columns the bar takes in a window of `columns`, none when it is
    /// not shown: when it is closed, when there is nothing to list
    /// (`entries` is false) and when the document would be left too narrow
    /// beside the default width. 30 cells and at most a third of the
    /// window, or what the reader dragged it to.
    pub(crate) fn width(&self, columns: u16, entries: bool) -> u16 {
        let leaves = match self.visibility {
            _ if !entries => return 0,
            Visibility::Closed => return 0,
            Visibility::Auto => AUTO_LEAVES,
            Visibility::Open => OPEN_LEAVES,
        };
        let width = WIDTH.min(columns / 3);
        if columns - width < leaves {
            return 0;
        }
        // The width the reader dragged to, between 16 cells and half the
        // window, and so that the document keeps 40 columns. Whether the bar
        // shows does not depend on it.
        let (least, most) = Self::limits(columns);
        self.dragged
            .map_or(width, |wanted| wanted.clamp(least, most))
    }
}

/// The entries of the contents that are folded: their children are not
/// listed. They belong to the session, by the name of the file (as the
/// session's sources have it) and the id of the heading, so a file opened
/// again, or come back to, has its folds as it left them.
#[derive(Debug, Default)]
pub(crate) struct Folds {
    /// The ids of the folded entries by file name; a file has no id, so
    /// its entry is `None`. Nested so that a lookup borrows its key.
    folded: BTreeMap<String, BTreeSet<Option<String>>>,
}

impl Folds {
    /// Whether the entry, which is of the file called `name`, is folded.
    pub(crate) fn is_folded(&self, name: &str, entry: &Entry) -> bool {
        (self.folded.get(name)).is_some_and(|ids| ids.contains(&entry.id))
    }

    /// Folds the entry, which is of the file called `name`, or opens it.
    pub(crate) fn fold(&mut self, name: &str, entry: &Entry, folded: bool) {
        match folded {
            true => {
                let ids = self.folded.entry(name.to_owned()).or_default();
                ids.insert(entry.id.clone());
            }
            false => {
                if let Some(ids) = self.folded.get_mut(name) {
                    ids.remove(&entry.id);
                    if ids.is_empty() {
                        self.folded.remove(name);
                    }
                }
            }
        }
    }

    /// Lets go of the folds of the headings that the files called `names`
    /// (by file number) no longer have in `contents`, as after a file was
    /// read again. The folds of files that are not among `names` are not
    /// touched: they are of documents that are not shown now.
    pub(crate) fn keep(&mut self, names: &[String], contents: &Contents) {
        self.folded.retain(|name, ids| {
            let files: Vec<usize> = (names.iter().enumerate())
                .filter(|(_, other)| *other == name)
                .map(|(file, _)| file)
                .collect();
            if files.is_empty() {
                return true;
            }
            ids.retain(|id| {
                (contents.entries().iter())
                    .any(|entry| files.contains(&entry.file) && entry.id.as_ref() == id.as_ref())
            });
            !ids.is_empty()
        });
    }
}

/// How the cells of one kind of row are drawn.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct RowLook {
    /// The text of an entry of level 1 or 2, or a file.
    pub(crate) title: Style,
    /// The text of a deeper entry.
    pub(crate) text: Style,
    /// The fold glyph.
    pub(crate) glyph: Style,
    /// The cells of the row that hold no text.
    pub(crate) fill: Style,
}

impl RowLook {
    /// A row all in `style`, with `glyph` for the fold glyph.
    fn of(style: &Style, glyph: &Style) -> Self {
        Self {
            title: style.clone(),
            text: style.clone(),
            glyph: glyph.clone(),
            fill: style.clone(),
        }
    }
}

/// The styles the bar's rows are drawn in, from the theme.
///
/// A bar in colours has an edge cell for the entry the reader is in and a
/// border at its right; a plain one, without colours, has neither and
/// shows the entry in reverse video.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Looks {
    /// The bar's own background: the rows below the last entry.
    pub(crate) page: Style,
    /// An entry.
    pub(crate) normal: RowLook,
    /// The entry the reader is in.
    pub(crate) current: RowLook,
    /// The selected entry while the bar has the focus.
    pub(crate) selected: RowLook,
    /// The entry the reader is in while another is selected.
    pub(crate) quiet: RowLook,
    /// The edge cell of the entry the reader is in; none for a plain bar.
    pub(crate) edge: Option<Style>,
    /// The border column, as it is without the focus and with it; none for
    /// a plain bar.
    pub(crate) border: Option<(Style, Style)>,
    /// The scroll bar's cells: the thumb as the foreground, the track as
    /// the background.
    pub(crate) scrollbar: Style,
    /// Whether the bar has no colours: its scroll bar is then drawn in
    /// block glyphs alone, the track blank.
    pub(crate) plain: bool,
}

/// The cell at the left edge of the row of the entry the reader is in.
const EDGE: &str = "▌";

/// The border between the bar and the document: a line at the right of
/// its last cell.
const BORDER: &str = "▕";

impl Looks {
    /// The looks of a bar for `theme` on a terminal with `caps`, over a
    /// page styled `page`, with `muted` for a fold glyph that the theme
    /// gives no colour and `mark` for the plain bar's entry in reverse.
    pub(crate) fn new(
        theme: &Theme,
        caps: &Capabilities,
        page: &Style,
        muted: &Style,
        mark: &Style,
    ) -> Self {
        match colors::derive(theme, caps) {
            Some(colors) => Self::coloured(&colors, muted.clone()),
            // Without colours the scroll bar's glyphs are the help's: the
            // thumb in blocks, the track blank.
            None => Self::plain(page.clone(), muted.clone(), mark.clone()),
        }
    }

    /// These looks while the border is dragged: the border in the colour it
    /// has with the focus, and, in a bar without colours, a thin line where
    /// there is none otherwise.
    pub(crate) fn dragged(&self) -> Self {
        let border = match &self.border {
            Some((_, focus)) => (focus.clone(), focus.clone()),
            None => (Style::default(), Style::default()),
        };
        Self {
            border: Some(border),
            ..self.clone()
        }
    }

    /// The looks of a bar without colours: the entry the reader is in has
    /// the mark's look (bold in a terminal without colour) and a `▌` at
    /// the left of its row in the terminal's own colour, the one
    /// selected the mark's in reverse video, and while another is selected
    /// the reader's is the page's look in italics.
    pub(crate) fn plain(page: Style, muted: Style, mark: Style) -> Self {
        let quiet = Style {
            italic: true,
            ..page.clone()
        };
        let chosen = Style {
            bold: true,
            reverse: true,
            ..mark.clone()
        };
        Self {
            normal: RowLook::of(&page, &muted),
            current: RowLook::of(&mark, &mark),
            selected: RowLook::of(&chosen, &chosen),
            quiet: RowLook::of(&quiet, &muted),
            page,
            // The mark at the left of the entry the reader is in, in the
            // terminal's own colour: white on a dark page.
            edge: Some(Style::default()),
            border: None,
            scrollbar: Style::default(),
            plain: true,
        }
    }

    /// The looks of a bar in `colors`: entries of level 1 and 2 in bold,
    /// the one the reader is in with an edge and a background of its own,
    /// the selected one in reverse. `muted` dims the fold glyph where
    /// `colors` has none for it.
    pub(crate) fn coloured(colors: &BarColors, muted: Style) -> Self {
        let on = |fg, bg| Style {
            fg,
            bg,
            ..Style::default()
        };
        let bold = |style: Style| Style {
            bold: true,
            ..style
        };
        let background = colors.background;
        let band = colors.current_background.or(background);
        let glyph = |bg| match colors.glyph {
            Some(_) => on(colors.glyph, bg),
            None => Style {
                bg,
                ..muted.clone()
            },
        };
        let row = |bg, bold_text: bool| RowLook {
            title: bold(on(colors.title, bg)),
            text: match bold_text {
                true => bold(on(colors.text, bg)),
                false => on(colors.text, bg),
            },
            glyph: glyph(bg),
            fill: on(None, bg),
        };
        // Reverse video: the text takes the bar's background and the row
        // the selected colour.
        let reversed = |style: Style| Style {
            fg: colors.selected,
            reverse: true,
            ..style
        };
        let picked = RowLook {
            title: reversed(bold(on(None, background))),
            text: reversed(on(None, background)),
            glyph: reversed(on(None, background)),
            fill: reversed(on(None, background)),
        };
        let border = |fg| Style {
            fg,
            dim: fg.is_none(),
            ..on(None, background)
        };
        let current = row(band, true);
        Self {
            page: on(None, background),
            normal: row(background, false),
            quiet: current.clone(),
            current,
            selected: picked,
            edge: Some(on(colors.current, band)),
            border: Some((border(colors.border), border(colors.selected))),
            // The help's greys where the bar's background is not known.
            scrollbar: Style {
                fg: Some(colors.thumb.unwrap_or(Color::Palette(250))),
                bg: Some(colors.track.unwrap_or(Color::Palette(237))),
                ..Style::default()
            },
            plain: false,
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
    /// A page up or down: as many rows of the list as the bar has.
    PageUp,
    PageDown,
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
            Key::PageUp => Self::PageUp,
            Key::PageDown => Self::PageDown,
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
/// the selection among the entries that are listed, a page being `rows` rows
/// of them, or folds or opens an entry in `folds`.
pub(crate) fn press(
    key: BarKey,
    selected: usize,
    contents: &Contents,
    (folds, names): (&mut Folds, &[String]),
    rows: usize,
) -> Chosen {
    let entries = contents.entries();
    let rows_list = listed(contents, (folds, names));
    // The entry that holds the selection, which a fold may have hidden.
    let Some(here) = (rows_list.iter()).rposition(|slot| slot.is_some_and(|at| at <= selected))
    else {
        return Chosen::Selected(selected);
    };
    let last = rows_list.len() - 1;
    // The entry of a list row, the next one when it is a gap (a gap is never
    // the last row), or the one before it.
    let below = |row: usize| {
        let row = row.min(last);
        rows_list[row].or(rows_list.get(row + 1).copied().flatten())
    };
    let above = |row: usize| rows_list[row].or(rows_list[row.saturating_sub(1)]);
    let at = rows_list[here].unwrap_or(selected);
    let entry = &entries[at];
    let name = names.get(entry.file).map_or("", String::as_str);
    let parent = || {
        let above = entries[..at].iter().enumerate().rev();
        let mut same_file = above.take_while(|(_, other)| other.file == entry.file);
        same_file.find(|(_, other)| other.level < entry.level)
    };
    let folded = has_children(entries, at) && folds.is_folded(name, entry);
    Chosen::Selected(match key {
        BarKey::Down => below(here + 1).unwrap_or(at),
        BarKey::Up => above(here.saturating_sub(1)).unwrap_or(at),
        BarKey::First => below(0).unwrap_or(at),
        BarKey::Last => below(last).unwrap_or(at),
        BarKey::PageDown => below(here + rows).unwrap_or(at),
        BarKey::PageUp => above(here.saturating_sub(rows)).unwrap_or(at),
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

/// The listed rows that hold the entry the reader is in, `current`, and
/// the one selected, and the one the bar keeps in view: the selection when
/// there is one, else the reader's. An entry that a fold hides is held by
/// the last listed one at or before it.
fn holding(
    listed: &[Option<usize>],
    (current, selected): (Option<usize>, Option<usize>),
) -> (Option<usize>, Option<usize>, Option<usize>) {
    let holding = |entry: Option<usize>| {
        entry.and_then(|entry| {
            listed
                .iter()
                .rposition(|slot| slot.is_some_and(|at| at <= entry))
        })
    };
    let (marked, selected) = (holding(current), holding(selected));
    (marked, selected, selected.or(marked))
}

/// The listed row at the top of a bar of `rows` rows that keeps row `kept`
/// of `listed` rows in view, in the middle when it can.
fn first_row(listed: usize, rows: usize, kept: Option<usize>) -> usize {
    match kept {
        Some(kept) if listed > rows => (kept.saturating_sub(rows / 2)).min(listed - rows),
        _ => 0,
    }
}

/// The listed row at the top of a bar of `rows` rows after something that
/// may have moved what it holds, `listed` being the rows of the list (the
/// entries that are listed, and the gaps before files).
/// The reader's own scroll, `free`, is kept, within the list. While an
/// entry is selected the bar keeps it in the middle. Otherwise the bar
/// stays at the row `stored` where it was, and moves only as far as it must
/// to show the entry the reader is in: that entry to the first row when it
/// is above, to the last when it is below.
pub(crate) fn settle(
    listed: &[Option<usize>],
    (current, selected): (Option<usize>, Option<usize>),
    rows: usize,
    (free, stored): (Option<usize>, usize),
) -> usize {
    let most = listed.len().saturating_sub(rows);
    let (marked, selected, kept) = holding(listed, (current, selected));
    match (free, selected, marked) {
        (Some(first), _, _) => first.min(most),
        (None, Some(_), _) => first_row(listed.len(), rows, kept),
        // The gap above a file goes into view with it.
        (None, None, Some(marked)) if marked < stored => {
            let gap = marked > 0 && listed[marked - 1].is_none();
            (marked - usize::from(gap)).min(most)
        }
        (None, None, Some(marked)) if marked >= stored + rows => (marked + 1 - rows).min(most),
        (None, None, _) => stored.min(most),
    }
}

/// The rows of a bar `columns` wide and `rows` tall: one line for each row
/// of the list (an entry that is listed, or a gap), from the listed row `first` (kept within the list;
/// [`settle`] says which it is), and blank lines after the last. Every line
/// is as wide as the bar. With an entry `selected` the bar has the focus: the selection has the mark's look
/// and the entry the reader is in the quiet one.
pub(crate) fn rows(
    contents: &Contents,
    (folds, names): (&Folds, &[String]),
    (current, selected): (Option<usize>, Option<usize>),
    (columns, rows): (usize, usize),
    (looks, first): (&Looks, usize),
) -> Vec<Line> {
    let entries = contents.entries();
    let is_folded = |entry: &Entry| {
        let name = names.get(entry.file).map_or("", String::as_str);
        folds.is_folded(name, entry)
    };
    let listed = listed(contents, (folds, names));
    let (marked, selected, _) = holding(&listed, (current, selected));
    let first = first.min(listed.len().saturating_sub(rows));
    let shift = shift(entries);
    // The bar has the focus when something is selected.
    let focused = selected.is_some();
    let border = looks.border.as_ref().map(|(plain, focus)| match focused {
        true => focus,
        false => plain,
    });
    let blank = || {
        let edge = Span::new(" ", looks.page.clone());
        row(
            ("", ' ', ""),
            columns,
            (&looks.normal, &looks.page, border, &looks.page),
            (edge, None, None),
        )
    };
    let thumb = thumb(listed.len(), rows, first);
    let shown = listed.iter().enumerate().skip(first).take(rows);
    let mut lines: Vec<Line> = shown
        .enumerate()
        .map(|(at_row, (position, &slot))| {
            // The cell before the scroll bar is the bar's own background, so
            // that the row's tint stops before it.
            let bar = thumb.map(|thumb| {
                let gap = Span::new(" ", looks.page.clone());
                (
                    gap,
                    bar_cell(
                        (at_row, rows),
                        thumb,
                        (&looks.page, &looks.scrollbar, looks.plain),
                    ),
                )
            });
            // A gap between two files is blank, as the rows after the last
            // entry are, but has the scroll bar's cell.
            let Some(at) = slot else {
                let edge = Span::new(" ", looks.page.clone());
                let blank = (&looks.normal, &looks.page, border, &looks.page);
                return row(("", ' ', ""), columns, blank, (edge, bar, None));
            };
            // The selection has its own look, and the entry the reader is
            // in a quieter one while another is selected: it is the other's
            // place that counts then.
            let is_current = Some(position) == marked;
            let look = match (Some(position) == selected, is_current, focused) {
                (true, _, _) => &looks.selected,
                (false, true, true) => &looks.quiet,
                (false, true, false) => &looks.current,
                (false, false, _) => &looks.normal,
            };
            let entry = &entries[at];
            let indent = indent(entry, shift);
            let glyph = match (has_children(entries, at), is_folded(entry)) {
                (false, _) => ' ',
                (true, false) => '▾',
                (true, true) => '▸',
            };
            // The edge cell of the entry the reader is in, in a bar with
            // one; otherwise the first cell is the row's own.
            let edge = match (&looks.edge, is_current) {
                (Some(edge), true) => Span::new(EDGE, edge.clone()),
                (Some(_), false) => Span::new(" ", looks.page.clone()),
                (None, _) => Span::new(" ", look.fill.clone()),
            };
            let style = match entry.level {
                0..=2 => &look.title,
                _ => &look.text,
            };
            // A file's name and the empty cells after it are underlined, in
            // the colour of the headings.
            let rule = (entry.level == 0).then_some(Underline::Single);
            row(
                (&" ".repeat(indent), glyph, &entry.text),
                columns,
                (look, style, border, &looks.page),
                (edge, bar, rule),
            )
        })
        .collect();
    lines.resize_with(rows, blank);
    lines
}

/// How many levels the headings go in when files are listed above them.
fn shift(entries: &[Entry]) -> usize {
    usize::from(entries.iter().any(|entry| entry.level == 0))
}

/// The cells `entry` goes in, after the edge cell.
fn indent(entry: &Entry, shift: usize) -> usize {
    match entry.level {
        0 => 0,
        level => 2 * (usize::from(level) - 1 + shift),
    }
}

/// The scroll bar's thumb for a list of `listed` rows in a bar of `rows`
/// rows whose first row is `first`: where it starts and how long it is, in
/// half rows, as the help's is; none when the list fits.
pub(crate) fn thumb(listed: usize, rows: usize, first: usize) -> Option<(usize, usize)> {
    (listed > rows && rows > 0).then(|| match rows >= INSET_MIN_ROWS {
        true => {
            let (start, size) = inner_thumb(listed, rows, first);
            (start + 1, size)
        }
        false => panels::half_thumb(listed, rows, first),
    })
}

/// The fewest rows with which the scroll bar leaves half a row free at its
/// top and at its bottom.
const INSET_MIN_ROWS: usize = 3;

/// The thumb in the half rows between the free half row at the top and the
/// one at the bottom, `2 * rows - 2` of them, as [`panels::half_thumb`]
/// works it out for all `2 * rows`: where it starts and how long it is,
/// at least two halves.
fn inner_thumb(listed: usize, rows: usize, first: usize) -> (usize, usize) {
    let halves = 2 * rows - 2;
    let lines = listed.saturating_sub(rows).max(1);
    let size = ((halves * rows * 2 + listed) / (2 * listed.max(1))).clamp(2, halves.max(2));
    let room = halves.saturating_sub(size);
    (((room * first.min(lines)) * 2 + lines) / (2 * lines), size)
}

/// The cell of the scroll bar in row `row` of `rows`, counted from 0, for a
/// thumb at `(start, size)` half rows: each half row is the row's own
/// background (`outside`: the bar's own, in the free half row at the top and
/// at the bottom, whatever tint the row has),
/// the track or the thumb, and a cell shows two of them, the upper one in
/// the foreground of an upper half block and the lower one in its
/// background.
fn bar_cell(
    (row, rows): (usize, usize),
    (start, size): (usize, usize),
    (outside, scrollbar, plain): (&Style, &Style, bool),
) -> Span {
    #[derive(Clone, Copy, PartialEq)]
    enum Half {
        Outside,
        Track,
        Thumb,
    }
    let of = |half: usize| {
        let free = rows >= INSET_MIN_ROWS && (half == 0 || half == 2 * rows - 1);
        match (free, (start..start + size).contains(&half)) {
            (true, _) => Half::Outside,
            (false, true) => Half::Thumb,
            (false, false) => Half::Track,
        }
    };
    let colour = |half: Half| match half {
        Half::Thumb => scrollbar.fg,
        Half::Track => scrollbar.bg,
        Half::Outside => outside.bg,
    };
    let (upper, lower) = (of(2 * row), of(2 * row + 1));
    // Without colours only the thumb is drawn, as the help's bar does.
    if plain {
        return Span::new(panels::bar_cell(row, (start, size)), Style::default());
    }
    let style = |fg, bg| Style {
        fg,
        bg,
        ..Style::default()
    };
    match (upper, lower) {
        (Half::Outside, Half::Outside) => Span::new(" ", style(None, colour(Half::Outside))),
        (Half::Outside, below) => Span::new("▄", style(colour(below), colour(Half::Outside))),
        (above, Half::Outside) => Span::new("▀", style(colour(above), colour(Half::Outside))),
        // The colour behind the glyph too: xterm draws a block short of the
        // cell and the terminal's own background would show in the gap.
        (above, below) if above == below => Span::new("█", style(colour(above), colour(above))),
        (above, below) => Span::new("▀", style(colour(above), colour(below))),
    }
}

/// What a press on the scroll bar's column asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grabbed {
    /// A page up or down, towards the press, on the track.
    Page { down: bool },
    /// The thumb taken hold of, this many half rows from its top.
    Grip(isize),
}

/// What a press on `row` of the scroll bar, from 1 in the bar, does, when
/// the bar's `rows` show the listed rows from `first` of `listed`.
pub(crate) fn press_thumb(
    (listed, rows, first): (usize, usize, usize),
    row: usize,
) -> Option<Grabbed> {
    let (start, size) = thumb(listed, rows, first)?;
    let row = row.checked_sub(1).filter(|&row| row < rows)?;
    let (upper, lower) = (2 * row, 2 * row + 1);
    Some(if lower < start {
        Grabbed::Page { down: false }
    } else if upper >= start + size {
        Grabbed::Page { down: true }
    } else {
        // Negative where the thumb starts in the lower half of the pressed cell.
        Grabbed::Grip(upper as isize - start as isize)
    })
}

/// The listed row at the top of the bar when the thumb, held `grip` half
/// rows from its top, is dragged to `row` of the bar, from 1: the other
/// way round from [`thumb`].
pub(crate) fn dragged_to((listed, rows): (usize, usize), grip: isize, row: usize) -> usize {
    let half = 2 * row.saturating_sub(1);
    let start = usize::try_from(half as isize - grip).unwrap_or(0);
    if rows < INSET_MIN_ROWS {
        return panels::top_at(listed, rows, start);
    }
    // The other way round from `inner_thumb`: its halves start after the
    // free half row at the top.
    let (_, size) = inner_thumb(listed, rows, 0);
    let room = (2 * rows - 2).saturating_sub(size).max(1);
    let lines = listed.saturating_sub(rows);
    let at = (start.saturating_sub(1).min(room) * lines + room / 2) / room;
    at.min(lines)
}

/// What a click on a cell of the bar chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Clicked {
    /// The entry, as an index of the entries, to go to.
    Entry(usize),
    /// The entry whose fold glyph it was: to fold or open.
    Fold(usize),
}

/// What a click at `column` and `row` of a bar `columns` wide chose, both
/// counted from 1 in the bar, when its top row shows the listed row
/// `first`. Nothing for an empty row and for the border, the last column.
pub(crate) fn hit(
    contents: &Contents,
    (folds, names): (&Folds, &[String]),
    first: usize,
    (columns, column, row): (usize, usize, usize),
) -> Option<Clicked> {
    if column >= columns {
        return None;
    }
    let entries = contents.entries();
    let at = (*listed(contents, (folds, names)).get(first + row.checked_sub(1)?)?)?;
    // The glyph is after the edge cell and the indent, and the space after
    // it belongs to it: the glyph is easier to hit then.
    let glyph = 2 + indent(&entries[at], shift(entries));
    match has_children(entries, at) && (glyph..=glyph + 1).contains(&column) {
        true => Some(Clicked::Fold(at)),
        false => Some(Clicked::Entry(at)),
    }
}

/// The rows of the list: the entries that are listed, as indexes of the
/// contents' entries (those that no folded entry above them hides), and
/// `None` for a gap, the blank row before each file but the first when
/// several files are listed.
pub(crate) fn listed(
    contents: &Contents,
    (folds, names): (&Folds, &[String]),
) -> Vec<Option<usize>> {
    let entries = contents.entries();
    let several = shift(entries) > 0;
    let mut listed = Vec::new();
    let mut hidden_by: Option<(usize, u8)> = None;
    for (at, entry) in entries.iter().enumerate() {
        if let Some((file, level)) = hidden_by {
            if entry.file == file && entry.level > level {
                continue;
            }
            hidden_by = None;
        }
        if several && entry.level == 0 && !listed.is_empty() {
            listed.push(None);
        }
        listed.push(Some(at));
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

/// A row of `columns` cells: the `edge` cell, the indent, the glyph, a
/// space and the text, cut with an ellipsis where it does not fit, and
/// filled to the width, with the border last when there is one. With a
/// scroll bar cell, `bar`, with the empty `gap` cell before it, the text has
/// two columns less: the gap, in the bar's background so that the row's tint
/// stops before it, and the bar's own, which is next
/// to the border: the last column, which a bar without a border leaves
/// empty then, is the one the mouse drags. With a `rule` the text and the
/// empty cells after it, up to where the tint ends, are underlined.
fn row(
    (indent, glyph, text): (&str, char, &str),
    columns: usize,
    (look, style, border, page): (&RowLook, &Style, Option<&Style>, &Style),
    (edge, bar, rule): (Span, Option<(Span, Span)>, Option<Underline>),
) -> Line {
    let border_columns = usize::from(border.is_some() && columns > 0);
    let bar = bar.filter(|_| columns > 3);
    // The gap before the scroll bar, its column, and the border's, which is
    // there but empty in a plain bar. Without a scroll bar too the last
    // column is blank: it is the one the mouse drags.
    let right = if bar.is_some() {
        3
    } else {
        usize::from(columns > 0)
    };
    let body = columns.saturating_sub(right);
    let room = body.saturating_sub(indent.width() + 3);
    let text = status::head(text, room);
    let used = 1 + indent.width() + 2 + text.width();
    // With a `rule` the name and the fill after it are underlined in the
    // colour of the text; the space before the name is not.
    let ruled = |style: &Style, fg| Style {
        underline: rule.unwrap_or_default(),
        fg,
        ..style.clone()
    };
    // A file's row is underlined from the cell after the edge to the end of
    // the row's tint: the indent, the glyph, the name and the fill.
    let (indent_style, glyph_style) = match rule {
        Some(_) => (
            ruled(&look.fill, look.fill.fg),
            ruled(&look.glyph, look.glyph.fg),
        ),
        None => (look.fill.clone(), look.glyph.clone()),
    };
    let mut spans = vec![
        edge,
        Span::new(indent, indent_style),
        Span::new(&glyph.to_string(), glyph_style),
    ];
    match rule {
        Some(_) => {
            spans.push(Span::new(" ", ruled(style, style.fg)));
            spans.push(Span::new(&text, ruled(style, style.fg)));
        }
        None => spans.push(Span::new(&format!(" {text}"), style.clone())),
    }
    if used < body {
        let fill = " ".repeat(body - used);
        let fill_style = match rule {
            Some(_) => ruled(&look.fill, style.fg),
            None => look.fill.clone(),
        };
        spans.push(Span::new(&fill, fill_style));
    }
    // An indent deeper than the bar leaves no room for the glyph: cut.
    let mut spans = crate::typeset::wrap::cut(spans, body);
    if let Some((gap, bar)) = bar {
        spans.push(gap);
        spans.push(bar);
    }
    match border.filter(|_| border_columns > 0) {
        Some(border) => spans.push(Span::new(BORDER, border.clone())),
        // The page's own, so that a selection's reverse video stops before it.
        None if right > 0 => spans.push(Span::new(" ", page.clone())),
        None => {}
    }
    Line::new(spans)
}

#[cfg(test)]
mod tests_coloured;

#[cfg(test)]
mod tests_files;

#[cfg(test)]
mod tests_scrollbar;

#[cfg(test)]
mod tests;
