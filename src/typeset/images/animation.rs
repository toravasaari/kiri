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

/// How long an image is in view before its animation starts.
pub const START_DELAY: Duration = Duration::from_secs(1);

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
    /// The bytes the frames would take, when that is more than the budget
    /// allows and only the first frame is kept.
    pub over_budget: Option<u64>,
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
    /// An animation whose frames together take more than the budget:
    /// `bytes` are what all its frames take, none of them kept.
    OverBudget {
        bytes: u64,
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
    let mut decoder_rest = decoder.into_frames();
    while let Some(frame) = decoder_rest.next() {
        let frame = match frame {
            Ok(frame) => frame,
            // What was read before the damage still plays.
            Err(_) if !frames.is_empty() => break,
            Err(error) => return Err(from_image_error(error)),
        };
        bytes += frame.buffer().as_raw().len() as u64;
        if bytes > budget {
            // The rest are counted and dropped, to tell what they take.
            frames.clear();
            let rest: u64 = (decoder_rest.by_ref().filter_map(Result::ok))
                .map(|frame| frame.buffer().as_raw().len() as u64)
                .sum();
            return Ok(Decoded::OverBudget {
                bytes: bytes + rest,
                plays,
            });
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
            over_budget: None,
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
mod tests {
    use super::*;
    use crate::load::tests::fixtures;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn read(name: &str) -> Vec<u8> {
        std::fs::read(fixtures().join("images").join(name)).expect(name)
    }

    fn animation(name: &str) -> Animation {
        match decode_animation(&read(name), FRAME_BUDGET) {
            Ok(Decoded::Animated(animation)) => animation,
            other => panic!("{name} is not an animation: {other:?}"),
        }
    }

    fn gaps(animation: &Animation) -> Vec<Duration> {
        animation.frames.iter().map(|frame| frame.gap).collect()
    }

    fn pixel(frame: &Frame, x: u32, y: u32) -> [u8; 4] {
        frame.image.get_pixel(x, y).0
    }

    #[test]
    fn a_gif_gives_every_frame() {
        let animation = animation("animated.gif");
        assert_eq!(animation.frames.len(), 3);
        let colours: Vec<_> = animation.frames.iter().map(|f| pixel(f, 5, 5)).collect();
        assert_eq!(
            colours,
            [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]
        );
    }

    #[test]
    fn a_webp_gives_every_frame() {
        let animation = animation("animated.webp");
        let colours: Vec<_> = animation.frames.iter().map(|f| pixel(f, 5, 5)).collect();
        // WebP keeps colours to within a unit.
        let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1);
        let expected = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
        assert!(
            colours.iter().zip(expected).all(|(a, b)| near(*a, b)),
            "{colours:?}"
        );
    }

    #[test]
    fn the_three_formats_play_twice_with_their_gaps() {
        // The GIF stores one repeat after the first play, the others two
        // plays.
        for name in ["animated-twice.gif", "animated-twice.webp"] {
            let animation = animation(name);
            assert_eq!(animation.plays, Plays::Times(2), "{name}");
            assert_eq!(gaps(&animation), [ms(100), ms(500), ms(1000)], "{name}");
        }
        let apng = animation("animated-twice.png");
        assert_eq!(apng.plays, Plays::Times(2));
        assert_eq!(gaps(&apng), [ms(500), ms(500), ms(500)]);
    }

    #[test]
    fn a_looping_gif_plays_forever_and_one_without_the_loop_extension_once() {
        assert_eq!(animation("animated.gif").plays, Plays::Forever);
        let once = animation("wuffs/animated-red-blue.gif");
        assert_eq!(once.plays, Plays::Times(3));
        // Without the NETSCAPE extension a GIF plays once.
        let mut bytes = read("animated.gif");
        let at = bytes
            .windows(11)
            .position(|w| w == b"NETSCAPE2.0")
            .expect("an extension");
        bytes[at..at + 11].copy_from_slice(b"NETSCAPE9.9");
        let Ok(Decoded::Animated(animation)) = decode_animation(&bytes, FRAME_BUDGET) else {
            panic!("an animation");
        };
        assert_eq!(animation.plays, Plays::Times(1));
    }

    #[test]
    fn one_animation_in_three_formats_gives_the_same_frames() {
        let gif = animation("wuffs/animated-red-blue.gif");
        let webp = animation("wuffs/animated-red-blue.webp");
        let apng = animation("wuffs/animated-red-blue.apng");
        for animation in [&gif, &webp, &apng] {
            assert_eq!(animation.frames.len(), 4);
            assert_eq!(animation.plays, Plays::Times(3));
            assert_eq!(gaps(animation), [ms(100), ms(200), ms(300), ms(400)]);
            assert_eq!(animation.frames[0].image.dimensions(), (64, 48));
        }
        for (a, b) in gif.frames.iter().zip(&apng.frames) {
            assert_eq!(a.image, b.image, "GIF and APNG");
        }
        // WebP's colours differ from the GIF's by a unit here and there.
        for (a, b) in gif.frames.iter().zip(&webp.frames) {
            let far = a
                .image
                .as_raw()
                .iter()
                .zip(b.image.as_raw())
                .filter(|(x, y)| x.abs_diff(**y) > 1)
                .count();
            assert_eq!(far, 0, "GIF and WebP");
        }
    }

    #[test]
    fn partial_frames_are_composed_with_every_gif_disposal() {
        let animation = animation("animated-disposal.gif");
        assert_eq!(animation.frames.len(), 4);
        assert_eq!(animation.frames[0].image.dimensions(), (64, 64));
        // Red top left; plus green bottom right; the green cleared and
        // blue bottom left; the blue taken back and yellow top right.
        let red = [255, 0, 0, 255];
        let green = [0, 255, 0, 255];
        let blue = [0, 0, 255, 255];
        let yellow = [255, 255, 0, 255];
        let clear = [0, 0, 0, 0];
        let at = |frame: usize| {
            let f = &animation.frames[frame];
            [
                pixel(f, 10, 10),
                pixel(f, 45, 45),
                pixel(f, 12, 45),
                pixel(f, 45, 12),
            ]
        };
        assert_eq!(at(0), [red, clear, clear, clear]);
        assert_eq!(at(1), [red, green, clear, clear]);
        assert_eq!(at(2), [red, clear, blue, clear]);
        assert_eq!(at(3), [red, clear, clear, yellow]);
    }

    #[test]
    fn mixed_disposal_gives_the_frames_image_rs_expects() {
        let animation = animation("image-rs/mixed-disposal.gif");
        assert_eq!(animation.frames.len(), 5);
        assert_eq!(animation.plays, Plays::Forever);
        assert_eq!(gaps(&animation), [ms(1000); 5]);
        for (n, frame) in animation.frames.iter().enumerate() {
            let name = format!("image-rs/mixed-disposal.gif.anim_{:02}.png", n + 1);
            let expected = image::load_from_memory(&read(&name)).unwrap().to_rgba8();
            assert_eq!(frame.image, expected, "{name}");
        }
    }

    #[test]
    fn an_apng_counts_its_plays_and_reads_its_delays() {
        let forever = animation("apng-suite/num_plays.png");
        assert_eq!(forever.plays, Plays::Forever);
        assert_eq!(gaps(&forever), [ms(1000), ms(1000)]);
        let once = animation("apng-suite/num_plays_1.png");
        assert_eq!(once.plays, Plays::Times(1));
        let delays = animation("apng-suite/delay.png");
        // Blue for half a second, yellow for one, twice.
        assert_eq!(gaps(&delays), [ms(500), ms(1000), ms(500), ms(1000)]);
    }

    #[test]
    fn a_gap_that_asks_for_no_time_is_what_browsers_make_of_it() {
        let animation = animation("apng-suite/delay_zero_numer.png");
        assert_eq!(gaps(&animation), [ms(100), ms(100), ms(500), ms(1000)]);
    }

    #[test]
    fn apng_disposal_and_blending_end_in_green() {
        for name in [
            "dispose_op_none",
            "dispose_op_none_region",
            "dispose_op_previous",
            "dispose_op_previous_region",
            "blend_op_over",
            "blend_op_source_solid",
        ] {
            let animation = animation(&format!("apng-suite/{name}.png"));
            let last = animation.frames.last().unwrap();
            let bad = last
                .image
                .pixels()
                .filter(|p| p.0 != [0, 255, 0, 255])
                .count();
            assert_eq!(bad, 0, "{name} ends with {bad} pixels that are not green");
        }
        for name in ["dispose_op_background", "blend_op_source_transparent"] {
            let animation = animation(&format!("apng-suite/{name}.png"));
            let last = animation.frames.last().unwrap();
            assert!(last.image.pixels().all(|p| p.0[3] == 0), "{name}");
        }
    }

    #[test]
    fn a_big_apng_and_a_big_gif_give_their_frames() {
        assert_eq!(animation("beachball-apng.png").frames[0].image.width(), 100);
        let horse = animation("muybridge-race-horse.gif");
        assert_eq!(horse.frames.len(), 15);
        assert_eq!(gaps(&horse), [ms(100); 15]);
        assert!(animation("jiggy.gif").frames.len() > 1);
    }

    #[test]
    fn stills_and_other_formats_are_not_animations() {
        for name in [
            "pattern.png",
            "photo-landscape.jpg",
            "lossless.webp",
            "icon.png",
            "pattern.svg",
            "not-an-image.png",
        ] {
            let result = decode_animation(&read(name), FRAME_BUDGET);
            assert!(matches!(result, Ok(Decoded::Still) | Err(_)), "{name}");
            assert!(!matches!(result, Ok(Decoded::Animated(_))), "{name}");
        }
        assert_eq!(
            decode_animation(&read("pattern.png"), FRAME_BUDGET),
            Ok(Decoded::Still)
        );
    }

    #[test]
    fn frames_over_the_budget_are_not_kept() {
        // Three frames of 64 × 64 pixels at 16 KiB each.
        let bytes = read("animated-disposal.gif");
        let four = 4 * 64 * 64 * 4;
        assert!(matches!(
            decode_animation(&bytes, four),
            Ok(Decoded::Animated(_))
        ));
        let result = decode_animation(&bytes, four - 1);
        assert!(
            matches!(result, Ok(Decoded::OverBudget { bytes, .. }) if bytes > four - 1),
            "{result:?}"
        );
    }

    #[test]
    fn broken_gifs_fail_cleanly_or_give_what_there_is() {
        for name in [
            "frame-out-of-bounds.gif",
            "zero-width-frame.gif",
            "pixel-data-not-enough.gif",
            "no-frames.gif",
        ] {
            let result = decode_animation(&read(&format!("wuffs/{name}")), FRAME_BUDGET);
            if let Ok(Decoded::Animated(animation)) = &result {
                assert!(animation.frames.len() >= 2, "{name}");
            }
        }
    }

    #[test]
    fn a_truncated_animation_gives_the_frames_before_the_cut() {
        let bytes = read("animated-twice.gif");
        let mut shown = 0;
        for end in (20..bytes.len()).step_by(7) {
            if let Ok(Decoded::Animated(animation)) = decode_animation(&bytes[..end], FRAME_BUDGET)
            {
                assert!(animation.frames.len() <= 3);
                shown += 1;
            }
        }
        assert!(shown > 0, "no cut kept frames");
    }
}
