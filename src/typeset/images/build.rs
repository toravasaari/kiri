//! Building the frames of an animated picture, off the pager's loop: a pure
//! function over owned inputs, so that a thread of its own can run it. The
//! inputs are copied out of what the pager holds (the page of reading mode
//! holds `Rc`s that cannot leave the loop), and the result is the pictures of
//! the frames, ready to be sent to the terminal.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use image::{RgbaImage, imageops};

use crate::typeset::images::animation::{Animation, FRAME_BUDGET};
use crate::typeset::reading::animated::rectangle;

/// What a picture's frames are made from.
#[derive(Debug, Clone)]
pub enum FramesJob {
    /// A picture shown as an image of its own: each frame laid on a
    /// transparent canvas, and the frame round the cells when it is
    /// selected.
    Image {
        animation: Arc<Animation>,
        /// The canvas, in pixels: the cells the picture covers.
        canvas: (u32, u32),
        /// The picture's place on it, and its size.
        at: (i64, i64),
        size: (u32, u32),
        /// The colour, the rows and the height of a cell of the frame that
        /// marks the selected picture.
        selected: Option<([u8; 3], Range<u16>, u16)>,
    },
    /// A picture in a part of a page of reading mode: each frame laid on
    /// the cells of the part it lies on, drawn without it, for its inset.
    Part {
        animation: Arc<Animation>,
        /// The cells of the part the picture lies on, as they are with the
        /// picture left out: the page under it, and the other pictures in
        /// their states.
        base: RgbaImage,
        at: (i64, i64),
        size: (u32, u32),
        /// The colour and the thickness of the rectangle round the picture
        /// when it is selected.
        outline: Option<([u8; 3], u32)>,
    },
}

impl FramesJob {
    fn animation(&self) -> &Animation {
        match self {
            FramesJob::Image { animation, .. } | FramesJob::Part { animation, .. } => animation,
        }
    }

    /// The bytes one frame takes.
    fn frame_bytes(&self) -> u64 {
        let (width, height) = match self {
            FramesJob::Image { canvas, .. } => *canvas,
            FramesJob::Part { base, .. } => base.dimensions(),
        };
        u64::from(width) * u64::from(height) * 4
    }

    /// The bytes all the frames take.
    fn bytes(&self) -> u64 {
        self.animation().frames.len() as u64 * self.frame_bytes()
    }
}

/// The bytes of frames a piece of a build makes at most, unless one frame
/// takes more: about as much as a thread should hold on to before another
/// build may have it.
const PIECE_BYTES: u64 = 8 << 20;

/// The frames of a picture, built a piece of frames at a time, so that
/// the thread that builds them can give way between two pieces. What the
/// pieces made is kept here until the last is done.
#[derive(Debug)]
pub struct Build {
    job: FramesJob,
    frames: Vec<RgbaImage>,
    /// How many frames a piece makes.
    per_piece: usize,
}

/// What a piece of a [`Build`] came to.
#[derive(Debug)]
pub enum Piece {
    /// Frames are left to build.
    More,
    /// The last piece is done: all the frames, each drawn as the image the
    /// terminal is sent.
    Done(Vec<RgbaImage>),
    /// The stop flag was set before the frames were done.
    Stopped,
}

impl Build {
    /// A build of the frames of `job`; `None` when they would take more
    /// than the budget.
    pub fn new(job: FramesJob) -> Option<Build> {
        if job.bytes() > FRAME_BUDGET {
            return None;
        }
        let per_piece = (PIECE_BYTES / job.frame_bytes().max(1)).max(1);
        Some(Build {
            frames: Vec::new(),
            per_piece: usize::try_from(per_piece).unwrap_or(usize::MAX),
            job,
        })
    }

    /// Builds the next piece of frames, unless `stop` is set before a
    /// frame of it.
    pub fn piece(&mut self, stop: &AtomicBool) -> Piece {
        let total = self.job.animation().frames.len();
        for _ in 0..self.per_piece {
            if self.frames.len() == total {
                break;
            }
            if stop.load(Ordering::SeqCst) {
                return Piece::Stopped;
            }
            self.frames.push(frame(&self.job, self.frames.len()));
        }
        match self.frames.len() == total {
            true => Piece::Done(std::mem::take(&mut self.frames)),
            false => Piece::More,
        }
    }
}

/// The frames of `job`, each drawn as the image the terminal is sent;
/// `None` when they would take more than the budget, or when `stop` was set
/// before they were done.
pub fn build(job: &FramesJob, stop: &AtomicBool) -> Option<Vec<RgbaImage>> {
    let mut build = Build::new(job.clone())?;
    loop {
        match build.piece(stop) {
            Piece::More => {}
            Piece::Done(frames) => return Some(frames),
            Piece::Stopped => return None,
        }
    }
}

/// Frame `index` of `job`.
fn frame(job: &FramesJob, index: usize) -> RgbaImage {
    match job {
        FramesJob::Image {
            animation,
            canvas,
            at,
            size,
            selected,
        } => {
            let mut pixels = RgbaImage::new(canvas.0, canvas.1);
            let shown = animation.render(index, size.0, size.1);
            imageops::replace(&mut pixels, &shown, at.0, at.1);
            if let Some((rgb, rows, cell)) = selected {
                framed(&mut pixels, *cell, (*rgb, rows.clone()));
            }
            pixels
        }
        FramesJob::Part {
            animation,
            base,
            at,
            size,
            outline,
        } => {
            let mut pixels = base.clone();
            let shown = animation.render(index, size.0, size.1);
            imageops::overlay(&mut pixels, &shown, at.0, at.1);
            if let Some((rgb, line)) = outline {
                rectangle(&mut pixels, (at.0, at.1, size.0, size.1), *rgb, *line);
            }
            pixels
        }
    }
}

/// A frame of `rgb` along the edge of the cells of the canvas `pixels` in
/// `rows`, as wide as a twentieth of the cell's height, and at least a
/// pixel: the cells that a marked image is on, outlined.
pub fn framed(pixels: &mut RgbaImage, cell_height: u16, (rgb, rows): ([u8; 3], Range<u16>)) {
    let line = ((u32::from(cell_height) + 10) / 20).max(1);
    let (width, height) = pixels.dimensions();
    let top = u32::from(rows.start) * u32::from(cell_height);
    let bottom = (u32::from(rows.end) * u32::from(cell_height)).min(height);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        if y < top || y >= bottom {
            continue;
        }
        if x < line || y < top + line || x + line >= width || y + line >= bottom {
            *pixel = image::Rgba([rgb[0], rgb[1], rgb[2], 255]);
        }
    }
}

#[cfg(test)]
mod tests;
