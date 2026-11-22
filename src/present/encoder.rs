//! Scales the images a layout shows and sends them through
//! `kitty::graphics`; what the terminal holds of them is kept in its
//! ledger (`src/present/ledger.rs`). The escape sequences themselves are
//! `kitty::graphics`'s; the encoder knows placements and ids.

use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use image::RgbaImage;
use image::imageops;

use crate::present::kitty::graphics::{self, Control, ImageId};
use crate::render::loading::{self, Ink};
use crate::typeset::caps::{Capabilities, Playback};
use crate::typeset::color::Color;
use crate::typeset::images::animation::{Animation, Plays};
use std::sync::Arc;

use crate::typeset::images::build::{FramesJob, framed};
use crate::typeset::images::marks::{self, State};
use crate::typeset::images::size::{Placement, Shows, Size, cell_size};
use crate::typeset::images::{ImageKey, Images, Placeholder, Source};
use crate::typeset::reading::animated::{Pic, Rect};
use crate::typeset::reading::engine::Parts;
use crate::typeset::reading::page::Overlay;

mod frames;
mod sixel;
use crate::present::ledger::{DRIVEN, Held, Kept, Ledger};
use crate::typeset::style::{ImageRow, Line};
use frames::Frames;

/// What the terminal is told to do with an animation, by its image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Stop, and go back to the first frame.
    Stop(ImageKey),
    /// Play from the first frame, `plays` times in all.
    Replay(ImageKey, Plays),
    /// Not for the terminal, but for the loop: have the frames of this
    /// picture built.
    Build(ImageKey),
    /// Not for the terminal, but for the loop: the build of this picture's
    /// frames is no longer wanted.
    Cancel(ImageKey),
}

impl Command {
    /// The image the command is for.
    pub fn key(&self) -> &ImageKey {
        match self {
            Command::Stop(key)
            | Command::Replay(key, _)
            | Command::Build(key)
            | Command::Cancel(key) => key,
        }
    }
}

/// The image of a picture that plays in a part of a page of reading mode,
/// laid over the cells of the part where the picture lies: its id, and
/// those cells as the part's columns and rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inset {
    pub id: ImageId,
    pub columns: Range<u16>,
    pub rows: Range<u16>,
}

/// The placements of the images `lines` show, whole lines and embeds.
fn placements(lines: &[Line]) -> impl Iterator<Item = &Placement> {
    lines
        .iter()
        .flat_map(|line| {
            let embeds = line.spans.iter().filter_map(|span| span.embed.as_ref());
            (line.image.as_ref().map(|image| &image.placement))
                .into_iter()
                .chain(embeds.map(|embed| &embed.placement))
        })
        .map(Rc::as_ref)
}

/// An image a walk over lines came to.
#[derive(Debug)]
pub struct Seen<'a> {
    pub placement: &'a Placement,
    pub row: Option<&'a ImageRow>,
    pub in_view: bool,
    pub held: Option<ImageId>,
}

/// What one area of the window shows, for [`Encoder::near`].
#[derive(Debug)]
pub struct Near<'a> {
    /// The lines of the area's layout that are near the view, which the
    /// budget keeps.
    pub lines: &'a [Line],
    /// The range of `lines` that is in view.
    pub visible: Range<usize>,
    /// The lines the frame draws for the area, painted as they are
    /// written: a part with a highlight is another placement than the same
    /// part without, and only these have it.
    pub drawn: &'a [Line],
}

/// The images the terminal holds, by where and how large they are shown.
/// One encoder can serve one layout after another, as the pager's does,
/// so that an image the terminal holds is not sent again.
#[derive(Debug, Default)]
pub struct Encoder {
    /// What the terminal holds of the images sent.
    ledger: Ledger,
    /// The pages of reading mode whose parts lines show, by their id,
    /// which draw a part when it is sent.
    pages: HashMap<u64, Rc<Parts>>,
    /// What is drawn over parts of those pages, by the hash in their keys.
    overlays: Overlays,
    /// The overlays the frame being drawn has used; those of the frames
    /// before are dropped when it is done.
    drawn: HashSet<(u64, u64)>,
    /// Who plays the animated images.
    playback: Playback,
    /// The frame each of those animations shows now, by picture.
    shown_frames: HashMap<ImageKey, usize>,
    /// Where each animation is in its life, which decides what is drawn in
    /// its picture.
    marks: HashMap<ImageKey, State>,
    /// The frames of pictures that were built off the loop, by the
    /// placement they are for and the picture.
    built: HashMap<(Placement, ImageKey), Rc<Vec<RgbaImage>>>,
    /// The animated images sent with their frames, which the terminal
    /// holds stopped on the first.
    fresh: Vec<ImageKey>,
    /// The slices of the rows of images, for a terminal that draws sixel.
    sixel: sixel::Cache,
    /// The most bytes of images the terminal is to hold, if any.
    budget: Option<usize>,
    /// The images of the frame about to be drawn and those near it.
    kept: Kept,
}

impl Encoder {
    /// An encoder that has sent nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the most bytes of images the terminal is to hold, as sent, or
    /// none. The budget is soft: [`Encoder::delete_unused`] lets go of
    /// images past it before a frame's own sends, so the terminal may be
    /// over by what one frame sends, and a screen that is over it alone
    /// stays.
    pub fn set_budget(&mut self, bytes: Option<usize>) {
        self.budget = bytes;
    }

    /// The budget set by [`Encoder::set_budget`].
    #[cfg(test)]
    pub(crate) fn budget(&self) -> Option<usize> {
        self.budget
    }

    /// Tells which images the frame about to be drawn shows and which are
    /// near them, for the `areas` of the window; the budget keeps these
    /// ([`Encoder::set_budget`]). The marks serve the next
    /// [`Encoder::delete_unused`] only.
    pub fn near(&mut self, areas: &[Near<'_>], _images: &Images, _caps: &Capabilities) {
        // Without a budget, or under it, nothing is let go, so nothing needs
        // marking: nothing is weighed between here and the delete.
        match self.budget {
            Some(budget) if self.ledger.bytes() > budget => {}
            _ => return,
        }
        let mut kept = Kept::default();
        let ids = |lines: &[Line], visible: Range<usize>, into: &mut HashSet<ImageId>| {
            for seen in self.placed(lines, visible) {
                // Whatever the terminal may hold for it, sent or not, frames
                // included.
                into.extend(self.ledger.ids_of(seen.placement));
            }
        };
        for area in areas {
            ids(area.lines, area.visible.clone(), &mut kept.near);
            ids(area.drawn, 0..area.drawn.len(), &mut kept.view);
        }
        self.kept = kept;
    }

    /// The id of the image `placement` shows, from `images`, on a terminal
    /// with `caps`: sent to `out` first unless the terminal holds it. An
    /// image sent for `placement` before, which the pixels no longer
    /// give, is deleted: at once, but inside tmux later, when kitty has
    /// answered the check of the new one. For a part of a page with a
    /// picture that plays, the part under the picture's inset, which
    /// [`Encoder::part_id`] gives too.
    pub fn id(
        &mut self,
        placement: &Placement,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        Ok(self.part_id(placement, images, caps, out)?.0)
    }

    /// [`Encoder::id`] for a row of an image shown as a block: the id of
    /// what the cells show, and for a part of a page with a picture that
    /// plays, the inset of that picture, which goes over the cells it lies on.
    pub fn part_id(
        &mut self,
        placement: &Placement,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<(ImageId, Option<Inset>)> {
        // A picture that plays is shown as its frames: those of the image the
        // terminal plays, sent when the reader starts it and not before, or,
        // for what kiri plays, the image of the current frame. One that is
        // stopped or has ended is a still image with its mark.
        let playing = (self.states_in(placement, images).into_iter())
            .find_map(|(picture, state)| (state == State::Playing).then_some(picture));
        let page = matches!(placement.key, ImageKey::Page { .. });
        let index = (playing.as_ref()).map_or(0, |picture| {
            self.shown_frames.get(picture).copied().unwrap_or(0)
        });
        match (playing, self.playback) {
            // In a part of a page the picture is an image of its own, the
            // size of the cells it lies on, over the part, which stands as it
            // would with the picture stopped.
            (Some(picture), Playback::Redraw | Playback::Terminal) if page => {
                let id = self.still_id(placement, images, caps, out)?;
                let Some(rect) = self.rect_of(placement, &picture, images) else {
                    return Ok((id, None));
                };
                let inset = match self.playback {
                    Playback::Redraw => {
                        self.frame_id(placement, (&picture, index), Some(&rect), images, caps, out)?
                    }
                    _ => self.driven_id(placement, &picture, Some(&rect), images, caps, out)?,
                };
                let (columns, rows) = (rect.columns, rect.rows);
                Ok((
                    id,
                    Some(Inset {
                        id: inset,
                        columns,
                        rows,
                    }),
                ))
            }
            (Some(picture), Playback::Redraw) => {
                let id = self.frame_id(placement, (&picture, index), None, images, caps, out)?;
                Ok((id, None))
            }
            (Some(picture), Playback::Terminal) => {
                let id = self.driven_id(placement, &picture, None, images, caps, out)?;
                Ok((id, None))
            }
            _ => Ok((self.still_id(placement, images, caps, out)?, None)),
        }
    }

    /// Where `picture` lies in the part of a page `placement` shows, spread
    /// out to whole cells; `None` when nothing of it is drawn there.
    fn rect_of(&self, placement: &Placement, picture: &ImageKey, images: &Images) -> Option<Rect> {
        let ImageKey::Page { page, slice, .. } = placement.key else {
            return None;
        };
        let parts = self.pages.get(&page)?;
        let pictures = parts.animated(images);
        let here = parts.in_part(slice, &pictures);
        let (pic, top) = here.iter().find(|(pic, _)| &pic.key == picture)?;
        parts.rect(slice, pic, *top)
    }

    /// The states the pictures are drawn in under the inset of a picture
    /// that plays, in a part of a page: as they are, with the one that
    /// plays as though it were stopped, its play button hidden by the
    /// inset. A part that stops is then the part the terminal already holds.
    fn still_marks(&self, placement: &Placement) -> HashMap<ImageKey, State> {
        // `Playing` implies playback `Terminal` or `Redraw`.
        let page = matches!(placement.key, ImageKey::Page { .. });
        (self.marks.iter())
            .map(|(key, state)| match (page, state) {
                (true, State::Playing) => (key.clone(), State::Stopped),
                _ => (key.clone(), *state),
            })
            .collect()
    }

    /// The animated pictures `placement` shows with their states: the
    /// picture itself, or those in a part of a page.
    fn states_in(&self, placement: &Placement, images: &Images) -> Vec<(ImageKey, State)> {
        self.states_with(placement, images, &self.marks)
    }

    /// [`Encoder::states_in`] with the states `marks`.
    fn states_with(
        &self,
        placement: &Placement,
        images: &Images,
        marks: &HashMap<ImageKey, State>,
    ) -> Vec<(ImageKey, State)> {
        if marks.is_empty() || placement.shows != Shows::Image {
            return Vec::new();
        }
        let keys = match placement.key {
            ImageKey::Page { page, slice, .. } => {
                let Some(parts) = self.pages.get(&page) else {
                    return Vec::new();
                };
                let pictures = parts.animated(images);
                let here = parts.in_part(slice, &pictures);
                here.into_iter().map(|(pic, _)| pic.key).collect()
            }
            ImageKey::Band { .. } => Vec::new(),
            _ => vec![placement.key.clone()],
        };
        (keys.into_iter())
            .filter_map(|key| Some((key.clone(), *marks.get(&key)?)))
            .collect()
    }

    /// What the frames of `picture` are made from, for each placement the
    /// encoder has sent that shows it, as owned data for a thread of its own:
    /// the jobs, each with the placement it is for.
    pub fn frame_jobs(
        &self,
        picture: &ImageKey,
        images: &Images,
        caps: &Capabilities,
    ) -> Vec<(Placement, FramesJob)> {
        let cell = cell_size(caps);
        let mut jobs = Vec::new();
        for placement in self.ledger.still_placements() {
            if !self
                .states_in(placement, images)
                .iter()
                .any(|(key, _)| key == picture)
            {
                continue;
            }
            if let Some(job) = self.frames_job(placement, picture, images, cell, caps) {
                jobs.push((placement.clone(), job));
            }
        }
        jobs
    }

    /// The job that builds the frames of `picture` for `placement`.
    fn frames_job(
        &self,
        placement: &Placement,
        picture: &ImageKey,
        images: &Images,
        cell: (u16, u16),
        caps: &Capabilities,
    ) -> Option<FramesJob> {
        let selected = frame_of(placement, caps);
        match placement.key {
            ImageKey::Page {
                page,
                slice,
                overlay,
                ..
            } => {
                let parts = self.pages.get(&page)?;
                let overlay = self.overlays.get(&(page, overlay)).map(Rc::as_ref);
                let pictures = parts.animated(images);
                let here = parts.in_part(slice, &pictures);
                let (pic, top) = here.iter().find(|(pic, _)| &pic.key == picture)?;
                // The part as it is with the picture left out, and the frame
                // round the selected picture if that is another.
                let drawn = |other: &Pic| match other.key == *picture {
                    true => (usize::MAX, None),
                    false => frames::static_look(&self.marks, other),
                };
                let chosen = selected_number(placement, &here);
                let this = here.iter().position(|(other, _)| &other.key == picture);
                let line = line_of(cell);
                let others = (selected.as_ref().zip(chosen))
                    .filter(|(_, number)| Some(*number) != this)
                    .map(|((rgb, _), number)| (number, *rgb, line));
                // Only where the picture lies: its frames are an inset over
                // the part.
                let rect = parts.rect(slice, pic, *top)?;
                let base = parts.draw_frame(slice, overlay, &pictures, &drawn, others);
                let base = imageops::crop_imm(&base, rect.x, rect.y, rect.width, rect.height);
                // And round this one, in each frame, when it is selected.
                let mine = (selected.as_ref().zip(chosen))
                    .filter(|(_, number)| Some(*number) == this)
                    .map(|((rgb, _), _)| (*rgb, line));
                Some(FramesJob::Part {
                    animation: Arc::clone(&pic.animation),
                    base: base.to_image(),
                    at: (
                        pic.x.round() as i64 - i64::from(rect.x),
                        top.round() as i64 - i64::from(rect.y),
                    ),
                    size: (pic.width, pic.height),
                    outline: mine,
                })
            }
            _ => {
                let Some(Ok(Source::Animated(animation))) = images.get(&placement.key) else {
                    return None;
                };
                let size = placement.size;
                Some(FramesJob::Image {
                    animation: Arc::clone(animation),
                    canvas: (
                        u32::from(size.columns) * u32::from(cell.0),
                        u32::from(size.rows) * u32::from(cell.1),
                    ),
                    at: (i64::from(placement.at.0), i64::from(placement.at.1)),
                    size: (size.width, size.height),
                    selected: selected.map(|(rgb, rows)| (rgb, rows, cell.1)),
                })
            }
        }
    }

    /// Takes the frames built for `picture` at `placement`, to send them
    /// when the picture plays.
    pub fn install_frames(
        &mut self,
        placement: Placement,
        picture: ImageKey,
        frames: Vec<RgbaImage>,
    ) {
        self.built.insert((placement, picture), Rc::new(frames));
    }

    /// Tells the encoder where each animation is in its life, which decides
    /// what is drawn in its picture.
    pub fn show_marks(&mut self, marks: HashMap<ImageKey, State>) {
        self.marks = marks;
    }

    /// The id of the image of `picture` with every frame: for a picture
    /// shown as an image, the image; for one in a part of a page, its inset,
    /// the part where `rect` says. Sent to `out` first unless the terminal
    /// holds it as drawn with the states the pictures in the part have now.
    fn driven_id(
        &mut self,
        placement: &Placement,
        picture: &ImageKey,
        rect: Option<&Rect>,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        let cell = cell_size(caps);
        let at = (placement.clone(), picture.clone(), DRIVEN);
        let states = self.state_hash(placement, images, &self.marks);
        let held = self.ledger.frame(&at).filter(|held| held.cell == cell);
        let current = |held: &&Held| held.checked && held.sent && held.states == states;
        if let Some(held) = held.filter(current) {
            let id = held.id;
            self.ledger.touch(id);
            return Ok(id);
        }
        let pages = (&self.pages, &self.overlays);
        let frame = frame_of(placement, caps);
        let marks = &self.marks;
        let built = self.built.get(&(placement.clone(), picture.clone()));
        let built = built.map(|frames| frames.as_slice());
        let mut scaled = match rect {
            Some(rect) => {
                let driver = (Some(picture), marks);
                let first = match built.and_then(|built| built.first()) {
                    Some(first) => Some(first.clone()),
                    None => Frames::of(placement, images, pages, cell, caps, driver)
                        .map(|frames| frames.pixels(0)),
                };
                let first = first.ok_or_else(|| no_frames(picture))?;
                Scaled::of_frame(cells_of(rect), first, caps)
            }
            None => Scaled::new(placement, images, pages, cell, caps.image_run, frame, marks)?,
        };
        let driver = (Some(picture), marks, built);
        scaled.add_frames(placement, images, pages, cell, caps, driver);
        let held = Held {
            id: scaled.id,
            cell,
            checked: true,
            sent: true,
            states,
        };
        let old = self.ledger.put_frame(at, held);
        self.ledger.weigh(scaled.id, scaled.bytes());
        if let Some(old) = old {
            // An inset whose pixels came out the same, as when only a
            // picture outside it changed, is the one the terminal holds.
            if rect.is_some() && old.id == scaled.id && old.sent {
                return Ok(scaled.id);
            }
            self.let_go(old.id, scaled.id, caps, out)?;
        }
        self.sending(&scaled, caps);
        scaled.send(out, caps.effort())?;
        if !scaled.later.is_empty() {
            self.fresh.push(picture.clone());
        }
        Ok(scaled.id)
    }

    /// The states `marks` of the animated pictures `placement` shows,
    /// hashed: the pixels are other pixels when they change.
    fn state_hash(
        &self,
        placement: &Placement,
        images: &Images,
        marks: &HashMap<ImageKey, State>,
    ) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.states_with(placement, images, marks).hash(&mut hasher);
        hasher.finish()
    }

    /// [`Encoder::id`] for what shows as one image.
    fn still_id(
        &mut self,
        placement: &Placement,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        let cell = cell_size(caps);
        let marks = self.still_marks(placement);
        let states = self.state_hash(placement, images, &marks);
        if let Some(id) = self.held_with(placement, cell, states) {
            self.ledger.touch(id);
            return Ok(id);
        }
        let scaled = Scaled::new(
            placement,
            images,
            (&self.pages, &self.overlays),
            cell,
            caps.image_run,
            frame_of(placement, caps),
            &marks,
        )?;
        let held = Held {
            id: scaled.id,
            cell,
            checked: true,
            sent: true,
            states,
        };
        let old = self.ledger.put_still(placement.clone(), held);
        self.ledger.weigh(scaled.id, scaled.bytes());
        if let Some(old) = old {
            if old.id == scaled.id && old.sent {
                return Ok(scaled.id);
            }
            self.let_go(old.id, scaled.id, caps, out)?;
        }
        self.send_marked(&scaled, caps, out)?;
        Ok(scaled.id)
    }

    /// The bytes the terminal holds for the images this has sent, as sent.
    pub fn image_bytes(&self) -> usize {
        self.ledger.bytes()
    }

    /// The image the terminal holds for `placement`, if any, without
    /// drawing it: the id of a still that is sent and is of the pixels as
    /// they are now.
    pub fn held(
        &self,
        placement: &Placement,
        images: &Images,
        caps: &Capabilities,
    ) -> Option<ImageId> {
        let cell = cell_size(caps);
        let marks = self.still_marks(placement);
        let states = self.state_hash(placement, images, &marks);
        self.held_with(placement, cell, states)
    }

    /// [`Encoder::held`] for the `cell` and `states` worked out.
    fn held_with(&self, placement: &Placement, cell: (u16, u16), states: u64) -> Option<ImageId> {
        let old = self
            .ledger
            .still(placement)
            .filter(|held| held.cell == cell);
        let current = |held: &&Held| held.checked && held.sent && held.states == states;
        old.filter(current).map(|held| held.id)
    }

    /// Walks the `lines`, of which the range `visible` is on the screen,
    /// and gives each image placed once, those in view first and then
    /// those near it, the nearest first, each with the id the terminal
    /// holds for it, if any. A line outside `lines` is not asked about. A
    /// part of a page spans many lines, so repeats are dropped; it is in
    /// view when any of its lines is.
    pub fn walk<'a>(
        &self,
        lines: &'a [Line],
        visible: Range<usize>,
        images: &Images,
        caps: &Capabilities,
    ) -> Vec<Seen<'a>> {
        (self.placed(lines, visible).into_iter())
            .map(|mut seen| {
                seen.held = self.held(seen.placement, images, caps);
                seen
            })
            .collect()
    }

    /// [`Encoder::walk`] without the ids held: each image placed once, in
    /// view first and then the nearest first, with `held` left empty.
    fn placed<'a>(&self, lines: &'a [Line], visible: Range<usize>) -> Vec<Seen<'a>> {
        // The nearest line of each placement, with the first line that is
        // as near and the row of the line that has one.
        let mut nearest: HashMap<&Placement, (usize, usize)> = HashMap::new();
        let mut order: Vec<(&Placement, Option<&ImageRow>)> = Vec::new();
        for (at, line) in lines.iter().enumerate() {
            let distance = if at < visible.start {
                visible.start - at
            } else if at >= visible.end {
                at - visible.end + 1
            } else {
                0
            };
            let row = line.image.as_deref();
            let embeds = line.spans.iter().filter_map(|span| span.embed.as_ref());
            let row_placement = row.map(|row| (row.placement.as_ref(), row));
            let embeds = embeds.map(|embed| (embed.placement.as_ref(), None));
            for (placement, row) in row_placement
                .map(|(p, r)| (p, Some(r)))
                .into_iter()
                .chain(embeds)
            {
                let entry = nearest.entry(placement).or_insert_with(|| {
                    order.push((placement, row));
                    (distance, order.len() - 1)
                });
                if distance < entry.0 {
                    entry.0 = distance;
                }
            }
        }
        let mut seen: Vec<(usize, usize, Seen<'a>)> = (order.into_iter().enumerate())
            .map(|(first, (placement, row))| {
                let distance = nearest[placement].0;
                (
                    distance,
                    first,
                    Seen {
                        placement,
                        row,
                        in_view: distance == 0,
                        held: None,
                    },
                )
            })
            .collect();
        seen.sort_by_key(|(distance, first, _)| (*distance, *first));
        seen.into_iter().map(|(_, _, seen)| seen).collect()
    }

    /// Asks the terminal whether it still holds the images shown by the
    /// `lines` that are on the screen, each once: those the ledger has as
    /// sent, which the terminal answers with [`graphics::checked`]'s answer.
    /// One not sent is skipped: the next draw sends it.
    pub fn check_view(
        &self,
        lines: &[Line],
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let mut asked: Vec<ImageId> = Vec::new();
        for seen in self.walk(lines, 0..lines.len(), images, caps) {
            let Some(id) = seen.held.filter(|id| !asked.contains(id)) else {
                continue;
            };
            asked.push(id);
            graphics::check(out, id, cells_of_size(seen.placement.size))?;
        }
        Ok(())
    }

    /// Deletes `old`, which `new` replaced in the ledger, unless they are
    /// one or something else holds `old`: at once, or through tmux once
    /// kitty has answered the check of `new`.
    fn let_go(
        &mut self,
        old: ImageId,
        new: ImageId,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<()> {
        // The old id is known to be stale only now that the image has been
        // scaled again.
        if old == new || self.ledger.holds(old) {
            return Ok(());
        }
        if caps.tmux_passthrough {
            // tmux passes a delete on at once but draws the cells of this
            // frame only after it: until then they would show the image
            // deleted, which is nothing. It goes once kitty has answered
            // the check of the new one.
            self.ledger.replace(old, new);
        } else if let Err(error) = graphics::delete(out, old) {
            // At once, not queued as `keep` does. The old id is out of the
            // ledger by now: a delete that fails is put back to be tried
            // again.
            self.ledger.unuse(&[old]);
            return Err(error);
        } else {
            self.ledger.forget(old);
        }
        Ok(())
    }

    /// Sets who plays the animated images: when the terminal does, each is
    /// sent with every frame; otherwise only its first frame is sent.
    pub fn play(&mut self, playback: Playback) {
        self.playback = playback;
    }

    /// Tells the terminal what to do with the animation `command` names,
    /// for each placement of the image that it holds.
    pub fn control(&mut self, command: &Command, out: &mut impl Write) -> io::Result<()> {
        // What kiri plays itself needs no word to the terminal.
        if self.playback == Playback::Redraw {
            return Ok(());
        }
        let (key, controls) = match command {
            // Stopped on the frame it is on, and then back on the first.
            Command::Stop(key) => (key, vec![Control::Stop, Control::Show(1)]),
            Command::Replay(key, plays) => (key, vec![Control::Replay(counted(*plays))]),
            // Asks of the loop, not of the terminal.
            Command::Build(_) | Command::Cancel(_) => return Ok(()),
        };
        // The images that were sent with the picture's frames: a still image
        // has no animation to tell.
        for id in self.ledger.driven(key) {
            for control in &controls {
                graphics::control(out, id, *control)?;
            }
        }
        Ok(())
    }

    /// Tells which frame each animation that kiri plays itself shows now:
    /// [`Encoder::id`] gives the id of that frame's image.
    pub fn show_frames(&mut self, frames: HashMap<ImageKey, usize>) {
        self.shown_frames = frames;
    }

    /// The id of frame `index` of `picture`, the animation
    /// `placement` shows, sent to `out` first unless the terminal holds
    /// it. Each frame is a still image of its own: of the cells of the
    /// placement, or for a picture in a part of a page, of those `rect`
    /// says, as its inset.
    fn frame_id(
        &mut self,
        placement: &Placement,
        (picture, index): (&ImageKey, usize),
        rect: Option<&Rect>,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        let cell = cell_size(caps);
        let at = (placement.clone(), picture.clone(), index);
        let states = self.state_hash(placement, images, &self.marks);
        let held = self.ledger.frame(&at).filter(|held| held.cell == cell);
        let current = |held: &&Held| held.checked && held.sent && held.states == states;
        if let Some(held) = held.filter(current) {
            let id = held.id;
            self.ledger.touch(id);
            return Ok(id);
        }
        let pages = (&self.pages, &self.overlays);
        let driver = (Some(picture), &self.marks);
        let Some(frames) = Frames::of(placement, images, pages, cell, caps, driver) else {
            if rect.is_some() {
                return Err(no_frames(picture));
            }
            return self.still_id(placement, images, caps, out);
        };
        let last = frames.animation.frames.len() - 1;
        let built = self.built.get(&(placement.clone(), picture.clone()));
        let pixels = match built.and_then(|built| built.get(index.min(last))) {
            Some(pixels) => pixels.clone(),
            None => frames.pixels(index.min(last)),
        };
        let cells = rect.map_or_else(|| cells_of_size(placement.size), cells_of);
        let scaled = Scaled::of_frame(cells, pixels, caps);
        let held = Held {
            id: scaled.id,
            cell,
            checked: true,
            sent: true,
            states,
        };
        // The same pixels under another frame, or placement, are held.
        let sent = self.ledger.holds_sent(scaled.id);
        let old = self.ledger.put_frame(at, held);
        self.ledger.weigh(scaled.id, scaled.bytes());
        if let Some(old) = old {
            self.let_go(old.id, scaled.id, caps, out)?;
        }
        if !sent {
            self.send_marked(&scaled, caps, out)?;
        }
        Ok(scaled.id)
    }

    /// The images that were sent with their frames since this was last
    /// asked: the terminal has each stopped on its first frame.
    pub fn take_fresh(&mut self) -> Vec<ImageKey> {
        std::mem::take(&mut self.fresh)
    }

    /// Takes `parts` as the page that draws the parts of reading mode's
    /// text that `key` names, for [`Encoder::id`] to send.
    pub fn page(&mut self, key: &ImageKey, parts: &Rc<Parts>, overlay: Option<&Rc<Overlay>>) {
        if let ImageKey::Page { page, .. } | ImageKey::Band { page, .. } = key {
            self.pages.entry(*page).or_insert_with(|| Rc::clone(parts));
        }
        if let (
            ImageKey::Page {
                page,
                overlay: hash,
                ..
            }
            | ImageKey::Band {
                page,
                overlay: hash,
                ..
            },
            Some(overlay),
        ) = (key, overlay)
        {
            // By page too: two pages may have the same marks in a part.
            self.drawn.insert((*page, *hash));
            self.overlays
                .entry((*page, *hash))
                .or_insert_with(|| Rc::clone(overlay));
        }
    }

    /// The overlay named by `hash` on page `page`.
    #[cfg(test)]
    fn overlay_of(&self, page: u64, hash: u64) -> Option<&Rc<Overlay>> {
        self.overlays.get(&(page, hash))
    }

    /// The id of the band `columns` cells of `cell` pixels wide, in
    /// `ink`, which is on the screen until it is next cleared: sent to
    /// `out` first, as it is `elapsed` after its start, unless the
    /// terminal holds it since the screen was last cleared. A band moves
    /// on as each frame sends it anew: its pixels cannot be changed where
    /// it is placed, as the terminal blends new pixels over the old ones.
    pub fn band(
        &mut self,
        columns: u16,
        cell: (u16, u16),
        ink: Ink,
        elapsed: Duration,
        effort: graphics::Effort,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        if let Some(id) = self.ledger.band(columns, cell) {
            return Ok(id);
        }
        let band = loading::band(columns, cell, ink, elapsed);
        let cells = graphics::Cells { columns, rows: 1 };
        // Under tmux, where ids are a run's own, no band is drawn.
        let id = ImageId::new(BAND, band.width(), band.height(), cells, None);
        graphics::transmit_only(out, id, &pixels(&band), effort)?;
        self.ledger.put_band(columns, cell, id);
        Ok(id)
    }

    /// Tells the encoder that the screen was cleared: no band is on it
    /// until one is drawn again, and the terminal no longer holds the
    /// bands, which nothing places any more.
    pub fn cleared(&mut self) {
        self.ledger.cleared();
    }

    /// Whether a band is on the screen.
    pub fn has_bands(&self) -> bool {
        self.ledger.has_bands()
    }

    /// Keeps only the images `lines` place on a terminal with `caps`, as
    /// the lines of a new layout do: the others are deleted by the next
    /// [`Encoder::delete_unused`]. An image is placed by a line whether
    /// or not the line is in view.
    pub fn keep(&mut self, lines: &[Line], caps: &Capabilities) {
        self.keep_views(lines, &[], caps);
    }

    /// As [`Encoder::keep`] for the lines `top` of the layout shown, and
    /// the images that the lines `below` place, of the layouts of views it
    /// covers, are held as well: the terminal keeps them, so that they are
    /// not sent again when the view is back. What is built or drawn for a
    /// layout only, the frames and the pages, is kept for `top`'s images
    /// alone.
    pub fn keep_views(&mut self, top: &[Line], below: &[&[Line]], caps: &Capabilities) {
        let cell = cell_size(caps);
        let placed: HashSet<&Placement> = placements(top).collect();
        let bands: HashSet<u16> = (top.iter())
            .filter_map(|line| line.loading.as_ref()?.band.as_ref())
            .map(|band| band.columns)
            .collect();
        let pages: HashSet<u64> = (placed.iter())
            .filter_map(|placement| match placement.key {
                ImageKey::Page { page, .. } | ImageKey::Band { page, .. } => Some(page),
                _ => None,
            })
            .collect();
        self.pages.retain(|page, _| pages.contains(page));
        self.built
            .retain(|(placement, _), _| placed.contains(placement));
        // The terminal holds the images of the layouts below too.
        let mut held = placed;
        held.extend(below.iter().flat_map(|lines| placements(lines)));
        self.ledger.keep(&held, &bands, cell);
    }

    /// Deletes from the terminal the images [`Encoder::keep`] let go.
    pub fn delete_unused(&mut self, out: &mut impl Write) -> io::Result<()> {
        self.end_frame();
        // Past the budget, images go that the frame does not draw, even
        // through tmux: they are not on the screen, so there is no cell to
        // wait for.
        let kept = std::mem::take(&mut self.kept);
        if let Some(budget) = self.budget {
            let over = self.ledger.over(budget, 0, &kept);
            self.ledger.let_go(&over);
        }
        let ids = self.ledger.take_unused();
        self.delete(&ids, out)
    }

    /// Deletes `ids`, which nothing holds, from the terminal: those a write
    /// that fails does not delete are put back among the unused.
    fn delete(&mut self, ids: &[ImageId], out: &mut impl Write) -> io::Result<()> {
        for (at, id) in ids.iter().enumerate() {
            if let Err(error) = graphics::delete(out, *id) {
                self.ledger.unuse(&ids[at..]);
                return Err(error);
            }
            self.ledger.forget(*id);
        }
        Ok(())
    }

    /// Tells the encoder that kitty answered the check of image `id`, which
    /// it does after tmux has drawn the cells that show it: the images it
    /// replaced through tmux are deleted now.
    pub fn answered(&mut self, id: ImageId, out: &mut impl Write) -> io::Result<()> {
        let ids = self.ledger.answered(id);
        self.delete(&ids, out)
    }

    /// Tells the encoder that kitty answered it holds image `id`: the
    /// tries to send it again for being lost start anew, so the cap of
    /// [`AGAIN`] means failures in a row.
    pub fn held_by_terminal(&mut self, id: ImageId) {
        self.ledger.held_by_terminal(id);
    }

    /// Tells the encoder that the pixels in the images it is given may
    /// have changed, as after a reload: each image is scaled again when it
    /// is next shown, and sent again only when that gives another id.
    pub fn changed(&mut self) {
        self.ledger.changed();
    }

    /// The images sent through tmux since this was last asked, with the
    /// cells they were sent for, so that a check can tell whether they
    /// came: tmux drops output for a client that cannot keep up. Empty
    /// without tmux.
    pub fn take_sent(&mut self) -> Vec<(ImageId, graphics::Cells)> {
        self.ledger.take_sent()
    }

    /// Sends image `id` again, from `images`, on a terminal with `caps`,
    /// as the terminal answered it does not hold it: a transmission only,
    /// as its placeholders are on the screen. A frame or an inset is sent
    /// by the next draw that shows it. Nothing when no placement shows the
    /// image any more, or after [`AGAIN`] tries.
    pub fn send_again(
        &mut self,
        id: ImageId,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<()> {
        if !self.ledger.may_try_again(id) {
            return Ok(());
        }
        let cell = cell_size(caps);
        let placement = self.ledger.placement_of(id, cell);
        if placement.is_none() && self.ledger.holds(id) {
            // A frame, or an inset: the next draw that shows it sends it.
            if self.ledger.try_again(id) {
                self.ledger.unsent(id);
            }
            return Ok(());
        }
        // Pixels that are gone, or that changed since, are no longer the
        // image's: the next frame sends what it shows.
        let scaled = placement.and_then(|placement| {
            Scaled::new(
                &placement,
                images,
                (&self.pages, &self.overlays),
                cell,
                caps.image_run,
                frame_of(&placement, caps),
                &self.still_marks(&placement),
            )
            .ok()
        });
        let Some(scaled) = scaled.filter(|scaled| scaled.id == id) else {
            return Ok(());
        };
        if !self.ledger.try_again(id) {
            return Ok(());
        }
        self.sending(&scaled, caps);
        scaled.send(out, caps.effort())
    }

    /// Sends `scaled` to `out` and marks every still and frame with its id
    /// as sent: another placement of the same image has it sent too.
    fn send_marked(
        &mut self,
        scaled: &Scaled,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<()> {
        self.ledger.mark_sent(scaled.id);
        self.sending(scaled, caps);
        scaled.send(out, caps.effort())
    }

    /// Notes that `scaled` is sent to a terminal with `caps`, to check
    /// that it came when it goes through tmux.
    fn sending(&mut self, scaled: &Scaled, caps: &Capabilities) {
        self.ledger
            .sending(scaled.id, scaled.cells, caps.tmux_passthrough);
    }

    /// Tells the encoder that the terminal may have lost the images it was
    /// sent, as tmux drops what goes to a pane that is not shown: each is
    /// sent again when it is next shown.
    pub fn lost(&mut self) {
        self.ledger.lost();
    }

    /// Tells the encoder that the terminal may have lost the images
    /// `ids`, as [`Encoder::lost`] does for all of them.
    pub fn lost_some(&mut self, ids: &[ImageId]) {
        self.ledger.lost_some(ids);
    }

    /// Deletes from the terminal every image it holds, as when the pager
    /// quits.
    pub fn delete_all(&mut self, out: &mut impl Write) -> io::Result<()> {
        self.end_frame();
        for id in self.ledger.drain() {
            graphics::delete(out, id)?;
        }
        Ok(())
    }

    /// Lets go of every image as the pager quits: sixel leaves nothing in
    /// the terminal to delete, other images are deleted from it.
    pub fn close(&mut self, out: &mut impl Write, sixel: bool) -> io::Result<()> {
        if sixel {
            self.discard_all();
            return Ok(());
        }
        self.delete_all(out)
    }

    /// A new frame starts: only the overlays the last one drew are kept,
    /// as the parts it showed are drawn anew from them.
    fn end_frame(&mut self) {
        self.ledger.tick();
        let drawn = std::mem::take(&mut self.drawn);
        self.overlays.retain(|hash, _| drawn.contains(hash));
    }
}

/// An image scaled to the cells it covers, and its id.
struct Scaled {
    id: ImageId,
    pixels: RgbaImage,
    cells: graphics::Cells,
    /// For an animation the terminal plays: how long the first frame
    /// stays, and the frames after it, each with how long it stays.
    first_gap: Duration,
    later: Vec<(RgbaImage, Duration)>,
}

/// The number, among the pictures `here` in a part, of the one that the
/// frame of `placement` goes round.
fn selected_number(placement: &Placement, here: &[(Pic, f32)]) -> Option<usize> {
    let picture = placement.frame.as_ref()?.picture.as_ref()?;
    here.iter().position(|(pic, _)| &pic.key == picture)
}

/// The thickness of a frame round an image on cells of `cell` pixels.
fn line_of(cell: (u16, u16)) -> u32 {
    ((u32::from(cell.1) + 10) / 20).max(1)
}

/// The pages of reading mode, by their id, and what is drawn over parts of
/// them, by the hash in their keys.
type Pages = HashMap<u64, Rc<Parts>>;
type Overlays = HashMap<(u64, u64), Rc<Overlay>>;

impl Scaled {
    /// The image `placement` shows, from `images`, scaled to its size in
    /// cells of `cell` pixels, with an id of `run`'s own when given.
    fn new(
        placement: &Placement,
        images: &Images,
        (pages, overlays): (&Pages, &Overlays),
        cell: (u16, u16),
        run: Option<u64>,
        frame: Option<([u8; 3], Range<u16>)>,
        marks: &HashMap<ImageKey, State>,
    ) -> io::Result<Self> {
        let key = &placement.key;
        let size = placement.size;
        let mut outlined = false;
        let drawn = match *key {
            ImageKey::Page {
                page,
                slice,
                overlay,
                ..
            } => (pages.get(&page)).map(|parts| {
                let overlay = overlays.get(&(page, overlay)).map(Rc::as_ref);
                // A part with animated pictures has each in the state it is in:
                // the first frame with a play button, its last with a replay
                // icon, or, for one that plays, its first; and the frame
                // round the picture.
                let pictures = parts.animated(images);
                // The frame goes round the picture that is selected.
                let chosen = selected_number(placement, &parts.in_part(slice, &pictures));
                let outline = (frame.as_ref().zip(chosen))
                    .map(|((rgb, _), number)| (number, *rgb, line_of(cell)));
                outlined = outline.is_some() && !parts.in_part(slice, &pictures).is_empty();
                match parts.in_part(slice, &pictures).is_empty() {
                    true => parts.draw(slice, overlay),
                    false => {
                        let drawn = |pic: &Pic| frames::static_look(marks, pic);
                        parts.draw_frame(slice, overlay, &pictures, &drawn, outline)
                    }
                }
            }),
            ImageKey::Band {
                page,
                band,
                slice,
                offset,
                selected,
                overlay,
                ..
            } => (pages.get(&page)).map(|parts| {
                let over = overlays.get(&(page, overlay)).map(Rc::as_ref);
                parts.draw_band(band, slice, offset, selected, over)
            }),
            _ => None,
        };
        let pixels = match (placement.shows, images.get(key), images.placeholder(key)) {
            _ if let Some(pixels) = drawn => pixels,
            (Shows::Image, Some(Ok(source)), _) => fit(source, size, placement.at, cell),
            (Shows::Placeholder, _, Some(Placeholder::Dimmed(old))) => {
                loading::dimmed(fit(old, size, placement.at, cell))
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("no pixels for image {key}"),
                ));
            }
        };
        let mut pixels = pixels;
        // A picture that is an animation shows the frame of its state, and
        // the mark over it.
        if let (Shows::Image, Some(Ok(Source::Animated(animation))), Some(state)) =
            (placement.shows, images.get(key), marks.get(key))
        {
            let (width, height) = (size.width, size.height);
            let big = marks::is_big(width, height);
            let (index, mark) = marks::drawn(*state, animation.frames.len(), big);
            if index > 0 {
                pixels = fit_frame(animation, index, placement, cell);
            }
            if let Some(mark) = mark {
                let (x, y) = (i64::from(placement.at.0), i64::from(placement.at.1));
                marks::draw(&mut pixels, (x, y, width, height), mark);
            }
        }
        if let Some(frame) = frame.filter(|_| !outlined) {
            framed(&mut pixels, cell.1, frame);
        }
        let cells = graphics::Cells {
            columns: placement.size.columns,
            rows: placement.size.rows,
        };
        let (width, height) = pixels.dimensions();
        // The terminal scales an image whose pixels are not its cells'.
        debug_assert_eq!(
            (width, height),
            (
                u32::from(cells.columns) * u32::from(cell.0),
                u32::from(cells.rows) * u32::from(cell.1)
            ),
            "the pixels of {key} are not its {}×{} cells",
            cells.columns,
            cells.rows,
        );
        let id = ImageId::new(pixels.as_raw(), width, height, cells, run);
        Ok(Self {
            id,
            pixels,
            cells,
            first_gap: Duration::ZERO,
            later: Vec::new(),
        })
    }

    /// The pixels of a frame, drawn for the image over `cells`.
    fn of_frame(cells: graphics::Cells, pixels: RgbaImage, caps: &Capabilities) -> Self {
        let (width, height) = pixels.dimensions();
        let id = ImageId::new(pixels.as_raw(), width, height, cells, caps.image_run);
        Self {
            id,
            pixels,
            cells,
            first_gap: Duration::ZERO,
            later: Vec::new(),
        }
    }

    /// Takes the frames after the first when `placement` shows an
    /// animation, scaled as the first is. The id then tells the animation
    /// from a still image that has the same first frame.
    fn add_frames(
        &mut self,
        placement: &Placement,
        images: &Images,
        pages: (&Pages, &Overlays),
        cell: (u16, u16),
        caps: &Capabilities,
        (driver, marks, built): (
            Option<&ImageKey>,
            &HashMap<ImageKey, State>,
            Option<&[RgbaImage]>,
        ),
    ) {
        let Some(frames) = Frames::of(placement, images, pages, cell, caps, (driver, marks)) else {
            return;
        };
        let animation = &frames.animation;
        if placement.shows != Shows::Image || animation.frames.len() < 2 {
            return;
        }
        self.first_gap = animation.frames[0].gap;
        for (at, frame_of) in animation.frames.iter().enumerate().skip(1) {
            let pixels = (built.and_then(|built| built.get(at)).cloned())
                .unwrap_or_else(|| frames.pixels(at));
            self.later.push((pixels, frame_of.gap));
        }
        // Every frame that is sent is in the id, so that animations that
        // differ only after the first frame are not one image; the frames
        // are hashed where they are, not copied together.
        let gaps: Vec<[u8; 16]> = (animation.frames.iter())
            .map(|frame| frame.gap.as_millis().to_le_bytes())
            .collect();
        let first = std::iter::once(self.pixels.as_raw().as_slice());
        let later = self
            .later
            .iter()
            .map(|(pixels, _)| pixels.as_raw().as_slice());
        let pieces = first.chain(later).chain(gaps.iter().map(|gap| &gap[..]));
        let (width, height) = self.pixels.dimensions();
        self.id = ImageId::of(pieces, width, height, self.cells, caps.image_run);
    }

    /// The bytes of the pixels as sent: those of the first frame and of
    /// every frame after it.
    fn bytes(&self) -> usize {
        let later = self.later.iter().map(|(pixels, _)| pixels.as_raw().len());
        let first = self.pixels.as_raw().len();
        if self.later.is_empty() && graphics::is_opaque(&self::pixels(&self.pixels)) {
            return first / 4 * 3;
        }
        first + later.sum::<usize>()
    }

    fn send(&self, out: &mut impl Write, effort: graphics::Effort) -> io::Result<()> {
        match self.later.is_empty() {
            true => send(out, self.id, &self.pixels, self.cells, effort)?,
            false => {
                let pixels = self::pixels(&self.pixels);
                graphics::transmit_rgba(out, self.id, &pixels, self.cells, effort)?;
            }
        }
        for (pixels, gap) in &self.later {
            graphics::transmit_frame(out, self.id, &self::pixels(pixels), *gap, effort)?;
        }
        if !self.later.is_empty() {
            graphics::first_frame_gap(out, self.id, self.first_gap)?;
        }
        Ok(())
    }
}

/// How many times an image is sent again at most for being lost.
pub use crate::present::ledger::AGAIN;

/// What a band's id is made from besides its size: it is the same image
/// whatever step of its light part it shows.
const BAND: &[u8] = b"kiri: the band of an image that is loading";

/// Sends `pixels` as image `id` over `cells`.
fn send(
    out: &mut impl Write,
    id: ImageId,
    pixels: &RgbaImage,
    cells: graphics::Cells,
    effort: graphics::Effort,
) -> io::Result<()> {
    graphics::transmit(out, id, &self::pixels(pixels), cells, effort)
}

/// `image` as the pixels `graphics` sends.
fn pixels(image: &RgbaImage) -> graphics::Pixels<'_> {
    let (width, height) = image.dimensions();
    graphics::Pixels {
        width,
        height,
        rgba: image.as_raw(),
    }
}

/// The colour of the frame `placement` has, as RGB where the terminal's
/// palette tells it.
fn frame_of(placement: &Placement, caps: &Capabilities) -> Option<([u8; 3], Range<u16>)> {
    let frame = placement.frame.as_ref()?;
    let Color::Rgb(red, green, blue) = frame.color.rgb_or_xterm(&caps.palette) else {
        unreachable!("a colour that is RGB or else xterm's is RGB");
    };
    Some(([red, green, blue], frame.rows.clone()))
}

/// A transparent canvas of the cells of `size`, each of `cell` pixels.
fn canvas(size: Size, cell: (u16, u16)) -> RgbaImage {
    RgbaImage::new(
        u32::from(size.columns) * u32::from(cell.0),
        u32::from(size.rows) * u32::from(cell.1),
    )
}

/// Frame `index` of `animation` laid out on the canvas of `placement` as
/// [`fit`] lays out a still image.
fn fit_frame(
    animation: &Animation,
    index: usize,
    placement: &Placement,
    cell: (u16, u16),
) -> RgbaImage {
    let mut canvas = canvas(placement.size, cell);
    let image = animation.render(index, placement.size.width, placement.size.height);
    let at = placement.at;
    imageops::replace(&mut canvas, &image, i64::from(at.0), i64::from(at.1));
    canvas
}

/// The cells of `rect`, as the terminal is told them.
fn cells_of(rect: &Rect) -> graphics::Cells {
    graphics::Cells {
        columns: rect.columns.end - rect.columns.start,
        rows: rect.rows.end - rect.rows.start,
    }
}

/// The cells of `size`, as the terminal is told them.
fn cells_of_size(size: Size) -> graphics::Cells {
    graphics::Cells {
        columns: size.columns,
        rows: size.rows,
    }
}

/// The error of a picture whose frames cannot be drawn.
fn no_frames(picture: &ImageKey) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("no frames for picture {picture}"),
    )
}

/// The plays that the protocol takes, as a number: `None` without end.
fn counted(plays: Plays) -> Option<u32> {
    match plays {
        Plays::Forever => None,
        Plays::Times(times) => Some(times),
    }
}

/// `source` drawn at `size` and placed at `at` pixels from the top left of
/// the cells it covers, the rest transparent; what lies outside them is cut
/// off. The terminal then shows it pixel for pixel
/// when the cells are `cell` pixels, and scales it as a whole when they are
/// not.
fn fit(source: &Source, size: Size, at: (i32, i32), cell: (u16, u16)) -> RgbaImage {
    let mut canvas = canvas(size, cell);
    let image = source.render(size.width, size.height);
    imageops::replace(&mut canvas, &image, i64::from(at.0), i64::from(at.1));
    canvas
}

#[cfg(test)]
mod tests;
