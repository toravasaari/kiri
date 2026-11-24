//! The header of a file among several: its rows, and the one row of it that
//! stays at the top of the pager when the rows are scrolled past.

use crate::typeset::block::Header;
use crate::typeset::layout::Layout;
use crate::typeset::style::{Line, Spaces, Span, Style, push};
use crate::typeset::wrap::{self, wrap};

impl Layout<'_> {
    /// The header of the file `name`: a band of its own colour across
    /// `window`, the whole document area and not only the text's `width`,
    /// an empty row, the name and another empty row, with a bar
    /// down its left edge and the name `margin` cells in, at least two:
    /// the directory muted and the file's own name in bold. Nothing in
    /// Markdown draws such a band. Plain text gets the name between
    /// arrows instead, as `head` writes it.
    pub(crate) fn file_header(
        &self,
        name: &str,
        width: usize,
        window: usize,
        margin: usize,
    ) -> Vec<Line> {
        if !self.caps.styling {
            let spans = [Span::new(&format!("==> {name} <=="), Style::default())];
            return wrap(&spans, width).into_iter().map(Line::new).collect();
        }
        let (directory, file) = self.name_spans(name);
        let mut spans = directory;
        spans.extend(file);
        // A cell of the band is left after the longest row.
        let rows = wrap(&spans, window.saturating_sub(margin.max(2) + 1));
        let mut lines = vec![self.name_row(Vec::new(), window, margin)];
        lines.extend(
            rows.into_iter()
                .map(|row| self.name_row(row, window, margin)),
        );
        lines.push(self.name_row(Vec::new(), window, margin));
        lines
    }

    /// What the layout records of the header of the file `name`, which it
    /// has laid out as `rows` lines: how many they are, where the name
    /// starts in them, and the name on one row, as it looks on its row
    /// in the header, cut from the left with `…` when it is longer than
    /// the room there is, so that the file's own name stays as far as it
    /// fits.
    pub(crate) fn record_header(&self, name: &str, rows: usize, window: usize, margin: usize) {
        let sticky = match self.caps.styling {
            false => {
                let room = window.max(1);
                let text = wrap::tail(&format!("==> {name} <=="), room);
                Line::new(vec![Span::new(&text, Style::default())])
            }
            true => {
                let room = window.saturating_sub(margin.max(2) + 1);
                self.name_row(self.name_that_fits(name, room), window, margin)
            }
        };
        let name = usize::from(self.caps.styling);
        let header = Header { rows, name, sticky };
        self.marks
            .borrow_mut()
            .table
            .set_header(self.document, header);
    }

    /// The styles of a name row: the band's with the spaces kept, and the
    /// bar's.
    fn name_styles(&self) -> (Style, Style) {
        let base = Style {
            bg: self.look.band,
            // A name keeps its spaces as they are.
            spaces: Spaces::Keep,
            ..Style::default()
        };
        let bar = Style {
            fg: self.look.file_header.bar,
            ..base.clone()
        };
        (base, bar)
    }

    /// The directory of `name`, muted, and the file's own name, in bold.
    fn name_spans(&self, name: &str) -> (Vec<Span>, Vec<Span>) {
        let (base, _) = self.name_styles();
        let (directory, file) = name.split_at(name.rfind('/').map_or(0, |at| at + 1));
        let mut directories = Vec::new();
        push(&mut directories, directory, &self.muted(base.clone()));
        let bold = Style { bold: true, ..base };
        let mut files = Vec::new();
        push(&mut files, file, &bold);
        (directories, files)
    }

    /// The name `name` in at most `room` cells: whole, or with `…` for
    /// the start of its directory, or, when not even the file's own name
    /// fits, the end of it after `…`.
    fn name_that_fits(&self, name: &str, room: usize) -> Vec<Span> {
        let (directory, file) = self.name_spans(name);
        let width = |spans: &[Span]| wrap::width(spans);
        if width(&directory) + width(&file) <= room {
            return directory.into_iter().chain(file).collect();
        }
        let (base, _) = self.name_styles();
        if width(&file) < room {
            let text = &directory
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>();
            let left = room - width(&file);
            let kept = wrap::tail(text, left);
            let kept = if kept.is_empty() {
                "…".to_owned()
            } else {
                kept
            };
            let mut spans = Vec::new();
            push(&mut spans, &kept, &self.muted(base));
            spans.extend(file);
            return spans;
        }
        let text = file
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        let bold = Style { bold: true, ..base };
        let mut spans = Vec::new();
        push(&mut spans, &wrap::tail(&text, room.max(1)), &bold);
        spans
    }

    /// One row of the header: the bar, the indent, `content` and the band
    /// to the window's edge. The empty rows and the name's rows are made
    /// by this, and so is the sticky row, so that they look alike.
    fn name_row(&self, content: Vec<Span>, window: usize, margin: usize) -> Line {
        let band = self.look.band;
        let (base, bar) = self.name_styles();
        let indent = margin.max(2);
        let mut spans = vec![Span::new("▌", bar)];
        // Without a band, an empty row ends at the bar.
        if band.is_some() || !content.is_empty() {
            push(&mut spans, &" ".repeat(indent - 1), &base);
        }
        spans.extend(content);
        if band.is_some() {
            let used = wrap::width(&spans);
            push(&mut spans, &" ".repeat(window.saturating_sub(used)), &base);
        }
        Line::new(spans)
    }
}
