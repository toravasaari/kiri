//! The frames of an animated GIF, WebP or APNG, composed to full frames.

use std::io::Cursor;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::metadata::LoopCount;
use image::{AnimationDecoder, ImageDecoder, ImageFormat, RgbaImage};

use crate::typeset::images::ImageError;
use crate::typeset::images::decode::{MAX_PIXELS, from_image_error};

/// The most RGBA bytes the frames of one animation may take together. An
/// animation over it shows its first frame, with a warning.
pub const FRAME_BUDGET: u64 = 64 * 1024 * 1024;

/// A frame shown for at most this long is shown for [`SLOW_GAP`] instead,
/// as browsers do for GIFs that ask for no delay at all.
const FASTEST_GAP: Duration = Duration::from_millis(10);

/// What browsers replace a gap of [`FASTEST_GAP`] or less with.
const SLOW_GAP: Duration = Duration::from_millis(100);

/// One frame of an animation: the whole canvas, not what changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub image: RgbaImage,
    /// How long it stays before the next frame.
    pub gap: Duration,
}

/// How often an animation plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plays {
    /// Without end.
    Forever,
    /// This many times in all, at least one.
    Times(u32),
}

/// The frames of an animation with how often it plays.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    /// At least two, unless `over_budget` says why there is one.
    pub frames: Vec<Frame>,
    pub plays: Plays,
    /// Whether the frames would take more than the budget allows, so that
    /// only the first frame is kept.
    pub over_budget: bool,
}

impl Animation {
    /// The first frame, which is what the animation shows when it does not
    /// play.
    pub fn first(&self) -> &RgbaImage {
        &self.frames[0].image
    }

    /// How often it plays, as something to hash.
    pub(crate) fn plays_print(&self) -> (bool, u32) {
        match self.plays {
            Plays::Forever => (true, 0),
            Plays::Times(times) => (false, times),
        }
    }

    /// Frame `index` drawn `width` × `height` pixels large.
    pub fn render(&self, index: usize, width: u32, height: u32) -> RgbaImage {
        match &self.frames[index].image {
            image if image.dimensions() == (width, height) => image.clone(),
            image => super::decode::scale(image, width, height),
        }
    }

    /// The time one play takes: the gaps of all the frames.
    pub fn duration(&self) -> Duration {
        self.frames.iter().map(|frame| frame.gap).sum()
    }
}

/// What `decode_animation` found.
#[derive(Debug, Clone, PartialEq)]
pub enum Decoded {
    /// A still image, or an animation of one frame.
    Still,
    Animated(Animation),
    /// An animation whose frames together take more than the budget, none
    /// of them kept; the frames after the one that passed it are not read.
    OverBudget {
        plays: Plays,
    },
}

/// The frames of the image in `bytes`, if it is an animated GIF, WebP or
/// APNG, as long as they take at most `budget` bytes in RGBA.
pub fn decode_animation(bytes: &[u8], budget: u64) -> Result<Decoded, ImageError> {
    let cursor = Cursor::new(bytes);
    match image::guess_format(bytes) {
        Ok(ImageFormat::Gif) => {
            let decoder = GifDecoder::new(cursor).map_err(from_image_error)?;
            // The image crate takes a GIF without the loop extension to
            // loop forever; browsers play it once.
            let plays = gif_plays(bytes);
            collect(decoder.dimensions(), decoder, plays, budget)
        }
        Ok(ImageFormat::WebP) => {
            let decoder = WebPDecoder::new(cursor).map_err(from_image_error)?;
            if !decoder.has_animation() {
                return Ok(Decoded::Still);
            }
            let plays = counted_plays(decoder.loop_count());
            collect(decoder.dimensions(), decoder, plays, budget)
        }
        Ok(ImageFormat::Png) => {
            let decoder = PngDecoder::new(cursor).map_err(from_image_error)?;
            if !decoder.is_apng().map_err(from_image_error)? {
                return Ok(Decoded::Still);
            }
            let size = decoder.dimensions();
            let decoder = decoder.apng().map_err(from_image_error)?;
            let plays = counted_plays(decoder.loop_count());
            collect(size, decoder, plays, budget)
        }
        _ => Ok(Decoded::Still),
    }
}

/// How often a GIF plays, from its NETSCAPE looping extension, which holds
/// the repeats after the first play: none, one play; 0, forever.
fn gif_plays(bytes: &[u8]) -> Plays {
    const EXTENSION: &[u8] = b"NETSCAPE2.0";
    let Some(at) = bytes.windows(EXTENSION.len()).position(|w| w == EXTENSION) else {
        return Plays::Times(1);
    };
    // Then the sub-block: a length of 3, the id 1 and the count, little
    // endian.
    match bytes[at + EXTENSION.len()..] {
        [3, 1, low, high, ..] => match u16::from_le_bytes([low, high]) {
            0 => Plays::Forever,
            repeats => Plays::Times(u32::from(repeats) + 1),
        },
        _ => Plays::Times(1),
    }
}

/// How often a WebP or an APNG plays: the count is of the plays.
fn counted_plays(count: LoopCount) -> Plays {
    match count {
        LoopCount::Infinite => Plays::Forever,
        LoopCount::Finite(times) => Plays::Times(times.get()),
    }
}

/// The frames of `decoder`, as far as they are readable and fit `budget`.
fn collect<'a>(
    (width, height): (u32, u32),
    decoder: impl AnimationDecoder<'a>,
    plays: Plays,
    budget: u64,
) -> Result<Decoded, ImageError> {
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(ImageError::TooLarge { width, height });
    }
    let mut frames = Vec::new();
    let mut bytes = 0u64;
    for frame in decoder.into_frames() {
        let frame = match frame {
            Ok(frame) => frame,
            // What was read before the damage still plays.
            Err(_) if !frames.is_empty() => break,
            Err(error) => return Err(from_image_error(error)),
        };
        bytes += frame.buffer().as_raw().len() as u64;
        if bytes > budget {
            return Ok(Decoded::OverBudget { plays });
        }
        let gap = gap(frame.delay());
        frames.push(Frame {
            image: frame.into_buffer(),
            gap,
        });
    }
    Ok(match frames.len() {
        0 | 1 => Decoded::Still,
        _ => Decoded::Animated(Animation {
            frames,
            plays,
            over_budget: false,
        }),
    })
}

/// The time `delay` asks for, as browsers show it.
fn gap(delay: image::Delay) -> Duration {
    let (numerator, denominator) = delay.numer_denom_ms();
    let micros = u64::from(numerator) * 1000 / u64::from(denominator.max(1));
    let gap = Duration::from_micros(micros);
    if gap <= FASTEST_GAP { SLOW_GAP } else { gap }
}

#[cfg(test)]
mod tests;
