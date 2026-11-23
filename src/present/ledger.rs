//! What the terminal holds of the images kiri sent it, kept from what kiri sends
//! and from the terminal's answers.
//!
//! The answers it reads are `lost`, `lost_some` and the checks behind tmux. The
//! `Ledger` records which image id each placement and frame was sent under, at
//! which cells, whether it is checked and sent, the ids to delete, the ids sent
//! through tmux that wait for a check, the stills replaced through tmux that wait
//! for the check of the image that replaced them, the tries to send a lost image
//! again, the bytes each image counts and when it was last asked for, which the
//! budget reads, and the bands that tell an image is loading.
//!
//! This is kitty's bookkeeping: ids, placements and the cell sizes they
//! were scaled for, which another image protocol may not share. It holds
//! no pixels (those are the encoder's) and writes no bytes
//! (`kitty::graphics` does). It was taken out of the encoder.

use std::collections::{HashMap, HashSet};

use crate::present::kitty::graphics::{Cells, ImageId};
use crate::typeset::images::ImageKey;
use crate::typeset::images::size::Placement;

/// How many times an image is sent again at most for being lost.
pub const AGAIN: u8 = 3;

/// A megabyte of images, as the budget counts them: 1 000 000 bytes.
pub const MEGABYTE: usize = 1_000_000;

/// The megabytes of images the terminal may hold for the pager when the
/// `image_memory` setting says nothing. Kitty and Ghostty keep about 320 MB
/// for a screen, so two windows at this size fit under it and three do not.
pub const DEFAULT_IMAGE_MEMORY_MB: u64 = 128;

/// The frame number under which an image sent with a picture's frames is
/// held, such as the inset of a picture in a part of a page, as opposed to
/// one frame that kiri plays.
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
    /// The frames of the animations kiri plays, which are held as images
    /// of their own, by placement, picture and frame; and the images sent
    /// with a picture's frames, under [`DRIVEN`]. For a picture in a part of
    /// a page they are its inset, and the part itself is a still.
    frames: HashMap<(Placement, ImageKey, usize), Held>,
    /// Images the terminal holds that no placement has any more, to be
    /// deleted before the next frame.
    unused: Vec<ImageId>,
    /// Images replaced through tmux, each with the image that replaced
    /// it. tmux passes a delete on at once, but draws the cells of the
    /// frame after it, so each waits until kitty has answered the check
    /// of the image after it ([`Ledger::answered`]), or until the screen
    /// is drawn anew, as by a new layout or after a loss.
    replaced: Vec<(ImageId, ImageId)>,
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
    /// What the terminal holds for each id, as sent, and when it was last
    /// asked for.
    images: HashMap<ImageId, Weight>,
    /// The use counter, which [`Ledger::tick`] moves on once a frame.
    now: u64,
}

/// The images a frame is about to draw and the images near it, which the
/// budget keeps.
#[derive(Debug, Default)]
pub(crate) struct Kept {
    /// The images of the lines the frame draws.
    pub(crate) view: HashSet<ImageId>,
    /// The images of the lines near the view.
    pub(crate) near: HashSet<ImageId>,
}

/// The size of an image the terminal holds and its age, by its id.
#[derive(Debug, Clone, Copy)]
struct Weight {
    /// The bytes of the pixels as they were sent, frames included.
    bytes: usize,
    /// The use counter when the image was last asked for.
    used: u64,
}

impl Ledger {
    /// Notes that `id` is asked for now and that the terminal holds `bytes`
    /// for it as sent: for an id that is already here, its size stays.
    pub(crate) fn weigh(&mut self, id: ImageId, bytes: usize) {
        let now = self.now;
        self.images
            .entry(id)
            .or_insert(Weight { bytes, used: now })
            .used = now;
    }

    /// Notes that `id` is asked for now, though nothing is sent for it.
    pub(crate) fn touch(&mut self, id: ImageId) {
        if let Some(weight) = self.images.get_mut(&id) {
            weight.used = self.now;
        }
    }

    /// A frame starts: the use counter moves on.
    pub(crate) fn tick(&mut self) {
        self.now += 1;
    }

    /// The use counter when `id` was last asked for.
    pub(crate) fn used(&self, id: ImageId) -> Option<u64> {
        self.images.get(&id).map(|weight| weight.used)
    }

    /// The bytes the terminal may hold for the images in the ledger, each
    /// id once, also those it may have lost. The loading bands are not
    /// counted.
    pub(crate) fn bytes(&self) -> usize {
        self.images.values().map(|weight| weight.bytes).sum()
    }

    /// The ids to let go of so that the terminal holds no more than `budget`
    /// bytes once `extra` more are sent: while the total is over, those not
    /// near, the ones asked for longest ago first, and then, only if the
    /// near ones alone are over, those near, again the oldest first. Never
    /// one in view, never one the last frame asked for, and none that no
    /// still or frame holds. What waits to be deleted is not counted.
    pub(crate) fn over(&self, budget: usize, extra: usize, marks: &Kept) -> Vec<ImageId> {
        let waiting = |id: &ImageId| self.unused.contains(id);
        let mut total = extra
            + (self.images.iter())
                .filter(|(id, _)| !waiting(id))
                .map(|(_, weight)| weight.bytes)
                .sum::<usize>();
        let mut candidates: Vec<(bool, u64, u32, ImageId)> = (self.images.keys().copied())
            .filter_map(|id| {
                let used = self.used(id)?;
                // The frame before this one has its images on the screen.
                let last = used + 1 >= self.now;
                let free = !waiting(&id) && !marks.view.contains(&id) && !last;
                (free && self.holds(id)).then(|| (marks.near.contains(&id), used, id.value(), id))
            })
            .collect();
        candidates.sort_by_key(|&(near, used, value, _)| (near, used, value));
        let mut ids = Vec::new();
        for (_, _, _, id) in candidates {
            if total <= budget {
                break;
            }
            total -= self.images[&id].bytes;
            ids.push(id);
        }
        ids
    }

    /// Lets go of `ids`: no still or frame holds them any more, and they
    /// go to the unused, to be deleted from the terminal.
    pub(crate) fn let_go(&mut self, ids: &[ImageId]) {
        self.stills.retain(|_, held| !ids.contains(&held.id));
        self.frames.retain(|_, held| !ids.contains(&held.id));
        self.unuse(ids);
    }

    /// The terminal deleted `id`: its size and age are forgotten, unless
    /// a still or a frame holds it, or it waits to be deleted or is
    /// replaced through tmux, in which case the terminal still has it.
    pub(crate) fn forget(&mut self, id: ImageId) {
        let waiting = self.unused.contains(&id) || self.replaced.iter().any(|&(old, _)| old == id);
        if !waiting && !self.holds(id) {
            self.images.remove(&id);
        }
    }

    /// Every image the terminal may hold for `placement`: its still, and
    /// its frames, such as those of an animation kiri plays or the inset
    /// of a picture in a part of a page. Whether they were sent or are
    /// current does not matter.
    pub(crate) fn ids_of<'a>(
        &'a self,
        placement: &'a Placement,
    ) -> impl Iterator<Item = ImageId> + 'a {
        let frames = (self.frames.iter())
            .filter(move |((at, _, _), _)| at == placement)
            .map(|(_, held)| held);
        self.still(placement)
            .into_iter()
            .chain(frames)
            .map(|held| held.id)
    }

    /// The still image held for `placement`.
    pub(crate) fn still(&self, placement: &Placement) -> Option<&Held> {
        self.stills.get(placement)
    }

    /// Holds `held` for `placement`, giving back what it held before.
    pub(crate) fn put_still(&mut self, placement: Placement, held: Held) -> Option<Held> {
        self.shown_again(held.id);
        self.stills.insert(placement, held)
    }

    /// `id` is shown again: it is no longer one replaced.
    fn shown_again(&mut self, id: ImageId) {
        self.replaced.retain(|&(old, _)| old != id);
    }

    /// Notes the image `old` replaced by `by` through tmux, to be deleted
    /// once the cells that show `by` are on the screen.
    pub(crate) fn replace(&mut self, old: ImageId, by: ImageId) {
        if !self.replaced.contains(&(old, by)) {
            self.replaced.push((old, by));
        }
    }

    /// kitty answered the check of `id`, so tmux has drawn its cells: the
    /// images it replaced, which nothing shows, to delete now.
    pub(crate) fn answered(&mut self, id: ImageId) -> Vec<ImageId> {
        let mut due = Vec::new();
        self.replaced.retain(|&(old, by)| {
            if by == id && !due.contains(&old) {
                due.push(old);
            }
            by != id
        });
        due.retain(|&old| !self.holds(old) && !self.unused.contains(&old));
        due
    }

    /// The screen is drawn anew: every image replaced goes to the unused.
    fn release(&mut self) {
        let olds: Vec<ImageId> = self.replaced.drain(..).map(|(old, _)| old).collect();
        self.unuse(&olds);
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

    /// Holds `held` at `at`, giving back what it held before.
    pub(crate) fn put_frame(
        &mut self,
        at: (Placement, ImageKey, usize),
        held: Held,
    ) -> Option<Held> {
        self.shown_again(held.id);
        self.frames.insert(at, held)
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
    /// `cell`; the ids nothing holds any more go to the unused, and so do
    /// the images replaced: a fallback, as the answer that would release
    /// them may never come, and the next frame does not always draw every
    /// row.
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
        self.release();
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

    /// Everything held, bands and images replaced included, to delete:
    /// each id once.
    pub(crate) fn drain(&mut self) -> Vec<ImageId> {
        self.release();
        self.unused.extend(self.bands.drain().map(|(_, id)| id));
        self.images.clear();
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
    /// again when it is next shown. The next frame is drawn whole, so the
    /// images replaced go then; an answer for them may never come.
    pub(crate) fn lost(&mut self) {
        for held in self.stills.values_mut().chain(self.frames.values_mut()) {
            held.sent = false;
        }
        self.again.clear();
        self.release();
    }

    /// The terminal may have lost the images `ids`; the images replaced
    /// go with the next frame, as with [`Ledger::lost`].
    pub(crate) fn lost_some(&mut self, ids: &[ImageId]) {
        self.release();
        let held = self.stills.values_mut().chain(self.frames.values_mut());
        for held in held.filter(|held| ids.contains(&held.id)) {
            held.sent = false;
        }
        self.again.retain(|id, _| !ids.contains(id));
    }

    /// The terminal may have lost the image `id`: each still and frame
    /// with it is sent again when it is next shown. Unlike
    /// [`Ledger::lost_some`], its tries so far still count.
    pub(crate) fn unsent(&mut self, id: ImageId) {
        let held = self.stills.values_mut().chain(self.frames.values_mut());
        for held in held.filter(|held| held.id == id) {
            held.sent = false;
        }
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

    /// The terminal answered that it holds `id`: its tries start anew.
    pub(crate) fn held_by_terminal(&mut self, id: ImageId) {
        self.again.remove(&id);
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
mod tests;
