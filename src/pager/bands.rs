//! Reading mode's bands in the pager: code blocks and tables that scroll
//! sideways, how far each is scrolled, and which one is selected.

use std::ops::Range;

use crate::render::block::BlockInfo;
use crate::render::reading::{self, BandRows};
use crate::render::style::Line;

/// A band, by the block its first line is in and its number among the
/// bands of that block, so that it is found again in a new layout.
type BandId = (Option<BlockInfo>, usize);

/// How far each band is scrolled, and which one is selected.
#[derive(Debug, Default)]
pub(super) struct Bands {
    offsets: Vec<(BandId, u32)>,
    selected: Option<BandId>,
}

/// Which way, and how far, a key scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Left,
    Right,
    PageLeft,
    PageRight,
}

impl Bands {
    /// Shows each band on `lines`, whose blocks are `places`, as far
    /// scrolled as it was, and the selected one as selected.
    pub fn apply(&self, lines: &mut [Line], places: &[Option<BlockInfo>]) {
        for (id, band) in identify(lines, places) {
            let offset = self.offset(&id).min(band.most());
            let selected = self.selected.as_ref() == Some(&id);
            if (offset, selected) != (band.offset, band.selected) {
                reading::set_band(&mut lines[band.lines.clone()], offset, selected);
            }
        }
    }

    /// Selects the topmost band in `visible` when the selected one is not
    /// there; true when that changed the selection.
    pub fn refresh(
        &mut self,
        lines: &[Line],
        places: &[Option<BlockInfo>],
        visible: Range<usize>,
    ) -> bool {
        let shown: Vec<BandId> = (identify(lines, places).into_iter())
            .filter(|(_, band)| band.lines.start < visible.end && band.lines.end > visible.start)
            .map(|(id, _)| id)
            .collect();
        if self.selected.as_ref().is_some_and(|id| shown.contains(id)) {
            return false;
        }
        let topmost = shown.into_iter().next();
        let changed = topmost != self.selected;
        self.selected = topmost;
        changed
    }

    /// Selects the next band in `visible`, or the one before it when
    /// `back`; true when that changed the selection.
    pub fn select_next(
        &mut self,
        lines: &[Line],
        places: &[Option<BlockInfo>],
        visible: Range<usize>,
        back: bool,
    ) -> bool {
        let shown: Vec<BandId> = (identify(lines, places).into_iter())
            .filter(|(_, band)| band.lines.start < visible.end && band.lines.end > visible.start)
            .map(|(id, _)| id)
            .collect();
        if shown.is_empty() {
            return false;
        }
        let at = (self.selected.as_ref()).and_then(|id| shown.iter().position(|shown| shown == id));
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => shown.len() - 1,
            (Some(at), false) => (at + 1) % shown.len(),
            (Some(at), true) => (at + shown.len() - 1) % shown.len(),
        };
        let changed = self.selected.as_ref() != Some(&shown[next]);
        self.selected = Some(shown[next].clone());
        changed
    }

    /// Scrolls the bands a key scrolls by `step`; true when one moved.
    pub fn scroll(&mut self, lines: &[Line], places: &[Option<BlockInfo>], step: Step) -> bool {
        let mut moved = false;
        for (id, band) in self.scrolled(lines, places) {
            let size = match step {
                Step::Left | Step::Right => band.parts.step(),
                Step::PageLeft | Step::PageRight => band.visible() * 4 / 5,
            };
            let offset = self.offset(&id);
            let new = match step {
                Step::Left | Step::PageLeft => offset.saturating_sub(size),
                Step::Right | Step::PageRight => offset.saturating_add(size).min(band.most()),
            };
            if new != offset {
                self.offsets.retain(|(kept, _)| *kept != id);
                self.offsets.push((id, new));
                moved = true;
            }
        }
        moved
    }

    /// The bands that the keys scroll: the selected one. All the bands
    /// on the screen, or all of them, would take only a change here.
    fn scrolled(&self, lines: &[Line], places: &[Option<BlockInfo>]) -> Vec<(BandId, BandRows)> {
        (identify(lines, places).into_iter())
            .filter(|(id, _)| self.selected.as_ref() == Some(id))
            .collect()
    }

    fn offset(&self, id: &BandId) -> u32 {
        (self.offsets.iter())
            .find(|(kept, _)| kept == id)
            .map_or(0, |(_, offset)| *offset)
    }
}

/// The bands on `lines`, each with its id.
fn identify(lines: &[Line], places: &[Option<BlockInfo>]) -> Vec<(BandId, BandRows)> {
    let mut counted: Vec<(Option<BlockInfo>, usize)> = Vec::new();
    (reading::bands(lines).into_iter())
        .map(|band| {
            let place = places.get(band.lines.start).cloned().flatten();
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
            ((place, nth), band)
        })
        .collect()
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
    use crate::render::reading::Reading;

    /// Two code blocks too wide for the area, a paragraph apart, laid out
    /// to scroll, with the blocks of their lines.
    fn laid_out() -> (Vec<Line>, Vec<Option<BlockInfo>>) {
        let long = "a".repeat(200);
        let source = format!("```\n{long}\n```\n\nText.\n\n```\n{long}\n```\n");
        let document = crate::parse::parse(&source);
        let inputs = Inputs {
            reading: Some(Reading {
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

    /// Each band's offset and whether it is selected, as `lines` show it.
    fn shown(lines: &[Line]) -> Vec<(u32, bool)> {
        (reading::bands(lines).iter())
            .map(|band| (band.offset, band.selected))
            .collect()
    }

    #[test]
    fn the_topmost_band_on_the_screen_is_selected() {
        let (mut lines, places) = laid_out();
        let mut bands = Bands::default();
        assert!(bands.refresh(&lines, &places, 0..lines.len()));
        bands.apply(&mut lines, &places);
        assert_eq!(shown(&lines), [(0, true), (0, false)]);
        // Scrolled past the first, the second takes its place.
        let second = reading::bands(&lines)[1].lines.clone();
        assert!(bands.refresh(&lines, &places, second.start..lines.len()));
        bands.apply(&mut lines, &places);
        assert_eq!(shown(&lines), [(0, false), (0, true)]);
        assert!(!bands.refresh(&lines, &places, second.start..lines.len()));
    }

    #[test]
    fn the_keys_scroll_the_selected_band_within_its_room() {
        let (mut lines, places) = laid_out();
        let mut bands = Bands::default();
        bands.refresh(&lines, &places, 0..lines.len());
        let most = reading::bands(&lines)[0].most();
        assert!(!bands.scroll(&lines, &places, Step::Left));
        assert!(bands.scroll(&lines, &places, Step::Right));
        bands.apply(&mut lines, &places);
        assert_eq!(shown(&lines), [(64, true), (0, false)]);
        for _ in 0..10 {
            bands.scroll(&lines, &places, Step::PageRight);
        }
        bands.apply(&mut lines, &places);
        assert_eq!(shown(&lines)[0].0, most);
        assert!(!bands.scroll(&lines, &places, Step::PageRight));
    }

    #[test]
    fn tab_selects_the_next_band_on_the_screen_and_shift_tab_the_one_before() {
        let (mut lines, places) = laid_out();
        let mut bands = Bands::default();
        let all = 0..lines.len();
        bands.refresh(&lines, &places, all.clone());
        assert!(bands.select_next(&lines, &places, all.clone(), false));
        bands.apply(&mut lines, &places);
        assert_eq!(shown(&lines), [(0, false), (0, true)]);
        assert!(bands.select_next(&lines, &places, all.clone(), false));
        bands.apply(&mut lines, &places);
        assert_eq!(shown(&lines), [(0, true), (0, false)]);
        assert!(bands.select_next(&lines, &places, all, true));
        bands.apply(&mut lines, &places);
        assert_eq!(shown(&lines), [(0, false), (0, true)]);
    }

    #[test]
    fn a_new_layout_keeps_each_band_where_it_was() {
        let (lines, places) = laid_out();
        let mut bands = Bands::default();
        bands.refresh(&lines, &places, 0..lines.len());
        bands.scroll(&lines, &places, Step::Right);
        let (mut again, places) = laid_out();
        bands.apply(&mut again, &places);
        assert_eq!(shown(&again), [(64, true), (0, false)]);
    }
}
