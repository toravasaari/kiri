//! Animated pictures in reading mode. A picture there is drawn into the
//! image of the part of the page it is in, so an animation of it is an
//! animation of that part: the part drawn again with each frame of the
//! picture.
//!
//! The page holds a hole where an animated picture goes: a transparent
//! image of the size it is shown at, whose first pixels carry the picture's
//! id. The parts that have one are drawn with the picture's frames laid in
//! the hole, by whoever draws them and has the frames.

use std::sync::Arc;

use image::{Rgba, RgbaImage, imageops};

use super::engine::{PART_ROWS, Parts};
use super::page::Shape;
use crate::typeset::images::animation::{Animation, FRAME_BUDGET};
use crate::typeset::images::{ImageKey, Images, Source};

/// The id of the animated picture `key` shown `size` pixels large, which
/// its hole carries.
fn hole_id(key: &ImageKey, size: (u32, u32)) -> u64 {
    // FNV-1a, as the image ids are made.
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    let text = format!("{key}{size:?}");
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01B3);
    }
    hash
}

/// The picture `source` of the image `key` at `size`, for a page: a hole
/// for an animation, which is drawn into the page's parts when they are
/// sent, and the image itself for others.
pub(super) fn render(source: &Source, key: &ImageKey, size: (u32, u32)) -> RgbaImage {
    let first = source.render(size.0, size.1);
    let animated = matches!(source, Source::Animated(animation) if animation.frames.len() > 1);
    if !animated || size.0 * size.1 < 3 {
        return first;
    }
    // Almost nothing: the id in three pixels of an alpha of one.
    let mut hole = RgbaImage::new(size.0, size.1);
    let id = hole_id(key, size).to_le_bytes();
    for (at, bytes) in id.chunks(3).enumerate() {
        let mut pixel = [0, 0, 0, 1];
        pixel[..bytes.len()].copy_from_slice(bytes);
        hole.put_pixel(at as u32 % size.0, at as u32 / size.0, Rgba(pixel));
    }
    hole
}

/// The id a hole carries, if `pixels` is one.
fn id_of(pixels: &RgbaImage) -> Option<u64> {
    let (width, height) = pixels.dimensions();
    if width * height < 3 {
        return None;
    }
    let mut id = [0u8; 8];
    for at in 0..3u32 {
        let Rgba(pixel) = *pixels.get_pixel(at % width, at / width);
        if pixel[3] != 1 {
            return None;
        }
        for (n, byte) in pixel[..3].iter().enumerate() {
            if let Some(slot) = id.get_mut(at as usize * 3 + n) {
                *slot = *byte;
            }
        }
    }
    Some(u64::from_le_bytes(id))
}

/// What plays on a page: the picture that drives each part, with its top in
/// the part, and the animations that stay on their first frame.
#[derive(Debug, Default)]
pub struct Players {
    pub by_part: std::collections::HashMap<u32, (Pic, f32)>,
    pub still: Vec<(Pic, Why)>,
}

/// Why an animated picture stays on its first frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The frames, a part of the page for each, take this many bytes in the
    /// largest part it is in, over the budget.
    Budget(u64),
    /// Another animation is first in a part it is in.
    SharesPart,
}

/// An animated picture on a page: where it is, in pixels from the top left
/// of the page, and its animation.
#[derive(Debug, Clone)]
pub struct Pic {
    pub key: ImageKey,
    pub animation: Arc<Animation>,
    pub x: f32,
    pub y: f32,
    pub width: u32,
    pub height: u32,
}

/// Notes in `images` the animations on the page `parts` that stay on their
/// first frame, and why: once for each picture and size, whatever number of
/// layouts say it.
pub(super) fn note_stills(parts: &Parts, images: &Images) {
    let pictures = parts.animated(images);
    for (pic, why) in parts.plays(&pictures, FRAME_BUDGET).still {
        let text = match why {
            Why::Budget(bytes) => format!(
                "its frames take {} MiB in the parts of the page it is in, over the limit of \
                 {} MiB: only the first frame shows",
                bytes.div_ceil(1024 * 1024),
                FRAME_BUDGET / (1024 * 1024)
            ),
            Why::SharesPart => "another animation is ahead of it in a part of the page it is in: \
                 only its first frame shows"
                .to_owned(),
        };
        images.note_still(&pic.key, (pic.width, pic.height), text);
    }
}

impl Parts {
    /// The height of a cell in pixels.
    pub fn cell_height(&self) -> f32 {
        f32::from(self.cell.1)
    }

    /// The animated pictures on the page, from the top, taken from the
    /// `images` that hold their animations.
    pub fn animated(&self, images: &Images) -> Vec<Pic> {
        let mut found = Vec::new();
        for item in &self.page.items {
            let Shape::Picture { x, y, pixels } = &item.shape else {
                continue;
            };
            let Some(id) = id_of(pixels) else { continue };
            let size = pixels.dimensions();
            let animation = images.iter().find_map(|(key, image)| match image {
                Ok(Source::Animated(animation)) if hole_id(key, size) == id => {
                    Some((key.clone(), Arc::clone(animation)))
                }
                _ => None,
            });
            if let Some((key, animation)) = animation {
                let (width, height) = size;
                found.push(Pic {
                    key,
                    animation,
                    x: *x,
                    y: *y,
                    width,
                    height,
                });
            }
        }
        found
    }

    /// The pictures of `pictures` that are in part `part`, with their
    /// top as pixels from the part's top.
    pub fn in_part(&self, part: u32, pictures: &[Pic]) -> Vec<(Pic, f32)> {
        let cell = f32::from(self.cell.1);
        let top = (part * u32::from(PART_ROWS)) as f32 * cell;
        let rows = (u32::from(self.rows) - part * u32::from(PART_ROWS)).min(u32::from(PART_ROWS));
        let bottom = top + rows as f32 * cell;
        (pictures.iter())
            .filter(|pic| pic.y < bottom && pic.y + pic.height as f32 > top)
            .map(|pic| (pic.clone(), pic.y - top))
            .collect()
    }

    /// How many parts of the page there are.
    fn part_count(&self) -> u32 {
        u32::from(self.rows).div_ceil(u32::from(PART_ROWS))
    }

    /// The bytes of part `part` as an image.
    fn part_bytes(&self, part: u32) -> u64 {
        let rows = (u32::from(self.rows) - part * u32::from(PART_ROWS)).min(u32::from(PART_ROWS));
        u64::from(self.columns)
            * u64::from(self.cell.0)
            * u64::from(rows)
            * u64::from(self.cell.1)
            * 4
    }

    /// Which of the `pictures` that are animations play and which stay on
    /// their first frame, with `budget` bytes for the frames of a part. A
    /// picture plays only when it drives every part it is in, so that it is
    /// never torn; a part drives one picture, the first that can play in it.
    /// That is decided in rounds: a picture over the budget in some part
    /// stays still; of the rest, one that is first in a part but not in all
    /// of its parts is dropped, the topmost first, and the others are looked
    /// at again without it, until nothing changes, so that a picture that cannot play does not
    /// keep one that can from playing. Those left are first in all of their
    /// parts, and play, or in none, and stay still behind them.
    pub fn plays(&self, pictures: &[Pic], budget: u64) -> Players {
        let mut plays = Players::default();
        let same = |a: &Pic, b: &Pic| a.y == b.y && a.x == b.x;
        let animations: Vec<&Pic> = (pictures.iter())
            .filter(|pic| pic.animation.frames.len() > 1)
            .collect();
        let parts_of = |pic: &Pic| -> Vec<u32> {
            (0..self.part_count())
                .filter(|part| {
                    self.in_part(*part, pictures)
                        .iter()
                        .any(|(p, _)| same(p, pic))
                })
                .collect()
        };
        let mut candidates: Vec<&Pic> = Vec::new();
        for pic in animations.iter().copied() {
            let frames = pic.animation.frames.len() as u64;
            let bytes = (parts_of(pic).iter())
                .map(|part| frames * self.part_bytes(*part))
                .max();
            match bytes {
                Some(bytes) if bytes > budget => {
                    plays.still.push((pic.clone(), Why::Budget(bytes)))
                }
                _ => candidates.push(pic),
            }
        }
        // The first of the candidates in a part.
        let first_in = |part: u32, candidates: &[&Pic]| -> Option<Pic> {
            (self.in_part(part, pictures).into_iter())
                .map(|(pic, _)| pic)
                .find(|pic| candidates.iter().any(|c| same(c, pic)))
        };
        // One at a time, the topmost first: what a picture that cannot play
        // was in the way of is looked at again without it.
        while let Some(at) = candidates.iter().position(|pic| {
            let parts = parts_of(pic);
            let leads = |part: &u32| first_in(*part, &candidates).is_some_and(|f| same(&f, pic));
            parts.iter().any(leads) && !parts.iter().all(leads)
        }) {
            let pic = candidates.remove(at);
            plays.still.push((pic.clone(), Why::SharesPart));
        }
        for pic in candidates.iter().copied() {
            let parts = parts_of(pic);
            let leads = |part: &u32| first_in(*part, &candidates).is_some_and(|f| same(&f, pic));
            // A picture that is in no part is not on the page to play or note.
            if parts.is_empty() {
                continue;
            }
            if !parts.iter().all(leads) {
                // Behind one that plays, in every part it is in.
                plays.still.push((pic.clone(), Why::SharesPart));
                continue;
            }
            plays.by_part.extend(parts.iter().map(|part| {
                let top = (self.in_part(*part, pictures).into_iter())
                    .find(|(p, _)| same(p, pic))
                    .map_or(0.0, |(_, top)| top);
                (*part, (pic.clone(), top))
            }));
        }
        plays
    }

    /// Part `part` with the first frame of each animated picture in it, or,
    /// of the one that `frame` names the frame of, that frame. `outline`
    /// puts a rectangle of that colour and thickness round the picture of
    /// that number among those in the part.
    pub fn draw_frame(
        &self,
        part: u32,
        overlay: Option<&super::page::Overlay>,
        pictures: &[Pic],
        frame: (usize, usize),
        outline: Option<([u8; 3], u32)>,
    ) -> RgbaImage {
        let mut image = self.draw(part, overlay);
        let here = self.in_part(part, pictures);
        for (number, (pic, top)) in here.iter().enumerate() {
            let index = if number == frame.0 { frame.1 } else { 0 };
            let shown = pic.animation.render(index, pic.width, pic.height);
            let (x, y) = (pic.x.round() as i64, top.round() as i64);
            imageops::overlay(&mut image, &shown, x, y);
            if let Some((rgb, line)) = outline.filter(|_| number == frame.0) {
                rectangle(&mut image, (x, y, pic.width, pic.height), rgb, line);
            }
        }
        image
    }
}

/// A rectangle outline `line` pixels thick round `(x, y, width, height)`,
/// inside it.
fn rectangle(
    image: &mut RgbaImage,
    (x, y, width, height): (i64, i64, u32, u32),
    rgb: [u8; 3],
    line: u32,
) {
    let colour = Rgba([rgb[0], rgb[1], rgb[2], 255]);
    let line = i64::from(line.max(1));
    for dy in 0..i64::from(height) {
        for dx in 0..i64::from(width) {
            let edge = dx < line
                || dy < line
                || dx + line >= i64::from(width)
                || dy + line >= i64::from(height);
            let (px, py) = (x + dx, y + dy);
            if edge
                && px >= 0
                && py >= 0
                && (px as u32) < image.width()
                && (py as u32) < image.height()
            {
                image.put_pixel(px as u32, py as u32, colour);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::render::Inputs;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::images::animation::{Frame, Plays};

    /// The page of a paragraph with the animation `a.gif` of `frames` frames.
    fn page_with(frames: usize) -> (Rc<Parts>, Images) {
        let frame = Frame {
            image: RgbaImage::from_pixel(20, 20, Rgba([255, 0, 0, 255])),
            gap: std::time::Duration::from_millis(100),
        };
        let animation = Animation {
            frames: vec![frame; frames],
            plays: Plays::Forever,
            over_budget: None,
        };
        let mut images = Images::new();
        images.insert("a.gif", Ok(Source::Animated(Arc::new(animation))));
        let mut inputs = Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(80)
        });
        inputs.reading = Some(crate::typeset::reading::engine::Reading {
            pictures: Default::default(),
            fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
        });
        inputs.images = images;
        let document = crate::typeset::parse::parse("![a](a.gif)\n");
        let lines = crate::typeset::layout::layout(&document, &inputs);
        let parts = Rc::clone(lines[0].image.as_ref().unwrap().page.as_ref().unwrap());
        (parts, inputs.images)
    }

    #[test]
    fn the_part_of_a_page_with_an_animation_plays_it_while_the_frames_fit_the_budget() {
        let (parts, images) = page_with(3);
        let pictures = parts.animated(&images);
        assert_eq!(pictures.len(), 1);
        let (width, height) = parts.draw(0, None).dimensions();
        let part = u64::from(width) * u64::from(height) * 4;
        assert!(parts.plays(&pictures, 3 * part).by_part.contains_key(&0));
        assert!(parts.plays(&pictures, 3 * part - 1).by_part.is_empty());
    }

    #[test]
    fn a_still_picture_leaves_no_hole_and_an_animation_does() {
        let (parts, images) = page_with(2);
        assert_eq!(parts.animated(&images).len(), 1);
        let (still, images) = page_with(1);
        assert!(still.animated(&images).is_empty());
    }
}
