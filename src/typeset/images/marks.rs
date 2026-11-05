//! The marks drawn into an animated picture: a play button over the first
//! frame of one that is stopped, and a replay icon in the corner of the
//! last frame of one that has played. They are drawn into the pixels, as a
//! cell shows one image and nothing can be laid over it.

use image::{Rgba, RgbaImage, imageops};
use resvg::tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform,
};

/// A picture whose smaller side is under this many pixels gets no mark;
/// Tab and the status line still work for it.
pub const SMALLEST: u32 = 48;

/// Where an animated picture is in its life, which decides what is drawn
/// in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// On its first frame, with a play button.
    Stopped,
    /// Playing: its frames, with no mark.
    Playing,
    /// Played as often as it is to: its last frame, with a replay icon.
    Ended,
}

/// A mark that goes into a picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// A play button in the middle.
    Play,
    /// A replay icon in the bottom right corner.
    Replay,
}

/// What the picture of `state` is drawn as: the frame it shows, of
/// `frames`, and the mark over it; `big` is whether the picture is large
/// enough for a mark.
pub fn drawn(state: State, frames: usize, big: bool) -> (usize, Option<Mark>) {
    match state {
        State::Stopped => (0, big.then_some(Mark::Play)),
        State::Playing => (0, None),
        State::Ended => (frames.saturating_sub(1), big.then_some(Mark::Replay)),
    }
}

/// Whether a picture of this size is large enough for a mark.
pub fn is_big(width: u32, height: u32) -> bool {
    width.min(height) >= SMALLEST
}

/// Draws `mark` over the picture that `image` holds in the box (`x`, `y`,
/// width, height): the play button in its middle, about a quarter of its
/// smaller side; the replay icon in its bottom right corner, about an
/// eighth of it.
pub fn draw(image: &mut RgbaImage, (x, y, width, height): (i64, i64, u32, u32), mark: Mark) {
    if !is_big(width, height) {
        return;
    }
    let side = |share: f32| ((width.min(height) as f32 * share) as u32).max(24);
    let (icon, left, top) = match mark {
        Mark::Play => {
            let size = side(0.25);
            let (dx, dy) = ((width - size) / 2, (height - size) / 2);
            (play_button(size), x + i64::from(dx), y + i64::from(dy))
        }
        Mark::Replay => {
            let size = side(0.14);
            let margin = (size / 4).max(4);
            let (dx, dy) = (width - size - margin, height - size - margin);
            (replay_icon(size), x + i64::from(dx), y + i64::from(dy))
        }
    };
    imageops::overlay(image, &icon, left, top);
}

fn paint(red: u8, green: u8, blue: u8, alpha: u8) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(red, green, blue, alpha);
    paint.anti_alias = true;
    paint
}

/// `pixmap` as an image, its colours no longer premultiplied.
fn image_of(pixmap: &Pixmap) -> RgbaImage {
    let mut image = RgbaImage::new(pixmap.width(), pixmap.height());
    for (pixel, drawn) in image.pixels_mut().zip(pixmap.pixels()) {
        let color = drawn.demultiply();
        *pixel = Rgba([color.red(), color.green(), color.blue(), color.alpha()]);
    }
    image
}

/// A partly transparent dark circle with a white triangle with rounded
/// corners in it, `size` pixels across.
pub fn play_button(size: u32) -> RgbaImage {
    let mut pixmap = Pixmap::new(size, size).expect("a button has a size");
    let big = size as f32;
    let circle = PathBuilder::from_circle(big / 2.0, big / 2.0, big / 2.0);
    let dark = paint(20, 20, 20, 150);
    if let Some(circle) = circle {
        pixmap.fill_path(
            &circle,
            &dark,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    // The triangle sits a little right of the middle, so that it looks
    // centred.
    let r = big * 0.24;
    let (cx, cy) = (big * 0.53, big * 0.5);
    let mut builder = PathBuilder::new();
    builder.move_to(cx - r * 0.8, cy - r);
    builder.line_to(cx - r * 0.8, cy + r);
    builder.line_to(cx + r, cy);
    builder.close();
    if let Some(triangle) = builder.finish() {
        let white = paint(255, 255, 255, 235);
        let identity = Transform::identity();
        pixmap.fill_path(&triangle, &white, FillRule::Winding, identity, None);
        let line = Stroke {
            width: (big / 18.0).max(2.0),
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(&triangle, &white, &line, identity, None);
    }
    image_of(&pixmap)
}

/// A partly transparent dark circle with a white arc and an arrow head in
/// it that goes on along the arc, `size` pixels across.
pub fn replay_icon(size: u32) -> RgbaImage {
    let mut pixmap = Pixmap::new(size, size).expect("an icon has a size");
    let big = size as f32;
    let (cx, cy) = (big / 2.0, big / 2.0);
    if let Some(circle) = PathBuilder::from_circle(cx, cy, big / 2.0) {
        let dark = paint(20, 20, 20, 160);
        let identity = Transform::identity();
        pixmap.fill_path(&circle, &dark, FillRule::Winding, identity, None);
    }
    let white = paint(255, 255, 255, 240);
    let width = (big / 14.0).max(2.0);
    // The arc runs from -70° to 200°, clockwise from three o'clock; its
    // outer edge is a quarter of the icon from the middle.
    let outer = big * 0.25;
    let middle = outer - width / 2.0;
    let (from, to) = (-70.0f32.to_radians(), 200.0f32.to_radians());
    let steps = 90;
    let mut builder = PathBuilder::new();
    for step in 0..=steps {
        let angle = from + (to - from) * step as f32 / steps as f32;
        let (x, y) = (cx + middle * angle.cos(), cy + middle * angle.sin());
        match step {
            0 => builder.move_to(x, y),
            _ => builder.line_to(x, y),
        }
    }
    if let Some(arc) = builder.finish() {
        let line = Stroke {
            width,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(&arc, &white, &line, Transform::identity(), None);
    }
    // The arrow head at the arc's end, pointing on along it.
    let head = big * 0.11;
    let (px, py) = (cx + middle * to.cos(), cy + middle * to.sin());
    let (on, out) = ((-to.sin(), to.cos()), (to.cos(), to.sin()));
    let mut builder = PathBuilder::new();
    builder.move_to(px + on.0 * head, py + on.1 * head);
    builder.line_to(px + out.0 * head * 0.85, py + out.1 * head * 0.85);
    builder.line_to(px - out.0 * head * 0.85, py - out.1 * head * 0.85);
    builder.close();
    if let Some(arrow) = builder.finish() {
        let identity = Transform::identity();
        pixmap.fill_path(&arrow, &white, FillRule::Winding, identity, None);
    }
    image_of(&pixmap)
}

#[cfg(test)]
mod tests;
