//! What a scene shows on the screen, as the scene meant it, for tests: the
//! window's rows as owned text, with an image as a marker that names it
//! and the row of it that is there, for example `⟦fern r2⟧`, and no escape
//! sequences. It knows no terminal and no protocol, so a test that reads it
//! holds for every adapter that draws the scene right. What was written, an
//! image sent once or a row not rewritten, is not here: it is the bytes of
//! the adapter, and its tests read those.

use unicode_width::UnicodeWidthChar;

use crate::present::scene::{Scene, Surface, flatten};
use crate::typeset::images::ImageKey;
use crate::typeset::images::size::{Placement, Shows};
use crate::typeset::size::TextSize;
use crate::typeset::style::{Embed, Line, Span, Style};
use crate::typeset::wrap::width;

/// One cell of the screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// The character in the cell, a marker for the first cell of an image
    /// row, and nothing for a cell that a wider character or an image's
    /// marker covers.
    pub text: String,
    pub style: Style,
}

impl Cell {
    fn blank() -> Self {
        Self::new(" ", Style::default())
    }

    fn new(text: &str, style: Style) -> Self {
        Self {
            text: text.to_owned(),
            style,
        }
    }
}

/// One row of the screen.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// Whether a panel over the document dims the row.
    pub dim: bool,
}

impl Row {
    /// The row's text, without the blanks at its end.
    pub fn text(&self) -> String {
        let text: String = self.cells.iter().map(|cell| cell.text.as_str()).collect();
        text.trim_end().to_owned()
    }

    /// The first cell of the first `needle` in the row's text: the cell
    /// that holds the needle's first character.
    pub fn find(&self, needle: &str) -> Option<usize> {
        let mut text = String::new();
        let mut starts = Vec::new();
        for (at, cell) in self.cells.iter().enumerate() {
            starts.extend(std::iter::repeat_n(at, cell.text.len()));
            text.push_str(&cell.text);
        }
        text.find(needle).map(|byte| starts[byte])
    }
}

/// The screen after a frame: a row for each row of the window.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Seen {
    pub rows: Vec<Row>,
}

impl Seen {
    /// The screen that `scene` means, `rows` telling how many rows a line
    /// takes.
    pub fn of(scene: &Scene<'_>, rows: impl Fn(&Line) -> u8) -> Self {
        let flat = flatten(scene, &rows);
        let columns = usize::from(scene.window.0);
        let mut screen: Vec<Row> = flat
            .rows
            .iter()
            .map(|row| {
                let mut cells = match row.line {
                    Some((line, at)) => line_cells(line, at, rows(line) > 1),
                    None => Vec::new(),
                };
                // The loading band is drawn over the first row of its line,
                // and not behind a panel.
                if let Some((line, 0)) = row.line
                    && let Some(band) = (line.loading.as_ref())
                        .and_then(|loading| loading.band.as_ref())
                        .filter(|_| !row.dim)
                {
                    let mut over = Vec::new();
                    image_cells(
                        "⟦loading⟧",
                        usize::from(band.columns),
                        &Style::default(),
                        &mut over,
                    );
                    if cells.len() < band.indent {
                        cells.resize(band.indent, Cell::blank());
                    }
                    for (at, cell) in over.into_iter().enumerate() {
                        match cells.get_mut(band.indent + at) {
                            Some(old) => *old = cell,
                            None => cells.push(cell),
                        }
                    }
                }
                Row {
                    cells,
                    dim: row.dim,
                }
            })
            .collect();
        for surface in &scene.surfaces {
            let Surface::Bar { row, column, spans } = surface else {
                continue;
            };
            let Some(shown) = row.checked_sub(1).and_then(|at| screen.get_mut(at)) else {
                continue;
            };
            let mut over = Vec::new();
            for span in *spans {
                span_cells(span, 0, true, &mut over);
            }
            let from = column.saturating_sub(1);
            if shown.cells.len() < from {
                shown.cells.resize(from, Cell::blank());
            }
            for (at, cell) in over.into_iter().enumerate() {
                match shown.cells.get_mut(from + at) {
                    Some(old) => *old = cell,
                    None => shown.cells.push(cell),
                }
            }
        }
        for row in &mut screen {
            row.cells.truncate(columns.max(1));
        }
        Self { rows: screen }
    }

    /// The rows' texts, one under the other.
    pub fn text(&self) -> String {
        let rows: Vec<String> = self.rows.iter().map(Row::text).collect();
        rows.join("\n")
    }

    /// The text of row `row`, counted from 1.
    pub fn row(&self, row: u16) -> String {
        self.rows[usize::from(row) - 1].text()
    }

    /// The first row, counted from 1, above row `rows` that shows
    /// `needle`: the window's rows without its status line.
    pub fn row_of(&self, rows: u16, needle: &str) -> Option<u16> {
        (1..rows).find(|&row| {
            self.rows
                .get(usize::from(row) - 1)
                .is_some_and(|shown| shown.find(needle).is_some())
        })
    }

    /// The style of the first `needle` on the screen, which is on the
    /// first row that shows it, where its first character is.
    pub fn style_of(&self, needle: &str) -> Option<&Style> {
        self.rows.iter().find_map(|row| {
            let at = row.find(needle)?;
            Some(&row.cells[at].style)
        })
    }

    /// The style of the first `needle` in row `row`, counted from 1.
    pub fn style_in(&self, row: u16, needle: &str) -> Option<&Style> {
        let shown = self.rows.get(usize::from(row) - 1)?;
        shown.find(needle).map(|at| &shown.cells[at].style)
    }
}

/// The cells of row `at` of `line`, laid as the writer lays them:
/// the lead on every row, the spans and the image on the first, the images
/// of the spans below it, and the trail on every row. `tall` tells that the
/// line takes more than one row.
fn line_cells(line: &Line, at: u8, tall: bool) -> Vec<Cell> {
    let mut cells = Vec::new();
    for (index, span) in line.lead.iter().enumerate() {
        if at > 0 && line.lead_once.get(index) == Some(&true) {
            let blank = " ".repeat(width(std::slice::from_ref(span)));
            span_cells(&Span::new(&blank, span.style.clone()), 0, true, &mut cells);
        } else {
            span_cells(span, 0, true, &mut cells);
        }
    }
    if at == 0 {
        for span in &line.spans {
            span_cells(span, 0, tall, &mut cells);
        }
        if let Some(image) = &line.image {
            let style = Style {
                bg: image.background,
                ..Style::default()
            };
            let name = marker(&image.placement, image.row);
            let first = cells.len();
            image_cells(&name, image.columns.len(), &style, &mut cells);
            // The links of a page are over its cells, by the image's own
            // columns.
            for (at, cell) in cells[first..].iter_mut().enumerate() {
                let column = image.columns.start + at as u16;
                if let Some(link) = image.links.iter().find(|l| l.columns.contains(&column)) {
                    cell.style.link = Some(link.url.clone());
                }
            }
        }
    } else {
        for span in &line.spans {
            match &span.embed {
                Some(embed) if u16::from(at) < embed.rows => {
                    span_cells(span, at, tall, &mut cells);
                }
                // What the writer draws blank below the first row, and
                // steps over otherwise.
                _ if span.style.size == TextSize::NORMAL && span.style.bg.is_some() => {
                    let blank = " ".repeat(width(std::slice::from_ref(span)));
                    let style = Style {
                        bg: span.style.bg,
                        ..Style::default()
                    };
                    span_cells(&Span::new(&blank, style), 0, true, &mut cells);
                }
                _ => {
                    let blank = " ".repeat(width(std::slice::from_ref(span)));
                    span_cells(&Span::new(&blank, Style::default()), 0, true, &mut cells);
                }
            }
        }
    }
    for span in &line.trail {
        span_cells(span, 0, true, &mut cells);
    }
    cells
}

/// The cells of `span` on row `row` of its line: its text, each character at
/// the size of the span when `sized`, or the row `row` of its image.
fn span_cells(span: &Span, row: u8, sized: bool, cells: &mut Vec<Cell>) {
    if let Some(embed) = &span.embed {
        let name = marker_of(embed, u16::from(row));
        let style = Style {
            bg: span.style.bg,
            link: span.style.link.clone(),
            ..Style::default()
        };
        image_cells(
            &name,
            usize::from(embed.placement.size.columns),
            &style,
            cells,
        );
        return;
    }
    let scale = match (sized, span.style.size.fraction()) {
        (true, None) => usize::from(span.style.size.scale()),
        _ => 1,
    };
    for c in span.text.chars() {
        let cols = c.width().unwrap_or(0) * scale;
        if cols == 0 {
            // A combining character belongs to the one before it.
            if let Some(last) = cells.iter_mut().rev().find(|cell| !cell.text.is_empty()) {
                last.text.push(c);
            }
            continue;
        }
        cells.push(Cell::new(&c.to_string(), span.style.clone()));
        for _ in 1..cols {
            cells.push(Cell::new("", span.style.clone()));
        }
    }
}

/// The cells of an image `columns` wide: its marker in the first.
fn image_cells(name: &str, columns: usize, style: &Style, cells: &mut Vec<Cell>) {
    cells.push(Cell::new(name, style.clone()));
    for _ in 1..columns {
        cells.push(Cell::new("", style.clone()));
    }
}

/// The marker of row `row` of the image of `embed`'s placement on a line's
/// row `at`.
fn marker_of(embed: &Embed, at: u16) -> String {
    marker(&embed.placement, embed.row + at)
}

/// The marker of row `row` of `placement`'s image.
fn marker(placement: &Placement, row: u16) -> String {
    let dimmed = if placement.shows == Shows::Placeholder {
        " dimmed"
    } else {
        ""
    };
    format!("⟦{} r{row}{dimmed}⟧", name_of(&placement.key))
}

/// A short name of an image, as a test would write it.
fn name_of(key: &ImageKey) -> String {
    match key {
        ImageKey::Url { url, .. } => {
            let file = url.rsplit('/').next().unwrap_or(url);
            file.rsplit_once('.')
                .map_or(file, |(stem, _)| stem)
                .to_owned()
        }
        ImageKey::Rendered { language, .. } => language.clone(),
        ImageKey::Heading { level, .. } => format!("h{level}"),
        ImageKey::Page { slice, .. } => format!("page{slice}"),
        ImageKey::Band { band, slice, .. } => format!("band{band}.{slice}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn lines_scene(lines: &[Line], rows: u16) -> Scene<'_> {
        Scene {
            window: (20, rows),
            surfaces: vec![Surface::Lines { lines, dim: false }],
        }
    }

    fn plain(text: &str) -> Line {
        Line::new(vec![Span::new(text, Style::default())])
    }

    #[test]
    fn rows_are_text_and_an_image_is_a_marker_with_its_row() {
        use crate::typeset::images::size::{Shows, Size};
        let size = Size {
            width: 40,
            height: 40,
            columns: 4,
            rows: 2,
        };
        let placement = Rc::new(Placement::block(
            ImageKey::from("img/fern.png"),
            size,
            Shows::Image,
        ));
        let lines = [
            plain("one"),
            Line::image(Rc::clone(&placement), 0),
            Line::image(placement, 1),
        ];
        let seen = Seen::of(&lines_scene(&lines, 4), Line::rows);
        assert_eq!(seen.text(), "one\n⟦fern r0⟧\n⟦fern r1⟧\n");
        assert_eq!(seen.rows[1].cells.len(), 4);
    }

    #[test]
    fn a_bar_is_laid_over_the_row_from_its_column() {
        let lines = [plain("0123456789")];
        let spans = [Span::new("ab", Style::default())];
        let scene = Scene {
            window: (20, 1),
            surfaces: vec![
                Surface::Lines {
                    lines: &lines,
                    dim: false,
                },
                Surface::Bar {
                    row: 1,
                    column: 4,
                    spans: &spans,
                },
            ],
        };
        assert_eq!(Seen::of(&scene, Line::rows).text(), "012ab56789");
    }

    #[test]
    fn a_scaled_line_shows_its_text_on_its_first_row_only() {
        let size = TextSize::new(2).unwrap();
        let big = Line::new(vec![Span::new(
            "Big",
            Style {
                size,
                ..Style::default()
            },
        )]);
        let lines = [big];
        let seen = Seen::of(&lines_scene(&lines, 3), Line::rows);
        assert_eq!(seen.text(), "Big\n\n");
        assert_eq!(seen.rows[0].cells.len(), 6);
    }

    #[test]
    fn the_style_of_a_word_is_the_style_of_its_first_cell() {
        let marked = Style {
            reverse: true,
            ..Style::default()
        };
        let lines = [Line::new(vec![
            Span::new("a ", Style::default()),
            Span::new("word", marked.clone()),
        ])];
        let seen = Seen::of(&lines_scene(&lines, 1), Line::rows);
        assert_eq!(seen.style_of("word"), Some(&marked));
        assert_eq!(seen.style_in(1, "a"), Some(&Style::default()));
        assert_eq!(seen.style_of("absent"), None);
    }
}
