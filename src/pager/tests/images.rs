use super::*;

/// All the pager writes for `document` with `inputs` on `terminal`,
/// reloading it from `then` when there is one.
fn written(
    document: &Document,
    mut inputs: Inputs,
    terminal: &mut Fake,
    then: Option<&mut Then>,
) -> String {
    run_document(
        document,
        &mut inputs,
        Options {
            reload: then.map(|then| then as &mut dyn Reload),
            ..Options::new(SearchCase::Smart)
        },
        terminal,
    );
    String::from_utf8(terminal.out.clone()).unwrap()
}

/// How many images `out` sends.
fn transmits(out: &str) -> usize {
    out.matches("\x1b_Ga=T,").count()
}

#[test]
fn an_image_is_sent_before_its_first_row() {
    let (document, inputs) = pictured([255, 0, 0, 255]);
    let mut terminal = Fake::new(60, 6, &[b"q"]);
    let out = written(&document, inputs, &mut terminal, None);
    assert_eq!(transmits(&out), 1, "{out:?}");
    let sent = out.find("\x1b_Ga=T,").unwrap();
    let shown = out.find(crate::kitty::graphics::PLACEHOLDER).unwrap();
    assert!(sent < shown, "{out:?}");
}

/// Reloads `document` with the image `square.png` in `rgba`.
struct Repaint {
    document: Document,
    rgba: [u8; 4],
}

impl Reload for Repaint {
    fn reload(&mut self, images: &mut Images, _: &mut Loads, _: &Capabilities) -> Reloaded {
        images.insert("square.png", Ok(square(self.rgba)));
        Reloaded::Document(self.document.clone())
    }

    fn change(&mut self) -> Change {
        Change::Unchanged
    }
}

/// All the pager writes for `document`, whose image is `square.png`
/// in `before`, when `r` reloads it with the image in `after`.
fn repainted(document: &Document, before: [u8; 4], after: [u8; 4]) -> String {
    let (_, mut inputs) = pictured(before);
    let mut terminal = Fake::new(60, 6, &[b"r"]);
    let mut repaint = Repaint {
        document: document.clone(),
        rgba: after,
    };
    let reload: &mut dyn Reload = &mut repaint;
    run_document(
        document,
        &mut inputs,
        Options {
            reload: Some(reload),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    String::from_utf8(terminal.out).unwrap()
}

#[test]
fn an_image_the_terminal_holds_is_not_sent_again_after_a_resize() {
    let (document, inputs) = pictured([255, 0, 0, 255]);
    let mut terminal = Fake::new(60, 6, &[b""]);
    // Wider, and the image at the same size.
    terminal.sizes.push_back(window(70, 6));
    let out = written(&document, inputs, &mut terminal, None);
    assert_eq!(terminal.frames().len(), 2, "{out:?}");
    assert_eq!(out.matches(crate::kitty::graphics::PLACEHOLDER).count(), 4);
    assert_eq!(transmits(&out), 1, "{out:?}");
}

#[test]
fn a_reload_that_leaves_the_image_as_it_was_does_not_send_it_again() {
    let (document, _) = pictured([255, 0, 0, 255]);
    let out = repainted(&document, [255, 0, 0, 255], [255, 0, 0, 255]);
    assert_eq!(transmits(&out), 1, "{out:?}");
}

#[test]
fn a_changed_image_is_sent_anew_and_the_old_one_deleted() {
    let (document, _) = pictured([255, 0, 0, 255]);
    let out = repainted(&document, [255, 0, 0, 255], [0, 0, 255, 255]);
    let sent = sent_ids(&out);
    assert_eq!(sent.len(), 2, "{out:?}");
    assert_ne!(sent[0], sent[1]);
    // The old image goes before the new one comes, and the new one on
    // quitting.
    assert_eq!(deleted_ids(&out), sent, "{out:?}");
    let deleted = out.find("\x1b_Ga=d,").unwrap();
    assert!(deleted < out.rfind("\x1b_Ga=T,").unwrap(), "{out:?}");
}

#[test]
fn a_layout_without_the_image_deletes_it_as_its_frame_is_drawn() {
    let (document, inputs) = pictured([255, 0, 0, 255]);
    let without = crate::parse::parse("Above\n\nBelow\n");
    let mut then = Then::new([Reloaded::Document(without)]);
    let mut terminal = Fake::new(60, 6, &[b"r"]);
    let out = written(&document, inputs, &mut terminal, Some(&mut then));
    let sent = sent_ids(&out);
    assert_eq!(sent.len(), 1, "{out:?}");
    assert_eq!(deleted_ids(&out), sent, "{out:?}");
    // In the frame of the new layout, before anything else in it.
    let frames = terminal.frames();
    let last = frames.last().unwrap();
    assert!(last.starts_with("\x1b_Ga=d,"), "{frames:#?}");
}

#[test]
fn a_resize_that_shows_the_image_smaller_deletes_the_larger_one() {
    let (document, mut inputs) = pictured([255, 0, 0, 255]);
    let wide = image::RgbaImage::from_pixel(400, 20, image::Rgba([0, 255, 0, 255]));
    let wide = crate::images::Source::Raster(wide);
    inputs.images.insert("square.png", Ok(wide));
    let mut terminal = Fake::new(60, 6, &[b""]);
    terminal.sizes.push_back(window(30, 6));
    let out = written(&document, inputs, &mut terminal, None);
    let sent = sent_ids(&out);
    assert_eq!(sent.len(), 2, "{out:?}");
    // The smaller one goes on quitting.
    assert_eq!(deleted_ids(&out), sent, "{out:?}");
}

#[test]
fn quitting_deletes_every_image_the_terminal_holds() {
    // q, and Ctrl+C once the keys run out, each after a resize.
    for keys in [&[b"".as_slice(), b"q"][..], &[b""]] {
        let (document, mut inputs) = pictured([255, 0, 0, 255]);
        let wide = image::RgbaImage::from_pixel(400, 20, image::Rgba([0, 255, 0, 255]));
        let wide = crate::images::Source::Raster(wide);
        inputs.images.insert("square.png", Ok(wide));
        let mut terminal = Fake::new(60, 6, keys);
        // The image smaller in a narrower window.
        terminal.sizes.push_back(window(30, 6));
        let out = written(&document, inputs, &mut terminal, None);
        let sent = sent_ids(&out);
        assert_eq!(sent.len(), 2, "{out:?}");
        // The wide image as the window narrows, the narrow one on
        // quitting, after the last frame.
        assert_eq!(deleted_ids(&out), sent, "{out:?}");
        let last = out.rsplit("\x1b[?2026l").next().unwrap();
        let quit = format!("\x1b_Ga=d,d=I,i={},q=2\x1b\\", sent[1]);
        assert_eq!(last, quit, "{out:?}");
    }
}

/// All the pager writes for a document that shows `square.png` and
/// `twin.png`, both of the same red pixels and so of the same id,
/// when `r` reloads it as `after` with `square.png` in `rgba`.
fn twins_reloaded(after: &str, rgba: [u8; 4]) -> String {
    let red = [255, 0, 0, 255];
    let document = crate::parse::parse("![a](square.png)\n\n![b](twin.png)\n");
    let (_, mut inputs) = pictured(red);
    inputs.images.insert("twin.png", Ok(square(red)));
    let mut repaint = Repaint {
        document: crate::parse::parse(after),
        rgba,
    };
    let reload: &mut dyn Reload = &mut repaint;
    let mut terminal = Fake::new(60, 6, &[b"r"]);
    run_document(
        &document,
        &mut inputs,
        Options {
            reload: Some(reload),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    String::from_utf8(terminal.out).unwrap()
}

#[test]
fn a_changed_image_keeps_the_id_another_placement_still_shows() {
    let out = twins_reloaded("![a](square.png)\n\n![b](twin.png)\n", [0, 0, 255, 255]);
    let sent = sent_ids(&out);
    // Both, of one id, and `square.png` anew.
    assert_eq!(sent.len(), 3, "{out:?}");
    assert_eq!(sent[0], sent[1], "{out:?}");
    assert_ne!(sent[1], sent[2], "{out:?}");
    // `twin.png` still shows the red one until the pager quits.
    let (running, quitting) = until_quit(&out);
    assert_eq!(deleted_ids(running), [0; 0], "{out:?}");
    assert_eq!(sorted(deleted_ids(quitting)), sorted(sent), "{out:?}");
}

#[test]
fn a_layout_without_one_placement_keeps_the_id_the_other_shows() {
    let out = twins_reloaded("![b](twin.png)\n", [255, 0, 0, 255]);
    let sent = sent_ids(&out);
    assert_eq!(sent.len(), 2, "{out:?}");
    assert_eq!(sent[0], sent[1], "{out:?}");
    let (running, quitting) = until_quit(&out);
    assert_eq!(deleted_ids(running), [0; 0], "{out:?}");
    assert_eq!(deleted_ids(quitting), [sent[0]], "{out:?}");
}

#[test]
fn new_cell_pixels_send_the_image_again_and_delete_the_old_one() {
    let (document, inputs) = pictured([255, 0, 0, 255]);
    let mut terminal = Fake::new(60, 6, &[b""]);
    // Wider cells, as after another font: 12 × 20 pixels. The image
    // is as large and covers as many cells, but they are others.
    terminal.sizes.push_back(WindowSize {
        width: 720,
        height: 120,
        ..window(60, 6)
    });
    let out = written(&document, inputs, &mut terminal, None);
    let sent = sent_ids(&out);
    assert_eq!(sent.len(), 2, "{out:?}");
    assert_ne!(sent[0], sent[1], "{out:?}");
    // The old one goes as the new layout's frame starts, before the
    // screen is cleared, and the new one on quitting.
    let frames = terminal.frames();
    let first = format!("\x1b_Ga=d,d=I,i={},q=2\x1b\\\x1b[H", sent[0]);
    assert!(frames[1].starts_with(&first), "{frames:#?}");
    assert_eq!(deleted_ids(&out), sent, "{out:?}");
}

#[test]
fn a_new_cell_size_loads_the_heading_images_for_it() {
    let key = |cell| crate::images::ImageKey::heading(1, "Kiri", cell);
    // Cells of 10 × 20 pixels, then of 8 × 16 when the font gets smaller,
    // or of the same size in a wider window. Without images, as
    // `--no-images` makes `auto` `sized`, nothing loads.
    let pixels = |columns: u16, cell: (u16, u16)| WindowSize {
        columns,
        rows: 6,
        width: columns * cell.0,
        height: 6 * cell.1,
    };
    let auto = crate::render::Headings::Auto;
    let sized = crate::render::Headings::Sized;
    for (after, headings, loads) in [
        ((8, 16), auto, true),
        ((10, 20), auto, false),
        ((8, 16), sized, false),
    ] {
        let mut terminal = Fake::new(60, 6, &[b"", b"", b"q"]);
        terminal.size = pixels(60, (10, 20));
        terminal.sizes = [pixels(60, (10, 20)), pixels(70, after)].into();
        let mut inputs = Inputs::new(Capabilities {
            text_sizing: false,
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(60)
        });
        let image = image::RgbaImage::from_pixel(100, 40, image::Rgba([200, 30, 30, 255]));
        let text = crate::images::Source::Text {
            image,
            breaks: Vec::new(),
            ends: vec![10],
        };
        inputs.images.insert(key((10, 20)), Ok(text));
        inputs.headings = headings;
        let mut background = Loads::new();
        run_document(
            &crate::parse::parse("# Kiri\n\nText\n"),
            &mut inputs,
            Options {
                loads: Some(&mut background),
                ..Options::new(SearchCase::Smart)
            },
            &mut terminal,
        );
        let wanted = key(after);
        // Asked for once the window told the new cells: loading, or come.
        let asked = background.loading(&wanted)
            || inputs.images.placeholder(&wanted).is_some()
            || (wanted != key((10, 20)) && inputs.images.get(&wanted).is_some());
        let keys: Vec<_> = inputs.images.iter().map(|(key, _)| key.clone()).collect();
        assert_eq!(asked, loads, "{after:?} {headings:?}: {keys:?}");
        background.halt();
    }
}

#[test]
fn a_narrower_window_rewraps_a_heading_image_without_a_new_image() {
    let mut terminal = Fake::new(100, 12, &[b"", b"", b"q"]);
    terminal.sizes = [window(100, 12), window(30, 12)].into();
    let mut inputs = Inputs::new(Capabilities {
        text_sizing: false,
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(100)
    });
    let image = image::RgbaImage::from_pixel(900, 40, image::Rgba([200, 30, 30, 255]));
    let text = crate::images::Source::Text {
        image,
        breaks: vec![30, 60],
        ends: vec![28, 58, 90],
    };
    let key = crate::images::ImageKey::heading(1, "One two three", (10, 20));
    inputs.images.insert(key, Ok(text));
    run_document(
        &crate::parse::parse("# One two three\n\nText\n"),
        &mut inputs,
        Options::new(SearchCase::Smart),
        &mut terminal,
    );
    let frames = terminal.frames();
    let out: String = frames.concat();
    // Sent once, and shown whole, then with a line that starts at its
    // column 60: the colour of the id, then that column's placeholder.
    assert_eq!(out.matches("a=T").count(), 1, "{frames:#?}");
    let column = |at: usize| {
        let diacritic = crate::kitty::graphics::DIACRITICS[at];
        format!("m\u{10EEEE}\u{0305}{diacritic}")
    };
    assert!(frames[0].contains(&column(0)), "{frames:#?}");
    assert!(!frames[0].contains(&column(60)), "{frames:#?}");
    assert!(frames.last().unwrap().contains(&column(60)), "{frames:#?}");
}

/// All the pager writes for the picture on a terminal with kitty's
/// graphics wrapped for tmux if `tmux`, and the frames it draws, when the
/// terminal gives `keys`.
fn pictured_with(tmux: bool, keys: &[&[u8]]) -> (String, Vec<String>) {
    let (document, mut inputs) = pictured([255, 0, 0, 255]);
    inputs.caps.tmux_passthrough = tmux;
    let mut terminal = Fake::new(60, 6, keys);
    let out = written(&document, inputs, &mut terminal, None);
    (out, terminal.frames())
}

#[test]
fn under_tmux_the_images_are_sent_again_when_the_pane_comes_back() {
    // tmux dropped what was sent while the pane was not shown.
    let (out, frames) = pictured_with(true, &[b"\x1b[I"]);
    assert_eq!(frames.len(), 2, "{out:?}");
    let sent = sent_ids(&out);
    assert_eq!(sent.len(), 2, "{out:?}");
    assert_eq!(sent[0], sent[1]);
    let again = frames[1].find("\x1b_Ga=T,").unwrap();
    let shown = frames[1].find(crate::kitty::graphics::PLACEHOLDER).unwrap();
    assert!(again < shown, "{out:?}");
}

#[test]
fn the_focus_coming_back_outside_tmux_changes_nothing() {
    let (out, frames) = pictured_with(false, &[b"\x1b[I"]);
    assert_eq!(frames.len(), 1, "{out:?}");
    assert_eq!(transmits(&out), 1, "{out:?}");
}

#[test]
fn the_focus_going_and_coming_back_leaves_the_help_open() {
    for tmux in [false, true] {
        let (out, frames) = pictured_with(tmux, &[b"?", b"\x1b[O", b"\x1b[I"]);
        let last = frames.last().unwrap();
        assert!(last.contains(" Keys "), "{out:?}");
    }
}

#[test]
fn ctrl_l_draws_the_screen_and_sends_its_images_again() {
    for tmux in [false, true] {
        let (out, frames) = pictured_with(tmux, &[b"\x0c"]);
        assert_eq!(frames.len(), 2, "{out:?}");
        let sent = sent_ids(&out);
        assert_eq!(sent.len(), 2, "{out:?}");
        assert_eq!(sent[0], sent[1]);
        assert_eq!(
            frames[1]
                .matches(crate::kitty::graphics::PLACEHOLDER)
                .count(),
            2
        );
    }
}

/// kitty's answer to a check of the picture's image, `ENOENT` unless
/// `held`.
fn checked(held: bool) -> Vec<u8> {
    let id = 3043215;
    let message = match held {
        true => "OK".to_owned(),
        false => format!("ENOENT:Put command refers to non-existent image with id: {id}"),
    };
    format!("\x1b_Gi={id},p=1263096393;{message}\x1b\\").into_bytes()
}

/// The start of a check of an image, up to its id.
const CHECK: &str = "\x1b_Ga=p,U=1,i=";

#[test]
fn under_tmux_an_image_kitty_lost_is_sent_again_without_a_frame() {
    let lost = checked(false);
    let (out, frames) = pictured_with(true, &[&lost]);
    // Checked after the frame that sent it, and again after it was sent
    // again.
    assert_eq!(frames.len(), 1, "{out:?}");
    let sent = sent_ids(&out);
    assert_eq!(sent, [3043215, 3043215], "{out:?}");
    assert_eq!(ids_after(&out, CHECK), [3043215, 3043215], "{out:?}");
    let (frame, after) = until_quit(&out);
    assert_eq!(transmits(frame), 1, "{out:?}");
    let again = after.find("\x1b_Ga=T,").unwrap();
    assert!(after[again..].contains(CHECK), "{out:?}");
    assert!(
        !after.contains(crate::kitty::graphics::PLACEHOLDER),
        "{out:?}"
    );
}

#[test]
fn an_image_kitty_holds_is_not_sent_again() {
    let held = checked(true);
    let (out, _) = pictured_with(true, &[&held]);
    assert_eq!(transmits(&out), 1, "{out:?}");
    assert_eq!(ids_after(&out, CHECK), [3043215], "{out:?}");
}

#[test]
fn without_tmux_the_pager_checks_nothing() {
    let (out, _) = pictured_with(false, &[b""]);
    assert!(!out.contains(CHECK), "{out:?}");
}

#[test]
fn kittys_answers_leave_the_help_open() {
    let (held, lost) = (checked(true), checked(false));
    let (out, frames) = pictured_with(true, &[b"?", &held, &lost]);
    assert!(frames.last().unwrap().contains(" Keys "), "{out:?}");
    assert_eq!(transmits(&out), 2, "{out:?}");
}

#[test]
fn the_pane_coming_back_sends_nothing_when_nothing_was_sent_meanwhile() {
    // kitty kept what it had before the pane was hidden.
    let (out, frames) = pictured_with(true, &[b"\x1b[O", b"", b"\x1b[I"]);
    assert_eq!(frames.len(), 1, "{out:?}");
    assert_eq!(transmits(&out), 1, "{out:?}");
}

#[test]
fn the_pane_coming_back_sends_again_what_was_sent_while_it_was_away() {
    let red = [255, 0, 0, 255];
    let document = crate::parse::parse("![a](square.png)\n\n![b](other.png)\n");
    let (_, mut inputs) = pictured(red);
    inputs.caps.tmux_passthrough = true;
    inputs
        .images
        .insert("other.png", Ok(square([0, 255, 0, 255])));
    // `r` while away shows `square.png` anew, in blue.
    let mut repaint = Repaint {
        document: document.clone(),
        rgba: [0, 0, 255, 255],
    };
    let reload: &mut dyn Reload = &mut repaint;
    let mut terminal = Fake::new(60, 6, &[b"\x1b[O", b"r", b"\x1b[I"]);
    run_document(
        &document,
        &mut inputs,
        Options {
            reload: Some(reload),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    let out = String::from_utf8(terminal.out).unwrap();
    let sent = sent_ids(&out);
    // Red and green, blue while away, and blue again on coming back.
    assert_eq!(sent.len(), 4, "{out:?}");
    assert_eq!(sent[2], sent[3], "{out:?}");
    assert!(!sent[..2].contains(&sent[2]), "{out:?}");
}

#[test]
fn an_answer_cut_by_a_reload_is_not_taken_for_keys() {
    // The rest of the answer has `d`, `e`, `f` and `g` in it.
    let answer = b"\x1b_Gi=5,p=1263096393;ENOENT:Put command refers to non-existent image\x1b\\";
    let (cut, rest) = answer.split_at(30);
    let first = [&b"r"[..], cut].concat();
    let document = numbers(30);
    // As many as the `r`s it has would ask for.
    let mut then = Then::new((0..3).map(|_| Reloaded::Document(document.clone())));
    let mut inputs = Inputs::new(Capabilities::ansi(80));
    let mut terminal = Fake::new(60, 6, &[&first, rest]);
    run_document(
        &document,
        &mut inputs,
        Options {
            reload: Some(&mut then),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    let frames = terminal.frames();
    assert_eq!(top_row(frames.last().unwrap()), "Line 1", "{frames:#?}");
    assert_eq!(then.asked, 1);
}
