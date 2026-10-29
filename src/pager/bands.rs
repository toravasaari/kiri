//! Reading mode's bands in the pager: code blocks and tables that scroll
//! sideways, how far each is scrolled, and which one is selected: the
//! topmost on the screen, or the one a click or a search selects.

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

    /// Selects the band that shows line `line`: whether that changed the
    /// selection, or `None` when no band shows it.
    pub fn select_at(
        &mut self,
        lines: &[Line],
        places: &[Option<BlockInfo>],
        line: usize,
    ) -> Option<bool> {
        let (id, _) =
            (identify(lines, places).into_iter()).find(|(_, band)| band.lines.contains(&line))?;
        let changed = self.selected.as_ref() != Some(&id);
        self.selected = Some(id);
        Some(changed)
    }

    /// Scrolls the band that shows line `line` so that the pixels `x` of
    /// its content are in view, and selects it: whether that changed the
    /// band, or `None` when no band shows the line.
    pub fn reveal(
        &mut self,
        lines: &[Line],
        places: &[Option<BlockInfo>],
        line: usize,
        x: Range<u32>,
    ) -> Option<bool> {
        let (id, band) =
            (identify(lines, places).into_iter()).find(|(_, band)| band.lines.contains(&line))?;
        let offset = self.offset(&id);
        let visible = band.visible();
        let new = match x.start >= offset && x.end <= offset + visible {
            true => offset,
            false => x.start.saturating_sub(visible / 4).min(band.most()),
        };
        let mut changed = self.selected.as_ref() != Some(&id);
        self.selected = Some(id.clone());
        if new != offset {
            self.offsets.retain(|(kept, _)| *kept != id);
            self.offsets.push((id, new));
            changed = true;
        }
        Some(changed)
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
        laid_out_from(&format!("```\n{long}\n```\n\nText.\n\n```\n{long}\n```\n"))
    }

    /// `source` laid out to scroll, with the blocks of its lines.
    fn laid_out_from(source: &str) -> (Vec<Line>, Vec<Option<BlockInfo>>) {
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
    fn a_band_is_scrolled_to_show_what_is_asked_for_and_selected() {
        let long = "a".repeat(200);
        let (mut lines, places) = laid_out_from(&format!(
            "```\n{long}\n```\n\nText.\n\n```\n{long} needle\n```\n"
        ));
        let mut bands = Bands::default();
        bands.refresh(&lines, &places, 0..lines.len());
        let second = reading::bands(&lines)[1].lines.clone();
        let visible = reading::bands(&lines)[1].visible();
        // In view already: only selected.
        assert_eq!(
            bands.reveal(&lines, &places, second.start, 0..30),
            Some(true)
        );
        bands.apply(&mut lines, &places);
        assert_eq!(shown(&lines), [(0, false), (0, true)]);
        // Far to the right: scrolled so that it is a quarter in.
        let x = 1500..1560;
        assert_eq!(
            bands.reveal(&lines, &places, second.start, x.clone()),
            Some(true)
        );
        bands.apply(&mut lines, &places);
        let offset = shown(&lines)[1].0;
        assert_eq!(
            offset,
            (x.start - visible / 4).min(reading::bands(&lines)[1].most())
        );
        assert!(x.start >= offset && x.end <= offset + visible);
        // Already there: nothing changes; no band on a line: none.
        assert_eq!(bands.reveal(&lines, &places, second.start, x), Some(false));
        let text = reading::bands(&lines)[0].lines.end;
        assert_eq!(bands.reveal(&lines, &places, text, 0..1), None);
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
    fn two_bands_in_one_block_keep_their_own_places_across_a_layout() {
        let long = "a".repeat(200);
        let code = format!("```\n{long}\n```\n");
        let quote: String = code.lines().map(|line| format!("> {line}\n")).collect();
        for source in [
            format!("{quote}>\n{quote}"),
            format!(
                "- item\n\n  {}\n  {}",
                code.replace('\n', "\n  "),
                code.replace('\n', "\n  ")
            ),
        ] {
            let (lines, places) = laid_out_from(&source);
            let found = reading::bands(&lines);
            assert_eq!(found.len(), 2, "{source:?}");
            // Both are in one block, whose place they share.
            assert_eq!(places[found[0].lines.start], places[found[1].lines.start]);
            let mut bands = Bands::default();
            bands.select_at(&lines, &places, found[1].lines.start);
            bands.scroll(&lines, &places, Step::Right);
            bands.scroll(&lines, &places, Step::Right);
            let (mut again, places) = laid_out_from(&source);
            bands.apply(&mut again, &places);
            // The second is scrolled and selected, the first neither.
            assert_eq!(shown(&again), [(0, false), (128, true)], "{source:?}");
        }
    }

    #[test]
    fn a_line_of_a_band_selects_it() {
        let (lines, places) = laid_out();
        let mut bands = Bands::default();
        let second = reading::bands(&lines)[1].lines.clone();
        assert_eq!(
            bands.select_at(&lines, &places, second.start + 1),
            Some(true)
        );
        assert_eq!(bands.select_at(&lines, &places, second.start), Some(false));
        let between = reading::bands(&lines)[0].lines.end;
        assert_eq!(bands.select_at(&lines, &places, between), None);
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
