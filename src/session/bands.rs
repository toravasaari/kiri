//! Reading mode's bands in the pager: the code blocks and tables that scroll
//! sideways. The module keeps how far each is scrolled, and which one is
//! selected: the topmost on the screen, or the one a click or a search selects.

use std::ops::Range;

use crate::session::edition::{Aligned, Edition, Held, Moves};
use crate::typeset::block::BlockInfo;
use crate::typeset::locate::{Band, Map};
use crate::typeset::reading::engine;
use crate::typeset::style::Line;

/// A band, by the innermost block it shows and its number among the bands
/// of that block (a block that a page break cuts has more than one), so that
/// it is found again in a new layout, and in a new version of the document
/// where the block is.
type BandId = (Option<BlockInfo>, usize);

impl Aligned for (Option<BlockInfo>, usize) {
    fn aligned(&self, moves: &Moves) -> Option<Self> {
        let block = match &self.0 {
            Some(block) => {
                let path = moves.block(block.file(), block.path())?;
                Some(BlockInfo::new(block.file(), path))
            }
            None => None,
        };
        Some((block, self.1))
    }
}

/// How far each band is scrolled, and which one is selected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Scrolled {
    offsets: Vec<(BandId, u32)>,
    selected: Option<BandId>,
}

impl Aligned for Scrolled {
    /// A band whose block is gone is forgotten, and the selection with it.
    fn aligned(&self, moves: &Moves) -> Option<Self> {
        Some(Self {
            offsets: (self.offsets.iter())
                .filter_map(|(id, offset)| Some((id.aligned(moves)?, *offset)))
                .collect(),
            selected: self.selected.as_ref().and_then(|id| id.aligned(moves)),
        })
    }
}

/// How far each band is scrolled, and which one is selected, with the
/// version of the document they are of.
#[derive(Debug, Default)]
pub(crate) struct Bands {
    held: Option<Held<Scrolled>>,
}

/// Which way, and how far, a key scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Left,
    Right,
    PageLeft,
    PageRight,
}

impl Bands {
    /// How the bands are in `edition`: nothing is scrolled or selected in
    /// another version than the one they were put in.
    fn at(&self, edition: &Edition) -> Scrolled {
        (self.held.as_ref())
            .and_then(|held| held.get(edition))
            .cloned()
            .unwrap_or_default()
    }

    fn put(&mut self, edition: &Edition, scrolled: Scrolled) {
        self.held = Some(Held::new(edition, scrolled));
    }

    /// Moves the bands to the version `edition` is of, by `moves`.
    pub(crate) fn align(&mut self, moves: &Moves, edition: &Edition) {
        self.held = self.held.take().and_then(|held| held.align(moves, edition));
    }

    /// Shows each band on `lines`, of which `map` is the map, as far
    /// scrolled as it was, and the selected one as selected.
    pub fn apply(&self, edition: &Edition, lines: &mut [Line], map: &Map) {
        let scrolled = self.at(edition);
        for (id, band) in identify(map) {
            let offset = scrolled.offset(&id).min(band.most());
            let selected = scrolled.selected.as_ref() == Some(&id);
            if (offset, selected) != band.scrolled(lines) {
                engine::set_band(&mut lines[band.lines.clone()], offset, selected);
            }
        }
    }

    /// Selects the topmost band in `visible` when the selected one is not
    /// there; true when that changed the selection.
    pub fn refresh(&mut self, edition: &Edition, map: &Map, visible: Range<usize>) -> bool {
        let mut scrolled = self.at(edition);
        let shown: Vec<BandId> = (identify(map).into_iter())
            .filter(|(_, band)| band.lines.start < visible.end && band.lines.end > visible.start)
            .map(|(id, _)| id)
            .collect();
        if (scrolled.selected.as_ref()).is_some_and(|id| shown.contains(id)) {
            return false;
        }
        let topmost = shown.into_iter().next();
        let changed = topmost != scrolled.selected;
        scrolled.selected = topmost;
        self.put(edition, scrolled);
        changed
    }

    /// Selects the band that shows line `line`: whether that changed the
    /// selection, or `None` when no band shows it.
    pub fn select_at(&mut self, edition: &Edition, map: &Map, line: usize) -> Option<bool> {
        let (id, _) = (identify(map).into_iter()).find(|(_, band)| band.lines.contains(&line))?;
        let mut scrolled = self.at(edition);
        let changed = scrolled.selected.as_ref() != Some(&id);
        scrolled.selected = Some(id);
        self.put(edition, scrolled);
        Some(changed)
    }

    /// A click on line `line` of the window, if any: whether it changed
    /// the selected band.
    pub fn click(&mut self, edition: &Edition, map: &Map, line: Option<usize>) -> bool {
        line.and_then(|line| self.select_at(edition, map, line)) == Some(true)
    }

    /// A notch of the sideways wheel, `right` or left, on line `line` of
    /// the window, if any: selects the band that shows the line and
    /// scrolls it. Whether that changed anything, or `None` when no band
    /// is there.
    pub fn wheel(
        &mut self,
        edition: &Edition,
        map: &Map,
        line: Option<usize>,
        right: bool,
    ) -> Option<bool> {
        let selected = self.select_at(edition, map, line?)?;
        let step = if right { Step::Right } else { Step::Left };
        Some(self.scroll(edition, map, step) || selected)
    }

    /// Whether the pixels `x` of the content of the band that shows line
    /// `line` are in the band's window as it is scrolled; true too when no
    /// band shows the line, as then nothing hides them.
    pub fn shows(&self, edition: &Edition, map: &Map, line: usize, x: &Range<u32>) -> bool {
        let found = identify(map).into_iter();
        let Some((id, band)) = found
            .into_iter()
            .find(|(_, band)| band.lines.contains(&line))
        else {
            return true;
        };
        let offset = self.at(edition).offset(&id);
        x.start < offset + band.visible() && offset < x.end
    }

    /// Scrolls the band that shows line `line` so that the pixels `x` of
    /// its content are in view, and selects it: whether that changed the
    /// band, or `None` when no band shows the line.
    pub fn reveal(
        &mut self,
        edition: &Edition,
        map: &Map,
        line: usize,
        x: Range<u32>,
    ) -> Option<bool> {
        let (id, band) =
            (identify(map).into_iter()).find(|(_, band)| band.lines.contains(&line))?;
        let mut scrolled = self.at(edition);
        let offset = scrolled.offset(&id);
        let visible = band.visible();
        let new = match x.start >= offset && x.end <= offset + visible {
            true => offset,
            false => x.start.saturating_sub(visible / 4).min(band.most()),
        };
        let mut changed = scrolled.selected.as_ref() != Some(&id);
        scrolled.selected = Some(id.clone());
        if new != offset {
            scrolled.set(id, new);
            changed = true;
        }
        self.put(edition, scrolled);
        Some(changed)
    }

    /// Scrolls the bands a key scrolls by `step`; true when one moved.
    pub fn scroll(&mut self, edition: &Edition, map: &Map, step: Step) -> bool {
        let mut scrolled = self.at(edition);
        let mut moved = false;
        // The bands that the keys scroll: the selected one. All the bands
        // on the screen, or all of them, would take only a change here.
        let selected: Vec<_> = (identify(map).into_iter())
            .filter(|(id, _)| scrolled.selected.as_ref() == Some(id))
            .collect();
        for (id, band) in selected {
            let size = match step {
                Step::Left | Step::Right => band.step(),
                Step::PageLeft | Step::PageRight => band.visible() * 4 / 5,
            };
            let offset = scrolled.offset(&id);
            let new = match step {
                Step::Left | Step::PageLeft => offset.saturating_sub(size),
                Step::Right | Step::PageRight => offset.saturating_add(size).min(band.most()),
            };
            if new != offset {
                scrolled.set(id, new);
                moved = true;
            }
        }
        self.put(edition, scrolled);
        moved
    }
}

impl Scrolled {
    fn offset(&self, id: &BandId) -> u32 {
        (self.offsets.iter())
            .find(|(kept, _)| kept == id)
            .map_or(0, |(_, offset)| *offset)
    }

    fn set(&mut self, id: BandId, offset: u32) {
        self.offsets.retain(|(kept, _)| *kept != id);
        self.offsets.push((id, offset));
    }
}

/// The bands of the lines that `map` is the map of, each with its id: the
/// block it shows (see [`Band::block`]) and its number among those of the
/// same block.
fn identify(map: &Map) -> Vec<(BandId, &Band)> {
    let mut counted: Vec<(Option<BlockInfo>, usize)> = Vec::new();
    (map.bands().iter())
        .map(|band| {
            let place = band.block().cloned();
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
mod tests;
