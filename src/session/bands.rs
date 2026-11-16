//! Reading mode's bands in the pager: code blocks and tables that scroll
//! sideways, how far each is scrolled, and which one is selected: the
//! topmost on the screen, or the one a click or a search selects.

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
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::render::Inputs;
    use crate::session::edition::Moves;
    use crate::session::sources::Session;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::compare::{Comparison, compare};
    use crate::typeset::driver::{Content, Stream, content};
    use crate::typeset::model::Document;
    use crate::typeset::parse::parse;
    use crate::typeset::reading::Fonts;
    use crate::typeset::reading::engine::Reading;
    use crate::typeset::reading::page::Wide;

    /// Two code blocks too wide for the area, a paragraph apart, laid out
    /// to scroll, with their map.
    fn laid_out() -> Laid {
        let long = "a".repeat(200);
        laid_out_from(&format!("```\n{long}\n```\n\nText.\n\n```\n{long}\n```\n"))
    }

    /// `source` laid out to scroll, with its map.
    fn laid_out_from(source: &str) -> Laid {
        laid_with(&parse(source), &Session::new(["a.md"]))
    }

    /// Reading mode, which scrolls what is too wide.
    fn scrolling() -> Inputs {
        Inputs {
            reading: Some(Reading {
                pictures: Default::default(),
                fonts: Rc::new(Fonts::fixture()),
                wide: Wide::Scroll,
                snap: false,
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
        let (mut lines, map, edition) = laid_out();
        let mut bands = Bands::default();
        assert!(bands.refresh(&edition, &map, 0..lines.len()));
        bands.apply(&edition, &mut lines, &map);
        assert_eq!(shown(&lines), [(0, true), (0, false)]);
        // Scrolled past the first, the second takes its place.
        let second = map.bands()[1].lines.clone();
        assert!(bands.refresh(&edition, &map, second.start..lines.len()));
        bands.apply(&edition, &mut lines, &map);
        assert_eq!(shown(&lines), [(0, false), (0, true)]);
        assert!(!bands.refresh(&edition, &map, second.start..lines.len()));
    }

    #[test]
    fn a_band_is_scrolled_to_show_what_is_asked_for_and_selected() {
        let long = "a".repeat(200);
        let (mut lines, map, edition) = laid_out_from(&format!(
            "```\n{long}\n```\n\nText.\n\n```\n{long} needle\n```\n"
        ));
        let mut bands = Bands::default();
        bands.refresh(&edition, &map, 0..lines.len());
        let second = map.bands()[1].lines.clone();
        let visible = map.bands()[1].visible();
        // In view already: only selected.
        assert_eq!(
            bands.reveal(&edition, &map, second.start, 0..30),
            Some(true)
        );
        bands.apply(&edition, &mut lines, &map);
        assert_eq!(shown(&lines), [(0, false), (0, true)]);
        // Far to the right: scrolled so that it is a quarter in.
        let x = 1500..1560;
        assert_eq!(
            bands.reveal(&edition, &map, second.start, x.clone()),
            Some(true)
        );
        bands.apply(&edition, &mut lines, &map);
        let offset = shown(&lines)[1].0;
        assert_eq!(offset, (x.start - visible / 4).min(map.bands()[1].most()));
        assert!(x.start >= offset && x.end <= offset + visible);
        // Already there: nothing changes; no band on a line: none.
        assert_eq!(bands.reveal(&edition, &map, second.start, x), Some(false));
        let text = map.bands()[0].lines.end;
        assert_eq!(bands.reveal(&edition, &map, text, 0..1), None);
    }

    #[test]
    fn the_keys_scroll_the_selected_band_within_its_room() {
        let (mut lines, map, edition) = laid_out();
        let mut bands = Bands::default();
        bands.refresh(&edition, &map, 0..lines.len());
        let most = map.bands()[0].most();
        assert!(!bands.scroll(&edition, &map, Step::Left));
        assert!(bands.scroll(&edition, &map, Step::Right));
        bands.apply(&edition, &mut lines, &map);
        assert_eq!(shown(&lines), [(64, true), (0, false)]);
        for _ in 0..10 {
            bands.scroll(&edition, &map, Step::PageRight);
        }
        bands.apply(&edition, &mut lines, &map);
        assert_eq!(shown(&lines)[0].0, most);
        assert!(!bands.scroll(&edition, &map, Step::PageRight));
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
            let (lines, map, edition) = laid_out_from(&source);
            let found = map.bands();
            assert_eq!(found.len(), 2, "{source:?}");
            // Both are in one block, whose place they share.
            assert_eq!(
                lines[found[0].lines.start].block,
                lines[found[1].lines.start].block
            );
            let mut bands = Bands::default();
            bands.select_at(&edition, &map, found[1].lines.start);
            bands.scroll(&edition, &map, Step::Right);
            bands.scroll(&edition, &map, Step::Right);
            let (mut again, again_map, _) = laid_out_from(&source);
            bands.apply(&edition, &mut again, &again_map);
            // The second is scrolled and selected, the first neither.
            assert_eq!(shown(&again), [(0, false), (128, true)], "{source:?}");
        }
    }

    #[test]
    fn a_band_is_known_by_the_innermost_block_it_shows() {
        // A code block in a list item and a table in a quote: the band is
        // the code block's or the table's, not the list's or the quote's.
        let long = "a".repeat(200);
        let cells = format!("| {long} | b |\n| --- | --- |\n| c | d |\n");
        for (source, starts) in [
            (format!("- item\n\n  ```\n  {long}\n  ```\n"), "```"),
            (format!("> {}", cells.replace('\n', "\n> ")), "|"),
        ] {
            let document = parse(&source);
            let (_, map, _) = laid_with(&document, &Session::new(["a.md"]));
            assert_eq!(map.bands().len(), 1, "{source:?}");
            let block = map.bands()[0].block().expect("a block");
            assert!(block.path().len() > 1, "{source:?}: {block:?}");
            let place = document.places.get(block.path()).expect("a place");
            let written = source[place.bytes.clone()].trim_start_matches([' ', '>']);
            assert!(written.starts_with(starts), "{source:?}: {written:?}");
        }
    }

    #[test]
    fn a_band_as_it_should_be_is_left_as_it_is() {
        // Setting a band anew makes its rows anew, which costs on a page with
        // many links; the pager shows the bands on every layout.
        let (mut lines, map, edition) = laid_out();
        let mut bands = Bands::default();
        bands.refresh(&edition, &map, 0..lines.len());
        bands.apply(&edition, &mut lines, &map);
        let rows = |lines: &[Line]| -> Vec<_> {
            (lines.iter())
                .filter_map(|line| Some(Rc::clone(&line.image.as_ref()?.placement)))
                .collect()
        };
        let before = rows(&lines);
        bands.apply(&edition, &mut lines, &map);
        let after = rows(&lines);
        assert!(!before.is_empty());
        assert!(before.iter().zip(&after).all(|(a, b)| Rc::ptr_eq(a, b)));
    }

    #[test]
    fn a_line_of_a_band_selects_it() {
        let (_, map, edition) = laid_out();
        let mut bands = Bands::default();
        let second = map.bands()[1].lines.clone();
        assert_eq!(
            bands.select_at(&edition, &map, second.start + 1),
            Some(true)
        );
        assert_eq!(bands.select_at(&edition, &map, second.start), Some(false));
        let between = map.bands()[0].lines.end;
        assert_eq!(bands.select_at(&edition, &map, between), None);
    }

    #[test]
    fn a_new_layout_keeps_each_band_where_it_was() {
        let (lines, map, edition) = laid_out();
        let mut bands = Bands::default();
        bands.refresh(&edition, &map, 0..lines.len());
        bands.scroll(&edition, &map, Step::Right);
        let (mut again, again_map, _) = laid_out();
        bands.apply(&edition, &mut again, &again_map);
        assert_eq!(shown(&again), [(64, true), (0, false)]);
    }

    #[test]
    fn a_band_keeps_its_place_when_its_block_starts_on_another_line() {
        let long = "a".repeat(200);
        let code = format!("```\n{long}\n```\n\nText.\n\n```\n{long}\n```\n");
        let (lines, map, edition) = laid_out_from(&code);
        let mut bands = Bands::default();
        bands.refresh(&edition, &map, 0..lines.len());
        bands.scroll(&edition, &map, Step::Right);
        // Blank lines above the blocks move every block down the file.
        let (mut moved, moved_map, _) = laid_out_from(&format!("\n\n{code}"));
        bands.apply(&edition, &mut moved, &moved_map);
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

    type Laid = (Vec<Line>, Map, Edition);

    fn laid_with(document: &Document, session: &Session) -> Laid {
        let Stream { lines, blocks } = content(Content::Document(document), &scrolling());
        let map = Map::new(&lines, &blocks);
        let edition = Edition::new(
            Content::Document(document),
            session,
            session.base().id(),
            None,
        );
        (lines, map, edition)
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
        let ((_, map, before), (mut again, new_map, after)) = (old, new);
        let mut bands = Bands::default();
        let second = map.bands()[1].lines.start;
        bands.select_at(&before, &map, second);
        bands.scroll(&before, &map, Step::Right);
        bands.scroll(&before, &map, Step::Right);
        bands.align(&Moves::new(&before, &after, Some(&changes)), &after);
        bands.apply(&after, &mut again, &new_map);
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
        let ((_, map, before), (mut again, new_map, after)) = (old, new);
        let mut bands = Bands::default();
        let found = map.bands();
        // The second band is scrolled and selected, then the third scrolled.
        bands.select_at(&before, &map, found[2].lines.start);
        bands.scroll(&before, &map, Step::Right);
        bands.select_at(&before, &map, found[1].lines.start);
        bands.scroll(&before, &map, Step::Right);
        bands.align(&Moves::new(&before, &after, Some(&changes)), &after);
        bands.apply(&after, &mut again, &new_map);
        // The removed one's scroll and the selection with it are gone; the
        // one after it stays scrolled as it was.
        assert_eq!(shown(&again), [(0, false), (64, false)], "{bands:?}");
    }
}
