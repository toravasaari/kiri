//! What the terminal holds of the images kiri sent it, and under which id.
//!
//! The encoder scales an image to the size a layout shows it at, sends it
//! through `kitty::graphics` and remembers the id it went under, so that a
//! later row of the same image only names the id. The escape sequences
//! themselves are `kitty::graphics`'s; the encoder knows placements and
//! ids.

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
use crate::typeset::images::animation::{Animation, Plays, START_DELAY};
use crate::typeset::images::size::{Placement, Shows, Size, cell_size};
use crate::typeset::images::{ImageKey, Images, Placeholder, Source};
use crate::typeset::reading::engine::Parts;
use crate::typeset::reading::page::Overlay;

mod frames;
use crate::typeset::style::Line;
use frames::Frames;

/// What the terminal is told to do with an animation, by its image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Play from the frame it is on, `plays` times in all.
    Start(ImageKey, Plays),
    /// Stay on the frame it is on.
    Stop(ImageKey),
    /// Play from the first frame, `plays` times in all.
    Replay(ImageKey, Plays),
    /// Show this frame, counting from 0, and stay on it: an image sent
    /// anew, which the terminal shows at its first.
    Show(ImageKey, usize),
}

/// The images the terminal holds, by where and how large they are shown.
/// One encoder can serve one layout after another, as the pager's does,
/// so that an image the terminal holds is not sent again.
#[derive(Debug, Default)]
pub struct Encoder {
    held: HashMap<Placement, Held>,
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
    /// until [`Encoder::take_sent`] takes them to check that they came.
    sent: Vec<(ImageId, graphics::Cells)>,
    /// How many times each image has been sent again for being lost.
    again: HashMap<ImageId, u8>,
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
    /// The frames after the first of the animations kiri plays, which are
    /// held as images of their own, by placement and frame.
    later: HashMap<(Placement, usize), Held>,
    /// The frame each of those animations shows now.
    shown_frames: HashMap<ImageKey, usize>,
    /// The animated images sent with their frames, which the terminal
    /// holds stopped on the first.
    fresh: Vec<ImageKey>,
}

/// An image the terminal holds.
#[derive(Debug, Clone, Copy)]
struct Held {
    id: ImageId,
    /// The pixels of the cells it was scaled to fit.
    cell: (u16, u16),
    /// Whether `id` is known to be that of the pixels as they are now:
    /// not after they may have changed, until the image is scaled again.
    checked: bool,
    /// Whether the terminal is known to have been sent the image: not
    /// after it may have lost it, until it is sent again.
    sent: bool,
}

impl Encoder {
    /// An encoder that has sent nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The id of the image `placement` shows, from `images`, on a terminal
    /// with `caps`: sent to `out` first unless the terminal holds it. An
    /// image sent for `placement` before, which the pixels no longer
    /// give, is deleted.
    pub fn id(
        &mut self,
        placement: &Placement,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        // An animation kiri plays shows the image of its current frame.
        if self.playback == Playback::Redraw
            && let Some(&index) = self.shown_frames.get(&placement.key)
            && index > 0
        {
            return self.frame_id(placement, index, images, caps, out);
        }
        self.still_id(placement, images, caps, out)
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
        let old = self.held.get(placement).filter(|held| held.cell == cell);
        if let Some(held) = old.filter(|held| held.checked && held.sent) {
            return Ok(held.id);
        }
        let mut scaled = Scaled::new(
            placement,
            images,
            (&self.pages, &self.overlays),
            cell,
            caps.image_run,
            frame_of(placement, caps),
        )?;
        if matches!(self.playback, Playback::Terminal | Playback::Started { .. }) {
            scaled.add_frames(
                placement,
                images,
                (&self.pages, &self.overlays),
                cell,
                caps,
                self.playback,
            );
        }
        let held = Held {
            id: scaled.id,
            cell,
            checked: true,
            sent: true,
        };
        if let Some(old) = self.held.insert(placement.clone(), held) {
            if old.id == scaled.id && old.sent {
                return Ok(scaled.id);
            }
            // At once, not queued as `keep` does: the old id is known to
            // be stale only now that the image has been scaled again.
            if old.id != scaled.id && !self.holds(old.id) {
                graphics::delete(out, old.id)?;
            }
        }
        // Another placement of the same image has it sent too.
        for held in self.held.values_mut().filter(|held| held.id == scaled.id) {
            held.sent = true;
        }
        self.sending(&scaled, caps);
        scaled.send(out)?;
        if !scaled.later.is_empty() {
            self.fresh.push(placement.key.clone());
        }
        Ok(scaled.id)
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
        let (key, control) = match command {
            Command::Start(key, plays) => (key, Control::Start(counted(*plays))),
            Command::Stop(key) => (key, Control::Stop),
            Command::Replay(key, plays) => (key, Control::Replay(counted(*plays))),
            // The terminal counts its frames from 1.
            Command::Show(key, frame) => (key, Control::Show(*frame as u32 + 1)),
        };
        let mut told = Vec::new();
        for (placement, held) in &self.held {
            if placement.key == *key && held.sent && !told.contains(&held.id) {
                told.push(held.id);
            }
        }
        // In the order of the ids, so that what is written does not depend
        // on the map.
        told.sort_by_key(|id| id.value());
        for id in told {
            graphics::control(out, id, control)?;
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
        index: usize,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<ImageId> {
        let cell = cell_size(caps);
        let at = (placement.clone(), index);
        let held = self.later.get(&at).filter(|held| held.cell == cell);
        if let Some(held) = held.filter(|held| held.checked && held.sent) {
            return Ok(held.id);
        }
        let pages = (&self.pages, &self.overlays);
        let Some(frames) = Frames::of(placement, images, pages, cell, caps) else {
            return self.still_id(placement, images, caps, out);
        };
        let pixels = frames.pixels(index.min(frames.animation.frames.len() - 1));
        let scaled = Scaled::of_frame(placement, pixels, caps);
        let held = Held {
            id: scaled.id,
            cell,
            checked: true,
            sent: true,
        };
        // The same pixels under another frame, or placement, are held.
        let sent = self.holds(scaled.id)
            && (self.held.values().chain(self.later.values()))
                .any(|other| other.id == scaled.id && other.sent);
        self.later.insert(at, held);
        if !sent {
            self.sending(&scaled, caps);
            scaled.send(out)?;
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
        self.shown.insert((columns, cell));
        if let Some(&id) = self.bands.get(&(columns, cell)) {
            return Ok(id);
        }
        let band = loading::band(columns, cell, ink, elapsed);
        let cells = graphics::Cells { columns, rows: 1 };
        // Under tmux, where ids are a run's own, no band is drawn.
        let id = ImageId::new(BAND, band.width(), band.height(), cells, None);
        graphics::transmit_only(out, id, &pixels(&band))?;
        self.bands.insert((columns, cell), id);
        Ok(id)
    }

    /// Tells the encoder that the screen was cleared: no band is on it
    /// until one is drawn again, and the terminal no longer holds the
    /// bands, which nothing places any more.
    pub fn cleared(&mut self) {
        self.shown.clear();
        self.bands.clear();
    }

    /// Whether a band is on the screen.
    pub fn has_bands(&self) -> bool {
        !self.shown.is_empty()
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
            .filter_map(|line| line.band.as_ref())
            .map(|band| band.columns)
            .collect();
        self.bands.retain(|&(columns, band_cell), &mut id| {
            let kept = band_cell == cell && bands.contains(&columns);
            if !kept {
                self.unused.push(id);
            }
            kept
        });
        let pages: HashSet<u64> = (placed.iter())
            .filter_map(|placement| match placement.key {
                ImageKey::Page { page, .. } | ImageKey::Band { page, .. } => Some(page),
                _ => None,
            })
            .collect();
        self.pages.retain(|page, _| pages.contains(page));
        let mut gone = Vec::new();
        self.held.retain(|placement, held| {
            let kept = held.cell == cell && placed.contains(placement);
            if !kept {
                gone.push(held.id);
            }
            kept
        });
        self.later.retain(|(placement, _), held| {
            let kept = held.cell == cell && placed.contains(placement);
            if !kept {
                gone.push(held.id);
            }
            kept
        });
        for id in gone {
            if !self.holds(id) && !self.unused.contains(&id) {
                self.unused.push(id);
            }
        }
    }

    /// Deletes from the terminal the images [`Encoder::keep`] let go.
    pub fn delete_unused(&mut self, out: &mut impl Write) -> io::Result<()> {
        // A new frame starts: only the overlays the last one drew are kept,
        // as the parts it showed are drawn anew from them.
        let drawn = std::mem::take(&mut self.drawn);
        self.overlays.retain(|hash, _| drawn.contains(hash));
        for id in self.unused.drain(..) {
            graphics::delete(out, id)?;
        }
        Ok(())
    }

    /// Tells the encoder that the pixels in the images it is given may
    /// have changed, as after a reload: each image is scaled again when it
    /// is next shown, and sent again only when that gives another id.
    pub fn changed(&mut self) {
        for held in self.held.values_mut().chain(self.later.values_mut()) {
            held.checked = false;
        }
    }

    /// The images sent through tmux since this was last asked, with the
    /// cells they were sent for, so that a check can tell whether they
    /// came: tmux drops output for a client that cannot keep up. Empty
    /// without tmux.
    pub fn take_sent(&mut self) -> Vec<(ImageId, graphics::Cells)> {
        std::mem::take(&mut self.sent)
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
        let cell = cell_size(caps);
        if self.again.get(&id).is_some_and(|&tries| tries >= AGAIN) {
            return Ok(());
        }
        let placement = (self.held.iter())
            .find(|(_, held)| held.id == id && held.cell == cell)
            .map(|(placement, _)| placement.clone());
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
            )
            .ok()
        });
        let Some(scaled) = scaled.filter(|scaled| scaled.id == id) else {
            return Ok(());
        };
        *self.again.entry(id).or_default() += 1;
        self.sending(&scaled, caps);
        scaled.send(out)
    }

    /// Notes that `scaled` is sent to a terminal with `caps`, to check
    /// that it came when it goes through tmux.
    fn sending(&mut self, scaled: &Scaled, caps: &Capabilities) {
        let sent = (scaled.id, scaled.cells);
        if caps.tmux_passthrough && !self.sent.contains(&sent) {
            self.sent.push(sent);
        }
    }

    /// Tells the encoder that the terminal may have lost the images it was
    /// sent, as tmux drops what goes to a pane that is not shown: each is
    /// sent again when it is next shown.
    pub fn lost(&mut self) {
        for held in self.held.values_mut().chain(self.later.values_mut()) {
            held.sent = false;
        }
        self.again.clear();
    }

    /// Tells the encoder that the terminal may have lost the images
    /// `ids`, as [`Encoder::lost`] does for all of them.
    pub fn lost_some(&mut self, ids: &[ImageId]) {
        let held = self.held.values_mut().chain(self.later.values_mut());
        for held in held.filter(|held| ids.contains(&held.id)) {
            held.sent = false;
        }
        self.again.retain(|id, _| !ids.contains(id));
    }

    /// Deletes from the terminal every image it holds, as when the pager
    /// quits.
    pub fn delete_all(&mut self, out: &mut impl Write) -> io::Result<()> {
        self.unused.extend(self.bands.drain().map(|(_, id)| id));
        let later = self.later.drain().map(|(_, held)| held);
        for held in self.held.drain().map(|(_, held)| held).chain(later) {
            if !self.unused.contains(&held.id) {
                self.unused.push(held.id);
            }
        }
        self.delete_unused(out)
    }

    /// Whether some placement still has image `id`.
    fn holds(&self, id: ImageId) -> bool {
        (self.held.values().chain(self.later.values())).any(|held| held.id == id)
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
    /// For one that is printed, how often it plays, from the start.
    start: Option<Plays>,
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
                // A part with an animated picture has its first frame in it,
                // and the frame round the picture.
                let pictures = parts.animated(images);
                let outline = frame.as_ref().map(|(rgb, _)| (*rgb, line_of(cell)));
                outlined = outline.is_some() && !parts.in_part(slice, &pictures).is_empty();
                match parts.in_part(slice, &pictures).is_empty() {
                    true => parts.draw(slice, overlay),
                    false => parts.draw_frame(slice, overlay, &pictures, (0, 0), outline),
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
        if let Some(frame) = frame.filter(|_| !outlined) {
            framed(&mut pixels, cell, frame);
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
            start: None,
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
            start: None,
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
        playback: Playback,
    ) {
        let Some(frames) = Frames::of(placement, images, pages, cell, caps) else {
            return;
        };
        let animation = &frames.animation;
        if placement.shows != Shows::Image || animation.frames.len() < 2 {
            return;
        }
        self.first_gap = animation.frames[0].gap;
        // A printed animation starts as it is sent, after the start delay,
        // which is the first frame's gap until the terminal plays it again.
        if let Playback::Started { once } = playback {
            self.first_gap = START_DELAY;
            self.start = Some(match once {
                true => Plays::Times(1),
                false => animation.plays,
            });
        }
        let mut content = self.pixels.as_raw().clone();
        for (at, frame_of) in animation.frames.iter().enumerate().skip(1) {
            self.later.push((frames.pixels(at), frame_of.gap));
        }
        for frame in &animation.frames {
            content.extend_from_slice(&frame.gap.as_millis().to_le_bytes());
        }
        let (width, height) = self.pixels.dimensions();
        self.id = ImageId::new(&content, width, height, self.cells, caps.image_run);
    }

    fn send(&self, out: &mut impl Write) -> io::Result<()> {
        send(out, self.id, &self.pixels, self.cells)?;
        for (pixels, gap) in &self.later {
            graphics::transmit_frame(out, self.id, &self::pixels(pixels), *gap)?;
        }
        if !self.later.is_empty() {
            graphics::first_frame_gap(out, self.id, self.first_gap)?;
        }
        if let Some(plays) = self.start {
            graphics::control(out, self.id, Control::Start(counted(plays)))?;
        }
        Ok(())
    }
}

/// How many times an image is sent again at most for being lost.
pub const AGAIN: u8 = 3;

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

/// A frame of `rgb` along the edge of the cells of the canvas `pixels`
/// in `rows`, as wide as a twentieth of the cell's height, and at least a
/// pixel: the cells that a marked image is on, outlined.
fn framed(pixels: &mut RgbaImage, cell: (u16, u16), (rgb, rows): ([u8; 3], Range<u16>)) {
    let line = ((u32::from(cell.1) + 10) / 20).max(1);
    let (width, height) = pixels.dimensions();
    let top = u32::from(rows.start) * u32::from(cell.1);
    let bottom = (u32::from(rows.end) * u32::from(cell.1)).min(height);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        if y < top || y >= bottom {
            continue;
        }
        if x < line || y < top + line || x + line >= width || y + line >= bottom {
            *pixel = image::Rgba([rgb[0], rgb[1], rgb[2], 255]);
        }
    }
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
                None
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
            over_budget: None,
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

    fn sent_animation(playback: Playback) -> (Encoder, ImageId, Vec<u8>) {
        let mut images = Images::new();
        images.insert("a.gif", Ok(animated([100, 200, 300], Plays::Forever)));
        let mut encoder = Encoder::new();
        encoder.play(playback);
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
    fn a_printed_animation_is_started_at_once_after_the_start_delay_of_its_first_frame() {
        let sent = |once, plays| {
            let mut images = Images::new();
            images.insert("a.gif", Ok(animated([100, 200, 300], plays)));
            let mut encoder = Encoder::new();
            encoder.play(Playback::Started { once });
            let mut out = Vec::new();
            let id = encoder
                .id(
                    &placed("a.gif", 2, 1, Shows::Image),
                    &images,
                    &caps(),
                    &mut out,
                )
                .unwrap();
            let sent = sequences(&out);
            (id.value(), sent)
        };
        let (id, once) = sent(true, Plays::Forever);
        let first: Vec<&str> = once.iter().map(|s| keys(s)).collect();
        assert_eq!(first.len(), 5, "{first:?}");
        assert!(first[0].starts_with("a=T,U=1,"));
        // The start delay is the first frame's gap, and one play is a loop
        // count of two.
        assert_eq!(first[3], format!("a=a,q=2,i={id},r=1,z=750"));
        assert_eq!(first[4], format!("a=a,q=2,i={id},s=3,v=2"));
        // As often as the file says: without end, or this many times.
        let (id, always) = sent(false, Plays::Forever);
        assert_eq!(keys(&always[4]), format!("a=a,q=2,i={id},s=3,v=1"));
        let (id, three) = sent(false, Plays::Times(3));
        assert_eq!(keys(&three[4]), format!("a=a,q=2,i={id},s=3,v=4"));
    }

    #[test]
    fn what_kiri_plays_is_sent_a_frame_at_a_time_each_under_an_id_of_its_own() {
        let mut images = Images::new();
        images.insert("a.gif", Ok(animated([100, 200, 300], Plays::Forever)));
        let placement = placed("a.gif", 2, 1, Shows::Image);
        let mut encoder = Encoder::new();
        encoder.play(Playback::Redraw);
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
    fn frames_that_the_terminal_plays_are_not_sent_one_at_a_time() {
        let mut images = Images::new();
        images.insert("a.gif", Ok(animated([100, 200, 300], Plays::Forever)));
        let placement = placed("a.gif", 2, 1, Shows::Image);
        let mut encoder = Encoder::new();
        encoder.play(Playback::Terminal);
        encoder.show_frames(HashMap::from([(ImageKey::from("a.gif"), 1)]));
        let mut out = Vec::new();
        let first = encoder.id(&placement, &images, &caps(), &mut out).unwrap();
        let again = encoder.id(&placement, &images, &caps(), &mut out).unwrap();
        assert_eq!(first, again);
        assert_eq!(sequences(&out).len(), 4);
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
            .control(&Command::Start(key.clone(), Plays::Times(1)), &mut out)
            .unwrap();
        encoder
            .control(&Command::Stop(key.clone()), &mut out)
            .unwrap();
        encoder
            .control(&Command::Replay(key.clone(), Plays::Forever), &mut out)
            .unwrap();
        encoder.control(&Command::Show(key, 2), &mut out).unwrap();
        let id = id.value();
        let expected = [
            format!("a=a,q=2,i={id},s=3,v=2"),
            format!("a=a,q=2,i={id},s=1"),
            format!("a=a,q=2,i={id},c=1,s=3,v=1"),
            format!("a=a,q=2,i={id},c=3"),
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
        Scaled::new(placement, &images, pages, cell, None, frame)
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
            let scaled = Scaled::new(placement, &images, pages, (10, 20), None, frame).unwrap();
            [(0, 0), (9, 19), (0, 10), (5, 0)].map(|(x, y)| scaled.pixels.get_pixel(x, y).0)
        };
        // Unmarked, the edge is the image's own; marked, the frame.
        assert_eq!(at(&placement), [[255, 0, 0, 255]; 4]);
        placement.frame = Some(Frame {
            color: crate::typeset::color::Color::Rgb(1, 2, 3),
            rows: 0..1,
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
        )
        .unwrap();
        let (width, height) = scaled.pixels.dimensions();
        let stable = ImageId::new(scaled.pixels.as_raw(), width, height, scaled.cells, None);
        assert_eq!(id(None), stable);
        assert_ne!(id(Some(1)), stable);
        assert_ne!(id(Some(1)), id(Some(2)));
    }
}
