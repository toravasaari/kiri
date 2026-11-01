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
            files: then.map(|then| then as &mut dyn SourceFiles),
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
    let shown = out
        .find(crate::present::kitty::graphics::PLACEHOLDER)
        .unwrap();
    assert!(sent < shown, "{out:?}");
}

/// Reloads `document` with the image `square.png` in `rgba`.
struct Repaint {
    document: Document,
    rgba: [u8; 4],
}

impl SourceFiles for Repaint {
    fn reread(
        &mut self,
        _: &Reread,
        images: &mut Images,
        _: &mut Loads,
        _: &Capabilities,
        _: bool,
    ) -> (Option<Look>, Reloaded) {
        images.insert("square.png", Ok(square(self.rgba)));
        (Some(READ), Reloaded::Document(self.document.clone()))
    }

    fn look(&mut self, _: &Path) -> Result<Look, String> {
        Ok(READ)
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
    let reload: &mut dyn SourceFiles = &mut repaint;
    run_document(
        document,
        &mut inputs,
        Options {
            files: Some(reload),
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
    assert_eq!(
        out.matches(crate::present::kitty::graphics::PLACEHOLDER)
            .count(),
        4
    );
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
    let without = crate::typeset::parse::parse("Above\n\nBelow\n");
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
    let document = crate::typeset::parse::parse("![a](square.png)\n\n![b](twin.png)\n");
    let (_, mut inputs) = pictured(red);
    inputs.images.insert("twin.png", Ok(square(red)));
    let mut repaint = Repaint {
        document: crate::typeset::parse::parse(after),
        rgba,
    };
    let reload: &mut dyn SourceFiles = &mut repaint;
    let mut terminal = Fake::new(60, 6, &[b"r"]);
    run_document(
        &document,
        &mut inputs,
        Options {
            files: Some(reload),
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
    let key = |cell, headings| kiri_heading(cell, headings);
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
        inputs.images.insert(key((10, 20), headings), Ok(text));
        inputs.headings = headings;
        let mut background = Loads::new();
        let (settings, env) = (plain_settings(), crate::config::Environment::default());
        let options = Options {
            planner: Some(heading_planner(
                headings,
                inputs.theme.clone(),
                &settings,
                &env,
            )),
            ..Options::new(SearchCase::Smart)
        };
        run_document_in(
            &crate::typeset::parse::parse("# Kiri\n\nText\n"),
            &mut inputs,
            options,
            &mut background,
            &mut terminal,
        );
        let wanted = key(after, headings);
        // Asked for once the window told the new cells: loading, or come.
        let asked = background.loading(&wanted)
            || inputs.images.placeholder(&wanted).is_some()
            || (wanted != key((10, 20), headings) && inputs.images.get(&wanted).is_some());
        let keys: Vec<_> = inputs.images.iter().map(|(key, _)| key.clone()).collect();
        assert_eq!(asked, loads, "{after:?} {headings:?}: {keys:?}");
        background.halt();
    }
}

#[test]
fn a_cell_size_answer_loads_the_heading_images_for_it() {
    let key = |cell| kiri_heading(cell, crate::render::Headings::Auto);
    // Inside tmux the window tells tmux's guess of 16 x 32 pixels a cell
    // before and after the font changes, and kitty's answer tells 8 × 16
    // instead of the 10 × 20 detection found.
    let pixels = |columns: u16| WindowSize {
        columns,
        rows: 6,
        width: columns * 16,
        height: 6 * 32,
    };
    let mut terminal = Fake::new(60, 6, &[b"", b"\x1b[6;16;8t", b"q"]);
    terminal.size = pixels(60);
    terminal.sizes = [pixels(70)].into();
    let mut inputs = Inputs::new(Capabilities {
        text_sizing: false,
        cell_pixels: Some((10, 20)),
        tmux_passthrough: true,
        ..Capabilities::kitty(60)
    });
    let image = image::RgbaImage::from_pixel(100, 40, image::Rgba([200, 30, 30, 255]));
    let text = crate::images::Source::Text {
        image,
        breaks: Vec::new(),
        ends: vec![10],
    };
    inputs.images.insert(key((10, 20)), Ok(text));
    inputs.headings = crate::render::Headings::Auto;
    let mut background = Loads::new();
    let (settings, env) = (plain_settings(), crate::config::Environment::default());
    let options = Options {
        planner: Some(heading_planner(
            crate::render::Headings::Auto,
            inputs.theme.clone(),
            &settings,
            &env,
        )),
        ..Options::new(SearchCase::Smart)
    };
    run_document_in(
        &crate::typeset::parse::parse("# Kiri\n\nText\n"),
        &mut inputs,
        options,
        &mut background,
        &mut terminal,
    );
    let wanted = key((8, 16));
    let asked = background.loading(&wanted)
        || inputs.images.placeholder(&wanted).is_some()
        || inputs.images.get(&wanted).is_some();
    let keys: Vec<_> = inputs.images.iter().map(|(key, _)| key.clone()).collect();
    assert!(asked, "{keys:?}");
    background.halt();
}

/// Reloads `document` and notes the cell size the terminal had then.
struct NoteCells<'a> {
    document: Document,
    cells: &'a std::cell::RefCell<Vec<Option<(u16, u16)>>>,
}

impl SourceFiles for NoteCells<'_> {
    fn reread(
        &mut self,
        _: &Reread,
        _: &mut Images,
        _: &mut Loads,
        caps: &Capabilities,
        _: bool,
    ) -> (Option<Look>, Reloaded) {
        self.cells.borrow_mut().push(caps.cell_pixels);
        (Some(READ), Reloaded::Document(self.document.clone()))
    }

    fn look(&mut self, _: &Path) -> Result<Look, String> {
        Ok(READ)
    }
}

#[test]
fn r_after_a_cell_size_answer_reloads_for_the_new_cells() {
    // The answer and `r` come in one read: the reload starts its loads
    // for the cells the answer tells.
    let cells = std::cell::RefCell::new(Vec::new());
    let document = crate::typeset::parse::parse("Text\n");
    let mut then = NoteCells {
        document: document.clone(),
        cells: &cells,
    };
    let mut terminal = Fake::new(60, 6, &[b"\x1b[6;16;8tr", b"q"]);
    let mut inputs = Inputs::new(Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(60)
    });
    run_document(
        &document,
        &mut inputs,
        Options {
            files: Some(&mut then),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    assert_eq!(*cells.borrow(), [Some((8, 16))]);
}

#[test]
fn an_answer_after_r_in_one_read_reloads_for_the_new_cells() {
    // `r` ends the read's keys at once, but the answer behind it in the
    // same read still tells the cells the reload starts its loads for.
    let cells = std::cell::RefCell::new(Vec::new());
    let document = crate::typeset::parse::parse("Text\n");
    let mut then = NoteCells {
        document: document.clone(),
        cells: &cells,
    };
    let mut terminal = Fake::new(60, 6, &[b"r\x1b[6;16;8t", b"q"]);
    let mut inputs = Inputs::new(Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(60)
    });
    run_document(
        &document,
        &mut inputs,
        Options {
            files: Some(&mut then),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    assert_eq!(*cells.borrow(), [Some((8, 16))]);
    // The answer is not lost for the layouts after the reload.
    assert_eq!(inputs.caps.cell_pixels, Some((8, 16)));
}

#[test]
fn the_terminal_s_cell_size_stays_until_the_window_s_changes() {
    // Inside tmux in a container the window tells tmux's guess of 16 x 32
    // pixels a cell, and kitty's answer of 10 x 20 came first. A window
    // whose cells stay the same does not replace it; a new font, which
    // changes the window's cells, does.
    let pixels = |cell: (u16, u16)| WindowSize {
        columns: 60,
        rows: 6,
        width: 60 * cell.0,
        height: 6 * cell.1,
    };
    let sent = |after| {
        let (document, mut inputs) = pictured([255, 0, 0, 255]);
        inputs.caps.cell_pixels = Some((10, 20));
        let mut terminal = Fake::new(60, 6, &[b"", b"", b"q"]);
        terminal.size = pixels((16, 32));
        terminal.sizes = [pixels((16, 32)), pixels(after)].into();
        let out = written(&document, inputs, &mut terminal, None);
        // The square's size in pixels, as many as a row's height.
        (out.split("\x1b_Ga=T,").skip(1))
            .map(|rest| {
                let at = rest.find(",s=").unwrap() + 3;
                let digits = rest[at..].chars().take_while(char::is_ascii_digit);
                digits.collect::<String>().parse::<u16>().unwrap()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(sent((16, 32)), [20]);
    assert_eq!(sent((8, 16)), [20, 16]);
}

/// The size in pixels of each square the pager sends when the window
/// goes from 60 to 72 columns, with the cells of `cells` pixels before and
/// after as the window tells them, where detection found 10 × 20, and
/// `answer` comes after that: the terminal's answer to the query for its
/// cell size, if any. Also all that was written.
fn squares_after_a_new_font(
    tmux: bool,
    cells: [(u16, u16); 2],
    answer: &[u8],
) -> (Vec<u16>, String) {
    let pixels = |columns: u16, cell: (u16, u16)| WindowSize {
        columns,
        rows: 6,
        width: columns * cell.0,
        height: 6 * cell.1,
    };
    let (document, mut inputs) = pictured([255, 0, 0, 255]);
    inputs.caps.cell_pixels = Some((10, 20));
    inputs.caps.tmux_passthrough = tmux;
    let mut terminal = Fake::new(60, 6, &[b"", answer, b"q"]);
    terminal.size = pixels(60, cells[0]);
    terminal.sizes = [pixels(72, cells[1])].into();
    let out = written(&document, inputs, &mut terminal, None);
    // The square's size in pixels, as many as a row's height.
    let sent = (out.split("\x1b_Ga=T,").skip(1))
        .map(|rest| {
            let at = rest.find(",s=").unwrap() + 3;
            let digits = rest[at..].chars().take_while(char::is_ascii_digit);
            digits.collect::<String>().parse::<u16>().unwrap()
        })
        .collect();
    (sent, out)
}

/// kitty's answer to the cell size query for cells of 10 × 24 pixels, which
/// make a 20 × 20 square 30 × 24 (whole cells).
const TEN_BY_24: &[u8] = b"\x1b[6;24;10t";

#[test]
fn in_tmux_a_new_font_is_followed_by_the_cell_size_kitty_answers() {
    // The window tells tmux's guess of 16 x 32 pixels a cell before and
    // after the font changes. The page is laid out at once for the cells
    // known, and again when the answer tells others.
    let (sent, out) = squares_after_a_new_font(true, [(16, 32); 2], TEN_BY_24);
    assert_eq!(sent, [20, 30], "{out:?}");
    let query = "\x1bPtmux;\x1b\x1b[16t\x1b\\";
    assert_eq!(out.matches(query).count(), 1, "{out:?}");
    assert_eq!(out.matches("\x1b[16t").count(), 1, "wrapped: {out:?}");
}

#[test]
fn outside_tmux_a_window_without_pixels_is_asked_for_the_cell_size_too() {
    let (sent, out) = squares_after_a_new_font(false, [(0, 0); 2], TEN_BY_24);
    assert_eq!(sent, [20, 30], "{out:?}");
    assert_eq!(out.matches("\x1b[16t").count(), 1, "{out:?}");
    assert!(!out.contains("\x1bPtmux;"), "{out:?}");
}

#[test]
fn a_window_that_tells_its_pixels_outside_tmux_is_not_asked() {
    // As in kitty on the host: the window's cells are the terminal's.
    let (sent, out) = squares_after_a_new_font(false, [(10, 20), (10, 24)], b"");
    assert_eq!(sent, [20, 30], "{out:?}");
    assert!(!out.contains("\x1b[16t"), "{out:?}");
}

#[test]
fn an_answer_that_is_the_cell_size_in_use_does_not_lay_the_page_out_again() {
    let (sent, out) = squares_after_a_new_font(true, [(16, 32); 2], b"\x1b[6;20;10t");
    assert_eq!(sent, [20], "{out:?}");
    let drawn = out.matches("\x1b[?2026h").count();
    assert_eq!(drawn, 2, "the first frame and the one after the resize");
}

#[test]
fn of_several_answers_in_one_read_the_last_one_counts() {
    // Two font changes before the pager reads: 20 × 40 pixels a cell and
    // then 10 × 24, which makes the square 30 × 24.
    let both = b"\x1b[6;40;20t\x1b[6;24;10t";
    let (sent, out) = squares_after_a_new_font(true, [(16, 32); 2], both);
    assert_eq!(sent, [20, 30], "{out:?}");
    // The last one is the cells in use: nothing is laid out again.
    let both = b"\x1b[6;40;20t\x1b[6;20;10t";
    let (sent, out) = squares_after_a_new_font(true, [(16, 32); 2], both);
    assert_eq!(sent, [20], "{out:?}");
}

#[test]
fn a_key_after_an_answer_in_the_same_read_counts() {
    let (sent, out) = squares_after_a_new_font(true, [(16, 32); 2], b"\x1b[6;24;10t?");
    assert_eq!(sent, [20, 30], "{out:?}");
    assert!(out.contains(" Keys "), "{out:?}");
}

#[test]
fn an_answer_kept_across_a_key_that_ends_the_frame_counts() {
    // `W` lays the page out again, and the answer before it in the read
    // is applied to that layout.
    let (sent, out) = squares_after_a_new_font(true, [(16, 32); 2], b"\x1b[6;24;10tW");
    assert_eq!(sent, [20, 30], "{out:?}");
}

#[test]
fn without_an_answer_the_pager_keeps_the_cell_size_it_has() {
    let (sent, out) = squares_after_a_new_font(true, [(16, 32); 2], b"");
    assert_eq!(sent, [20], "{out:?}");
    assert_eq!(out.matches("\x1b[16t").count(), 1, "{out:?}");
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
    let key = inputs.drawn(crate::images::ImageKey::heading(
        1,
        "One two three",
        (10, 20),
    ));
    inputs.images.insert(key, Ok(text));
    run_document(
        &crate::typeset::parse::parse("# One two three\n\nText\n"),
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
        let diacritic = crate::present::kitty::graphics::DIACRITICS[at];
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
    let shown = frames[1]
        .find(crate::present::kitty::graphics::PLACEHOLDER)
        .unwrap();
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
                .matches(crate::present::kitty::graphics::PLACEHOLDER)
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
        !after.contains(crate::present::kitty::graphics::PLACEHOLDER),
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
    let (out, sent) = repainted_while_away(&[b"\x1b[O", b"r", b"\x1b[I"]);
    // Red and green, blue while away, and blue again on coming back.
    assert_eq!(sent.len(), 4, "{out:?}");
    assert_eq!(sent[2], sent[3], "{out:?}");
    assert!(!sent[..2].contains(&sent[2]), "{out:?}");
}

#[test]
fn a_second_focus_out_keeps_what_was_sent_while_away() {
    // kitty losing focus, and then tmux hiding the pane, say so twice.
    let (out, sent) = repainted_while_away(&[b"\x1b[O", b"r", b"\x1b[O", b"\x1b[I"]);
    assert_eq!(sent.len(), 4, "{out:?}");
    assert_eq!(sent[2], sent[3], "{out:?}");
}

/// The output of the pager under tmux, given `keys`, and the ids of the
/// images it sent: a red and a green image, where `r` shows the red one
/// anew in blue.
fn repainted_while_away(keys: &[&[u8]]) -> (String, Vec<u32>) {
    let red = [255, 0, 0, 255];
    let document = crate::typeset::parse::parse("![a](square.png)\n\n![b](other.png)\n");
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
    let reload: &mut dyn SourceFiles = &mut repaint;
    let mut terminal = Fake::new(60, 6, keys);
    run_document(
        &document,
        &mut inputs,
        Options {
            files: Some(reload),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    let out = String::from_utf8(terminal.out).unwrap();
    let sent = sent_ids(&out);
    (out, sent)
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
            files: Some(&mut then),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    let frames = terminal.frames();
    assert_eq!(top_row(frames.last().unwrap()), "Line 1", "{frames:#?}");
    assert_eq!(then.asked, 1);
}

/// A planner that wants the heading images of the document shown, drawn as
/// `headings` says for the cells of the terminal it is asked for.
fn heading_planner<'a>(
    headings: crate::render::Headings,
    theme: crate::typeset::theme::Theme,
    settings: &'a crate::config::Settings,
    env: &'a crate::config::Environment,
) -> crate::pager::Planner<'a> {
    Box::new(move |content, caps, _| {
        let Content::Document(document) = content else {
            return Vec::new();
        };
        let part = crate::images::Part {
            document,
            number: 0,
            base: std::path::Path::new(""),
            by_line: true,
            inline_images: false,
        };
        let loading = crate::images::Loading {
            env,
            settings,
            max_file_size: crate::images::DEFAULT_MAX_FILE_SIZE,
            theme: crate::diagrams::Theme::Light,
            formula: crate::diagrams::FormulaColors::default(),
            largest: None,
            headings: crate::render::heading_look(headings, false, &theme, caps),
        };
        crate::images::jobs(&[part], &loading)
    })
}

/// The key of the heading "Kiri" drawn for cells of `cell` pixels, as the
/// layout of a window of kitty with those cells and `headings` looks it up.
fn kiri_heading(cell: (u16, u16), headings: crate::render::Headings) -> crate::images::ImageKey {
    let inputs = Inputs {
        headings,
        ..Inputs::new(Capabilities {
            text_sizing: false,
            cell_pixels: Some(cell),
            ..Capabilities::kitty(60)
        })
    };
    inputs.drawn(crate::images::ImageKey::heading(1, "Kiri", cell))
}
