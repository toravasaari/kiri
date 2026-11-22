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
            glyphs: Default::default(),
            fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
            snap: false,
            zoom: 100,
        }),
        ..Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(80)
        })
    };
    let lines = crate::typeset::driver::layout(&crate::typeset::parse::parse("Text.\n"), &inputs);
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
            glyphs: Default::default(),
            fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
            snap: false,
            zoom: 100,
        }),
        ..Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(80)
        })
    };
    let page_of = |source: &str| {
        let lines = crate::typeset::driver::layout(&crate::typeset::parse::parse(source), &inputs);
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
    let first = encoder
        .band(4, cell, INK, start, graphics::Effort::DEFAULT, &mut out)
        .unwrap();
    let second = encoder
        .band(4, cell, INK, start, graphics::Effort::DEFAULT, &mut out)
        .unwrap();
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
    assert_eq!(
        encoder
            .band(4, cell, INK, later, graphics::Effort::DEFAULT, &mut out)
            .unwrap(),
        first
    );
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

/// A frame of 2 × 2 pixels, for an animation built off the loop.
fn built_frames() -> Rc<Vec<RgbaImage>> {
    Rc::new(vec![RgbaImage::new(2, 2)])
}

#[test]
fn keep_views_holds_the_ids_of_the_lines_below() {
    let mut images = Images::new();
    let red = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]));
    images.insert("below.png", Ok(Source::Raster(red.clone())));
    images.insert("top.png", Ok(Source::Raster(RgbaImage::new(20, 20))));
    let below = embedding(&placed("below.png", 2, 1, Shows::Image));
    let top = embedding(&placed("top.png", 2, 1, Shows::Image));
    let (mut encoder, mut out) = (Encoder::new(), Vec::new());
    let id = |encoder: &mut Encoder, line: &Line, out: &mut Vec<u8>| {
        let embed = line.spans[0].embed.as_ref().expect("an embed");
        encoder.id(&embed.placement, &images, &caps(), out).unwrap()
    };
    let below_id = id(&mut encoder, &below, &mut out);
    id(&mut encoder, &top, &mut out);
    out.clear();
    // A view covers the one that placed `below.png`: its id stays.
    let covered: &[Line] = std::slice::from_ref(&below);
    encoder.keep_views(std::slice::from_ref(&top), &[covered], &caps());
    encoder.delete_unused(&mut out).unwrap();
    assert!(out.is_empty(), "{out:?}");
    // The view is closed and the layout under it is no longer held.
    encoder.keep_views(std::slice::from_ref(&top), &[], &caps());
    encoder.delete_unused(&mut out).unwrap();
    let mut expected = Vec::new();
    graphics::delete(&mut expected, below_id).unwrap();
    assert_eq!(out, expected);
}

#[test]
fn keep_views_drops_the_built_frames_and_pages_of_the_lines_below() {
    let mut images = Images::new();
    images.insert("below.png", Ok(Source::Raster(RgbaImage::new(20, 20))));
    let placement = placed("below.png", 2, 1, Shows::Image);
    let below = embedding(&placement);
    let (mut encoder, mut out) = (Encoder::new(), Vec::new());
    let id = encoder.id(&placement, &images, &caps(), &mut out).unwrap();
    let picture = ImageKey::from("below.gif");
    encoder
        .built
        .insert((placement.clone(), picture), built_frames());
    out.clear();
    encoder.keep_views(&[], &[std::slice::from_ref(&below)], &caps());
    assert!(encoder.built.is_empty(), "the frames below are dropped");
    assert!(encoder.pages.is_empty());
    // The image the terminal holds stays, as the view will be back.
    encoder.delete_unused(&mut out).unwrap();
    assert!(out.is_empty(), "{id:?} was deleted: {out:?}");
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

/// A writer that takes one escape sequence, as far as its end, and then
/// refuses.
struct OneSequence {
    closed: bool,
    taken: Vec<u8>,
}

impl Write for OneSequence {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.closed {
            return Err(io::Error::other("the terminal is gone"));
        }
        self.taken.extend_from_slice(bytes);
        self.closed = bytes.ends_with(b"\x1b\\");
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_delete_that_fails_halfway_forgets_the_first_and_keeps_the_second() {
    let (mut images, first) = red();
    images.insert(
        "b.png",
        Ok(Source::Raster(RgbaImage::from_pixel(
            20,
            20,
            image::Rgba(BLUE),
        ))),
    );
    let second = placed("b.png", 2, 1, Shows::Image);
    let mut encoder = Encoder::new();
    let mut out = Vec::new();
    let a = encoder.id(&first, &images, &caps(), &mut out).unwrap();
    let b = encoder.id(&second, &images, &caps(), &mut out).unwrap();
    encoder.keep(&[], &caps());
    // Room for one delete, whichever of the two comes first.
    let mut writer = OneSequence {
        closed: false,
        taken: Vec::new(),
    };
    assert!(encoder.delete_unused(&mut writer).is_err());
    let (done, left) = if writer.taken == deletes(&[a]) {
        (a, b)
    } else {
        (b, a)
    };
    assert_eq!(writer.taken, deletes(&[done]));
    // The first is forgotten, the second counts and is put back.
    assert_eq!(encoder.image_bytes(), 20 * 20 * 3);
    let mut out = Vec::new();
    encoder.delete_unused(&mut out).unwrap();
    assert_eq!(out, deletes(&[left]));
    assert_eq!(encoder.image_bytes(), 0);
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

#[test]
fn an_opaque_still_goes_as_three_bytes_a_pixel_and_counts_so() {
    let (images, placement) = red();
    let mut encoder = Encoder::new();
    let mut out = Vec::new();
    encoder.id(&placement, &images, &caps(), &mut out).unwrap();
    let first = &sequences(&out)[0];
    assert!(keys(first).contains(",f=24,"), "{first:?}");
    assert_eq!(encoder.image_bytes(), 20 * 20 * 3);
}

#[test]
fn a_still_with_one_translucent_pixel_goes_as_four_bytes_a_pixel() {
    let mut images = Images::new();
    let mut pixels = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]));
    pixels.put_pixel(7, 3, image::Rgba([255, 0, 0, 254]));
    images.insert("a.png", Ok(Source::Raster(pixels)));
    let placement = placed("a.png", 2, 1, Shows::Image);
    let mut encoder = Encoder::new();
    let mut out = Vec::new();
    encoder.id(&placement, &images, &caps(), &mut out).unwrap();
    let first = &sequences(&out)[0];
    assert!(keys(first).contains(",f=32,"), "{first:?}");
    assert_eq!(encoder.image_bytes(), 20 * 20 * 4);
}

#[test]
fn an_opaque_animation_goes_as_four_bytes_a_pixel_with_its_frames() {
    let (encoder, _, out) = sent_animation(Playback::Terminal);
    let sent = sequences(&out);
    assert!(keys(&sent[0]).contains(",f=32,"), "{:?}", sent[0]);
    assert_eq!(encoder.image_bytes(), 3 * 20 * 20 * 4);
}

#[test]
fn an_animation_with_a_translucent_first_frame_goes_as_four_bytes_a_pixel() {
    let mut images = Images::new();
    let frame = |alpha| AnimFrame {
        image: RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, alpha])),
        gap: Duration::from_millis(100),
    };
    images.insert(
        "a.gif",
        Ok(Source::Animated(std::sync::Arc::new(Animation {
            frames: vec![frame(200), frame(255)],
            plays: Plays::Forever,
            over_budget: false,
        }))),
    );
    let mut encoder = Encoder::new();
    encoder.play(Playback::Terminal);
    encoder.show_marks(in_state("a.gif", State::Playing));
    let mut out = Vec::new();
    let placement = placed("a.gif", 2, 1, Shows::Image);
    encoder.id(&placement, &images, &caps(), &mut out).unwrap();
    let sent = sequences(&out);
    assert!(keys(&sent[0]).contains(",f=32,"), "{:?}", sent[0]);
    assert_eq!(encoder.image_bytes(), 2 * 20 * 20 * 4);
}

#[test]
fn the_id_does_not_depend_on_the_format() {
    let (images, placement) = red();
    let id = Encoder::new()
        .id(&placement, &images, &caps(), &mut Vec::new())
        .unwrap();
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
    let four = ImageId::new(scaled.pixels.as_raw(), width, height, scaled.cells, None);
    assert_eq!(id, four);
}

/// `caps()` with graphics wrapped for tmux.
fn through_tmux() -> Capabilities {
    Capabilities {
        tmux_passthrough: true,
        ..caps()
    }
}

#[test]
fn what_is_sent_is_compressed_at_the_level_the_capabilities_say() {
    let mut images = Images::new();
    let picture = RgbaImage::from_fn(200, 40, |x, y| {
        let (column, row) = (x / 6, y / 5);
        image::Rgba([
            (column * 40) as u8,
            (row * 30) as u8,
            ((column + row) % 3 * 80) as u8,
            255,
        ])
    });
    images.insert("g.png", Ok(Source::Raster(picture)));
    let placement = placed("g.png", 20, 2, Shows::Image);
    let sent = |caps: &Capabilities| {
        let mut out = Vec::new();
        let id = Encoder::new().id(&placement, &images, caps, &mut out);
        id.unwrap();
        out.len()
    };
    let at = |level| {
        sent(&Capabilities {
            image_compression: Some(level),
            ..caps()
        })
    };
    // Without a level asked for, the default is used; a higher level sends
    // fewer bytes.
    assert_eq!(sent(&caps()), at(2));
    assert!(at(1) > at(6), "{} against {}", at(1), at(6));
}

/// Images with a red `a.png`, and its placement over 2 × 1 cells.
fn red() -> (Images, Placement) {
    let mut images = Images::new();
    let red = RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]));
    images.insert("a.png", Ok(Source::Raster(red)));
    (images, placed("a.png", 2, 1, Shows::Image))
}

/// A page of `parts` parts, each of 30 lines, as reading mode lays it out.
fn parts_of_a_page(parts: u16) -> (Vec<Line>, Vec<Rc<Placement>>) {
    let placements: Vec<Rc<Placement>> = (0..parts)
        .map(|part| Rc::new(placed(&format!("part{part}.png"), 2, 30, Shows::Image)))
        .collect();
    let lines = (placements.iter())
        .flat_map(|placement| (0..30).map(|row| Line::image(Rc::clone(placement), row)))
        .collect();
    (lines, placements)
}

fn images_of(parts: u16) -> Images {
    let mut images = Images::new();
    for part in 0..parts {
        let pixels = RgbaImage::from_pixel(20, 600, image::Rgba([part as u8, 0, 0, 255]));
        images.insert(format!("part{part}.png"), Ok(Source::Raster(pixels)));
    }
    images
}

#[test]
fn the_bytes_held_are_those_of_the_pixels_sent_and_stay_when_the_image_is_lost() {
    let (images, placement) = red();
    let mut encoder = Encoder::new();
    assert_eq!(encoder.image_bytes(), 0);
    encoder
        .id(&placement, &images, &caps(), &mut Vec::new())
        .unwrap();
    assert_eq!(encoder.image_bytes(), 20 * 20 * 3);
    encoder.lost();
    assert_eq!(encoder.image_bytes(), 20 * 20 * 3);
}

#[test]
fn a_walk_gives_a_part_of_30_lines_once_nearest_first_with_the_id_held() {
    let (lines, placements) = parts_of_a_page(4);
    let (images, caps) = (images_of(4), caps());
    let (mut encoder, mut out) = (Encoder::new(), Vec::new());
    // The terminal holds the second and the fourth part.
    let second = encoder
        .id(&placements[1], &images, &caps, &mut out)
        .unwrap();
    let fourth = encoder
        .id(&placements[3], &images, &caps, &mut out)
        .unwrap();
    // The view is the last lines of the second part and the first of the third.
    let seen = encoder.walk(&lines, 55..65, &images, &caps);
    let keys: Vec<_> = seen.iter().map(|seen| seen.placement.key.clone()).collect();
    let want: Vec<ImageKey> = [1, 2, 0, 3]
        .iter()
        .map(|n| ImageKey::from(format!("part{n}.png").as_str()))
        .collect();
    assert_eq!(keys, want);
    let in_view: Vec<bool> = seen.iter().map(|seen| seen.in_view).collect();
    assert_eq!(in_view, [true, true, false, false]);
    let held: Vec<Option<ImageId>> = seen.iter().map(|seen| seen.held).collect();
    assert_eq!(held, [Some(second), None, None, Some(fourth)]);
    // The row is the first the walk met, which has the page and the overlay.
    assert_eq!(seen[0].row.map(|row| row.row), Some(0));
    assert!(seen.iter().all(|seen| seen.row.is_some()));
}

/// An encoder that holds the first `parts` parts of a page, sent
/// `frames` frames ago, and what it was sent.
fn holding(parts: u16, frames: usize, caps: &Capabilities) -> (Encoder, Vec<ImageId>, Images) {
    let (_, placements) = parts_of_a_page(parts);
    let images = images_of(parts);
    let mut encoder = Encoder::new();
    let mut out = Vec::new();
    let ids = (placements.iter())
        .map(|placement| encoder.id(placement, &images, caps, &mut out).unwrap())
        .collect();
    for _ in 0..frames {
        encoder.delete_unused(&mut out).unwrap();
    }
    (encoder, ids, images)
}

/// 36 000 bytes: a part of 20 × 600 pixels, opaque, so three bytes a
/// pixel as the ledger counts it.
const PART: usize = 20 * 600 * 3;

#[test]
fn with_no_budget_nothing_is_let_go() {
    let caps = caps();
    let (mut encoder, _, images) = holding(3, 2, &caps);
    let (lines, _) = parts_of_a_page(3);
    let drawn = &lines[..1];
    let area = Near {
        lines: &lines[60..],
        visible: 0..1,
        drawn,
    };
    encoder.near(&[area], &images, &caps);
    // Nothing is marked either: the walk is not made.
    assert!(encoder.kept.view.is_empty() && encoder.kept.near.is_empty());
    let mut out = Vec::new();
    encoder.delete_unused(&mut out).unwrap();
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn under_the_budget_the_walk_is_not_made_and_nothing_is_let_go() {
    let caps = caps();
    let (mut encoder, _, images) = holding(3, 2, &caps);
    let (lines, _) = parts_of_a_page(3);
    encoder.set_budget(Some(3 * PART));
    let area = Near {
        lines: &lines[60..],
        visible: 0..1,
        drawn: &lines[..1],
    };
    encoder.near(&[area], &images, &caps);
    assert!(
        encoder.kept.view.is_empty() && encoder.kept.near.is_empty(),
        "marked: {:?}",
        encoder.kept
    );
    let mut out = Vec::new();
    encoder.delete_unused(&mut out).unwrap();
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn over_the_budget_the_parts_in_view_and_near_are_marked() {
    let caps = caps();
    let (mut encoder, _, images) = holding(3, 2, &caps);
    let (lines, _) = parts_of_a_page(3);
    encoder.set_budget(Some(3 * PART - 1));
    let area = Near {
        lines: &lines[60..],
        visible: 0..1,
        drawn: &lines[..1],
    };
    encoder.near(&[area], &images, &caps);
    assert!(!encoder.kept.view.is_empty() && !encoder.kept.near.is_empty());
}

#[test]
fn past_the_budget_the_parts_far_from_the_view_are_deleted_and_sent_again_under_the_same_id() {
    let caps = caps();
    let (mut encoder, ids, images) = holding(3, 2, &caps);
    let (lines, placements) = parts_of_a_page(3);
    encoder.set_budget(Some(2 * PART));
    // The first part is in view and the second near it.
    let area = Near {
        lines: &lines[..60],
        visible: 0..30,
        drawn: &lines[..30],
    };
    encoder.near(&[area], &images, &caps);
    let mut out = Vec::new();
    encoder.delete_unused(&mut out).unwrap();
    assert_eq!(out, deletes(&ids[2..]));
    assert_eq!(encoder.image_bytes(), 2 * PART);
    // Asked for again, it is sent under the id it had.
    out.clear();
    assert_eq!(
        encoder
            .id(&placements[2], &images, &caps, &mut out)
            .unwrap(),
        ids[2]
    );
    assert!(
        String::from_utf8_lossy(&out).contains("\x1b_Ga=T,"),
        "{out:?}"
    );
}

#[test]
fn a_part_in_view_with_a_highlight_is_not_let_go_and_the_one_without_is() {
    let caps = caps();
    let images = images_of(1);
    let (lines, placements) = parts_of_a_page(1);
    // What the frame draws is the part with a frame round a selected
    // picture, which is another placement and another image.
    let mut painted = (*placements[0]).clone();
    painted.frame = Some(Frame {
        color: crate::typeset::color::Color::Rgb(1, 2, 3),
        rows: 0..1,
        picture: None,
    });
    let painted = Rc::new(painted);
    let drawn: Vec<Line> = (0..30)
        .map(|row| Line::image(Rc::clone(&painted), row))
        .collect();
    let (mut encoder, mut out) = (Encoder::new(), Vec::new());
    let plain = encoder
        .id(&placements[0], &images, &caps, &mut out)
        .unwrap();
    let framed = encoder.id(&painted, &images, &caps, &mut out).unwrap();
    assert_ne!(plain, framed);
    for _ in 0..2 {
        encoder.delete_unused(&mut out).unwrap();
    }
    encoder.set_budget(Some(PART));
    // The layout's lines have the plain part in view; the painted copy
    // of them is what is drawn.
    let area = Near {
        lines: &lines,
        visible: 0..30,
        drawn: &drawn,
    };
    encoder.near(&[area], &images, &caps);
    out.clear();
    encoder.delete_unused(&mut out).unwrap();
    assert_eq!(out, deletes(&[plain]));
}

#[test]
fn the_frames_kiri_plays_in_view_are_not_let_go_while_it_plays() {
    let mut images = Images::new();
    images.insert("a.gif", Ok(animated([100, 200, 300], Plays::Forever)));
    let placement = Rc::new(placed("a.gif", 2, 1, Shows::Image));
    let line = Line::image(Rc::clone(&placement), 0);
    let lines = std::slice::from_ref(&line);
    let caps = caps();
    let mut encoder = Encoder::new();
    encoder.play(Playback::Redraw);
    encoder.show_marks(in_state("a.gif", State::Playing));
    encoder.set_budget(Some(0));
    let (mut out, mut ids) = (Vec::new(), Vec::new());
    for frame in [0, 1, 2, 0] {
        encoder.show_frames(HashMap::from([(ImageKey::from("a.gif"), frame)]));
        encoder.near(
            &[Near {
                lines,
                visible: 0..1,
                drawn: lines,
            }],
            &images,
            &caps,
        );
        let before = out.len();
        encoder.delete_unused(&mut out).unwrap();
        assert!(
            out[before..].is_empty(),
            "frame {frame}: {:?}",
            &out[before..]
        );
        ids.push(encoder.id(&placement, &images, &caps, &mut out).unwrap());
    }
    // The last time round nothing is sent: frame 0 is still held.
    assert_eq!(ids[0], ids[3]);
}

#[test]
fn what_the_last_frame_asked_for_is_not_let_go() {
    let caps = caps();
    let (mut encoder, _, _) = holding(3, 0, &caps);
    encoder.set_budget(Some(0));
    let mut out = Vec::new();
    encoder.delete_unused(&mut out).unwrap();
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn the_near_images_of_two_areas_are_both_kept() {
    let caps = caps();
    let (mut encoder, ids, images) = holding(4, 2, &caps);
    let (lines, _) = parts_of_a_page(4);
    encoder.set_budget(Some(2 * PART));
    let (first, second) = (&lines[..30], &lines[90..]);
    let areas = [
        Near {
            lines: first,
            visible: 0..1,
            drawn: &[],
        },
        Near {
            lines: second,
            visible: 0..1,
            drawn: &[],
        },
    ];
    encoder.near(&areas, &images, &caps);
    let mut out = Vec::new();
    encoder.delete_unused(&mut out).unwrap();
    assert_eq!(out, deletes(&ids[1..3]));
}

#[test]
fn through_tmux_what_is_let_go_is_deleted_at_once() {
    let caps = through_tmux();
    let (mut encoder, ids, _) = holding(2, 2, &caps);
    encoder.set_budget(Some(PART));
    let mut out = Vec::new();
    encoder.delete_unused(&mut out).unwrap();
    // Both were asked for last in the same frame, so the one with the
    // lower id value is the one let go.
    let lower = ids.iter().copied().min_by_key(|id| id.value()).unwrap();
    assert_eq!(out, deletes(&[lower]));
}

#[test]
fn every_call_for_an_image_notes_its_use_also_when_nothing_is_sent() {
    let (images, placement) = red();
    let (mut encoder, mut out) = (Encoder::new(), Vec::new());
    let id = encoder.id(&placement, &images, &caps(), &mut out).unwrap();
    let first = encoder.ledger.used(id).expect("an age");
    encoder.id(&placement, &images, &caps(), &mut out).unwrap();
    assert_eq!(encoder.ledger.used(id), Some(first));
    encoder.delete_unused(&mut out).unwrap();
    encoder.id(&placement, &images, &caps(), &mut out).unwrap();
    assert!(encoder.ledger.used(id) > Some(first));
}

#[test]
fn what_a_still_sends_it_sends_again_after_a_loss_and_not_before() {
    let (images, placement) = red();
    let mut encoder = Encoder::new();
    let (mut first, mut again) = (Vec::new(), Vec::new());
    let id = encoder
        .id(&placement, &images, &caps(), &mut first)
        .unwrap();
    assert!(!first.is_empty());
    assert_eq!(encoder.held(&placement, &images, &caps()), Some(id));
    let mut none = Vec::new();
    encoder.id(&placement, &images, &caps(), &mut none).unwrap();
    assert!(none.is_empty());
    encoder.lost();
    assert_eq!(encoder.held(&placement, &images, &caps()), None);
    encoder
        .id(&placement, &images, &caps(), &mut again)
        .unwrap();
    assert_eq!(again, first);
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
fn through_tmux_a_replaced_still_counts_until_the_check_is_answered() {
    let (mut images, placement) = red();
    let caps = through_tmux();
    let mut encoder = Encoder::new();
    let mut out = Vec::new();
    let red = encoder.id(&placement, &images, &caps, &mut out).unwrap();
    let (blue, _) = shown_in(&mut encoder, &mut images, &placement, BLUE, &caps);
    // The terminal holds both until the delete of the old one is sent.
    assert_eq!(encoder.image_bytes(), 2 * 20 * 20 * 3);
    encoder.answered(red, &mut out).unwrap();
    assert_eq!(encoder.image_bytes(), 2 * 20 * 20 * 3);
    encoder.answered(blue, &mut out).unwrap();
    assert_eq!(encoder.image_bytes(), 20 * 20 * 3);
}

#[test]
fn outside_tmux_a_replaced_still_stops_counting_when_the_delete_goes_out() {
    let (mut images, placement) = red();
    let mut encoder = Encoder::new();
    encoder
        .id(&placement, &images, &caps(), &mut Vec::new())
        .unwrap();
    shown_in(&mut encoder, &mut images, &placement, BLUE, &caps());
    assert_eq!(encoder.image_bytes(), 20 * 20 * 3);
}

#[test]
fn an_image_whose_deletion_failed_still_counts() {
    let (mut images, placement) = red();
    let mut encoder = Encoder::new();
    encoder
        .id(&placement, &images, &caps(), &mut Vec::new())
        .unwrap();
    encoder.keep(&[], &caps());
    assert!(encoder.delete_unused(&mut Refuses).is_err());
    assert_eq!(encoder.image_bytes(), 20 * 20 * 3);
    encoder.delete_unused(&mut Vec::new()).unwrap();
    assert_eq!(encoder.image_bytes(), 0);
    // The same when the delete of a replaced image fails at once.
    let id = encoder
        .id(&placement, &images, &caps(), &mut Vec::new())
        .unwrap();
    repaint(&mut encoder, &mut images, BLUE);
    assert!(
        encoder
            .id(&placement, &images, &caps(), &mut Refuses)
            .is_err()
    );
    assert_eq!(encoder.image_bytes(), 2 * 20 * 20 * 3, "{id:?}");
    let mut out = Vec::new();
    encoder.delete_unused(&mut out).unwrap();
    assert_eq!(out, deletes(&[id]));
}

#[test]
fn an_image_sent_with_its_frames_counts_every_frame_once() {
    let (encoder, _, _) = sent_animation(Playback::Terminal);
    assert_eq!(encoder.image_bytes(), 3 * 20 * 20 * 4);
    // The same image shown again adds nothing.
    let (mut encoder, _, _) = sent_animation(Playback::Terminal);
    let mut images = Images::new();
    images.insert("a.gif", Ok(animated([100, 200, 300], Plays::Forever)));
    encoder
        .id(
            &placed("a.gif", 2, 1, Shows::Image),
            &images,
            &caps(),
            &mut Vec::new(),
        )
        .unwrap();
    assert_eq!(encoder.image_bytes(), 3 * 20 * 20 * 4);
}

#[test]
fn nothing_counts_on_sixel() {
    let (images, placement) = red();
    let mut encoder = Encoder::new();
    let caps = Capabilities::sixel(40);
    encoder
        .sixel_row(&placement, 0, &(0..2), None, &images, &caps)
        .unwrap();
    assert_eq!(encoder.image_bytes(), 0);
}

#[test]
fn a_frame_and_a_driven_image_note_their_use_when_nothing_is_sent() {
    // What kiri plays: the image of each frame.
    let mut images = Images::new();
    images.insert("a.gif", Ok(animated([100, 200, 300], Plays::Forever)));
    let placement = placed("a.gif", 2, 1, Shows::Image);
    let mut encoder = Encoder::new();
    encoder.play(Playback::Redraw);
    encoder.show_marks(in_state("a.gif", State::Playing));
    let mut out = Vec::new();
    let id = encoder.id(&placement, &images, &caps(), &mut out).unwrap();
    let first = encoder.ledger.used(id).expect("an age");
    encoder.delete_unused(&mut out).unwrap();
    out.clear();
    assert_eq!(
        encoder.id(&placement, &images, &caps(), &mut out).unwrap(),
        id
    );
    assert!(out.is_empty());
    assert!(encoder.ledger.used(id) > Some(first));
    // What the terminal plays: one image with every frame.
    let (mut encoder, id, _) = sent_animation(Playback::Terminal);
    let first = encoder.ledger.used(id).expect("an age");
    encoder.delete_unused(&mut Vec::new()).unwrap();
    let mut out = Vec::new();
    assert_eq!(
        encoder.id(&placement, &images, &caps(), &mut out).unwrap(),
        id
    );
    assert!(out.is_empty());
    assert!(encoder.ledger.used(id) > Some(first));
}

#[test]
fn what_sixel_discards_is_forgotten_with_its_weight() {
    let (images, placement) = red();
    let mut encoder = Encoder::new();
    encoder
        .id(&placement, &images, &caps(), &mut Vec::new())
        .unwrap();
    encoder.keep(&[], &caps());
    encoder.discard_unused();
    assert_eq!(encoder.image_bytes(), 0);
}

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

/// What the pictures of a part of a page look like on the screen, frame
/// by frame, for the pictures that play, as kitty plays them and as
/// kiri redraws them, from the frames built off the loop and from the
/// ones the encoder draws itself: the part, with the inset of the picture
/// that plays laid over it where its cells say. Pinned when the whole
/// part was the animation, and the screen stays as it was.
mod parts_on_screen {
    use super::*;
    use crate::render::Inputs;
    use crate::typeset::images::size::Frame as Outline;
    use std::sync::atomic::AtomicBool;

    /// The rows of a part of a page (`PART_ROWS` in the reading engine,
    /// which the case with two parts checks by having two).
    const PART_ROWS: i64 = 30;
    const YELLOW: [u8; 4] = [255, 255, 0, 255];

    /// A clip of three frames of one colour, `side` pixels square.
    fn clip(colours: [[u8; 4]; 3], side: u32) -> Source {
        let frames = colours
            .into_iter()
            .map(|rgba| AnimFrame {
                image: RgbaImage::from_pixel(side, side, image::Rgba(rgba)),
                gap: Duration::from_millis(100),
            })
            .collect();
        Source::Animated(Arc::new(Animation {
            frames,
            plays: Plays::Forever,
            over_budget: false,
        }))
    }

    /// The clip of `a.gif`, in shades of red, and of `b.gif`, in blue.
    fn clips(side: u32) -> [(&'static str, Source); 2] {
        let shades = |base: [u8; 3]| {
            [0, 20, 40].map(|more| [base[0] + more, base[1], base[2] + more / 2, 255])
        };
        [
            ("a.gif", clip(shades([180, 30, 30]), side)),
            ("b.gif", clip(shades([30, 60, 180]), side)),
        ]
    }

    /// A page of reading mode laid out from `source` with cells of
    /// `cell` pixels, and the placement of each of its parts.
    struct Page {
        images: Images,
        parts: Rc<Parts>,
        placements: Vec<Placement>,
        caps: Capabilities,
    }

    impl Page {
        fn of(source: &str, cell: (u16, u16), side: u32) -> Self {
            let caps = Capabilities {
                cell_pixels: Some(cell),
                ..Capabilities::kitty(80)
            };
            let mut inputs = Inputs {
                reading: Some(crate::typeset::reading::engine::Reading {
                    pictures: Default::default(),
                    glyphs: Default::default(),
                    fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                    wide: crate::typeset::reading::page::Wide::Wrap,
                    snap: false,
                    zoom: 100,
                }),
                ..Inputs::new(caps.clone())
            };
            for (name, source) in clips(side) {
                inputs.images.insert(name, Ok(source));
            }
            let document = crate::typeset::parse::parse(source);
            let lines = crate::typeset::driver::layout(&document, &inputs);
            let rows: Vec<_> = lines
                .iter()
                .filter_map(|line| line.image.as_ref())
                .collect();
            let parts = Rc::clone(rows[0].page.as_ref().expect("the parts of a page"));
            let mut placements: Vec<Placement> = Vec::new();
            for row in rows {
                if !placements.iter().any(|p| p.key == row.placement.key) {
                    placements.push((*row.placement).clone());
                }
            }
            Self {
                images: inputs.images,
                parts,
                placements,
                caps,
            }
        }

        /// The animated pictures on the page, from the top.
        fn pictures(&self) -> Vec<Pic> {
            self.parts.animated(&self.images)
        }

        /// The top of part `part` in pixels.
        fn top(&self, part: usize) -> i64 {
            part as i64 * PART_ROWS * i64::from(self.caps.cell_pixels.unwrap().1)
        }

        /// The pictures' states, `playing` playing and the others
        /// stopped.
        fn marks(&self, playing: Option<usize>) -> HashMap<ImageKey, State> {
            (self.pictures().into_iter().enumerate())
                .map(|(n, pic)| {
                    let state = match Some(n) == playing {
                        true => State::Playing,
                        false => State::Stopped,
                    };
                    (pic.key, state)
                })
                .collect()
        }

        /// The page's placements with the frame round picture `selected`.
        fn selecting(&mut self, selected: usize) {
            let picture = self.pictures()[selected].key.clone();
            for placement in &mut self.placements {
                placement.frame = Some(Outline {
                    color: Color::Rgb(255, 255, 0),
                    rows: 0..placement.size.rows,
                    picture: Some(picture.clone()),
                });
            }
        }
    }

    /// The images `out` sends by id: the first frame as `a=T` sends it,
    /// and each frame `a=f` adds.
    fn read(out: &[u8], held: &mut HashMap<u32, Vec<RgbaImage>>) {
        use base64::Engine;
        use std::io::Read;
        let out = String::from_utf8_lossy(out);
        let mut open: Option<(String, String)> = None;
        for sequence in out.split("\x1b_G").skip(1) {
            let body = sequence.split("\x1b\\").next().unwrap();
            let (keys, data) = body.split_once(';').unwrap_or((body, ""));
            let more = keys.split(',').any(|key| key == "m=1");
            let (keys, data) = match open.take() {
                Some((first, so_far)) if !keys.contains(",i=") => (first, so_far + data),
                _ => (keys.to_owned(), data.to_owned()),
            };
            if more {
                open = Some((keys, data));
                continue;
            }
            let value =
                |key: &str| (keys.split(',')).find_map(|k| k.strip_prefix(key)?.strip_prefix('='));
            let (Some(id), Some(action)) = (value("i"), value("a")) else {
                continue;
            };
            if !["T", "f"].contains(&action) {
                continue;
            }
            let id: u32 = id.parse().unwrap();
            let size = |key| value(key).unwrap().parse::<u32>().unwrap();
            let packed = base64::engine::general_purpose::STANDARD
                .decode(&data)
                .unwrap();
            let mut rgba = Vec::new();
            flate2::read::ZlibDecoder::new(&packed[..])
                .read_to_end(&mut rgba)
                .unwrap();
            if value("f") == Some("24") {
                // Three bytes a pixel, as sent for an opaque image.
                rgba = (rgba.as_chunks::<3>().0.iter())
                    .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
                    .collect();
            }
            let image = RgbaImage::from_raw(size("s"), size("v"), rgba).unwrap();
            match action {
                "T" => drop(held.insert(id, vec![image])),
                _ => held
                    .get_mut(&id)
                    .expect("a frame of an image sent")
                    .push(image),
            }
        }
    }

    /// FNV-1a of an image's size and pixels, which does not change with
    /// the version of Rust as `DefaultHasher` may.
    fn fnv(image: &RgbaImage) -> u64 {
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        let (width, height) = image.dimensions();
        let sizes = [width.to_le_bytes(), height.to_le_bytes()].concat();
        for byte in sizes.iter().chain(image.as_raw()) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01B3);
        }
        hash
    }

    /// How a picture's frames get to the encoder.
    #[derive(Clone, Copy, Debug)]
    enum Source3 {
        /// Built off the loop and installed, as the pager does.
        Built,
        /// Drawn by the encoder when it sends them.
        Drawn,
    }

    /// The frames of every part of `page`, as the screen shows them,
    /// with picture `playing` playing, for `playback` and `source`: for
    /// each part, the pixels of each frame.
    fn frames_of(
        page: &Page,
        playing: usize,
        playback: Playback,
        source: Source3,
    ) -> Vec<Vec<RgbaImage>> {
        let mut encoder = Encoder::new();
        encoder.play(playback);
        for placement in &page.placements {
            encoder.page(&placement.key, &page.parts, None);
        }
        let mut held: HashMap<u32, Vec<RgbaImage>> = HashMap::new();
        let mut out = Vec::new();
        // The parts as they stand before the picture starts.
        encoder.show_marks(page.marks(None));
        for placement in &page.placements {
            encoder
                .id(placement, &page.images, &page.caps, &mut out)
                .unwrap();
        }
        let picture = page.pictures()[playing].key.clone();
        if let Source3::Built = source {
            let jobs = encoder.frame_jobs(&picture, &page.images, &page.caps);
            for (placement, job) in jobs {
                let stop = AtomicBool::new(false);
                let frames = crate::typeset::images::build::build(&job, &stop);
                encoder.install_frames(placement, picture.clone(), frames.expect("built"));
            }
        }
        encoder.show_marks(page.marks(Some(playing)));
        let count = page.pictures()[playing].animation.frames.len();
        let mut parts = Vec::new();
        // What has been read of `out` into `held`.
        let mut read_to = 0;
        for placement in &page.placements {
            let mut shown = Vec::new();
            for index in 0..count {
                if playback == Playback::Redraw {
                    encoder.show_frames(HashMap::from([(picture.clone(), index)]));
                }
                let (part, inset) = encoder
                    .part_id(placement, &page.images, &page.caps, &mut out)
                    .unwrap();
                read(&out[read_to..], &mut held);
                read_to = out.len();
                // The frame the terminal shows of the inset: the one kiri
                // sent, or the one the terminal plays.
                let at = match playback {
                    Playback::Redraw => 0,
                    _ => index,
                };
                shown.push(on_screen(&held, part, inset.as_ref(), at, page));
            }
            parts.push(shown);
        }
        parts
    }

    /// What the screen shows of a part: its image, and over it frame
    /// `frame` of the inset in the inset's cells.
    fn on_screen(
        held: &HashMap<u32, Vec<RgbaImage>>,
        part: ImageId,
        inset: Option<&Inset>,
        frame: usize,
        page: &Page,
    ) -> RgbaImage {
        let (width, height) = page.caps.cell_pixels.unwrap();
        let part_frames = &held[&part.value()];
        let Some(inset) = inset else {
            // No inset: the part is all, with its frames if it has any.
            return part_frames[frame.min(part_frames.len() - 1)].clone();
        };
        let mut image = part_frames[0].clone();
        let frames = &held[&inset.id.value()];
        let x = i64::from(inset.columns.start) * i64::from(width);
        let y = i64::from(inset.rows.start) * i64::from(height);
        imageops::replace(&mut image, &frames[frame.min(frames.len() - 1)], x, y);
        image
    }

    /// What `frames` of a case look like, one line a frame.
    fn described(
        name: &str,
        page: &Page,
        playing: usize,
        lines: &mut Vec<String>,
    ) -> Vec<Vec<Vec<RgbaImage>>> {
        let mut all = Vec::new();
        for playback in [Playback::Terminal, Playback::Redraw] {
            for source in [Source3::Built, Source3::Drawn] {
                let parts = frames_of(page, playing, playback, source);
                // The ways show the same pixels (`all_alike` checks it), so
                // the snapshot has the first of them.
                for (part, frames) in parts.iter().enumerate().filter(|_| all.is_empty()) {
                    for (frame, image) in frames.iter().enumerate() {
                        let (width, height) = image.dimensions();
                        lines.push(format!(
                            "{name} part {part} frame {frame}: {width}x{height} {:016x}",
                            fnv(image)
                        ));
                    }
                }
                all.push(parts);
            }
        }
        all
    }

    /// A page of fourteen paragraphs and then a picture, which starts
    /// at 560 pixels of 20-pixel cells and so lies over the edge between
    /// the first part, 600 pixels, and the second.
    fn over_the_edge() -> String {
        format!("{}![a](a.gif)\n", "Line of text here.\n\n".repeat(14))
    }

    /// Whether `region` of `image` (left, top, width, height) has a
    /// pixel that is nearly white, a play button's triangle.
    fn white_in(image: &RgbaImage, (x, y, width, height): (i64, i64, u32, u32)) -> bool {
        (0..height).any(|dy| {
            (0..width).any(|dx| {
                let (px, py) = ((x + i64::from(dx)) as u32, (y + i64::from(dy)) as u32);
                let [r, g, b, a] = image.get_pixel(px, py).0;
                r > 235 && g > 235 && b > 235 && a > 200
            })
        })
    }

    /// How many pixels of `region` of `image` are `rgba`.
    fn count_in(image: &RgbaImage, rgba: [u8; 4], region: (i64, i64, u32, u32)) -> usize {
        let (x, y, width, height) = region;
        (0..height)
            .flat_map(|dy| (0..width).map(move |dx| (dx, dy)))
            .filter(|(dx, dy)| {
                let at = ((x + i64::from(*dx)) as u32, (y + i64::from(*dy)) as u32);
                image.get_pixel(at.0, at.1).0 == rgba
            })
            .count()
    }

    /// Every way of getting a picture's frames to the screen shows the
    /// same pixels: the terminal's own playing and kiri's redrawing,
    /// frames built off the loop and frames drawn by the encoder.
    fn all_alike(name: &str, all: &[Vec<Vec<RgbaImage>>]) {
        let hashes: Vec<Vec<Vec<u64>>> = (all.iter())
            .map(|parts| {
                (parts.iter())
                    .map(|f| f.iter().map(fnv).collect())
                    .collect()
            })
            .collect();
        for (n, other) in hashes.iter().enumerate().skip(1) {
            assert_eq!(&hashes[0], other, "{name}: way {n} shows other pixels");
        }
    }

    /// The two pictures of [`Page`] in one part, the second playing, sent
    /// for `playback`: the encoder, the page, and the part's id and inset.
    fn second_plays(playback: Playback) -> (Encoder, Page, ImageId, Option<Inset>) {
        let page = Page::of("![a](a.gif)\n\n![b](b.gif)\n", (10, 20), 100);
        let mut encoder = Encoder::new();
        encoder.play(playback);
        encoder.page(&page.placements[0].key, &page.parts, None);
        encoder.show_marks(page.marks(None));
        let placement = &page.placements[0];
        let mut out = Vec::new();
        encoder
            .id(placement, &page.images, &page.caps, &mut out)
            .unwrap();
        encoder.show_marks(page.marks(Some(1)));
        let (part, inset) = encoder
            .part_id(placement, &page.images, &page.caps, &mut out)
            .unwrap();
        (encoder, page, part, inset)
    }

    /// The ids of the images `out` sends with `a=T`.
    fn sent(out: &[u8]) -> Vec<u32> {
        let mut held = HashMap::new();
        read(out, &mut held);
        held.into_keys().collect()
    }

    #[test]
    fn the_fallback_frame_of_a_narrow_part_goes_round_the_part_and_not_the_page() {
        let page = Page::of("Text.\n", (10, 20), 100);
        let placement = &page.placements[0];
        let (columns, rows) = (placement.size.columns, placement.size.rows);
        assert!(columns < page.parts.cells().0.0, "{columns} columns");
        let mark = [1, 2, 3];
        let pages = HashMap::from([(page.parts.id, Rc::clone(&page.parts))]);
        let scaled = Scaled::new(
            placement,
            &page.images,
            (&pages, &HashMap::new()),
            (10, 20),
            None,
            Some((mark, 0..rows)),
            &HashMap::new(),
        )
        .unwrap();
        // The canvas is the part's cells, and the frame is on its edge.
        let (width, height) = scaled.pixels.dimensions();
        assert_eq!(
            (width, height),
            (u32::from(columns) * 10, u32::from(rows) * 20)
        );
        for (x, y) in [
            (0, 0),
            (width - 1, 0),
            (0, height - 1),
            (width - 1, height - 1),
        ] {
            assert_eq!(scaled.pixels.get_pixel(x, y).0, [1, 2, 3, 255], "{x} {y}");
        }
        assert_eq!(scaled.pixels.get_pixel(width - 2, height / 2).0[3], 0);
    }

    #[test]
    fn while_a_picture_plays_the_part_under_it_is_sent_again_when_the_terminal_lost_it() {
        for playback in [Playback::Terminal, Playback::Redraw] {
            let (mut encoder, page, part, _) = second_plays(playback);
            let mut out = Vec::new();
            encoder
                .send_again(part, &page.images, &page.caps, &mut out)
                .unwrap();
            assert_eq!(sent(&out), [part.value()], "{playback:?}");
        }
    }

    #[test]
    fn while_a_picture_plays_its_inset_is_sent_with_the_next_draw_when_the_terminal_lost_it() {
        for playback in [Playback::Terminal, Playback::Redraw] {
            let (mut encoder, page, _, inset) = second_plays(playback);
            let inset = inset.expect("an inset");
            let mut out = Vec::new();
            (encoder.send_again(inset.id, &page.images, &page.caps, &mut out)).unwrap();
            let placement = &page.placements[0];
            let (_, again) = encoder
                .part_id(placement, &page.images, &page.caps, &mut out)
                .unwrap();
            assert_eq!(again.as_ref().map(|again| again.id), Some(inset.id));
            assert!(sent(&out).contains(&inset.id.value()), "{playback:?}");
            // Once: the draw after it has it.
            let mut more = Vec::new();
            (encoder.part_id(placement, &page.images, &page.caps, &mut more)).unwrap();
            assert!(sent(&more).is_empty(), "{playback:?}");
        }
    }

    #[test]
    fn the_frames_of_a_part_on_the_screen() {
        let mut lines = Vec::new();
        // A picture that starts at 45 pixels down and 18 across, on cells
        // of 11 × 23 pixels: on no cell's edge, and the rest of the cell
        // it ends in is the page's.
        let off_grid = Page::of("> Text.\n>\n> ![a](a.gif)\n", (11, 23), 100);
        let pics = off_grid.pictures();
        assert_eq!(
            (pics[0].x, pics[0].y, pics[0].width, pics[0].height),
            (18.0, 45.0, 115, 115)
        );
        let all = described("off-grid", &off_grid, 0, &mut lines);
        all_alike("off-grid", &all);

        // Two pictures in one part: the one that plays, selected, has
        // its frame in each of its frames, and the other stays still
        // with its play button.
        let mut two = Page::of("![a](a.gif)\n\n![b](b.gif)\n", (10, 20), 100);
        two.selecting(1);
        let pics = two.pictures();
        assert_eq!(pics.len(), 2);
        let all = described("two", &two, 1, &mut lines);
        all_alike("two", &all);
        let region = |pic: &Pic| (pic.x as i64, pic.y as i64, pic.width, pic.height);
        for frame in &all[0][0] {
            assert!(
                white_in(frame, region(&pics[0])),
                "the other one has a button"
            );
            assert!(
                !white_in(frame, region(&pics[1])),
                "the one that plays has none"
            );
            // The frame goes round the second picture, inside its edge,
            // and not round the first.
            let (b, a) = (region(&pics[1]), region(&pics[0]));
            assert!(
                count_in(frame, YELLOW, (b.0, b.1, 1, b.3)) > 80,
                "its left edge"
            );
            assert_eq!(count_in(frame, YELLOW, a), 0, "none round the other");
        }
        // And when the first plays, the second has the button.
        let mut two = Page::of("![a](a.gif)\n\n![b](b.gif)\n", (10, 20), 100);
        two.selecting(0);
        let all = described("two-first", &two, 0, &mut lines);
        all_alike("two-first", &all);
        let pics = two.pictures();
        for frame in &all[0][0] {
            assert!(white_in(frame, region(&pics[1])), "the second has a button");
            assert!(!white_in(frame, region(&pics[0])));
        }

        // The same with cells of 16 × 32 pixels.
        let mut large = Page::of("![a](a.gif)\n\n![b](b.gif)\n", (16, 32), 100);
        large.selecting(1);
        let all = described("two-16x32", &large, 1, &mut lines);
        all_alike("two-16x32", &all);
        let pics = large.pictures();
        for frame in &all[0][0] {
            assert!(
                white_in(frame, region(&pics[0])),
                "the other one has a button"
            );
            assert!(!white_in(frame, region(&pics[1])));
        }

        // A picture over the edge of two parts: two parts, each with
        // its share of the picture.
        let edge = Page::of(&over_the_edge(), (10, 20), 100);
        assert_eq!(edge.placements.len(), 2, "two parts");
        let pics = edge.pictures();
        assert_eq!((pics[0].y, pics[0].height), (560.0, 100));
        let all = described("edge", &edge, 0, &mut lines);
        all_alike("edge", &all);
        assert_eq!(all[0].len(), 2);

        // And selected: the frame is cut by the edge too.
        let mut selected = Page::of(&over_the_edge(), (10, 20), 100);
        selected.selecting(0);
        let all = described("edge-selected", &selected, 0, &mut lines);
        all_alike("edge-selected", &all);
        let (first, second) = (&all[0][0][1], &all[0][1][1]);
        let (x, y) = (pics[0].x as i64, pics[0].y as i64);
        // The rectangle is cut where the part ends, at 600: its top is in
        // the first part and its bottom in the second, and each has the
        // sides of it that lie in it.
        let below = y + 100 - selected.top(1);
        assert!(count_in(first, YELLOW, (x, y, 100, 1)) > 90, "its top edge");
        assert_eq!(
            count_in(first, YELLOW, (x + 3, 599, 94, 1)),
            0,
            "not cut across"
        );
        assert!(
            count_in(first, YELLOW, (x, 599, 1, 1)) == 1,
            "its side goes on"
        );
        assert_eq!(count_in(second, YELLOW, (x + 3, 0, 94, 1)), 0, "not across");
        assert!(
            count_in(second, YELLOW, (x, 0, 1, 1)) == 1,
            "its side comes on"
        );
        assert!(
            count_in(second, YELLOW, (x, below - 1, 100, 1)) > 90,
            "its bottom edge"
        );

        insta::assert_snapshot!(lines.join("\n"));
    }
}
