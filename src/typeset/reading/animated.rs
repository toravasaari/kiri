//! Animated pictures in reading mode. A picture there is drawn into the
//! image of the part of the page it is in; one that plays is an image of
//! its own over the part, its inset: the cells the picture lies on, with
//! the page under it, drawn again with each frame of the picture.
//!
//! The page holds a hole where an animated picture goes: a transparent
//! image of the size it is shown at, whose first pixels carry the picture's
//! id. The parts that have one are drawn with the picture's frames laid in
//! the hole, by whoever draws them and has the frames.

use std::rc::Rc;
use std::sync::Arc;

use image::{Rgba, RgbaImage, imageops};

use super::engine::{PART_ROWS, Parts};
use super::page::{Page, Shape};
use crate::typeset::images::animation::{Animation, FRAME_BUDGET};
use crate::typeset::images::marks::{self, Mark};
use crate::typeset::images::{ImageKey, Images, Source};

/// The id of the animated picture `key` shown `size` pixels large, which
/// its hole carries.
pub fn hole_id(key: &ImageKey, size: (u32, u32)) -> u64 {
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
pub(super) fn id_of(pixels: &RgbaImage) -> Option<u64> {
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

/// The animation an image of `images` has, if `pixels` is its hole at
/// the size the hole has.
fn animation_in(images: &Images, pixels: &RgbaImage) -> Option<(ImageKey, Arc<Animation>)> {
    let id = id_of(pixels)?;
    let size = pixels.dimensions();
    images.iter().find_map(|(key, image)| match image {
        Ok(Source::Animated(animation)) if hole_id(key, size) == id => {
            Some((key.clone(), Arc::clone(animation)))
        }
        _ => None,
    })
}

/// Puts the first frame in the place of the holes that stay holes: those in
/// the bands of `page`, which scroll as images of their own and play no
/// animation, and those in a line of text, which is not a picture of its
/// own that frames could be laid in. An animation there stays on its first
/// frame. The holes of pictures on their own, in the page itself, are
/// filled when its parts are drawn.
pub(super) fn show_first_frames(page: &mut Page, images: &Images) {
    let first = |pixels: &mut Rc<RgbaImage>| {
        if let Some((_, animation)) = animation_in(images, pixels) {
            let (width, height) = pixels.dimensions();
            *pixels = Rc::new(animation.render(0, width, height));
        }
    };
    let on_page = page.items.iter_mut().map(|item| (false, item));
    let in_bands =
        (page.bands.iter_mut()).flat_map(|band| band.items.iter_mut().map(|i| (true, i)));
    for (band, item) in on_page.chain(in_bands) {
        match &mut item.shape {
            Shape::Text { paragraph, .. } => {
                (paragraph.pictures.iter_mut()).for_each(|picture| first(&mut picture.pixels));
            }
            Shape::Picture { pixels, .. } if band => first(pixels),
            _ => {}
        }
    }
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

/// Where an animated picture lies in a part of a page, spread out to whole
/// cells: in pixels from the part's top left corner, and as the columns and
/// rows of the part's cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub columns: std::ops::Range<u16>,
    pub rows: std::ops::Range<u16>,
}

/// Notes in `images` the animations on the page `parts` that stay on their
/// first frame, and why: once for each picture and size, whatever number of
/// layouts say it. A picture stays still when its frames would not fit the
/// memory budget, and the note says it so that the reader knows why it has
/// no play button.
pub(super) fn note_stills(parts: &Parts, images: &Images) {
    let pictures = parts.animated(images);
    for (pic, bytes) in parts.unfit(&pictures, FRAME_BUDGET) {
        let text = format!(
            "shows only its first frame: its frames would take {} MiB, over the limit of {} MiB",
            bytes.div_ceil(1024 * 1024),
            FRAME_BUDGET / (1024 * 1024)
        );
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
            let size = pixels.dimensions();
            let animation = animation_in(images, pixels);
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

    /// The rectangle of picture `pic`, whose top is `top` pixels below the
    /// top of part `part`, in that part: the picture's place rounded to
    /// whole pixels as it is drawn, spread out to whole cells and cut at the
    /// edges of the part. `None` when nothing of the picture is drawn in the
    /// part, as for one whose bottom reaches less than half a pixel into it.
    pub fn rect(&self, part: u32, pic: &Pic, top: f32) -> Option<Rect> {
        let (cell_width, cell_height) = (i64::from(self.cell.0), i64::from(self.cell.1));
        let rows = (u32::from(self.rows) - part * u32::from(PART_ROWS)).min(u32::from(PART_ROWS));
        let (width, height) = (
            i64::from(self.columns) * cell_width,
            i64::from(rows) * cell_height,
        );
        let (left, top) = (pic.x.round() as i64, top.round() as i64);
        let (right, bottom) = (left + i64::from(pic.width), top + i64::from(pic.height));
        let (left, top) = (left.max(0), top.max(0));
        let (right, bottom) = (right.min(width), bottom.min(height));
        if left >= right || top >= bottom {
            return None;
        }
        let columns = left / cell_width..(right + cell_width - 1) / cell_width;
        let rows = top / cell_height..(bottom + cell_height - 1) / cell_height;
        Some(Rect {
            x: (columns.start * cell_width) as u32,
            y: (rows.start * cell_height) as u32,
            width: ((columns.end - columns.start) * cell_width) as u32,
            height: ((rows.end - rows.start) * cell_height) as u32,
            columns: columns.start as u16..columns.end as u16,
            rows: rows.start as u16..rows.end as u16,
        })
    }

    /// The pictures of `pictures` that are animations whose frames would
    /// not fit `budget` bytes, with the bytes they would take: all their
    /// frames, as the rectangle of each part of the page the picture is in
    /// (`Parts::rect`), added together. What it takes does not depend on the
    /// width of the window or the length of the page, and is at least what
    /// any one part's build takes. The others can play.
    pub fn unfit(&self, pictures: &[Pic], budget: u64) -> Vec<(Pic, u64)> {
        let same = |a: &Pic, b: &Pic| a.y == b.y && a.x == b.x;
        let mut unfit = Vec::new();
        for pic in (pictures.iter()).filter(|pic| pic.animation.frames.len() > 1) {
            let frames = pic.animation.frames.len() as u64;
            let cells: u64 = (0..self.part_count())
                .flat_map(|part| {
                    let here = self.in_part(part, pictures);
                    let found = here.into_iter().find(|(other, _)| same(other, pic));
                    found.and_then(|(other, top)| self.rect(part, &other, top))
                })
                .map(|rect| u64::from(rect.width) * u64::from(rect.height) * 4)
                .sum();
            let bytes = frames * cells;
            if bytes > budget {
                unfit.push((pic.clone(), bytes));
            }
        }
        unfit
    }

    /// Part `part` with each animated picture in it as `drawn` says: the frame
    /// it shows, `usize::MAX` for none, and the mark over it. `outline` puts a rectangle of that colour
    /// and thickness round the picture of that number among those in the part.
    pub fn draw_frame(
        &self,
        part: u32,
        overlay: Option<&super::page::Overlay>,
        pictures: &[Pic],
        drawn: &dyn Fn(&Pic) -> (usize, Option<Mark>),
        outline: Option<(usize, [u8; 3], u32)>,
    ) -> RgbaImage {
        let mut image = self.draw(part, overlay);
        let here = self.in_part(part, pictures);
        for (number, (pic, top)) in here.iter().enumerate() {
            let (index, mark) = drawn(pic);
            // A frame past the last leaves the picture out.
            if index == usize::MAX {
                continue;
            }
            let mut shown = pic.animation.render(index, pic.width, pic.height);
            if let Some(mark) = mark {
                marks::draw(&mut shown, (0, 0, pic.width, pic.height), mark);
            }
            let (x, y) = (pic.x.round() as i64, top.round() as i64);
            imageops::overlay(&mut image, &shown, x, y);
            if let Some((_, rgb, line)) = outline.filter(|(at, _, _)| *at == number) {
                rectangle(&mut image, (x, y, pic.width, pic.height), rgb, line);
            }
        }
        image
    }
}

/// A rectangle outline `line` pixels thick round `(x, y, width, height)`,
/// inside it.
pub(crate) fn rectangle(
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
    use crate::typeset::reading::page::Wide;

    /// The page of a paragraph with the animation `a.gif` of `frames` frames,
    /// in a window of `width` columns of 10 × 20 pixels.
    fn page_in(width: u16, frames: usize) -> (Rc<Parts>, Images) {
        let frame = Frame {
            image: RgbaImage::from_pixel(20, 20, Rgba([255, 0, 0, 255])),
            gap: std::time::Duration::from_millis(100),
        };
        let animation = Animation {
            frames: vec![frame; frames],
            plays: Plays::Forever,
            over_budget: false,
        };
        let mut images = Images::new();
        images.insert("a.gif", Ok(Source::Animated(Arc::new(animation))));
        let mut inputs = Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(width)
        });
        inputs.reading = Some(crate::typeset::reading::engine::Reading {
            pictures: Default::default(),
            fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
        });
        inputs.images = images;
        let document = crate::typeset::parse::parse("![a](a.gif)\n");
        let lines = crate::typeset::driver::layout(&document, &inputs);
        let parts = Rc::clone(lines[0].image.as_ref().unwrap().page.as_ref().unwrap());
        (parts, inputs.images)
    }

    fn page_with(frames: usize) -> (Rc<Parts>, Images) {
        page_in(80, frames)
    }

    #[test]
    fn an_animation_is_unfit_when_its_frames_do_not_fit_the_budget_in_the_cells_it_lies_on() {
        let (parts, images) = page_with(3);
        let pictures = parts.animated(&images);
        assert_eq!(pictures.len(), 1);
        // 20 × 20 pixels on cells of 10 × 20: two columns, one row.
        let rect = parts.rect(0, &pictures[0], pictures[0].y).unwrap();
        let cells = u64::from(rect.width) * u64::from(rect.height) * 4;
        assert_eq!(cells, 20 * 20 * 4);
        assert!(parts.unfit(&pictures, 3 * cells).is_empty());
        let unfit = parts.unfit(&pictures, 3 * cells - 1);
        assert_eq!(unfit.len(), 1);
        assert_eq!(unfit[0].1, 3 * cells);
    }

    #[test]
    fn what_an_animation_takes_does_not_depend_on_the_width_of_the_window() {
        let take = |width: u16| {
            let (parts, images) = page_in(width, 3);
            let pictures = parts.animated(&images);
            let unfit = parts.unfit(&pictures, 0);
            assert_eq!(unfit.len(), 1);
            unfit[0].1
        };
        // Three frames of the rectangle, 20 × 20 pixels, in 80 and in 300
        // columns.
        assert_eq!(take(80), 3 * 20 * 20 * 4);
        assert_eq!(take(300), take(80));
    }

    #[test]
    fn an_animation_in_two_parts_takes_the_rectangles_of_both() {
        let (parts, images) = long_page();
        let mut pic = parts.animated(&images)[0].clone();
        (pic.width, pic.height) = (20, 40);
        // 20 pixels in the first part and 20 in the second, 2 frames.
        pic.y = 580.0;
        let two = |at: &Pic| {
            let unfit = parts.unfit(std::slice::from_ref(at), 0);
            assert_eq!(unfit.len(), 1);
            unfit[0].1
        };
        assert_eq!(two(&pic), 2 * (20 * 20 * 4 + 20 * 20 * 4));
    }

    fn is_red(pixel: &Rgba<u8>) -> bool {
        pixel.0[0] > 200 && pixel.0[1] < 50 && pixel.0[3] == 255
    }

    /// The page of `markdown`, which shows the animation `a.gif`, red, laid
    /// out with `wide`.
    fn red_animation_page(markdown: &str, wide: Wide) -> (Rc<Parts>, Images) {
        let frame = Frame {
            image: RgbaImage::from_pixel(20, 20, Rgba([255, 0, 0, 255])),
            gap: std::time::Duration::from_millis(100),
        };
        let animation = Animation {
            frames: vec![frame; 2],
            plays: Plays::Forever,
            over_budget: false,
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
            wide,
        });
        inputs.images = images;
        let document = crate::typeset::parse::parse(markdown);
        let lines = crate::typeset::driver::layout(&document, &inputs);
        let parts = (lines.iter())
            .find_map(|line| line.image.as_ref()?.page.clone())
            .expect("a page");
        (parts, inputs.images)
    }

    #[test]
    fn an_animation_in_a_band_shows_its_first_frame() {
        let (long_a, long_b) = ("a".repeat(200), "b".repeat(200));
        let table = format!("| {long_a} | {long_b} |\n|---|---|\n| ![a](a.gif) | x |\n");
        let (parts, _images) = red_animation_page(&table, Wide::Scroll);
        assert_eq!(parts.page.bands.len(), 1, "the table scrolls");
        let band = parts.draw_band(0, 0, 0, false, None);
        assert!(
            band.pixels()
                .any(|p| p.0[0] > 200 && p.0[1] < 50 && p.0[3] == 255),
            "no pixel of the red first frame in the band"
        );
    }

    #[test]
    fn an_animation_in_a_line_of_text_shows_its_first_frame() {
        let (parts, _images) = red_animation_page("Text ![a](a.gif) text.\n", Wide::Wrap);
        assert!(
            parts.draw(0, None).pixels().any(is_red),
            "no pixel of the red first frame in the line"
        );
    }

    #[test]
    fn a_still_picture_leaves_no_hole_and_an_animation_does() {
        let (parts, images) = page_with(2);
        assert_eq!(parts.animated(&images).len(), 1);
        let (still, images) = page_with(1);
        assert!(still.animated(&images).is_empty());
    }

    /// The page of the picture `a.gif` with forty paragraphs under it, which
    /// fill its first part and go on into the second.
    fn long_page() -> (Rc<Parts>, Images) {
        let markdown = format!("![a](a.gif)\n\n{}", "Text.\n\n".repeat(40));
        red_animation_page(&markdown, Wide::Wrap)
    }

    fn rect(x: u32, y: u32, width: u32, height: u32) -> Option<Rect> {
        Some(Rect {
            x,
            y,
            width,
            height,
            columns: (x / 10) as u16..((x + width) / 10) as u16,
            rows: (y / 20) as u16..((y + height) / 20) as u16,
        })
    }

    #[test]
    fn a_picture_s_rect_is_its_place_rounded_and_spread_out_to_whole_cells() {
        let (parts, images) = long_page();
        let mut pic = parts.animated(&images)[0].clone();
        (pic.width, pic.height) = (20, 30);
        // 19.4 rounds to 19: 19 to 39 across, in the cells of 10 to 40.
        pic.x = 19.4;
        assert_eq!(parts.rect(0, &pic, 19.4), rect(10, 0, 30, 60));
        // 19.6 rounds to 20: 20 to 40, and down 20 to 50.
        pic.x = 19.6;
        assert_eq!(parts.rect(0, &pic, 19.6), rect(20, 20, 20, 40));
    }

    #[test]
    fn a_picture_s_rect_is_cut_at_the_edges_of_its_part() {
        let (parts, images) = long_page();
        let mut pic = parts.animated(&images)[0].clone();
        (pic.width, pic.height) = (20, 30);
        // Above the part and left of it.
        pic.x = -3.0;
        assert_eq!(parts.rect(0, &pic, -5.0), rect(0, 0, 20, 40));
        // Past its right edge and its bottom, 30 rows down.
        let (width, height) = parts.draw(0, None).dimensions();
        assert_eq!(height, 600);
        pic.x = (width - 5) as f32;
        assert_eq!(parts.rect(0, &pic, 590.0), rect(width - 10, 580, 10, 20));
    }

    #[test]
    fn a_picture_that_reaches_less_than_half_a_pixel_into_a_part_has_no_rect_there() {
        let (parts, images) = long_page();
        let mut pic = parts.animated(&images)[0].clone();
        (pic.width, pic.height) = (20, 30);
        // Its bottom is 0.4 pixels into the second part, which counts it as
        // one of its pictures but draws nothing of it.
        pic.y = 600.0 - 29.6;
        let here = parts.in_part(1, std::slice::from_ref(&pic));
        assert_eq!(here.len(), 1);
        assert_eq!(parts.rect(1, &pic, here[0].1), None);
        assert!(parts.rect(0, &pic, pic.y).is_some());
    }
}
