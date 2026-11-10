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
use crate::typeset::reading::animated::Pic;
use crate::typeset::reading::engine::Parts;
use crate::typeset::reading::page::Overlay;

mod frames;
mod sixel;
use crate::present::ledger::{DRIVEN, Held, Ledger};
use crate::typeset::style::Line;
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
}

impl Encoder {
    /// An encoder that has sent nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The id of the image `placement` shows, from `images`, on a terminal
    /// with `caps`: sent to `out` first unless the terminal holds it. An
    /// image sent for `placement` before, which the pixels no longer
    /// give, is deleted: at once, but inside tmux later, when kitty has
    /// answered the check of the new one.
    pub fn id(
        &mut self,
        placement: &Placement,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        // A picture that plays is shown as its frames: those of the image the
        // terminal plays, sent when the reader starts it and not before, or,
        // for what kiri plays, the image of the current frame. One that is
        // stopped or has ended is a still image with its mark.
        let playing = (self.states_in(placement, images).into_iter())
            .find_map(|(picture, state)| (state == State::Playing).then_some(picture));
        match (playing, self.playback) {
            (Some(picture), Playback::Redraw) => {
                let index = self.shown_frames.get(&picture).copied().unwrap_or(0);
                self.frame_id(placement, &picture, index, images, caps, out)
            }
            (Some(picture), Playback::Terminal) => {
                self.driven_id(placement, &picture, images, caps, out)
            }
            _ => self.still_id(placement, images, caps, out),
        }
    }

    /// The animated pictures `placement` shows with their states: the
    /// picture itself, or those in a part of a page.
    fn states_in(&self, placement: &Placement, images: &Images) -> Vec<(ImageKey, State)> {
        if self.marks.is_empty() || placement.shows != Shows::Image {
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
            .filter_map(|key| Some((key.clone(), *self.marks.get(&key)?)))
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
                let base = parts.draw_frame(slice, overlay, &pictures, &drawn, others);
                // And round this one, in each frame, when it is selected.
                let mine = (selected.as_ref().zip(chosen))
                    .filter(|(_, number)| Some(*number) == this)
                    .map(|((rgb, _), _)| (*rgb, line));
                Some(FramesJob::Part {
                    animation: Arc::clone(&pic.animation),
                    base,
                    at: (pic.x.round() as i64, top.round() as i64),
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

    /// The id of the part `placement` with the frames of `picture` in it,
    /// sent to `out` first, with every frame, unless the terminal holds it
    /// as drawn with the states the pictures in it have now.
    fn driven_id(
        &mut self,
        placement: &Placement,
        picture: &ImageKey,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        let cell = cell_size(caps);
        let at = (placement.clone(), picture.clone(), DRIVEN);
        let states = self.state_hash(placement, images);
        let held = self.ledger.frame(&at).filter(|held| held.cell == cell);
        let current = |held: &&Held| held.checked && held.sent && held.states == states;
        if let Some(held) = held.filter(current) {
            return Ok(held.id);
        }
        let pages = (&self.pages, &self.overlays);
        let frame = frame_of(placement, caps);
        let marks = &self.marks;
        let mut scaled = Scaled::new(placement, images, pages, cell, caps.image_run, frame, marks)?;
        let built = self.built.get(&(placement.clone(), picture.clone()));
        let built = built.map(|frames| frames.as_slice());
        let driver = (Some(picture), marks, built);
        scaled.add_frames(placement, images, pages, cell, caps, driver);
        let held = Held {
            id: scaled.id,
            cell,
            checked: true,
            sent: true,
            states,
        };
        if let Some(old) = self.ledger.put_frame(at, held) {
            self.let_go(old.id, scaled.id, caps, out)?;
        }
        self.sending(&scaled, caps);
        scaled.send(out)?;
        if !scaled.later.is_empty() {
            self.fresh.push(picture.clone());
        }
        Ok(scaled.id)
    }

    /// The states of the animated pictures `placement` shows, hashed: the
    /// pixels are other pixels when they change.
    fn state_hash(&self, placement: &Placement, images: &Images) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.states_in(placement, images).hash(&mut hasher);
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
        let states = self.state_hash(placement, images);
        let old = self
            .ledger
            .still(placement)
            .filter(|held| held.cell == cell);
        let current = |held: &&Held| held.checked && held.sent && held.states == states;
        if let Some(held) = old.filter(current) {
            return Ok(held.id);
        }
        let scaled = Scaled::new(
            placement,
            images,
            (&self.pages, &self.overlays),
            cell,
            caps.image_run,
            frame_of(placement, caps),
            &self.marks,
        )?;
        let held = Held {
            id: scaled.id,
            cell,
            checked: true,
            sent: true,
            states,
        };
        if let Some(old) = self.ledger.put_still(placement.clone(), held) {
            if old.id == scaled.id && old.sent {
                return Ok(scaled.id);
            }
            self.let_go(old.id, scaled.id, caps, out)?;
        }
        self.send_marked(&scaled, caps, out)?;
        Ok(scaled.id)
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

    /// The id of frame `index` after the first of the animation
    /// `placement` shows, sent to `out` first unless the terminal holds
    /// it. Each frame is a still image of its own.
    fn frame_id(
        &mut self,
        placement: &Placement,
        picture: &ImageKey,
        index: usize,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        let cell = cell_size(caps);
        let at = (placement.clone(), picture.clone(), index);
        let states = self.state_hash(placement, images);
        let held = self.ledger.frame(&at).filter(|held| held.cell == cell);
        let current = |held: &&Held| held.checked && held.sent && held.states == states;
        if let Some(held) = held.filter(current) {
            return Ok(held.id);
        }
        let pages = (&self.pages, &self.overlays);
        let driver = (Some(picture), &self.marks);
        let Some(frames) = Frames::of(placement, images, pages, cell, caps, driver) else {
            return self.still_id(placement, images, caps, out);
        };
        let last = frames.animation.frames.len() - 1;
        let built = self.built.get(&(placement.clone(), picture.clone()));
        let pixels = match built.and_then(|built| built.get(index.min(last))) {
            Some(pixels) => pixels.clone(),
            None => frames.pixels(index.min(last)),
        };
        let scaled = Scaled::of_frame(placement, pixels, caps);
        let held = Held {
            id: scaled.id,
            cell,
            checked: true,
            sent: true,
            states,
        };
        // The same pixels under another frame, or placement, are held.
        let sent = self.ledger.holds_sent(scaled.id);
        if let Some(old) = self.ledger.put_frame(at, held) {
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
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        if let Some(id) = self.ledger.band(columns, cell) {
            return Ok(id);
        }
        let band = loading::band(columns, cell, ink, elapsed);
        let cells = graphics::Cells { columns, rows: 1 };
        // Under tmux, where ids are a run's own, no band is drawn.
        let id = ImageId::new(BAND, band.width(), band.height(), cells, None);
        graphics::transmit_only(out, id, &pixels(&band))?;
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
        let cell = cell_size(caps);
        let placed: HashSet<&Placement> = (lines.iter())
            .flat_map(|line| {
                let embeds = line.spans.iter().filter_map(|span| span.embed.as_ref());
                (line.image.as_ref().map(|image| &image.placement))
                    .into_iter()
                    .chain(embeds.map(|embed| &embed.placement))
            })
            .map(Rc::as_ref)
            .collect();
        let bands: HashSet<u16> = (lines.iter())
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
        self.ledger.keep(&placed, &bands, cell);
    }

    /// Deletes from the terminal the images [`Encoder::keep`] let go.
    pub fn delete_unused(&mut self, out: &mut impl Write) -> io::Result<()> {
        self.end_frame();
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
        }
        Ok(())
    }

    /// Tells the encoder that kitty answered the check of image `id`, which
    /// it does after tmux has drawn the cells that show it: the stills it
    /// replaced through tmux are deleted now.
    pub fn answered(&mut self, id: ImageId, out: &mut impl Write) -> io::Result<()> {
        let ids = self.ledger.answered(id);
        self.delete(&ids, out)
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
    /// as its placeholders are on the screen. Nothing when no placement
    /// shows the image any more, or after [`AGAIN`] tries.
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
                &self.marks,
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
        scaled.send(out)
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
        scaled.send(out)
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
        let id = ImageId::new(pixels.as_raw(), width, height, cells, run);
        Ok(Self {
            id,
            pixels,
            cells,
            first_gap: Duration::ZERO,
            later: Vec::new(),
        })
    }

    /// Frame `index` of `animation` as the image of `placement` is made:
    /// scaled to its cells and framed when it is marked.
    fn of_frame(placement: &Placement, pixels: RgbaImage, caps: &Capabilities) -> Self {
        let cells = graphics::Cells {
            columns: placement.size.columns,
            rows: placement.size.rows,
        };
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

    fn send(&self, out: &mut impl Write) -> io::Result<()> {
        send(out, self.id, &self.pixels, self.cells)?;
        for (pixels, gap) in &self.later {
            graphics::transmit_frame(out, self.id, &self::pixels(pixels), *gap)?;
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
) -> io::Result<()> {
    graphics::transmit(out, id, &self::pixels(pixels), cells)
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
mod tests {
    use super::*;
    use crate::typeset::images::ImageKey;
    use crate::typeset::images::animation::{Animation, Frame as AnimFrame, Plays};
    use crate::typeset::images::size::Frame;

    const INK: Ink = Ink {
        rgb: [200, 200, 200],
        dark: true,
    };

    /// `key` placed over `columns` × `rows` cells of 10 × 20 pixels,
    /// showing `shows`.
    fn placed(key: &str, columns: u16, rows: u16, shows: Shows) -> Placement {
        Placement::block(
            ImageKey::from(key),
            Size {
                width: u32::from(columns) * 10,
                height: u32::from(rows) * 20,
                columns,
                rows,
            },
            shows,
        )
    }

    fn caps() -> Capabilities {
        Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(80)
        }
    }

    #[test]
    fn the_overlays_of_frames_gone_by_are_dropped() {
        use crate::render::Inputs;
        use crate::typeset::reading::page::{MarkColors, Overlay};
        // A page of reading mode, to register overlays against.
        let inputs = Inputs {
            reading: Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Wrap,
            }),
            ..Inputs::new(Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            })
        };
        let lines =
            crate::typeset::layout::layout(&crate::typeset::parse::parse("Text.\n"), &inputs);
        let image = lines[0].image.as_ref().expect("a page");
        let parts = Rc::clone(image.page.as_ref().expect("its parts"));
        let ImageKey::Page {
            document,
            page,
            slice,
            ..
        } = image.placement.key
        else {
            panic!("a page's key");
        };
        let colors = MarkColors {
            found: (None, None),
            current: (None, None),
        };
        let mut encoder = Encoder::default();
        for hash in 1..=50u64 {
            // Each frame, one `n` further: a new overlay, drawn.
            encoder.delete_unused(&mut Vec::new()).unwrap();
            let overlay = Rc::new(Overlay {
                colors,
                marks: Vec::new(),
                link: Some(hash as usize),
                rows: Vec::new(),
                link_rows: None,
            });
            let key = ImageKey::Page {
                document,
                page,
                slice,
                overlay: hash,
            };
            encoder.page(&key, &parts, Some(&overlay));
            assert!(
                encoder.overlays.len() <= 2,
                "{} after {hash}",
                encoder.overlays.len()
            );
        }
    }

    #[test]
    fn two_pages_with_the_same_hash_have_their_own_overlays() {
        use crate::render::Inputs;
        use crate::typeset::reading::page::{MarkColors, Overlay};
        let inputs = Inputs {
            reading: Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Wrap,
            }),
            ..Inputs::new(Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            })
        };
        let page_of = |source: &str| {
            let lines =
                crate::typeset::layout::layout(&crate::typeset::parse::parse(source), &inputs);
            let image = lines[0].image.as_ref().expect("a page");
            let parts = Rc::clone(image.page.as_ref().expect("parts"));
            let ImageKey::Page {
                document,
                page,
                slice,
                ..
            } = image.placement.key
            else {
                panic!("a page's key");
            };
            (parts, document, page, slice)
        };
        let (parts_a, document, page_a, slice) = page_of("Page A.\n");
        let (parts_b, _, page_b, _) = page_of("Page B.\n");
        assert_ne!(page_a, page_b);
        let colors = MarkColors {
            found: (None, None),
            current: (None, None),
        };
        let overlay = |link| {
            Rc::new(Overlay {
                colors,
                marks: Vec::new(),
                link: Some(link),
                rows: Vec::new(),
                link_rows: None,
            })
        };
        let (a, b) = (overlay(1), overlay(2));
        let mut encoder = Encoder::default();
        // The same hash on two pages, as the same marks in a part of each
        // give.
        let key = |page| ImageKey::Page {
            document,
            page,
            slice,
            overlay: 77,
        };
        encoder.page(&key(page_a), &parts_a, Some(&a));
        encoder.page(&key(page_b), &parts_b, Some(&b));
        assert_eq!(
            encoder.overlay_of(page_a, 77).map(|o| o.link),
            Some(Some(1))
        );
        assert_eq!(
            encoder.overlay_of(page_b, 77).map(|o| o.link),
            Some(Some(2))
        );
    }

    #[test]
    fn a_placeholder_is_the_old_image_dimmed() {
        let mut images = Images::new();
        let red = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 200]));
        images.wait("a.png".into(), Placeholder::Dimmed(Source::Raster(red)));
        let dimmed = Scaled::new(
            &placed("a.png", 2, 1, Shows::Placeholder),
            &images,
            (&HashMap::new(), &HashMap::new()),
            (10, 20),
            None,
            None,
            &HashMap::new(),
        );
        assert_eq!(dimmed.unwrap().pixels.get_pixel(5, 5).0, [255, 0, 0, 120]);
        // Nothing to show as the image itself.
        let image = placed("a.png", 2, 1, Shows::Image);
        assert!(
            Scaled::new(
                &image,
                &images,
                (&HashMap::new(), &HashMap::new()),
                (10, 20),
                None,
                None,
                &HashMap::new(),
            )
            .is_err()
        );
    }

    /// An animation of red, green and blue frames, as `Source`.
    fn animated(gaps: [u64; 3], plays: Plays) -> Source {
        let colours = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
        let frames = colours
            .iter()
            .zip(gaps)
            .map(|(rgba, gap)| AnimFrame {
                image: RgbaImage::from_pixel(20, 20, image::Rgba(*rgba)),
                gap: Duration::from_millis(gap),
            })
            .collect();
        Source::Animated(std::sync::Arc::new(Animation {
            frames,
            plays,
            over_budget: false,
        }))
    }

    /// The graphics sequences in `out`, one string each.
    fn sequences(out: &[u8]) -> Vec<String> {
        let out = String::from_utf8_lossy(out);
        out.split("\x1b_G")
            .skip(1)
            .map(|sequence| sequence.trim_end_matches("\x1b\\").to_owned())
            .collect()
    }

    /// The part of a sequence before its data.
    fn keys(sequence: &str) -> &str {
        sequence.split(';').next().unwrap()
    }

    /// The state `state` for the animation `name`, as the encoder is told.
    fn in_state(name: &str, state: State) -> HashMap<ImageKey, State> {
        HashMap::from([(ImageKey::from(name), state)])
    }

    /// `a.gif`, as the encoder sends it when it plays.
    fn sent_animation(playback: Playback) -> (Encoder, ImageId, Vec<u8>) {
        let mut images = Images::new();
        images.insert("a.gif", Ok(animated([100, 200, 300], Plays::Forever)));
        let mut encoder = Encoder::new();
        encoder.play(playback);
        encoder.show_marks(in_state("a.gif", State::Playing));
        let mut out = Vec::new();
        let id = encoder
            .id(
                &placed("a.gif", 2, 1, Shows::Image),
                &images,
                &caps(),
                &mut out,
            )
            .unwrap();
        (encoder, id, out)
    }

    #[test]
    fn an_animation_the_terminal_plays_is_sent_with_every_frame_and_its_gaps() {
        let (_, id, out) = sent_animation(Playback::Terminal);
        let sent = sequences(&out);
        let keys: Vec<&str> = sent.iter().map(|s| keys(s)).collect();
        let id = format!("i={}", id.value());
        assert_eq!(keys.len(), 4, "{keys:?}");
        assert!(keys[0].starts_with("a=T,U=1,"), "{keys:?}");
        assert!(
            keys[1].starts_with("a=f,") && keys[1].contains("z=200"),
            "{keys:?}"
        );
        assert!(
            keys[2].starts_with("a=f,") && keys[2].contains("z=300"),
            "{keys:?}"
        );
        // The root frame's gap, which a frame cannot carry when it is sent.
        assert_eq!(keys[3], format!("a=a,q=2,{id},r=1,z=100"));
    }

    #[test]
    fn what_kiri_plays_is_sent_a_frame_at_a_time_each_under_an_id_of_its_own() {
        let mut images = Images::new();
        images.insert("a.gif", Ok(animated([100, 200, 300], Plays::Forever)));
        let placement = placed("a.gif", 2, 1, Shows::Image);
        let mut encoder = Encoder::new();
        encoder.play(Playback::Redraw);
        encoder.show_marks(in_state("a.gif", State::Playing));
        let mut ids = Vec::new();
        let mut out = Vec::new();
        for frame in [0, 1, 2, 1, 0] {
            let before = out.len();
            encoder.show_frames(HashMap::from([(ImageKey::from("a.gif"), frame)]));
            ids.push(encoder.id(&placement, &images, &caps(), &mut out).unwrap());
            // Sent as a still image, the first time it is shown.
            let sent = sequences(&out[before..]);
            let first_time = [0, 1, 2].contains(&frame) && ids.len() <= 3;
            assert_eq!(
                sent.len(),
                usize::from(first_time),
                "frame {frame}: {sent:?}"
            );
            assert!(sent.iter().all(|s| keys(s).starts_with("a=T,U=1,")));
        }
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
        assert_ne!(ids[0], ids[2]);
        // Back to a frame shown before: the id it had, not sent again.
        assert_eq!((ids[3], ids[4]), (ids[1], ids[0]));
    }

    #[test]
    fn a_frame_sent_has_the_still_with_its_pixels_sent_too() {
        let mut images = Images::new();
        images.insert("a.gif", Ok(animated([100, 200, 300], Plays::Forever)));
        let red = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]));
        images.insert("b.png", Ok(Source::Raster(red)));
        let frame = placed("a.gif", 2, 1, Shows::Image);
        let still = placed("b.png", 2, 1, Shows::Image);
        let mut encoder = Encoder::new();
        encoder.play(Playback::Redraw);
        encoder.show_marks(in_state("a.gif", State::Playing));
        encoder.show_frames(HashMap::from([(ImageKey::from("a.gif"), 0)]));
        let mut out = Vec::new();
        let id = encoder.id(&still, &images, &caps(), &mut out).unwrap();
        encoder.lost();
        // The first frame has the pixels of the still: sent now, for both.
        out.clear();
        assert_eq!(encoder.id(&frame, &images, &caps(), &mut out).unwrap(), id);
        assert_eq!(sequences(&out).len(), 1, "{:?}", sequences(&out));
        out.clear();
        assert_eq!(encoder.id(&still, &images, &caps(), &mut out).unwrap(), id);
        assert!(out.is_empty(), "{:?}", sequences(&out));
    }

    #[test]
    fn frames_that_the_terminal_plays_are_not_sent_one_at_a_time() {
        let mut images = Images::new();
        images.insert("a.gif", Ok(animated([100, 200, 300], Plays::Forever)));
        let placement = placed("a.gif", 2, 1, Shows::Image);
        let mut encoder = Encoder::new();
        encoder.play(Playback::Terminal);
        encoder.show_marks(in_state("a.gif", State::Playing));
        encoder.show_frames(HashMap::from([(ImageKey::from("a.gif"), 1)]));
        let mut out = Vec::new();
        let first = encoder.id(&placement, &images, &caps(), &mut out).unwrap();
        let again = encoder.id(&placement, &images, &caps(), &mut out).unwrap();
        assert_eq!(first, again);
        assert_eq!(sequences(&out).len(), 4);
    }

    #[test]
    fn animations_that_differ_after_the_first_frame_get_ids_of_their_own() {
        let two = |second: [u8; 4]| {
            let frame = |rgba: [u8; 4]| AnimFrame {
                image: RgbaImage::from_pixel(20, 20, image::Rgba(rgba)),
                gap: Duration::from_millis(100),
            };
            Source::Animated(std::sync::Arc::new(Animation {
                frames: vec![frame([255, 0, 0, 255]), frame(second)],
                plays: Plays::Forever,
                over_budget: false,
            }))
        };
        let mut images = Images::new();
        images.insert("green.gif", Ok(two([0, 255, 0, 255])));
        images.insert("blue.gif", Ok(two([0, 0, 255, 255])));
        let mut encoder = Encoder::new();
        encoder.play(Playback::Terminal);
        let mut marks = in_state("green.gif", State::Playing);
        marks.extend(in_state("blue.gif", State::Playing));
        encoder.show_marks(marks);
        let mut id = |name| {
            let placement = placed(name, 2, 1, Shows::Image);
            let id = encoder.id(&placement, &images, &caps(), &mut Vec::new());
            id.unwrap()
        };
        assert_ne!(id("green.gif"), id("blue.gif"));
    }

    /// The graphics sequences of `out`, one a line, with the data left out.
    fn elided(out: &[u8]) -> String {
        let out = String::from_utf8_lossy(out);
        let lines: Vec<String> = (out.split("\x1b_G").skip(1))
            .map(|sequence| {
                let body = sequence.trim_end_matches("\x1b\\");
                match body.split_once(';') {
                    Some((keys, _)) => format!("{keys};…"),
                    None => body.to_owned(),
                }
            })
            .collect();
        lines.join("\n")
    }

    #[test]
    fn the_bytes_of_an_animation_for_kitty_to_play() {
        let (_, _, out) = sent_animation(Playback::Terminal);
        insta::assert_snapshot!(elided(&out));
    }

    #[test]
    fn an_animation_nobody_plays_is_sent_as_its_first_frame() {
        for playback in [Playback::None, Playback::Redraw] {
            let (_, _, out) = sent_animation(playback);
            let sent = sequences(&out);
            assert_eq!(sent.len(), 1, "{playback:?}: {sent:?}");
            assert!(keys(&sent[0]).starts_with("a=T,U=1,"));
        }
    }

    #[test]
    fn a_stopped_animation_is_one_still_image_and_its_frames_are_sent_when_it_plays() {
        let mut images = Images::new();
        images.insert("a.gif", Ok(animated([100, 200, 300], Plays::Forever)));
        let placement = placed("a.gif", 2, 1, Shows::Image);
        let mut encoder = Encoder::new();
        encoder.play(Playback::Terminal);
        encoder.show_marks(in_state("a.gif", State::Stopped));
        let mut out = Vec::new();
        let still = encoder.id(&placement, &images, &caps(), &mut out).unwrap();
        assert_eq!(sequences(&out).len(), 1, "{:?}", sequences(&out));
        assert_eq!(encoder.take_fresh(), []);
        // Playing: another image, sent with every frame.
        encoder.show_marks(in_state("a.gif", State::Playing));
        let before = out.len();
        let playing = encoder.id(&placement, &images, &caps(), &mut out).unwrap();
        assert_ne!(still, playing);
        assert_eq!(sequences(&out[before..]).len(), 4);
        assert_eq!(encoder.take_fresh(), [ImageKey::from("a.gif")]);
        // Stopped again: the still image the terminal still holds.
        encoder.show_marks(in_state("a.gif", State::Stopped));
        let before = out.len();
        let stopped = encoder.id(&placement, &images, &caps(), &mut out).unwrap();
        assert_eq!(stopped, still);
        assert_eq!(out.len(), before, "nothing is sent again");
    }

    #[test]
    fn an_animation_that_has_ended_is_a_still_image_of_its_last_frame() {
        let mut images = Images::new();
        images.insert("a.gif", Ok(animated([100, 200, 300], Plays::Forever)));
        let placement = placed("a.gif", 2, 1, Shows::Image);
        let mut encoder = Encoder::new();
        encoder.play(Playback::Terminal);
        let mut id = |state| {
            encoder.show_marks(in_state("a.gif", state));
            encoder
                .id(&placement, &images, &caps(), &mut Vec::new())
                .unwrap()
        };
        let (first, last) = (id(State::Stopped), id(State::Ended));
        assert_ne!(first, last, "the last frame is another picture");
        assert_eq!(
            sequences(&{
                let mut out = Vec::new();
                encoder.show_marks(in_state("a.gif", State::Ended));
                encoder.id(&placement, &images, &caps(), &mut out).unwrap();
                out
            })
            .len(),
            0,
            "and held"
        );
    }

    #[test]
    fn the_frames_are_scaled_to_the_cells_as_the_first_is() {
        let (_, _, out) = sent_animation(Playback::Terminal);
        let sent = sequences(&out);
        for sequence in &sent[..3] {
            assert!(keys(sequence).contains(",s=20,v=20"), "{sequence}");
        }
    }

    #[test]
    fn the_controls_name_the_image_the_animation_was_sent_as() {
        let (mut encoder, id, _) = sent_animation(Playback::Terminal);
        let key = ImageKey::from("a.gif");
        let mut out = Vec::new();
        encoder
            .control(&Command::Stop(key.clone()), &mut out)
            .unwrap();
        encoder
            .control(&Command::Replay(key.clone(), Plays::Forever), &mut out)
            .unwrap();
        let id = id.value();
        let expected = [
            format!("a=a,q=2,i={id},s=1"),
            format!("a=a,q=2,i={id},c=1"),
            format!("a=a,q=2,i={id},c=1,s=3,v=1"),
        ];
        assert_eq!(sequences(&out), expected);
        // An image it holds none of is not told anything.
        let mut out = Vec::new();
        let other = Command::Stop(ImageKey::from("b.gif"));
        encoder.control(&other, &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn an_animation_sent_with_its_frames_is_fresh_once() {
        let (mut encoder, _, _) = sent_animation(Playback::Terminal);
        assert_eq!(encoder.take_fresh(), [ImageKey::from("a.gif")]);
        assert_eq!(encoder.take_fresh(), []);
        let (mut encoder, _, _) = sent_animation(Playback::None);
        assert_eq!(encoder.take_fresh(), []);
    }

    #[test]
    fn a_band_is_sent_once_a_frame_as_it_is_at_the_frame_s_time() {
        let mut encoder = Encoder::new();
        let mut out = Vec::new();
        let cell = (10, 20);
        let start = Duration::ZERO;
        let first = encoder.band(4, cell, INK, start, &mut out).unwrap();
        let second = encoder.band(4, cell, INK, start, &mut out).unwrap();
        assert_eq!(first, second);
        let sent = String::from_utf8(std::mem::take(&mut out)).unwrap();
        // Sent without a placement, which the writer makes.
        assert_eq!(sent.matches("\x1b_Ga=t,").count(), 1, "{sent:?}");
        assert!(encoder.has_bands());
        // Clearing the screen frees it, as nothing places it then: sent
        // again for the next frame, as it is then, under the same id.
        encoder.cleared();
        assert!(!encoder.has_bands());
        let later = Duration::from_millis(600);
        assert_eq!(encoder.band(4, cell, INK, later, &mut out).unwrap(), first);
        let again = String::from_utf8(std::mem::take(&mut out)).unwrap();
        assert_eq!(again.matches("\x1b_Ga=t,").count(), 1, "{again:?}");
        assert_ne!(again, sent);
        // Gone with the last line that has it.
        encoder.keep(&[], &caps());
        encoder.delete_unused(&mut out).unwrap();
        let mut deleted = Vec::new();
        graphics::delete(&mut deleted, first).unwrap();
        assert_eq!(out, deleted);
    }

    /// A line of one span that is the cells of `placement`.
    fn embedding(placement: &Placement) -> Line {
        use crate::typeset::style::{Embed, Span, Style};
        let embed = Embed::new(Rc::new(placement.clone()), 0, 1);
        Line::new(vec![Span::embed("alt", Style::default(), embed)])
    }

    #[test]
    fn an_embed_lies_on_its_canvas_where_its_placement_says_and_is_cut_there() {
        let mut images = Images::new();
        let red = RgbaImage::from_pixel(4, 6, image::Rgba([255, 0, 0, 255]));
        images.insert("e.png", Ok(Source::Raster(red)));
        let mut placement = placed("e.png", 2, 1, Shows::Image);
        placement.size.width = 4;
        placement.size.height = 6;
        placement.at = (2, -3);
        let scaled = Scaled::new(
            &placement,
            &images,
            (&Pages::new(), &Overlays::new()),
            (10, 20),
            None,
            None,
            &HashMap::new(),
        )
        .unwrap();
        // The canvas is its cells; the image, its lower half, is in the
        // top rows from the second column.
        assert_eq!(scaled.pixels.dimensions(), (20, 20));
        let red = |x, y| scaled.pixels.get_pixel(x, y).0 == [255, 0, 0, 255];
        assert!(
            red(2, 0) && red(5, 2),
            "{:?}",
            scaled.pixels.get_pixel(2, 0)
        );
        assert!(!red(1, 0) && !red(6, 0) && !red(2, 3));
    }

    /// The canvas of `placement` on cells of `cell` pixels, the image an
    /// opaque red square.
    fn canvas_of(placement: &Placement, cell: (u16, u16)) -> RgbaImage {
        let mut images = Images::new();
        let red = RgbaImage::from_pixel(10, 10, image::Rgba([255, 0, 0, 255]));
        images.insert("e.png", Ok(Source::Raster(red)));
        let frame = frame_of(placement, &caps());
        let pages = (&Pages::new(), &Overlays::new());
        Scaled::new(
            placement,
            &images,
            pages,
            cell,
            None,
            frame,
            &HashMap::new(),
        )
        .unwrap()
        .pixels
    }

    #[test]
    fn a_marked_embed_has_a_frame_on_its_canvas_scaled_with_the_cell() {
        let mut placement = placed("e.png", 2, 1, Shows::Image);
        placement.size.width = 10;
        placement.size.height = 10;
        placement.at = (5, 5);
        let plain = canvas_of(&placement, (10, 20));
        assert_eq!(plain.get_pixel(0, 0).0, [0, 0, 0, 0]);
        placement.frame = Some(Frame {
            color: crate::typeset::color::Color::Rgb(1, 2, 3),
            rows: 0..1,
            picture: None,
        });
        let framed = canvas_of(&placement, (10, 20));
        let mark = [1, 2, 3, 255];
        // One pixel wide at 20 pixels to the row, all round the cells.
        let (width, height) = framed.dimensions();
        for (x, y) in [
            (0, 0),
            (width - 1, 0),
            (0, height - 1),
            (width - 1, height - 1),
            (9, 0),
            (0, 10),
        ] {
            assert_eq!(framed.get_pixel(x, y).0, mark, "{x} {y}");
        }
        assert_eq!(framed.get_pixel(1, 1).0, [0, 0, 0, 0]);
        assert_eq!(framed.get_pixel(5, 5).0, [255, 0, 0, 255]);
        // Twice as wide at 40.
        let tall = canvas_of(&placement, (10, 40));
        assert_eq!(tall.get_pixel(1, 5).0, mark);
        assert_eq!(tall.get_pixel(2, 5).0, [0, 0, 0, 0]);
        // A different frame is another image to the terminal.
        assert_ne!(plain.as_raw(), framed.as_raw());
    }

    #[test]
    fn the_frame_goes_round_the_cells_of_its_rows_only() {
        let mut placement = placed("e.png", 2, 2, Shows::Image);
        placement.size.width = 10;
        placement.size.height = 10;
        placement.at = (5, 5);
        placement.frame = Some(Frame {
            color: crate::typeset::color::Color::Rgb(1, 2, 3),
            rows: 1..2,
            picture: None,
        });
        let canvas = canvas_of(&placement, (10, 20));
        assert_eq!(canvas.dimensions(), (20, 40));
        // Nothing round the first row; the second is framed all round.
        assert_eq!(canvas.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(canvas.get_pixel(0, 19).0, [0, 0, 0, 0]);
        let mark = [1, 2, 3, 255];
        for (x, y) in [(0, 20), (19, 20), (0, 39), (19, 39), (10, 20), (10, 39)] {
            assert_eq!(canvas.get_pixel(x, y).0, mark, "{x} {y}");
        }
        assert_eq!(canvas.get_pixel(10, 30).0, [0, 0, 0, 0]);
    }

    #[test]
    fn a_frame_is_drawn_over_an_image_that_reaches_the_edge_of_its_cells() {
        let mut placement = placed("e.png", 1, 1, Shows::Image);
        placement.size.width = 10;
        placement.size.height = 20;
        placement.at = (0, 0);
        let mut images = Images::new();
        let red = RgbaImage::from_pixel(10, 20, image::Rgba([255, 0, 0, 255]));
        images.insert("e.png", Ok(Source::Raster(red)));
        let pages = (&Pages::new(), &Overlays::new());
        let at = |placement: &Placement| {
            let frame = frame_of(placement, &caps());
            let scaled = Scaled::new(
                placement,
                &images,
                pages,
                (10, 20),
                None,
                frame,
                &HashMap::new(),
            )
            .unwrap();
            [(0, 0), (9, 19), (0, 10), (5, 0)].map(|(x, y)| scaled.pixels.get_pixel(x, y).0)
        };
        // Unmarked, the edge is the image's own; marked, the frame.
        assert_eq!(at(&placement), [[255, 0, 0, 255]; 4]);
        placement.frame = Some(Frame {
            color: crate::typeset::color::Color::Rgb(1, 2, 3),
            rows: 0..1,
            picture: None,
        });
        assert_eq!(at(&placement), [[1, 2, 3, 255]; 4]);
    }

    #[test]
    fn the_images_of_embeds_are_kept_and_those_of_embeds_gone_are_deleted() {
        let mut images = Images::new();
        let red = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]));
        images.insert("old.png", Ok(Source::Raster(red.clone())));
        images.insert("new.png", Ok(Source::Raster(RgbaImage::new(20, 20))));
        let old = embedding(&placed("old.png", 2, 1, Shows::Image));
        let mut encoder = Encoder::new();
        let mut out = Vec::new();
        let id = {
            let Some(embed) = &old.spans[0].embed else {
                panic!("an embed")
            };
            encoder
                .id(&embed.placement, &images, &caps(), &mut out)
                .unwrap()
        };
        // Still placed by a new layout: kept.
        encoder.keep(std::slice::from_ref(&old), &caps());
        out.clear();
        encoder.delete_unused(&mut out).unwrap();
        assert!(out.is_empty(), "{out:?}");
        // A reload changed the image of the line: the old one goes.
        let new = embedding(&placed("new.png", 2, 1, Shows::Image));
        encoder.keep(&[new], &caps());
        encoder.delete_unused(&mut out).unwrap();
        let mut expected = Vec::new();
        graphics::delete(&mut expected, id).unwrap();
        assert_eq!(out, expected);
    }

    /// A writer that takes nothing.
    struct Refuses;

    impl Write for Refuses {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("the terminal is gone"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn images_whose_deletion_failed_are_deleted_by_the_next_try() {
        let mut images = Images::new();
        let red = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]));
        images.insert("old.png", Ok(Source::Raster(red)));
        let old = embedding(&placed("old.png", 2, 1, Shows::Image));
        let Some(embed) = &old.spans[0].embed else {
            panic!("an embed")
        };
        let mut encoder = Encoder::new();
        let id = encoder
            .id(&embed.placement, &images, &caps(), &mut Vec::new())
            .unwrap();
        encoder.keep(&[], &caps());
        assert!(encoder.delete_unused(&mut Refuses).is_err());
        let mut out = Vec::new();
        encoder.delete_unused(&mut out).unwrap();
        let mut expected = Vec::new();
        graphics::delete(&mut expected, id).unwrap();
        assert_eq!(out, expected);
    }

    #[test]
    fn an_image_held_for_other_cells_is_sent_again() {
        let mut images = Images::new();
        let red = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]));
        images.insert("square.png", Ok(Source::Raster(red)));
        let placement = Placement::block(
            ImageKey::from("square.png"),
            Size {
                width: 20,
                height: 20,
                columns: 2,
                rows: 1,
            },
            Shows::Image,
        );
        let caps = Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(80)
        };
        let taller = Capabilities {
            cell_pixels: Some((10, 25)),
            ..caps.clone()
        };
        let mut encoder = Encoder::new();
        let mut out = Vec::new();
        let old = encoder.id(&placement, &images, &caps, &mut out).unwrap();
        out.clear();
        // The same placement in taller cells, without a layout between:
        // scaled for them, under another id.
        let new = encoder.id(&placement, &images, &taller, &mut out).unwrap();
        assert_ne!(old, new);
        let mut expected = Vec::new();
        graphics::delete(&mut expected, old).unwrap();
        let out = String::from_utf8(out).unwrap();
        let expected = String::from_utf8(expected).unwrap();
        assert!(out.starts_with(&expected), "{out:?}");
        assert!(out.contains("\x1b_Ga=T,"), "{out:?}");
    }

    #[test]
    fn a_lost_image_is_sent_again_once_for_all_its_placements() {
        let mut images = Images::new();
        let red = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]));
        images.insert("a.png", Ok(Source::Raster(red.clone())));
        images.insert("b.png", Ok(Source::Raster(red)));
        let (a, b) = (
            placed("a.png", 2, 1, Shows::Image),
            placed("b.png", 2, 1, Shows::Image),
        );
        let mut encoder = Encoder::new();
        let mut out = Vec::new();
        let id = encoder.id(&a, &images, &caps(), &mut out).unwrap();
        assert_eq!(encoder.id(&b, &images, &caps(), &mut out).unwrap(), id);
        out.clear();
        assert_eq!(encoder.id(&a, &images, &caps(), &mut out).unwrap(), id);
        assert!(out.is_empty());
        encoder.lost();
        assert_eq!(encoder.id(&a, &images, &caps(), &mut out).unwrap(), id);
        assert_eq!(encoder.id(&b, &images, &caps(), &mut out).unwrap(), id);
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out.matches("\x1b_Ga=T,").count(), 1, "{out:?}");
        assert!(!out.contains("a=d"), "{out:?}");
    }

    /// `caps()` with graphics wrapped for tmux.
    fn through_tmux() -> Capabilities {
        Capabilities {
            tmux_passthrough: true,
            ..caps()
        }
    }

    /// Images with a red `a.png`, and its placement over 2 × 1 cells.
    fn red() -> (Images, Placement) {
        let mut images = Images::new();
        let red = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]));
        images.insert("a.png", Ok(Source::Raster(red)));
        (images, placed("a.png", 2, 1, Shows::Image))
    }

    #[test]
    fn the_images_sent_through_tmux_are_told_once_to_be_checked() {
        let (images, placement) = red();
        let mut encoder = Encoder::new();
        let mut out = Vec::new();
        let id = encoder
            .id(&placement, &images, &through_tmux(), &mut out)
            .unwrap();
        // Not sent again, so not told again.
        encoder
            .id(&placement, &images, &through_tmux(), &mut out)
            .unwrap();
        let cells = graphics::Cells {
            columns: 2,
            rows: 1,
        };
        assert_eq!(encoder.take_sent(), [(id, cells)]);
        assert_eq!(encoder.take_sent(), []);
        // Without tmux, nothing is checked.
        let mut encoder = Encoder::new();
        encoder.id(&placement, &images, &caps(), &mut out).unwrap();
        assert_eq!(encoder.take_sent(), []);
    }

    #[test]
    fn a_lost_image_is_sent_again_a_few_times_at_most() {
        let (images, placement) = red();
        let caps = through_tmux();
        let mut encoder = Encoder::new();
        let mut out = Vec::new();
        let id = encoder.id(&placement, &images, &caps, &mut out).unwrap();
        let first = std::mem::take(&mut out);
        encoder.take_sent();
        encoder.send_again(id, &images, &caps, &mut out).unwrap();
        // The same transmission, which also places it again.
        assert_eq!(std::mem::take(&mut out), first);
        assert_eq!(encoder.take_sent().len(), 1);
        for _ in 1..AGAIN {
            encoder.send_again(id, &images, &caps, &mut out).unwrap();
        }
        let sent = String::from_utf8(std::mem::take(&mut out)).unwrap();
        assert_eq!(sent.matches("\x1b_Ga=T,").count(), usize::from(AGAIN) - 1);
        encoder.send_again(id, &images, &caps, &mut out).unwrap();
        assert!(out.is_empty());
        // An image no placement shows any more is not sent.
        let mut other = Encoder::new();
        other.send_again(id, &images, &caps, &mut out).unwrap();
        encoder.keep(&[], &caps);
        encoder.lost();
        encoder.send_again(id, &images, &caps, &mut out).unwrap();
        assert!(out.is_empty());
    }

    /// `a.png` of [`red`] in `rgba` from now on, as after a reload: the
    /// encoder scales it again when it is next shown.
    fn repaint(encoder: &mut Encoder, images: &mut Images, rgba: [u8; 4]) {
        let pixels = RgbaImage::from_pixel(20, 20, image::Rgba(rgba));
        images.insert("a.png", Ok(Source::Raster(pixels)));
        encoder.changed();
    }

    /// The bytes that delete `ids`.
    fn deletes(ids: &[ImageId]) -> Vec<u8> {
        let mut out = Vec::new();
        for &id in ids {
            graphics::delete(&mut out, id).unwrap();
        }
        out
    }

    /// The id the placement of [`red`] has in `rgba`, shown through
    /// `encoder`, and what showing it wrote.
    fn shown_in(
        encoder: &mut Encoder,
        images: &mut Images,
        placement: &Placement,
        rgba: [u8; 4],
        caps: &Capabilities,
    ) -> (ImageId, String) {
        repaint(encoder, images, rgba);
        let mut out = Vec::new();
        let id = encoder.id(placement, images, caps, &mut out).unwrap();
        (id, String::from_utf8(out).unwrap())
    }

    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const RED: [u8; 4] = [255, 0, 0, 255];

    #[test]
    fn through_tmux_a_replaced_still_goes_once_kitty_answers_for_the_image_after_it() {
        // tmux passes the delete on at once, but draws the cells that show
        // the new image only after the update: until then they would show
        // the deleted image, which is nothing.
        let (mut images, placement) = red();
        let caps = through_tmux();
        let mut encoder = Encoder::new();
        let mut out = Vec::new();
        let red = encoder.id(&placement, &images, &caps, &mut out).unwrap();
        let (blue, sent) = shown_in(&mut encoder, &mut images, &placement, BLUE, &caps);
        assert!(!sent.contains("a=d"), "{sent:?}");
        assert!(sent.contains("\x1b_Ga=T,"), "{sent:?}");
        // An answer for another image deletes nothing.
        out.clear();
        encoder.answered(red, &mut out).unwrap();
        assert!(out.is_empty(), "{out:?}");
        encoder.answered(blue, &mut out).unwrap();
        assert_eq!(out, deletes(&[red]));
        // And only once.
        out.clear();
        encoder.answered(blue, &mut out).unwrap();
        assert!(out.is_empty(), "{out:?}");
        // Each in turn, as the steps of a turning mark replace each other.
        let (green, _) = shown_in(&mut encoder, &mut images, &placement, GREEN, &caps);
        let (red, _) = shown_in(&mut encoder, &mut images, &placement, RED, &caps);
        encoder.answered(green, &mut out).unwrap();
        assert_eq!(out, deletes(&[blue]));
        out.clear();
        encoder.answered(red, &mut out).unwrap();
        assert_eq!(out, deletes(&[green]));
    }

    #[test]
    fn through_tmux_an_image_shown_again_before_the_answer_is_not_deleted() {
        let (mut images, placement) = red();
        let caps = through_tmux();
        let mut encoder = Encoder::new();
        let mut out = Vec::new();
        let red = encoder.id(&placement, &images, &caps, &mut out).unwrap();
        let (blue, _) = shown_in(&mut encoder, &mut images, &placement, BLUE, &caps);
        // Red again before kitty answered for blue: the cells show red.
        let (again, _) = shown_in(&mut encoder, &mut images, &placement, RED, &caps);
        assert_eq!(again, red);
        out.clear();
        // Green after that, still with no answer: red is replaced again.
        let (green, _) = shown_in(&mut encoder, &mut images, &placement, GREEN, &caps);
        out.clear();
        encoder.answered(blue, &mut out).unwrap();
        assert!(out.is_empty(), "{out:?}");
        encoder.answered(red, &mut out).unwrap();
        assert_eq!(out, deletes(&[blue]));
        out.clear();
        encoder.answered(green, &mut out).unwrap();
        assert_eq!(out, deletes(&[red]));
        // Nothing else waits: quitting deletes only green.
        out.clear();
        encoder.delete_all(&mut out).unwrap();
        assert_eq!(out, deletes(&[green]));
    }

    #[test]
    fn through_tmux_a_replaced_still_kitty_never_answers_for_goes_with_a_new_layout_a_loss_or_the_quit()
     {
        type Then = fn(&mut Encoder, &Placement, &Capabilities);
        // A new layout that still shows the image.
        let keep: Then = |encoder, placement, caps| encoder.keep(&[embedding(placement)], caps);
        let lost: Then = |encoder, _, _| encoder.lost();
        let lost_some: Then = |encoder, _, _| encoder.lost_some(&[]);
        for (name, then) in [("keep", keep), ("lost", lost), ("lost_some", lost_some)] {
            let (mut images, placement) = red();
            let caps = through_tmux();
            let mut encoder = Encoder::new();
            let mut out = Vec::new();
            let red = encoder.id(&placement, &images, &caps, &mut out).unwrap();
            shown_in(&mut encoder, &mut images, &placement, BLUE, &caps);
            out.clear();
            encoder.delete_unused(&mut out).unwrap();
            assert!(out.is_empty(), "{name}: {out:?}");
            then(&mut encoder, &placement, &caps);
            encoder.delete_unused(&mut out).unwrap();
            assert_eq!(out, deletes(&[red]), "{name}");
        }
        // Quitting deletes it with the image shown.
        let (mut images, placement) = red();
        let caps = through_tmux();
        let mut encoder = Encoder::new();
        let mut out = Vec::new();
        let red = encoder.id(&placement, &images, &caps, &mut out).unwrap();
        let (blue, _) = shown_in(&mut encoder, &mut images, &placement, BLUE, &caps);
        out.clear();
        encoder.delete_all(&mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        for id in [red, blue] {
            let delete = String::from_utf8(deletes(&[id])).unwrap();
            assert_eq!(out.matches(&delete).count(), 1, "{out:?}");
        }
    }

    #[test]
    fn outside_tmux_a_replaced_still_goes_at_once() {
        let (mut images, placement) = red();
        let caps = caps();
        let mut encoder = Encoder::new();
        let red = (encoder.id(&placement, &images, &caps, &mut Vec::new())).unwrap();
        let (_, sent) = shown_in(&mut encoder, &mut images, &placement, BLUE, &caps);
        let delete = String::from_utf8(deletes(&[red])).unwrap();
        assert!(sent.starts_with(&delete), "{sent:?}");
        assert!(sent.contains("\x1b_Ga=T,"), "{sent:?}");
    }

    #[test]
    fn a_run_of_its_own_sends_the_image_under_an_id_of_its_own() {
        let mut images = Images::new();
        let red = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]));
        images.insert("a.png", Ok(Source::Raster(red)));
        let placement = placed("a.png", 2, 1, Shows::Image);
        let id = |run| {
            let caps = Capabilities {
                image_run: run,
                ..caps()
            };
            (Encoder::new().id(&placement, &images, &caps, &mut Vec::new())).unwrap()
        };
        // As on every run without one.
        let scaled = Scaled::new(
            &placement,
            &images,
            (&HashMap::new(), &HashMap::new()),
            (10, 20),
            None,
            None,
            &HashMap::new(),
        )
        .unwrap();
        let (width, height) = scaled.pixels.dimensions();
        let stable = ImageId::new(scaled.pixels.as_raw(), width, height, scaled.cells, None);
        assert_eq!(id(None), stable);
        assert_ne!(id(Some(1)), stable);
        assert_ne!(id(Some(1)), id(Some(2)));
    }
}
