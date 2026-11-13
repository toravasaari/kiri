//! Reading mode's bands in the pager: code blocks and tables that scroll
//! sideways, how far each is scrolled, and which one is selected: the
//! topmost on the screen, or the one a click or a search selects.

use std::ops::Range;

use crate::session::edition::{Aligned, Edition, Held, Moves};
use crate::typeset::block::{BlockInfo, Blocks};
use crate::typeset::reading::engine::{self, BandRows};
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

    /// Shows each band on `lines`, whose blocks are `blocks`, as far
    /// scrolled as it was, and the selected one as selected.
    pub fn apply(&self, edition: &Edition, lines: &mut [Line], blocks: &Blocks) {
        let scrolled = self.at(edition);
        for (id, band) in identify(lines, blocks) {
            let offset = scrolled.offset(&id).min(band.most());
            let selected = scrolled.selected.as_ref() == Some(&id);
            if (offset, selected) != (band.offset, band.selected) {
                engine::set_band(&mut lines[band.lines.clone()], offset, selected);
            }
        }
    }

    /// Selects the topmost band in `visible` when the selected one is not
    /// there; true when that changed the selection.
    pub fn refresh(
        &mut self,
        edition: &Edition,
        (lines, blocks): (&[Line], &Blocks),
        visible: Range<usize>,
    ) -> bool {
        let mut scrolled = self.at(edition);
        let shown: Vec<BandId> = (identify(lines, blocks).into_iter())
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
    pub fn select_at(
        &mut self,
        edition: &Edition,
        (lines, blocks): (&[Line], &Blocks),
        line: usize,
    ) -> Option<bool> {
        let (id, _) =
            (identify(lines, blocks).into_iter()).find(|(_, band)| band.lines.contains(&line))?;
        let mut scrolled = self.at(edition);
        let changed = scrolled.selected.as_ref() != Some(&id);
        scrolled.selected = Some(id);
        self.put(edition, scrolled);
        Some(changed)
    }

    /// A click on line `line` of the window, if any: whether it changed
    /// the selected band.
    pub fn click(
        &mut self,
        edition: &Edition,
        shown: (&[Line], &Blocks),
        line: Option<usize>,
    ) -> bool {
        line.and_then(|line| self.select_at(edition, shown, line)) == Some(true)
    }

    /// A notch of the sideways wheel, `right` or left, on line `line` of
    /// the window, if any: selects the band that shows the line and
    /// scrolls it. Whether that changed anything, or `None` when no band
    /// is there.
    pub fn wheel(
        &mut self,
        edition: &Edition,
        shown: (&[Line], &Blocks),
        line: Option<usize>,
        right: bool,
    ) -> Option<bool> {
        let selected = self.select_at(edition, shown, line?)?;
        let step = if right { Step::Right } else { Step::Left };
        Some(self.scroll(edition, shown, step) || selected)
    }

    /// Whether the pixels `x` of the content of the band that shows line
    /// `line` are in the band's window as it is scrolled; true too when no
    /// band shows the line, as then nothing hides them.
    pub fn shows(
        &self,
        edition: &Edition,
        (lines, blocks): (&[Line], &Blocks),
        line: usize,
        x: &Range<u32>,
    ) -> bool {
        let found = identify(lines, blocks).into_iter();
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
        (lines, blocks): (&[Line], &Blocks),
        line: usize,
        x: Range<u32>,
    ) -> Option<bool> {
        let (id, band) =
            (identify(lines, blocks).into_iter()).find(|(_, band)| band.lines.contains(&line))?;
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
    pub fn scroll(
        &mut self,
        edition: &Edition,
        (lines, blocks): (&[Line], &Blocks),
        step: Step,
    ) -> bool {
        let mut scrolled = self.at(edition);
        let mut moved = false;
        // The bands that the keys scroll: the selected one. All the bands
        // on the screen, or all of them, would take only a change here.
        let selected: Vec<_> = (identify(lines, blocks).into_iter())
            .filter(|(id, _)| scrolled.selected.as_ref() == Some(id))
            .collect();
        for (id, band) in selected {
            let size = match step {
                Step::Left | Step::Right => band.parts.step(),
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

/// The bands on `lines`, each with its id: the innermost block it shows, or
/// when a page does not say, the block its first line is of, and its number
/// among those of the same block.
fn identify(lines: &[Line], blocks: &Blocks) -> Vec<(BandId, BandRows)> {
    let mut counted: Vec<(Option<BlockInfo>, usize)> = Vec::new();
    (engine::bands(lines).into_iter())
        .map(|band| {
            let inner = band.parts.band_block(band.band);
            let block = inner.or_else(|| lines.get(band.lines.start).and_then(|line| line.block));
            let place = block.map(|block| blocks.get(block).clone());
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
    use crate::render::Inputs;
    use crate::session::edition::Moves;
    use crate::session::sources::Session;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::compare::{Comparison, compare};
    use crate::typeset::layout::{Content, Stream, content};
    use crate::typeset::model::Document;
    use crate::typeset::parse::parse;
    use crate::typeset::reading::Fonts;
    use crate::typeset::reading::engine::Reading;
    use crate::typeset::reading::page::Wide;

    /// Two code blocks too wide for the area, a paragraph apart, laid out
    /// to scroll, with the blocks of their lines.
    fn laid_out() -> (Vec<Line>, Blocks, Edition) {
        let long = "a".repeat(200);
        laid_out_from(&format!("```\n{long}\n```\n\nText.\n\n```\n{long}\n```\n"))
    }

    /// `source` laid out to scroll, with the blocks of its lines.
    fn laid_out_from(source: &str) -> (Vec<Line>, Blocks, Edition) {
        laid_with(&parse(source), &Session::new(["a.md"]))
    }

    /// Reading mode, which scrolls what is too wide.
    fn scrolling() -> Inputs {
        Inputs {
            reading: Some(Reading {
                pictures: Default::default(),
                fonts: Rc::new(Fonts::fixture()),
                wide: Wide::Scroll,
            }),
            ..Inputs::new(Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            })
        }
    }

    /// Each band's offset and whether it is selected, as `lines` show it.
    fn shown(lines: &[Line]) -> Vec<(u32, bool)> {
        (engine::bands(lines).iter())
            .map(|band| (band.offset, band.selected))
            .collect()
    }

    #[test]
    fn the_topmost_band_on_the_screen_is_selected() {
        let (mut lines, blocks, edition) = laid_out();
        let mut bands = Bands::default();
        assert!(bands.refresh(&edition, (&lines, &blocks), 0..lines.len()));
        bands.apply(&edition, &mut lines, &blocks);
        assert_eq!(shown(&lines), [(0, true), (0, false)]);
        // Scrolled past the first, the second takes its place.
        let second = engine::bands(&lines)[1].lines.clone();
        assert!(bands.refresh(&edition, (&lines, &blocks), second.start..lines.len()));
        bands.apply(&edition, &mut lines, &blocks);
        assert_eq!(shown(&lines), [(0, false), (0, true)]);
        assert!(!bands.refresh(&edition, (&lines, &blocks), second.start..lines.len()));
    }

    #[test]
    fn a_band_is_scrolled_to_show_what_is_asked_for_and_selected() {
        let long = "a".repeat(200);
        let (mut lines, blocks, edition) = laid_out_from(&format!(
            "```\n{long}\n```\n\nText.\n\n```\n{long} needle\n```\n"
        ));
        let mut bands = Bands::default();
        bands.refresh(&edition, (&lines, &blocks), 0..lines.len());
        let second = engine::bands(&lines)[1].lines.clone();
        let visible = engine::bands(&lines)[1].visible();
        // In view already: only selected.
        assert_eq!(
            bands.reveal(&edition, (&lines, &blocks), second.start, 0..30),
            Some(true)
        );
        bands.apply(&edition, &mut lines, &blocks);
        assert_eq!(shown(&lines), [(0, false), (0, true)]);
        // Far to the right: scrolled so that it is a quarter in.
        let x = 1500..1560;
        assert_eq!(
            bands.reveal(&edition, (&lines, &blocks), second.start, x.clone()),
            Some(true)
        );
        bands.apply(&edition, &mut lines, &blocks);
        let offset = shown(&lines)[1].0;
        assert_eq!(
            offset,
            (x.start - visible / 4).min(engine::bands(&lines)[1].most())
        );
        assert!(x.start >= offset && x.end <= offset + visible);
        // Already there: nothing changes; no band on a line: none.
        assert_eq!(
            bands.reveal(&edition, (&lines, &blocks), second.start, x),
            Some(false)
        );
        let text = engine::bands(&lines)[0].lines.end;
        assert_eq!(bands.reveal(&edition, (&lines, &blocks), text, 0..1), None);
    }

    #[test]
    fn the_keys_scroll_the_selected_band_within_its_room() {
        let (mut lines, blocks, edition) = laid_out();
        let mut bands = Bands::default();
        bands.refresh(&edition, (&lines, &blocks), 0..lines.len());
        let most = engine::bands(&lines)[0].most();
        assert!(!bands.scroll(&edition, (&lines, &blocks), Step::Left));
        assert!(bands.scroll(&edition, (&lines, &blocks), Step::Right));
        bands.apply(&edition, &mut lines, &blocks);
        assert_eq!(shown(&lines), [(64, true), (0, false)]);
        for _ in 0..10 {
            bands.scroll(&edition, (&lines, &blocks), Step::PageRight);
        }
        bands.apply(&edition, &mut lines, &blocks);
        assert_eq!(shown(&lines)[0].0, most);
        assert!(!bands.scroll(&edition, (&lines, &blocks), Step::PageRight));
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
            let (lines, blocks, edition) = laid_out_from(&source);
            let found = engine::bands(&lines);
            assert_eq!(found.len(), 2, "{source:?}");
            // Both are in one block, whose place they share.
            assert_eq!(
                lines[found[0].lines.start].block,
                lines[found[1].lines.start].block
            );
            let mut bands = Bands::default();
            bands.select_at(&edition, (&lines, &blocks), found[1].lines.start);
            bands.scroll(&edition, (&lines, &blocks), Step::Right);
            bands.scroll(&edition, (&lines, &blocks), Step::Right);
            let (mut again, blocks, _) = laid_out_from(&source);
            bands.apply(&edition, &mut again, &blocks);
            // The second is scrolled and selected, the first neither.
            assert_eq!(shown(&again), [(0, false), (128, true)], "{source:?}");
        }
    }

    #[test]
    fn a_line_of_a_band_selects_it() {
        let (lines, blocks, edition) = laid_out();
        let mut bands = Bands::default();
        let second = engine::bands(&lines)[1].lines.clone();
        assert_eq!(
            bands.select_at(&edition, (&lines, &blocks), second.start + 1),
            Some(true)
        );
        assert_eq!(
            bands.select_at(&edition, (&lines, &blocks), second.start),
            Some(false)
        );
        let between = engine::bands(&lines)[0].lines.end;
        assert_eq!(bands.select_at(&edition, (&lines, &blocks), between), None);
    }

    #[test]
    fn a_new_layout_keeps_each_band_where_it_was() {
        let (lines, blocks, edition) = laid_out();
        let mut bands = Bands::default();
        bands.refresh(&edition, (&lines, &blocks), 0..lines.len());
        bands.scroll(&edition, (&lines, &blocks), Step::Right);
        let (mut again, blocks, _) = laid_out();
        bands.apply(&edition, &mut again, &blocks);
        assert_eq!(shown(&again), [(64, true), (0, false)]);
    }

    #[test]
    fn a_band_keeps_its_place_when_its_block_starts_on_another_line() {
        let long = "a".repeat(200);
        let code = format!("```\n{long}\n```\n\nText.\n\n```\n{long}\n```\n");
        let (lines, blocks, edition) = laid_out_from(&code);
        let mut bands = Bands::default();
        bands.refresh(&edition, (&lines, &blocks), 0..lines.len());
        bands.scroll(&edition, (&lines, &blocks), Step::Right);
        // Blank lines above the blocks move every block down the file.
        let (mut moved, blocks, _) = laid_out_from(&format!("\n\n{code}"));
        bands.apply(&edition, &mut moved, &blocks);
        assert_eq!(shown(&moved), [(64, true), (0, false)]);
    }

    /// `old` and then `new` laid out to scroll, as two versions of one
    /// document, with how the bands of the one move to the other.
    fn versions(old: &str, new: &str) -> (Laid, Laid, Comparison) {
        let (old_document, new_document) = (parse(old), parse(new));
        let mut session = Session::new(["a.md"]);
        let before = laid_with(&old_document, &session);
        let id = session.sources()[0].id();
        session.reload(id, None);
        let after = laid_with(&new_document, &session);
        (before, after, compare(&old_document, &new_document))
    }

    type Laid = (Vec<Line>, Blocks, Edition);

    fn laid_with(document: &Document, session: &Session) -> Laid {
        let Stream { lines, blocks } = content(Content::Document(document), &scrolling());
        let edition = Edition::new(Content::Document(document), session, None);
        (lines, blocks, edition)
    }

    /// Three code blocks too wide for the area.
    fn three(first: &str) -> String {
        let long = "a".repeat(200);
        let code = format!("```\n{long}\n```\n\n");
        format!("{first}{code}{code}{code}")
    }

    #[test]
    fn a_band_keeps_its_scroll_and_selection_when_a_reload_adds_a_block_above_it() {
        let (old, new, changes) = versions(&three(""), &three("Added above.\n\n"));
        let ((lines, blocks, before), (mut again, new_blocks, after)) = (old, new);
        let mut bands = Bands::default();
        let second = engine::bands(&lines)[1].lines.start;
        bands.select_at(&before, (&lines, &blocks), second);
        bands.scroll(&before, (&lines, &blocks), Step::Right);
        bands.scroll(&before, (&lines, &blocks), Step::Right);
        bands.align(&Moves::new(&before, &after, Some(&changes)), &after);
        bands.apply(&after, &mut again, &new_blocks);
        assert_eq!(
            shown(&again),
            [(0, false), (128, true), (0, false)],
            "{:?}",
            bands
        );
    }

    #[test]
    fn a_band_whose_block_a_reload_removes_is_forgotten_and_the_next_keeps_its_place() {
        let long = "a".repeat(200);
        let code = |word: &str| format!("```\n{long} {word}\n```\n\n");
        let source = |words: &[&str]| words.iter().map(|word| code(word)).collect::<String>();
        let (old, new, changes) = versions(
            &source(&["one", "two", "three"]),
            &source(&["one", "three"]),
        );
        let ((lines, blocks, before), (mut again, new_blocks, after)) = (old, new);
        let mut bands = Bands::default();
        let found = engine::bands(&lines);
        // The second band is scrolled and selected, then the third scrolled.
        bands.select_at(&before, (&lines, &blocks), found[2].lines.start);
        bands.scroll(&before, (&lines, &blocks), Step::Right);
        bands.select_at(&before, (&lines, &blocks), found[1].lines.start);
        bands.scroll(&before, (&lines, &blocks), Step::Right);
        bands.align(&Moves::new(&before, &after, Some(&changes)), &after);
        bands.apply(&after, &mut again, &new_blocks);
        // The removed one's scroll and the selection with it are gone; the
        // one after it stays scrolled as it was.
        assert_eq!(shown(&again), [(0, false), (64, false)], "{bands:?}");
    }

    #[test]
    fn the_map_of_the_lines_is_the_same_after_the_bands_are_scrolled() {
        // The pager makes the map before the bands are shown as scrolled,
        // and the layout keeps it.
        let long = "a".repeat(200);
        let source = format!("```\n{long} needle\n```\n\nText.\n");
        let document = parse(&source);
        let (mut lines, blocks, edition) = laid_out_from(&source);
        let before = crate::typeset::locate::Map::new(&lines, &blocks);
        let range = &crate::typeset::text::DocumentText::new([&document]).find("needle", false)[0];
        let first = before.locate(&lines, range);
        let mut bands = Bands::default();
        bands.refresh(&edition, (&lines, &blocks), 0..lines.len());
        bands.scroll(&edition, (&lines, &blocks), Step::Right);
        bands.apply(&edition, &mut lines, &blocks);
        assert_eq!(shown(&lines)[0].0, 64);
        let after = crate::typeset::locate::Map::new(&lines, &blocks);
        assert_eq!(before.locate(&lines, range), first);
        assert_eq!(after.locate(&lines, range), first);
    }
}
