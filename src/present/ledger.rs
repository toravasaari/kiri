//! What the terminal holds of the images kiri sent it, kept from what kiri
//! sends and from the terminal's answers (`lost`, `lost_some`, the checks
//! behind tmux).
//!
//! This is kitty's bookkeeping: ids, placements and the cell sizes they
//! were scaled for, which another image protocol may not share. It holds
//! no pixels (those are the encoder's) and writes no bytes
//! (`kitty::graphics` does).

use std::collections::{HashMap, HashSet};

use crate::present::kitty::graphics::{Cells, ImageId};
use crate::typeset::images::ImageKey;
use crate::typeset::images::size::Placement;

/// How many times an image is sent again at most for being lost.
pub const AGAIN: u8 = 3;

/// The frame number under which a part sent with a picture's frames is
/// held, as opposed to one frame that kiri plays.
pub(crate) const DRIVEN: usize = usize::MAX;

/// An image the terminal holds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Held {
    pub(crate) id: ImageId,
    /// The pixels of the cells it was scaled to fit.
    pub(crate) cell: (u16, u16),
    /// Whether `id` is known to be that of the pixels as they are now:
    /// not after they may have changed, until the image is scaled again.
    pub(crate) checked: bool,
    /// Whether the terminal is known to have been sent the image: not
    /// after it may have lost it, until it is sent again.
    pub(crate) sent: bool,
    /// The states of the animated pictures in it that its pixels were drawn
    /// with, hashed: another state is another image.
    pub(crate) states: u64,
}

/// The images the terminal holds, by where and how large they are shown.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    /// The images shown as one, by placement.
    stills: HashMap<Placement, Held>,
    /// The frames after the first of the animations kiri plays, which are
    /// held as images of their own, by placement and frame; and the parts
    /// sent with a picture's frames, under [`DRIVEN`].
    frames: HashMap<(Placement, ImageKey, usize), Held>,
    /// Images the terminal holds that no placement has any more, to be
    /// deleted before the next frame.
    unused: Vec<ImageId>,
    /// The bands that tell an image is loading, by their columns and the
    /// pixels of their cells: one image for all the bands of a width
    /// drawn since the screen was last cleared.
    bands: HashMap<(u16, (u16, u16)), ImageId>,
    /// The bands drawn since the screen was last cleared, which are the
    /// ones on it.
    shown: HashSet<(u16, (u16, u16))>,
    /// The images sent through tmux and the cells they were sent for,
    /// until [`Ledger::take_sent`] takes them to check that they came.
    sent: Vec<(ImageId, Cells)>,
    /// How many times each image has been sent again for being lost.
    again: HashMap<ImageId, u8>,
}

impl Ledger {
    /// The still image held for `placement`.
    pub(crate) fn still(&self, placement: &Placement) -> Option<&Held> {
        self.stills.get(placement)
    }

    /// Holds `held` for `placement`, giving back what it held before.
    pub(crate) fn put_still(&mut self, placement: Placement, held: Held) -> Option<Held> {
        self.stills.insert(placement, held)
    }

    /// The placements that have a still image.
    pub(crate) fn still_placements(&self) -> impl Iterator<Item = &Placement> {
        self.stills.keys()
    }

    /// The placement whose still image is `id` at `cell`.
    pub(crate) fn placement_of(&self, id: ImageId, cell: (u16, u16)) -> Option<Placement> {
        (self.stills.iter())
            .find(|(_, held)| held.id == id && held.cell == cell)
            .map(|(placement, _)| placement.clone())
    }

    /// The frame image held at `at`.
    pub(crate) fn frame(&self, at: &(Placement, ImageKey, usize)) -> Option<&Held> {
        self.frames.get(at)
    }

    /// Holds `held` at `at`.
    pub(crate) fn put_frame(&mut self, at: (Placement, ImageKey, usize), held: Held) {
        self.frames.insert(at, held);
    }

    /// The ids sent with `picture`'s frames, each once, in the order of
    /// the ids, so that what is written does not depend on the map.
    pub(crate) fn driven(&self, picture: &ImageKey) -> Vec<ImageId> {
        let mut ids = Vec::new();
        for ((_, key, index), held) in &self.frames {
            if *index == DRIVEN && key == picture && held.sent && !ids.contains(&held.id) {
                ids.push(held.id);
            }
        }
        ids.sort_by_key(|id| id.value());
        ids
    }

    /// Whether some still or frame has `id`.
    pub(crate) fn holds(&self, id: ImageId) -> bool {
        (self.stills.values().chain(self.frames.values())).any(|held| held.id == id)
    }

    /// Whether some still or frame has `id` and it was sent.
    pub(crate) fn holds_sent(&self, id: ImageId) -> bool {
        (self.stills.values().chain(self.frames.values())).any(|held| held.id == id && held.sent)
    }

    /// Every still and every frame kiri plays with `id` is sent. The parts
    /// sent with a picture's frames, under [`DRIVEN`], are not: what was
    /// sent is one image, and they are set sent by the encoder when it
    /// puts them here, before their frames are written. A write that fails
    /// on the way is undone by [`Ledger::lost`].
    pub(crate) fn mark_sent(&mut self, id: ImageId) {
        let stills = self.stills.values_mut();
        let frames = (self.frames.iter_mut())
            .filter(|((_, _, index), _)| *index != DRIVEN)
            .map(|(_, held)| held);
        for held in stills.chain(frames).filter(|held| held.id == id) {
            held.sent = true;
        }
    }

    /// Keeps the stills, frames and bands of `placed` and `bands` at
    /// `cell`; the ids nothing holds any more go to the unused.
    pub(crate) fn keep(
        &mut self,
        placed: &HashSet<&Placement>,
        bands: &HashSet<u16>,
        cell: (u16, u16),
    ) {
        self.bands.retain(|&(columns, band_cell), &mut id| {
            let kept = band_cell == cell && bands.contains(&columns);
            if !kept {
                self.unused.push(id);
            }
            kept
        });
        let mut gone = Vec::new();
        self.stills.retain(|placement, held| {
            let kept = held.cell == cell && placed.contains(placement);
            if !kept {
                gone.push(held.id);
            }
            kept
        });
        self.frames.retain(|(placement, _, _), held| {
            let kept = held.cell == cell && placed.contains(placement);
            if !kept {
                gone.push(held.id);
            }
            kept
        });
        self.unuse(&gone);
    }

    /// The ids [`Ledger::keep`] let go, which the terminal is to delete.
    pub(crate) fn take_unused(&mut self) -> Vec<ImageId> {
        std::mem::take(&mut self.unused)
    }

    /// Puts back among the unused the `ids` whose deletion did not get to the
    /// terminal, except those something holds again.
    pub(crate) fn unuse(&mut self, ids: &[ImageId]) {
        for &id in ids {
            if !self.holds(id) && !self.unused.contains(&id) {
                self.unused.push(id);
            }
        }
    }

    /// Everything held, bands included, to delete: each id once.
    pub(crate) fn drain(&mut self) -> Vec<ImageId> {
        self.unused.extend(self.bands.drain().map(|(_, id)| id));
        let frames = self.frames.drain().map(|(_, held)| held);
        for held in self.stills.drain().map(|(_, held)| held).chain(frames) {
            if !self.unused.contains(&held.id) {
                self.unused.push(held.id);
            }
        }
        std::mem::take(&mut self.unused)
    }

    /// The pixels of the images may have changed: each is scaled again
    /// when it is next shown.
    pub(crate) fn changed(&mut self) {
        for held in self.stills.values_mut().chain(self.frames.values_mut()) {
            held.checked = false;
        }
    }

    /// The terminal may have lost the images it was sent: each is sent
    /// again when it is next shown.
    pub(crate) fn lost(&mut self) {
        for held in self.stills.values_mut().chain(self.frames.values_mut()) {
            held.sent = false;
        }
        self.again.clear();
    }

    /// The terminal may have lost the images `ids`.
    pub(crate) fn lost_some(&mut self, ids: &[ImageId]) {
        let held = self.stills.values_mut().chain(self.frames.values_mut());
        for held in held.filter(|held| ids.contains(&held.id)) {
            held.sent = false;
        }
        self.again.retain(|id, _| !ids.contains(id));
    }

    /// Notes `id` sent over `cells`, to check it came when `through_tmux`.
    pub(crate) fn sending(&mut self, id: ImageId, cells: Cells, through_tmux: bool) {
        let sent = (id, cells);
        if through_tmux && !self.sent.contains(&sent) {
            self.sent.push(sent);
        }
    }

    /// The images sent through tmux since this was last asked.
    pub(crate) fn take_sent(&mut self) -> Vec<(ImageId, Cells)> {
        std::mem::take(&mut self.sent)
    }

    /// Whether `id` may be sent again for being lost: it has had fewer
    /// than [`AGAIN`] tries.
    pub(crate) fn may_try_again(&self, id: ImageId) -> bool {
        self.again.get(&id).is_none_or(|&tries| tries < AGAIN)
    }

    /// Whether `id` may be sent again for being lost (fewer than
    /// [`AGAIN`] tries), counting this one when it may.
    pub(crate) fn try_again(&mut self, id: ImageId) -> bool {
        if !self.may_try_again(id) {
            return false;
        }
        *self.again.entry(id).or_default() += 1;
        true
    }

    /// The band of `columns` at `cell`, which is on the screen from now
    /// until it is next cleared, if the terminal holds it.
    pub(crate) fn band(&mut self, columns: u16, cell: (u16, u16)) -> Option<ImageId> {
        self.shown.insert((columns, cell));
        self.bands.get(&(columns, cell)).copied()
    }

    /// Holds `id` as the band of `columns` at `cell`.
    pub(crate) fn put_band(&mut self, columns: u16, cell: (u16, u16), id: ImageId) {
        self.bands.insert((columns, cell), id);
    }

    /// The screen was cleared: no band is on it, and the terminal no
    /// longer holds the bands.
    pub(crate) fn cleared(&mut self) {
        self.shown.clear();
        self.bands.clear();
    }

    /// Whether a band is on the screen.
    pub(crate) fn has_bands(&self) -> bool {
        !self.shown.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::images::size::{Shows, Size};

    fn placement(name: &str) -> Placement {
        Placement::block(
            ImageKey::from(name),
            Size {
                width: 10,
                height: 20,
                columns: 1,
                rows: 1,
            },
            Shows::Image,
        )
    }

    fn cells() -> Cells {
        Cells {
            columns: 1,
            rows: 1,
        }
    }

    fn id(n: u8) -> ImageId {
        ImageId::new(&[n], 1, 1, cells(), None)
    }

    fn held(n: u8) -> Held {
        Held {
            id: id(n),
            cell: (10, 20),
            checked: true,
            sent: true,
            states: 0,
        }
    }

    fn keep(ledger: &mut Ledger, placed: &[&Placement], cell: (u16, u16)) {
        let placed: HashSet<&Placement> = placed.iter().copied().collect();
        ledger.keep(&placed, &HashSet::new(), cell);
    }

    #[test]
    fn an_id_two_placements_share_goes_only_when_neither_keeps_it() {
        let (a, b) = (placement("a"), placement("b"));
        let mut ledger = Ledger::default();
        ledger.put_still(a.clone(), held(1));
        ledger.put_still(b.clone(), held(1));
        keep(&mut ledger, &[&b], (10, 20));
        assert!(ledger.take_unused().is_empty());
        keep(&mut ledger, &[], (10, 20));
        assert_eq!(ledger.take_unused(), vec![id(1)]);
    }

    #[test]
    fn ids_put_back_are_unused_again_unless_held_or_already_there() {
        let a = placement("a");
        let mut ledger = Ledger::default();
        ledger.put_still(a, held(1));
        ledger.unuse(&[id(1), id(2), id(2), id(3)]);
        assert_eq!(ledger.take_unused(), vec![id(2), id(3)]);
    }

    #[test]
    fn a_new_cell_size_lets_every_image_go() {
        let a = placement("a");
        let mut ledger = Ledger::default();
        ledger.put_still(a.clone(), held(1));
        keep(&mut ledger, &[&a], (12, 24));
        assert_eq!(ledger.take_unused(), vec![id(1)]);
    }

    #[test]
    fn lost_some_resets_the_tries_of_those_images_alone() {
        let mut ledger = Ledger::default();
        for _ in 0..AGAIN {
            assert!(ledger.try_again(id(1)));
            assert!(ledger.try_again(id(2)));
        }
        assert!(!ledger.try_again(id(1)));
        assert!(!ledger.try_again(id(2)));
        ledger.lost_some(&[id(1)]);
        assert!(ledger.try_again(id(1)));
        assert!(!ledger.try_again(id(2)));
        ledger.lost();
        assert!(ledger.try_again(id(1)));
        assert!(ledger.try_again(id(2)));
    }

    #[test]
    fn pixels_held_and_sent_under_another_slot_are_not_sent_again() {
        let mut ledger = Ledger::default();
        ledger.put_still(placement("a"), held(5));
        assert!(ledger.holds_sent(id(5)));
        ledger.lost_some(&[id(5)]);
        assert!(!ledger.holds_sent(id(5)));
        assert!(ledger.holds(id(5)));
    }

    #[test]
    fn sending_an_image_marks_the_stills_and_frames_that_share_its_id_sent() {
        let mut ledger = Ledger::default();
        let lost = |n| Held {
            sent: false,
            ..held(n)
        };
        ledger.put_still(placement("a"), lost(1));
        let frame = (placement("b"), ImageKey::from("b"), 1);
        ledger.put_frame(frame.clone(), lost(1));
        let driven = (placement("c"), ImageKey::from("c"), DRIVEN);
        ledger.put_frame(driven.clone(), lost(1));
        ledger.put_still(placement("d"), lost(2));
        ledger.mark_sent(id(1));
        assert!(ledger.still(&placement("a")).unwrap().sent);
        assert!(ledger.frame(&frame).unwrap().sent);
        assert!(!ledger.frame(&driven).unwrap().sent);
        assert!(!ledger.still(&placement("d")).unwrap().sent);
    }

    #[test]
    fn ids_sent_through_tmux_wait_for_one_check_each() {
        let mut ledger = Ledger::default();
        ledger.sending(id(1), cells(), true);
        ledger.sending(id(1), cells(), true);
        ledger.sending(id(2), cells(), false);
        assert_eq!(ledger.take_sent(), vec![(id(1), cells())]);
        assert!(ledger.take_sent().is_empty());
    }

    #[test]
    fn delete_all_gives_every_id_held_once_bands_included() {
        let mut ledger = Ledger::default();
        ledger.put_still(placement("a"), held(1));
        ledger.put_still(placement("b"), held(1));
        let at = (placement("c"), ImageKey::from("c"), 1);
        ledger.put_frame(at, held(2));
        ledger.put_band(4, (10, 20), id(3));
        let mut ids = ledger.drain();
        ids.sort_by_key(|id| id.value());
        let mut want = vec![id(1), id(2), id(3)];
        want.sort_by_key(|id| id.value());
        assert_eq!(ids, want);
    }
}
