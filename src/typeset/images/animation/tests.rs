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
        matches!(result, Ok(Decoded::OverBudget { .. })),
        "{result:?}"
    );
}

#[test]
fn an_animation_over_the_budget_with_a_broken_frame_ends() {
    let mut gif = b"GIF89a".to_vec();
    gif.extend([0x02, 0x00, 0x02, 0x00, 0x80, 0x00, 0x00]);
    gif.extend([0x00, 0x00, 0x00, 0xff, 0x00, 0x00]);
    for n in 0..6 {
        gif.extend([0x21, 0xf9, 0x04, 0x00, 0x05, 0x00, 0x00, 0x00]);
        gif.extend([0x2c, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
        // The fourth frame holds an undefined LZW code.
        if n == 3 {
            gif.extend([0x02, 0x02, 0x3c, 0x00, 0x00]);
        } else {
            gif.extend([0x02, 0x02, 0x4c, 0x01, 0x00]);
        }
    }
    gif.push(0x3b);
    match decode_animation(&gif, FRAME_BUDGET) {
        Ok(Decoded::Animated(animation)) => assert_eq!(animation.frames.len(), 3),
        other => panic!("the bytes are not as meant: {other:?}"),
    }
    // A frame is 16 bytes, so the second one passes 20.
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(decode_animation(&gif, 20));
    });
    let result = receive
        .recv_timeout(Duration::from_secs(5))
        .expect("decoding did not end");
    assert!(
        matches!(result, Ok(Decoded::OverBudget { .. })),
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
        if let Ok(Decoded::Animated(animation)) = decode_animation(&bytes[..end], FRAME_BUDGET) {
            assert!(animation.frames.len() <= 3);
            shown += 1;
        }
    }
    assert!(shown > 0, "no cut kept frames");
}
