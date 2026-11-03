//! The links in the pager: which of the links on the screen is selected,
//! by Tab and Shift+Tab. A link is a hyperlink, which kitty opens on its
//! own; the selection draws a box round it in a page of reading mode and
//! shows it in reverse video in text, and the status line says where it
//! goes.

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use crate::session::bands::Bands;
use crate::typeset::block::BlockInfo;
use crate::typeset::reading::engine;
use crate::typeset::style::{Line, Span};
use crate::typeset::wrap::width;

/// A link, by the block its first line is in and its number among the
/// links of that block, so that it is found again in a new layout.
type LinkId = (Option<BlockInfo>, usize);

/// A link on the lines of a layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    id: LinkId,
    /// For a link in a page of reading mode, the page it is on, by the
    /// address of its parts, and its number among the page's links.
    pub link: Option<(usize, usize)>,
    pub url: String,
    /// The lines that have cells of it, none while it has none.
    lines: Vec<usize>,
    /// For a link in text, the spans of each of its lines, which are
    /// consecutive and of the link alone.
    pub runs: Vec<(usize, Range<usize>)>,
    /// For a link in a band, the line it is on and the pixels it covers
    /// across the band's content, to scroll the band to it.
    pub band_at: Option<(usize, Range<u32>)>,
}

impl Found {
    /// Whether the link is in a band that shows none of its cells.
    pub fn is_scrolled_out(&self) -> bool {
        self.lines.is_empty() && self.band_at.is_some()
    }

    /// The line the link comes first on, for putting it in order among
    /// the others.
    fn first(&self) -> usize {
        let band = self.band_at.as_ref().map(|(line, _)| *line);
        self.lines.first().copied().or(band).unwrap_or(usize::MAX)
    }

    /// Whether the link is on the lines `visible`: any of its cells is, or
    /// while a band shows none of them, the band's line is.
    fn is_on(&self, visible: &Range<usize>) -> bool {
        match self.lines.is_empty() {
            false => self.lines.iter().any(|line| visible.contains(line)),
            true => (self.band_at.as_ref()).is_some_and(|(line, _)| visible.contains(line)),
        }
    }
}

/// Which link is selected.
#[derive(Debug, Default)]
pub(crate) struct Links {
    selected: Option<LinkId>,
}

impl Links {
    /// Selects the next link on the lines `visible`, or the one before it
    /// when `back`, from the selected one, or from the first or the last
    /// when none is on the screen; whether that changed the selection.
    pub fn select_next(
        &mut self,
        lines: &[Line],
        places: &[Option<BlockInfo>],
        visible: Range<usize>,
        back: bool,
    ) -> bool {
        let shown: Vec<Found> = (identify(lines, places).into_iter())
            .filter(|found| found.is_on(&visible))
            .collect();
        if shown.is_empty() {
            return false;
        }
        let at = (self.selected.as_ref()).and_then(|id| shown.iter().position(|f| f.id == *id));
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => shown.len() - 1,
            (Some(at), false) => (at + 1) % shown.len(),
            (Some(at), true) => (at + shown.len() - 1) % shown.len(),
        };
        let changed = self.selected.as_ref() != Some(&shown[next].id);
        self.selected = Some(shown[next].id.clone());
        changed
    }

    /// As [`Links::select_next`], and a link in a band that shows none of
    /// its cells scrolls `bands` to it, as a search match does: whether
    /// a band was scrolled.
    pub fn select_scrolling(
        &mut self,
        bands: &mut Bands,
        lines: &[Line],
        places: &[Option<BlockInfo>],
        visible: Range<usize>,
        back: bool,
    ) -> bool {
        self.select_next(lines, places, visible, back);
        let target = self.target(lines, places).filter(Found::is_scrolled_out);
        let Some((line, x)) = target.and_then(|found| found.band_at) else {
            return false;
        };
        bands.reveal(lines, places, line, x) == Some(true)
    }

    /// The selected link whether or not a band shows its cells: where to
    /// scroll the band to, when it does not.
    pub fn target(&self, lines: &[Line], places: &[Option<BlockInfo>]) -> Option<Found> {
        let id = self.selected.as_ref()?;
        (identify(lines, places).into_iter()).find(|found| found.id == *id)
    }

    /// Selects no link.
    pub fn clear(&mut self) -> bool {
        self.selected.take().is_some()
    }

    /// The selected link when any of its cells is on the lines `visible`,
    /// as the lines of this layout have it.
    pub fn selected_on(
        &self,
        lines: &[Line],
        places: &[Option<BlockInfo>],
        visible: Range<usize>,
    ) -> Option<Found> {
        (self.selected(lines, places))
            .filter(|found| found.lines.iter().any(|line| visible.contains(line)))
    }

    /// The selected link, as the lines of this layout have it, when it is
    /// still on them.
    pub fn selected(&self, lines: &[Line], places: &[Option<BlockInfo>]) -> Option<Found> {
        let id = self.selected.as_ref()?;
        let found = identify(lines, places)
            .into_iter()
            .find(|found| found.id == *id)?;
        // While a band shows none of its cells, there is nothing to show.
        (!found.lines.is_empty()).then_some(found)
    }
}

/// The links of the pages and the text on `lines`, each once, in order,
/// numbered from the pages' own lists for the pages, so that a link keeps
/// its id whether or not a band shows it now; `lines` are where its cells
/// are, none while it has none, and `band_at` where to scroll its band to.
/// A link in text is what `text_links` finds: spans side by side on a line
/// with one address, and one that wraps is one link. A link without an
/// address is left out, as the terminal shows no link there.
fn identify(lines: &[Line], places: &[Option<BlockInfo>]) -> Vec<Found> {
    // The lines that have cells of each link of a page.
    let mut cells: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        for link in line.image.iter().flat_map(|image| &image.links) {
            let at = cells.entry(link.link).or_default();
            if at.last() != Some(&index) {
                at.push(index);
            }
        }
    }
    let mut counted: Vec<(Option<BlockInfo>, usize)> = Vec::new();
    // The number of a link among those of the block it first comes in.
    let mut number_in =
        |place: &Option<BlockInfo>| match counted.iter_mut().find(|(kept, _)| kept == place) {
            Some((_, count)) => {
                *count += 1;
                *count - 1
            }
            None => {
                counted.push((place.clone(), 1));
                0
            }
        };
    let mut found = Vec::new();
    for page in engine::pages(lines) {
        let address = Rc::as_ptr(&page.parts) as usize;
        for (number, link) in page.parts.links().iter().enumerate() {
            if link.url.is_empty() {
                continue;
            }
            let at = (page.lines.start + page.parts.link_row(number)).min(page.lines.end - 1);
            let place = places.get(at).cloned().flatten();
            let nth = number_in(&place);
            // Where to scroll its band to, to show the first piece of it.
            let band_at = link.band.and(link.pieces.first()).map(|piece| {
                let (start, end) = (piece.x.floor().max(0.0), (piece.x + piece.width).ceil());
                (at, start as u32..end as u32)
            });
            found.push(Found {
                id: (place, nth),
                link: Some((address, number)),
                url: link.url.to_string(),
                lines: cells.remove(&(address, number)).unwrap_or_default(),
                runs: Vec::new(),
                band_at,
            });
        }
    }
    let in_pages = !found.is_empty();
    for (url, runs) in text_links(lines) {
        if url.is_empty() {
            continue;
        }
        let first = runs[0].0;
        let place = places.get(first).cloned().flatten();
        let mut on: Vec<usize> = runs.iter().map(|(line, _)| *line).collect();
        on.dedup();
        found.push(Found {
            id: (place.clone(), number_in(&place)),
            link: None,
            url: url.to_string(),
            lines: on,
            runs,
            band_at: None,
        });
    }
    // Pages and text are not mixed in a layout as a rule; when they are,
    // the links follow the lines.
    if in_pages && found.iter().any(|found| found.link.is_none()) {
        found.sort_by_key(Found::first);
    }
    found
}

/// The bars a quote or an alert puts in front of each of its lines
/// (`src/typeset/layout/quote.rs`), which are an indent too, and the
/// borders of a table, which are the edges of its cells.
const BARS: [char; 2] = ['│', '▎'];

/// Whether `span` is the edge of a cell: the border of a table, or the bar
/// of a quote.
fn is_edge(span: &Span) -> bool {
    let text = span.text.trim();
    span.embed.is_none() && !text.is_empty() && text.chars().all(|c| BARS.contains(&c))
}

/// A link in text: its address and the spans of each line it is on.
type TextLink = (Rc<str>, Vec<(usize, Range<usize>)>);

/// The links in the spans of `lines`, each by its address and the spans of
/// each line it is on. Spans side by side on a line with one address are
/// one link, as the terminal shows them as one hyperlink. A link continues
/// from the line before when that has it at the end of a cell, as the same
/// link (one `Rc`, so two links to one address stay apart, as the rows of
/// a table do), of the same block: nothing but blanks come after it there
/// up to the cell's edge (a table's border or the end of the line), and
/// before it here back to the cell's edge, such as an indent in a list or
/// a quote. A cell is the same one when the last edge before the link ends
/// in the same column on both lines, so a bar in the text of a cell to the
/// left does not count.
fn text_links(lines: &[Line]) -> Vec<TextLink> {
    let blank = |spans: &[Span]| {
        spans
            .iter()
            .all(|span| span.embed.is_none() && span.text.trim().is_empty())
    };
    let mut links: Vec<TextLink> = Vec::new();
    // The links the line before ends a cell with, the cell of each, and the
    // link of the last span of each.
    let mut open: Vec<(usize, usize, Rc<str>)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let carried = std::mem::take(&mut open);
        // The runs of spans that are of one address, with the link of the
        // first span and of the last.
        let mut runs: Vec<(Rc<str>, Rc<str>, Range<usize>)> = Vec::new();
        if line.image.is_none() {
            for (at, span) in line.spans.iter().enumerate() {
                let Some(url) = &span.style.link else {
                    continue;
                };
                match runs.last_mut() {
                    Some((_, last, range)) if range.end == at && **last == **url => {
                        *last = Rc::clone(url);
                        range.end = at + 1
                    }
                    _ => runs.push((Rc::clone(url), Rc::clone(url), at..at + 1)),
                }
            }
        }
        for (url, last, range) in runs {
            let (before, after) = (&line.spans[..range.start], &line.spans[range.end..]);
            let edge = before.iter().rposition(is_edge).map_or(0, |at| at + 1);
            let cell = width(&before[..edge]);
            let goes_on = carried
                .iter()
                .find(|(_, kept, above)| *kept == cell && Rc::ptr_eq(above, &url))
                .filter(|_| lines[index - 1].block == line.block && blank(&before[edge..]))
                .map(|&(link, ..)| link);
            let link = match goes_on {
                Some(link) => link,
                None => {
                    links.push((url, Vec::new()));
                    links.len() - 1
                }
            };
            let end = after.iter().position(is_edge).unwrap_or(after.len());
            if blank(&after[..end]) {
                open.push((link, cell, last));
            }
            links[link].1.push((index, range));
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::render::Inputs;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::layout::{Content, Stream, content};
    use crate::typeset::reading::Fonts;
    use crate::typeset::reading::engine::{self, Reading};
    use crate::typeset::reading::page::Wide;

    /// `source` laid out in reading mode, with the blocks of its lines.
    fn laid_out(source: &str) -> (Vec<Line>, Vec<Option<BlockInfo>>) {
        let document = crate::typeset::parse::parse(source);
        let inputs = Inputs {
            reading: Some(Reading {
                pictures: Default::default(),
                fonts: Rc::new(Fonts::fixture()),
                wide: Wide::Wrap,
            }),
            ..Inputs::new(Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            })
        };
        let Stream { lines, blocks } = content(Content::Document(&document), &inputs);
        let places = (lines.iter())
            .map(|line| line.block.map(|id| blocks.get(id).clone()))
            .collect();
        (lines, places)
    }

    /// A span of the link `url`, as text or as an image's cells.
    fn linked(text: &str, url: &Rc<str>, embed: bool) -> Span {
        let style = crate::typeset::style::Style {
            link: Some(Rc::clone(url)),
            ..Default::default()
        };
        match embed {
            true => Span::image(text, 2, style),
            false => Span::new(text, style),
        }
    }

    #[test]
    fn text_an_image_and_text_of_one_link_are_one_run() {
        let url: Rc<str> = Rc::from("u");
        let line = Line::new(vec![
            Span::new("a ", Default::default()),
            linked("see ", &url, false),
            linked("alt", &url, true),
            linked(" more", &url, false),
        ]);
        let found = text_links(&[line]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, [(0, 1..4)]);
    }

    #[test]
    fn an_image_before_a_link_keeps_it_from_continuing_the_link_above() {
        let url: Rc<str> = Rc::from("u");
        let above = Line::new(vec![linked("one", &url, false)]);
        // Text before it on the second line: the same link goes on.
        let plain = Line::new(vec![linked("two", &url, false)]);
        assert_eq!(text_links(&[above.clone(), plain]).len(), 1);
        // An image is not a blank: the link there is another.
        let image = Span::image("", 2, Default::default());
        let after = Line::new(vec![image, linked("two", &url, false)]);
        assert_eq!(text_links(&[above, after]).len(), 2);
    }

    #[test]
    fn an_image_whose_text_is_a_bar_is_not_the_edge_of_a_cell() {
        let url: Rc<str> = Rc::from("u");
        let bar = Span::image("│", 2, Default::default());
        let line = Line::new(vec![bar, linked("a", &url, false)]);
        assert!(!is_edge(&line.spans[0]));
    }

    fn urls(found: Option<Found>) -> Option<String> {
        found.map(|found| found.url)
    }

    const DOCUMENT: &str = "A [one](u1) and [two](u2) on a line.\n\nBetween.\n\n- a [three](u3)\n\n| h |\n|---|\n| [four](u4) |\n";

    #[test]
    fn tab_selects_the_next_link_on_the_screen_and_shift_tab_the_one_before() {
        let (lines, places) = laid_out(DOCUMENT);
        let all = 0..lines.len();
        let mut links = Links::default();
        assert_eq!(urls(links.selected(&lines, &places)), None);
        let selected = |links: &Links| urls(links.selected(&lines, &places));
        // From none: the first, or backwards the last; and on round.
        assert!(links.select_next(&lines, &places, all.clone(), false));
        assert_eq!(selected(&links).as_deref(), Some("u1"));
        for expected in ["u2", "u3", "u4", "u1"] {
            assert!(links.select_next(&lines, &places, all.clone(), false));
            assert_eq!(selected(&links).as_deref(), Some(expected));
        }
        assert!(links.select_next(&lines, &places, all.clone(), true));
        assert_eq!(selected(&links).as_deref(), Some("u4"));
        assert!(links.clear());
        assert!(links.select_next(&lines, &places, all, true));
        assert_eq!(selected(&links).as_deref(), Some("u4"));
    }

    #[test]
    fn only_the_links_on_the_screen_are_there_to_select() {
        let (lines, places) = laid_out(DOCUMENT);
        let found = identify(&lines, &places);
        assert_eq!(found.len(), 4);
        // A screen that shows only the rows of the list item's link.
        let third = found[2].lines[0];
        let mut links = Links::default();
        assert!(links.select_next(&lines, &places, third..third + 1, false));
        assert_eq!(urls(links.selected(&lines, &places)).as_deref(), Some("u3"));
        // Nothing on a screen with no link.
        let mut none = Links::default();
        assert!(!none.select_next(&lines, &places, found[1].lines[0] + 1..third, false));
    }

    #[test]
    fn a_selected_link_is_found_again_in_a_new_layout_of_the_same_blocks() {
        let (lines, places) = laid_out(DOCUMENT);
        let mut links = Links::default();
        links.select_next(&lines, &places, 0..lines.len(), false);
        links.select_next(&lines, &places, 0..lines.len(), false);
        let (again, places_again) = laid_out(DOCUMENT);
        let found = links.selected(&again, &places_again).expect("found again");
        assert_eq!(found.url, "u2");
        // The page of the new layout, not the old one.
        let old = links.selected(&lines, &places).unwrap();
        assert_ne!(found.link.unwrap().0, old.link.unwrap().0);
        // A document without it has no selection to show.
        let (other, places_other) = laid_out("Nothing here.\n");
        assert_eq!(links.selected(&other, &places_other), None);
    }

    /// `source` laid out as text in 40 columns, with the blocks of its
    /// lines.
    fn text(source: &str) -> (Vec<Line>, Vec<Option<BlockInfo>>) {
        text_in(source, 40)
    }

    /// `source` laid out as text in `columns` columns.
    fn text_in(source: &str, columns: u16) -> (Vec<Line>, Vec<Option<BlockInfo>>) {
        let document = crate::typeset::parse::parse(source);
        let inputs = Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(columns)
        });
        let Stream { lines, blocks } = content(Content::Document(&document), &inputs);
        let places = (lines.iter())
            .map(|line| line.block.map(|id| blocks.get(id).clone()))
            .collect();
        (lines, places)
    }

    const TEXT: &str = "First [one](u1) and [two](u2)[three](u3) then [a long link text that wraps over lines](u4) end.\n\n> A quote with [a link that wraps over two lines in a quote](u5).\n\n- Item [six](u6)\n- Item [six](u6) again\n\nNo links.\n";

    #[test]
    fn links_in_text_are_found_in_order_a_wrapped_one_once_and_adjacent_ones_apart() {
        let (lines, places) = text(TEXT);
        let found = identify(&lines, &places);
        let urls: Vec<&str> = found.iter().map(|found| found.url.as_str()).collect();
        assert_eq!(urls, ["u1", "u2", "u3", "u4", "u5", "u6", "u6"]);
        // A wrapped link has cells on each of its lines, one after the
        // other, and the others on one.
        let lines_of = |found: &Found| found.lines.len();
        assert_eq!(
            found.iter().map(lines_of).collect::<Vec<_>>(),
            [1, 1, 1, 2, 2, 1, 1]
        );
        assert_eq!(found[3].lines[1], found[3].lines[0] + 1);
        // Each has an id of its own, which a new layout of the same
        // document gives it again.
        let (again, places_again) = text(TEXT);
        let ids = |found: &[Found]| {
            found
                .iter()
                .map(|found| found.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&identify(&again, &places_again)), ids(&found));
        let all = ids(&found);
        for (at, id) in all.iter().enumerate() {
            assert!(!all[at + 1..].contains(id), "{id:?} twice");
        }
    }

    /// A table whose first cell has a link that wraps over three or more
    /// lines in 30 columns, with links in the cells and rows after it.
    const TABLE: &str = "| a | b |\n|---|---|\n| [one two three four five six seven eight nine ten](u1) | x [y](u2) |\n| [next](u3) | [last](u3) |\n| [next](u3) | z |\n";

    #[test]
    fn a_link_wrapped_inside_a_table_cell_is_one_link() {
        let (lines, places) = text_in(TABLE, 30);
        let found = identify(&lines, &places);
        let urls: Vec<&str> = found.iter().map(|found| found.url.as_str()).collect();
        assert_eq!(urls, ["u1", "u2", "u3", "u3", "u3"]);
        // On three lines or more, one after the other.
        let on = &found[0].lines;
        assert!(on.len() >= 3, "{on:?}");
        assert!(on.windows(2).all(|pair| pair[1] == pair[0] + 1), "{on:?}");
        // The whole of it is selected, on each line, and the next link is
        // another.
        assert_eq!(found[0].runs.len(), on.len());
        assert_eq!(found[1].lines, [on[0]]);
    }

    #[test]
    fn the_links_in_a_table_have_the_same_ids_whatever_the_width() {
        let ids = |columns| {
            let (lines, places) = text_in(TABLE, columns);
            let found = identify(&lines, &places);
            found.into_iter().map(|found| found.id).collect::<Vec<_>>()
        };
        assert_eq!(ids(30), ids(80));
    }

    #[test]
    fn links_of_one_address_in_a_table_stay_apart() {
        // Under each other in a column, and side by side.
        let source = "| a | b |\n|---|---|\n| [x](u) | [y](u) |\n| [x](u) | [y](u) |\n";
        let (lines, places) = text_in(source, 30);
        assert_eq!(identify(&lines, &places).len(), 4);
    }

    #[test]
    fn a_bar_in_the_cell_beside_does_not_split_a_wrapped_link() {
        // The bar is in the first line of the cell on the left only.
        let source = "| a | b |\n|---|---|\n| `│` x | [one two three four five six seven eight nine ten](u1) |\n";
        let (lines, places) = text_in(source, 30);
        let found = identify(&lines, &places);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].lines.len() >= 3, "{:?}", found[0].lines);
    }

    #[test]
    fn two_links_of_one_address_a_soft_wrap_falls_between_stay_apart() {
        let source = "A line to fill [aaaa](u) [bbbb](u) [cccc](v) end.\n";
        for columns in [24, 80] {
            let (lines, places) = text_in(source, columns);
            let found = identify(&lines, &places);
            let urls: Vec<&str> = found.iter().map(|found| found.url.as_str()).collect();
            assert_eq!(urls, ["u", "u", "v"], "{columns}");
        }
    }

    #[test]
    fn spans_of_one_address_side_by_side_on_a_line_are_one_link() {
        // The terminal shows one hyperlink there, so Tab marks all of it.
        let (lines, places) = text("Text [aaaa](u)**[bbbb](u)** end.\n");
        let found = identify(&lines, &places);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].url, "u");
        let [(line, range)] = found[0].runs.as_slice() else {
            panic!("one run: {:?}", found[0].runs);
        };
        let shown: String = lines[*line].spans[range.clone()]
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(shown, "aaaabbbb");
        // Also where it wraps over lines, at any width.
        let source = "A [aaaa bbbb cccc](u)**[dddd eeee](u)** end.\n";
        for columns in 4..=80 {
            let (lines, places) = text_in(source, columns);
            let found = identify(&lines, &places);
            assert_eq!(found.len(), 1, "{columns}");
        }
    }

    #[test]
    fn touching_links_of_one_address_stay_one_where_a_line_breaks_between() {
        // The last source breaks between the two at 4 to 6 columns.
        for source in [
            "Text [aaaa](u)**[bbbb](u)** end.\n",
            "A line to fill [aaaa](u)**[bbbb](u)** end.\n",
            "[aaaaaa](u)**[bbbbbb](u)**\n",
        ] {
            for columns in 4..=40 {
                let (lines, places) = text_in(source, columns);
                let found = identify(&lines, &places);
                let urls: Vec<&str> = found.iter().map(|found| found.url.as_str()).collect();
                assert_eq!(urls, ["u"], "{source:?} at {columns}");
            }
        }
    }

    #[test]
    fn spans_of_one_address_side_by_side_are_one_link_also_in_a_quote() {
        let source = "> A [aaaa bbbb cccc](u)**[dddd eeee](u)** end.\n";
        for columns in [14, 20, 80] {
            let (lines, places) = text_in(source, columns);
            let found = identify(&lines, &places);
            assert_eq!(found.len(), 1, "{columns}");
        }
        // It does wrap there, so the check covers a link over lines.
        let (lines, places) = text_in(source, 14);
        assert!(identify(&lines, &places)[0].lines.len() >= 2);
    }

    #[test]
    fn a_link_split_by_a_style_change_in_a_table_cell_is_one_link() {
        // The same address in the cell below is another link.
        let source = "| a | b |\n|---|---|\n| [x](u)**[y](u)** | z |\n| [x](u)**[y](u)** | z |\n";
        for columns in [30, 80] {
            let (lines, places) = text_in(source, columns);
            let found = identify(&lines, &places);
            assert_eq!(found.len(), 2, "{columns}");
            for one in &found {
                let [(line, range)] = one.runs.as_slice() else {
                    panic!("one run: {:?}", one.runs);
                };
                let shown: String = lines[*line].spans[range.clone()]
                    .iter()
                    .map(|span| span.text.as_str())
                    .collect();
                assert_eq!(shown, "xy", "{columns}");
            }
        }
    }

    #[test]
    fn tab_cycles_through_the_links_of_the_text_on_the_screen() {
        let (lines, places) = text(TEXT);
        let found = identify(&lines, &places);
        let mut links = Links::default();
        let selected = |links: &Links| urls(links.selected(&lines, &places));
        let all = 0..lines.len();
        for expected in ["u1", "u2", "u3", "u4", "u5", "u6", "u6", "u1"] {
            assert!(links.select_next(&lines, &places, all.clone(), false) || expected == "u1");
            assert_eq!(selected(&links).as_deref(), Some(expected));
        }
        assert!(links.select_next(&lines, &places, all.clone(), true));
        assert_eq!(selected(&links).as_deref(), Some("u6"));
        // A screen with the wrapped link's second line, and a link before
        // and after it: the cycle is over those three alone.
        let screen = found[3].lines[1]..found[4].lines[0] + 1;
        let mut links = Links::default();
        for expected in ["u4", "u5", "u4"] {
            assert!(links.select_next(&lines, &places, screen.clone(), false));
            assert_eq!(selected(&links).as_deref(), Some(expected));
        }
        // None on a screen without a link.
        let last = lines.len() - 1;
        assert!(!Links::default().select_next(&lines, &places, last..last + 1, false));
    }

    #[test]
    fn a_link_in_a_heading_drawn_as_an_image_cannot_be_selected() {
        use crate::typeset::images::{ImageKey, Source, heading_words};
        use crate::typeset::model::Block;
        let document = crate::typeset::parse::parse("# A [link](u1) here\n\nBody [other](u2)\n");
        let Block::Heading {
            level,
            content: words,
        } = &document.blocks[0]
        else {
            panic!("a heading");
        };
        let words = heading_words(words);
        let count = u16::try_from(words.len()).unwrap();
        let mut inputs = Inputs::new(Capabilities {
            text_sizing: false,
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(40)
        });
        let key = inputs.drawn(ImageKey::heading_of(*level, words, (10, 20)).in_document(0));
        let image = image::RgbaImage::from_pixel(u32::from(count) * 40, 20, image::Rgba([9; 4]));
        let text = Source::Text {
            image,
            breaks: (1..count).map(|word| word * 40).collect(),
            ends: (1..=count).map(|word| word * 40 - 10).collect(),
        };
        inputs.images.insert(key, Ok(text));
        let Stream { lines, blocks } = content(Content::Document(&document), &inputs);
        let heading = lines.iter().filter(|line| line.image.is_some()).count();
        assert!(heading > 0, "the heading is drawn as an image");
        let places: Vec<_> = (lines.iter())
            .map(|line| line.block.map(|id| blocks.get(id).clone()))
            .collect();
        let found = identify(&lines, &places);
        let urls: Vec<&str> = found.iter().map(|found| found.url.as_str()).collect();
        assert_eq!(urls, ["u2"]);
    }

    /// A document whose first link is long enough to wrap, a tail after it.
    fn wrapped_link() -> String {
        format!("A [{}end](u1) tail.\n", "word ".repeat(60))
    }

    #[test]
    fn a_link_wrapped_over_lines_is_on_the_screen_when_any_of_them_is() {
        let (lines, places) = laid_out(&wrapped_link());
        let all = 0..lines.len();
        let mut links = Links::default();
        assert!(links.select_next(&lines, &places, all.clone(), false));
        // Where its cells are: the first and the last of the lines.
        let at: Vec<usize> = (all.clone())
            .filter(|&line| {
                let screen = line..line + 1;
                (links.selected_on(&lines, &places, screen)).is_some()
            })
            .collect();
        assert!(at.len() >= 2, "{at:?}");
        let last = *at.last().unwrap();
        assert!(last > at[0]);
        // A screen that shows only the last of them still has the link, and
        // Tab selects it from none.
        assert_eq!(
            urls(links.selected_on(&lines, &places, last..last + 1)).as_deref(),
            Some("u1")
        );
        let mut fresh = Links::default();
        assert!(fresh.select_next(&lines, &places, last..last + 1, false));
        // A screen with none of its lines has no link, though one is
        // selected.
        let past = last + 1..lines.len();
        assert_eq!(links.selected_on(&lines, &places, past), None);
    }

    /// `source` laid out in reading mode to scroll, with the blocks of its
    /// lines.
    fn scrolling(source: &str) -> (Vec<Line>, Vec<Option<BlockInfo>>) {
        let document = crate::typeset::parse::parse(source);
        let inputs = Inputs {
            reading: Some(Reading {
                pictures: Default::default(),
                fonts: Rc::new(Fonts::fixture()),
                wide: Wide::Scroll,
            }),
            ..Inputs::new(Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            })
        };
        let Stream { lines, blocks } = content(Content::Document(&document), &inputs);
        let places = (lines.iter())
            .map(|line| line.block.map(|id| blocks.get(id).clone()))
            .collect();
        (lines, places)
    }

    #[test]
    fn a_selected_link_scrolled_out_of_its_band_is_not_another_link() {
        let long = "x".repeat(150);
        let (mut lines, places) = scrolling(&format!(
            "| a | b |\n|---|---|\n| [near](u1) | {long}[far](u2) |\n"
        ));
        let mut links = Links::default();
        assert!(links.select_next(&lines, &places, 0..lines.len(), false));
        assert_eq!(urls(links.selected(&lines, &places)).as_deref(), Some("u1"));
        // Scrolled to the end of the band, `near` has no cells and `far`
        // has: the selection is none, not `far`.
        let band = &engine::bands(&lines)[0];
        let (rows, most) = (band.lines.clone(), band.most());
        engine::set_band(&mut lines[rows.clone()], most, true);
        assert_eq!(urls(links.selected(&lines, &places)), None);
        // Scrolled back, it is `near` again.
        engine::set_band(&mut lines[rows], 0, true);
        assert_eq!(urls(links.selected(&lines, &places)).as_deref(), Some("u1"));
    }

    #[test]
    fn tab_reaches_a_link_in_a_band_scrolled_out_of_its_view() {
        let long = "x".repeat(150);
        let (lines, places) = scrolling(&format!(
            "| a | b |\n|---|---|\n| [near](u1) | {long}[far](u2) |\n"
        ));
        let all = 0..lines.len();
        // At the start of the band `far` has no cells to show.
        let identified = identify(&lines, &places);
        assert!(identified[1].lines.is_empty());
        let target = |links: &Links| links.target(&lines, &places).map(|found| found.url);
        let mut links = Links::default();
        assert!(links.select_next(&lines, &places, all.clone(), false));
        assert_eq!(target(&links).as_deref(), Some("u1"));
        assert!(links.select_next(&lines, &places, all.clone(), false));
        assert_eq!(target(&links).as_deref(), Some("u2"));
        // And on round, and backwards.
        assert!(links.select_next(&lines, &places, all.clone(), false));
        assert_eq!(target(&links).as_deref(), Some("u1"));
        assert!(links.select_next(&lines, &places, all, true));
        assert_eq!(target(&links).as_deref(), Some("u2"));
        // It says where in its band to scroll to, on the band's line.
        let found = links.target(&lines, &places).unwrap();
        let (line, x) = found.band_at.expect("in a band");
        assert!(engine::bands(&lines)[0].lines.contains(&line));
        assert!(x.start > 1000, "{x:?}");
    }

    #[test]
    fn a_link_without_an_address_is_no_stop_for_tab() {
        // The terminal shows no link there: an empty address in OSC 8
        // closes one.
        let source = "A [one](u1), [none]() and [two](u2).\n";
        for (lines, places) in [laid_out(source), text(source)] {
            let found = identify(&lines, &places);
            let urls: Vec<&str> = found.iter().map(|found| found.url.as_str()).collect();
            assert_eq!(urls, ["u1", "u2"]);
        }
    }
}
