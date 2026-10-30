//! Reading mode's links in the pager: which of the links on the screen is
//! selected, by Tab and Shift+Tab. A link in a page is a hyperlink, which
//! kitty opens on its own; the selection draws a box round it and the
//! status line says where it goes.

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use crate::render::block::BlockInfo;
use crate::render::reading;
use crate::render::style::Line;

/// A link, by the block its first line is in and its number among the
/// links of that block, so that it is found again in a new layout.
type LinkId = (Option<BlockInfo>, usize);

/// A link on the lines of a layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Found {
    id: LinkId,
    /// The page it is on, by the address of its parts, and its number among
    /// the page's links.
    pub link: (usize, usize),
    pub url: String,
    /// The line its first cell is on, none while it has none.
    line: Option<usize>,
    /// For a link in a band, the line it is on and the pixels it covers
    /// across the band's content, to scroll the band to it.
    pub band_at: Option<(usize, Range<u32>)>,
}

impl Found {
    /// Whether the link is in a band that shows none of its cells.
    pub fn is_scrolled_out(&self) -> bool {
        self.line.is_none() && self.band_at.is_some()
    }

    /// The line the link is on for Tab: its first cell's, or while a band
    /// shows none of its cells, the line of the band it is on.
    fn reached_at(&self) -> Option<usize> {
        self.line.or(self.band_at.as_ref().map(|(line, _)| *line))
    }
}

/// Which link is selected.
#[derive(Debug, Default)]
pub(super) struct Links {
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
            .filter(|found| {
                found
                    .reached_at()
                    .is_some_and(|line| visible.contains(&line))
            })
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

    /// The selected link when it is on the lines `visible`, as the lines
    /// of this layout have it.
    pub fn selected_on(
        &self,
        lines: &[Line],
        places: &[Option<BlockInfo>],
        visible: Range<usize>,
    ) -> Option<Found> {
        (self.selected(lines, places))
            .filter(|found| found.line.is_some_and(|l| visible.contains(&l)))
    }

    /// The selected link, as the lines of this layout have it, when it is
    /// still on them.
    pub fn selected(&self, lines: &[Line], places: &[Option<BlockInfo>]) -> Option<Found> {
        let id = self.selected.as_ref()?;
        let found = identify(lines, places)
            .into_iter()
            .find(|found| found.id == *id)?;
        // While a band shows none of its cells, there is nothing to show.
        found.line.map(|_| found)
    }
}

/// The links of the pages on `lines`, each once, in order, numbered from
/// the pages' own lists, so that a link keeps its id whether or not a band
/// shows it now; `line` is where its first cells are, none while it has
/// none, and `band_at` where to scroll its band to.
fn identify(lines: &[Line], places: &[Option<BlockInfo>]) -> Vec<Found> {
    // Where each link that has cells first has them.
    let mut cells: HashMap<(usize, usize), usize> = HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        for link in line.image.iter().flat_map(|image| &image.links) {
            cells.entry(link.link).or_insert(index);
        }
    }
    let mut counted: Vec<(Option<BlockInfo>, usize)> = Vec::new();
    let mut found = Vec::new();
    for page in reading::pages(lines) {
        let address = Rc::as_ptr(&page.parts) as usize;
        for (number, link) in page.parts.links().iter().enumerate() {
            let at = (page.lines.start + page.parts.link_row(number)).min(page.lines.end - 1);
            let place = places.get(at).cloned().flatten();
            let nth = match counted.iter_mut().find(|(kept, _)| *kept == place) {
                Some((_, count)) => {
                    *count += 1;
                    *count - 1
                }
                None => {
                    counted.push((place.clone(), 1));
                    0
                }
            };
            // Where to scroll its band to, to show the first piece of it.
            let band_at = link.band.and(link.pieces.first()).map(|piece| {
                let (start, end) = (piece.x.floor().max(0.0), (piece.x + piece.width).ceil());
                (at, start as u32..end as u32)
            });
            found.push(Found {
                id: (place, nth),
                link: (address, number),
                url: link.url.to_string(),
                line: cells.get(&(address, number)).copied(),
                band_at,
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::caps::Capabilities;
    use crate::reading::Fonts;
    use crate::reading::page::Wide;
    use crate::render::Inputs;
    use crate::render::layout::{Content, Stream, content};
    use crate::render::reading::{self, Reading};

    /// `source` laid out in reading mode, with the blocks of its lines.
    fn laid_out(source: &str) -> (Vec<Line>, Vec<Option<BlockInfo>>) {
        let document = crate::parse::parse(source);
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
        let third = found[2].line.unwrap();
        let mut links = Links::default();
        assert!(links.select_next(&lines, &places, third..third + 1, false));
        assert_eq!(urls(links.selected(&lines, &places)).as_deref(), Some("u3"));
        // Nothing on a screen with no link.
        let mut none = Links::default();
        assert!(!none.select_next(&lines, &places, found[1].line.unwrap() + 1..third, false));
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
        assert_ne!(found.link.0, old.link.0);
        // A document without it has no selection to show.
        let (other, places_other) = laid_out("Nothing here.\n");
        assert_eq!(links.selected(&other, &places_other), None);
    }

    /// `source` laid out in reading mode to scroll, with the blocks of its
    /// lines.
    fn scrolling(source: &str) -> (Vec<Line>, Vec<Option<BlockInfo>>) {
        let document = crate::parse::parse(source);
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
        let band = &reading::bands(&lines)[0];
        let (rows, most) = (band.lines.clone(), band.most());
        reading::set_band(&mut lines[rows.clone()], most, true);
        assert_eq!(urls(links.selected(&lines, &places)), None);
        // Scrolled back, it is `near` again.
        reading::set_band(&mut lines[rows], 0, true);
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
        assert_eq!(identified[1].line, None);
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
        assert!(reading::bands(&lines)[0].lines.contains(&line));
        assert!(x.start > 1000, "{x:?}");
    }
}
