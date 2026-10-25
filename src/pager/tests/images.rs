use super::*;

/// All the pager writes for `document` with `inputs` on `terminal`,
/// reloading it from `then` when there is one.
fn written(
    document: &Document,
    mut inputs: Inputs,
    terminal: &mut Fake,
    then: Option<&mut Then>,
) -> String {
    run(
        Content::Document(document),
        None,
        "notes.md",
        &mut inputs,
        Options {
            reload: then.map(|then| then as &mut dyn Reload),
            ..Options::new(SearchCase::Smart)
        },
        terminal,
    )
    .unwrap();
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
    run(
        Content::Document(document),
        None,
        "notes.md",
        &mut inputs,
        Options {
            reload: Some(reload),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    )
    .unwrap();
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
    run(
        Content::Document(&document),
        None,
        "notes.md",
        &mut inputs,
        Options {
            reload: Some(reload),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    )
    .unwrap();
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
