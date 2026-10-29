use super::*;

/// Images loading in a test, each with what it comes as.
type Coming = std::rc::Rc<
    std::cell::RefCell<
        Vec<(
            crate::images::Arrival,
            Result<crate::images::Source, ImageError>,
        )>,
    >,
>;

/// Reloads the document as `document`, with the images under `wanted`
/// keys loading in the background, to come as their pixels, and the
/// ones under `aside` set aside as no longer shown. Those under
/// `fails` come as errors. A reload after the first fails with
/// `afterwards`, when it is given.
struct Background {
    document: Document,
    wanted: Vec<(ImageKey, crate::images::Source)>,
    aside: Vec<ImageKey>,
    coming: Coming,
    fails: Vec<ImageKey>,
    afterwards: Option<String>,
    asked: usize,
}

impl Reload for Background {
    fn reload(
        &mut self,
        images: &mut Images,
        loads: &mut Loads,
        _: &Capabilities,
        _: bool,
    ) -> Reloaded {
        self.asked += 1;
        if let Some(why) = self.afterwards.clone().filter(|_| self.asked > 1) {
            return Reloaded::Failed(why);
        }
        for key in &self.aside {
            images.set_aside(key);
        }
        for (key, image) in self.wanted.drain(..) {
            let image = match self.fails.contains(&key) {
                true => Err(ImageError::Missing("gone.png".into())),
                false => Ok(image),
            };
            let arrival = loads.expect(key);
            self.coming.borrow_mut().push((arrival, image));
        }
        Reloaded::Document(self.document.clone())
    }

    fn change(&mut self) -> Change {
        Change::Unchanged
    }
}

/// The pixels of an image `width` × `height` in `rgba`.
fn filled(width: u32, height: u32, rgba: [u8; 4]) -> crate::images::Source {
    crate::images::Source::Raster(image::RgbaImage::from_pixel(
        width,
        height,
        image::Rgba(rgba),
    ))
}

/// A pager for kitty in a window of 60 × `rows` showing `before`,
/// with the images in `images`, that reloads it on `r` as `reload`
/// says, with the reads `reads`; the images loading come in order,
/// one at each read numbered in `comes`.
fn loading_in(
    rows: u16,
    before: &Document,
    images: Vec<(&str, crate::images::Source)>,
    reload: &mut Background,
    reads: &[&[u8]],
    comes: &[usize],
    follow: config::Follow,
) -> Fake {
    let caps = Capabilities::kitty(80);
    loading_with(caps, rows, before, images, reload, reads, comes, follow)
}

/// As [`loading_in`], on a terminal with `caps`.
#[allow(clippy::too_many_arguments)]
fn loading_with(
    caps: Capabilities,
    rows: u16,
    before: &Document,
    images: Vec<(&str, crate::images::Source)>,
    reload: &mut Background,
    reads: &[&[u8]],
    comes: &[usize],
    follow: config::Follow,
) -> Fake {
    let mut inputs = Inputs::new(caps);
    for (url, image) in images {
        inputs.images.insert(url, Ok(image));
    }
    let mut terminal = Fake::new(60, rows, reads);
    let coming = std::rc::Rc::clone(&reload.coming);
    let comes = comes.to_vec();
    terminal.hook = Some(Box::new(move |read| {
        if comes.contains(&read) {
            let (arrival, image) = coming.borrow_mut().remove(0);
            arrival.come(image);
        }
    }));
    run_document(
        before,
        &mut inputs,
        Options {
            reload: Some(reload),
            follow,
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    terminal
}

/// A document with `above` lines above the image `url` and three
/// below.
fn around(above: usize, url: &str) -> Document {
    let mut source: String = (1..=above).map(|n| format!("Line {n}\n\n")).collect();
    source.push_str(&format!(
        "![an image]({url})\n\nAfter 1\n\nAfter 2\n\nAfter 3\n"
    ));
    crate::parse::parse(&source)
}

/// Reloads `around(2, "a.png")`, whose image is a red square, as
/// `around(2, "b.png")` whose image is `b` loading.
fn red_then(b: crate::images::Source) -> Background {
    Background {
        document: around(2, "b.png"),
        wanted: vec![(ImageKey::from("b.png"), b)],
        aside: vec![ImageKey::from("a.png")],
        coming: Default::default(),
        fails: Vec::new(),
        afterwards: None,
        asked: 0,
    }
}

/// The text of the status line of `frame` in a window of `rows`.
fn status_text(frame: &str, rows: u16) -> String {
    unstyled(&status_row(frame, rows))
}

#[test]
fn a_reload_shows_the_new_text_at_once_and_the_image_when_it_comes() {
    let mut reload = red_then(filled(20, 20, [0, 0, 255, 255]));
    let red = filled(20, 20, [255, 0, 0, 255]);
    let terminal = loading_in(
        12,
        &around(2, "a.png"),
        vec![("a.png", red)],
        &mut reload,
        &[b"r", b"", b"", b""],
        &[3],
        config::Follow::Off,
    );
    let frames = terminal.frames();
    // The first frame, the one that says it reloads, the reload's,
    // drawn anew while the band moves, then the image's.
    assert_eq!(frames.len(), 6, "{frames:#?}");
    let reloaded = &frames[2];
    assert!(unstyled(reloaded).contains("After 2"), "{reloaded:?}");
    assert!(
        status_text(reloaded, 12).contains("[rendering: 0/1]"),
        "{reloaded:?}"
    );
    assert!(
        !status_text(reloaded, 12).contains("notes.md"),
        "{reloaded:?}"
    );
    let came = frames.last().unwrap();
    assert!(status_text(came, 12).contains("notes.md"), "{came:?}");
    assert!(!status_text(came, 12).contains("rendering"), "{came:?}");
    // Red, red dimmed, then blue; and the band, placed in the reload's
    // frame.
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    let sent = sent_ids(&out);
    assert_eq!(sorted(sent.clone()).len(), 3, "{sent:?}");
    let bands = band_ids(&out);
    assert_eq!(sorted(bands.clone()).len(), 1, "{bands:?}");
    assert_eq!(reloaded.matches("\x1b_Ga=p,").count(), 1, "{reloaded:?}");
    assert!(!came.contains("\x1b_Ga=p,"), "{came:?}");
    // The band went with the last placeholder, and the rest on quitting.
    let all = sorted(sent.into_iter().chain(bands).collect());
    assert_eq!(sorted(deleted_ids(&out)), all, "{out:?}");
}

#[test]
fn the_band_moves_on_at_each_frame_while_an_image_loads() {
    let mut reload = red_then(filled(20, 20, [0, 0, 255, 255]));
    let red = filled(20, 20, [255, 0, 0, 255]);
    let terminal = loading_in(
        12,
        &around(2, "a.png"),
        vec![("a.png", red)],
        &mut reload,
        &[b"r", b"", b"", b"", b"", b""],
        &[5],
        config::Follow::Off,
    );
    // Waits a frame's time while the image loads, and a poll's after.
    assert_eq!(terminal.waits[1..6], [FRAME; 5], "{:?}", terminal.waits);
    assert_eq!(terminal.waits[6], POLL, "{:?}", terminal.waits);
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    // Sent with the frame that shows it, then with a frame drawn anew
    // after each of the four reads before the image came, as it is
    // then.
    let bands = band_ids(&out);
    assert_eq!(bands.len(), 5, "{out:?}");
    assert_eq!(sorted(bands).len(), 1, "{out:?}");
    let frames = terminal.frames();
    assert_eq!(frames.len(), 8, "{frames:#?}");
}

#[test]
fn a_new_image_without_an_old_one_shows_its_alt_text_with_the_band_below_it() {
    let mut reload = Background {
        document: around(2, "b.png"),
        wanted: vec![(ImageKey::from("b.png"), filled(20, 20, [0, 0, 255, 255]))],
        aside: Vec::new(),
        coming: Default::default(),
        fails: Vec::new(),
        afterwards: None,
        asked: 0,
    };
    let terminal = loading_in(
        12,
        &numbers(2),
        Vec::new(),
        &mut reload,
        &[b"r"],
        &[],
        config::Follow::Off,
    );
    let frames = terminal.frames();
    let reloaded = unstyled(&frames[2]);
    assert!(reloaded.contains("an image"), "{reloaded:?}");
    // Placed on the blank line below the alt text, two cells in.
    let (_, below) = reloaded.split_once("an image").unwrap();
    let (_, blank) = below.split_once("\x1b[6;1H").unwrap();
    let (blank, _) = blank.split_once("\x1b[7;1H").unwrap();
    assert!(blank.contains("\x1b7\x1b[2C\x1b_Ga=p,"), "{reloaded:?}");
    assert!(!reloaded.contains(crate::kitty::graphics::PLACEHOLDER));
}

#[test]
fn an_image_that_comes_keeps_the_top_line_in_place() {
    // Ten rows for one: below the top line, and above it.
    let short = crate::parse::parse("Line 1\n\nLine 2\n\n![an image](b.png)\n\nAfter\n");
    for (document, rows, keys, top) in [
        // At the end, which the view would follow down otherwise.
        (short, 6, b"G".as_slice(), "Line 2"),
        (around(2, "b.png"), 4, b"jjjjjj", "After 1"),
    ] {
        let mut reload = red_then(filled(20, 200, [0, 0, 255, 255]));
        reload.document = document;
        let red = filled(20, 20, [255, 0, 0, 255]);
        let terminal = loading_in(
            rows,
            &around(2, "a.png"),
            vec![("a.png", red)],
            &mut reload,
            &[keys, b"r", b"", b""],
            &[3],
            config::Follow::Off,
        );
        let frames = terminal.frames();
        let (before, after) = (&frames[frames.len() - 2], frames.last().unwrap());
        assert!(
            status_text(before, rows).contains("rendering"),
            "{frames:#?}"
        );
        assert!(
            !status_text(after, rows).contains("rendering"),
            "{frames:#?}"
        );
        assert_eq!(top_row(before), top, "{frames:#?}");
        assert_eq!(top_row(after), top, "{frames:#?}");
    }
}

#[test]
fn auto_following_jumps_when_the_text_comes_and_not_when_the_image_does() {
    let before = numbers(30);
    let mut changed: String = (1..=30).map(|n| format!("Line {n}\n\n")).collect();
    changed = changed.replace("Line 20\n", "![changed](b.png)\n");
    let mut reload = Background {
        document: crate::parse::parse(&changed),
        wanted: vec![(ImageKey::from("b.png"), filled(20, 200, [0, 0, 255, 255]))],
        aside: Vec::new(),
        coming: Default::default(),
        fails: Vec::new(),
        afterwards: None,
        asked: 0,
    };
    let terminal = loading_in(
        8,
        &before,
        Vec::new(),
        &mut reload,
        &[b"r", b"", b""],
        &[2],
        config::Follow::Auto,
    );
    let frames = terminal.frames();
    let top = |frame: &str| top_row(frame);
    let jumped = top(&frames[2]);
    assert_ne!(jumped, top(&frames[0]), "{frames:#?}");
    assert_eq!(top(frames.last().unwrap()), jumped, "{frames:#?}");
}

#[test]
fn the_status_line_counts_the_images_that_came_until_all_have() {
    let mut reload = Background {
        document: crate::parse::parse("![a](a.png)\n\n![b](b.png)\n"),
        wanted: vec![
            (ImageKey::from("a.png"), filled(20, 20, [0, 0, 255, 255])),
            (ImageKey::from("b.png"), filled(20, 20, [0, 255, 0, 255])),
        ],
        aside: Vec::new(),
        coming: Default::default(),
        fails: Vec::new(),
        afterwards: None,
        asked: 0,
    };
    let terminal = loading_in(
        12,
        &numbers(2),
        Vec::new(),
        &mut reload,
        &[b"r", b"", b"", b""],
        &[2, 3],
        config::Follow::Off,
    );
    let statuses: Vec<String> = (terminal.frames().iter())
        .map(|frame| status_text(frame, 12))
        .collect();
    // After the row's cursor move and kiri's name.
    let named = |status: &String| status.split_whitespace().nth(2).unwrap_or("").to_owned();
    let mut names: Vec<String> = statuses.iter().map(named).collect();
    // Frames drawn anew as the band moves say the same.
    names.dedup();
    assert_eq!(
        names,
        ["notes.md", "[rendering:", "notes.md"],
        "{statuses:#?}"
    );
    let at = |count: &str| statuses.iter().position(|status| status.contains(count));
    let (none, one) = (at("[rendering: 0/2]"), at("[rendering: 1/2]"));
    assert!(
        none.is_some() && one.is_some() && none < one,
        "{statuses:#?}"
    );
}

#[test]
fn the_tint_stays_on_its_lines_when_an_image_above_comes_taller() {
    let source = |changed: bool| {
        let mut source = String::from("Line 1\n\n![a](a.png)\n\n");
        for n in 2..=30 {
            match n == 25 && changed {
                true => source.push_str("Line 25 changed\n\n"),
                false => source.push_str(&format!("Line {n}\n\n")),
            }
        }
        crate::parse::parse(&source)
    };
    // The file of the image changed too, and loads again, taller.
    let mut reload = Background {
        document: source(true),
        wanted: vec![(ImageKey::from("a.png"), filled(20, 100, [0, 0, 255, 255]))],
        aside: vec![ImageKey::from("a.png")],
        coming: Default::default(),
        fails: Vec::new(),
        afterwards: None,
        asked: 0,
    };
    let mut caps = known_colours();
    caps.graphics = true;
    caps.max_image_cells = crate::kitty::graphics::MAX_CELLS;
    let terminal = loading_with(
        caps,
        21,
        &source(false),
        vec![("a.png", filled(20, 20, [255, 0, 0, 255]))],
        &mut reload,
        &[b"r", b""],
        &[1],
        config::Follow::Auto,
    );
    let frames = terminal.frames();
    let tint = |frame: &str, text: &str| background(&row_style(frame, text));
    assert_eq!(frames.len(), 4, "{frames:#?}");
    for frame in &frames[2..] {
        assert_eq!(tint(frame, "Line 25 changed"), Some("30;30;30".to_owned()));
        assert_eq!(tint(frame, "Line 24"), None);
    }
    // The image came, above the view, which kept its place.
    assert!(status_text(&frames[2], 21).contains("[rendering: 0/1]"));
    assert!(status_text(&frames[3], 21).contains("notes.md"));
    let row = |frame: &str| row_of(frame, 21, "Line 25 changed");
    assert_eq!(row(&frames[3]), row(&frames[2]));
}

#[test]
fn quitting_stops_the_loads_before_it_deletes_the_images() {
    let mut reload = red_then(filled(20, 20, [0, 0, 255, 255]));
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs
        .images
        .insert("a.png", Ok(filled(20, 20, [255, 0, 0, 255])));
    let mut terminal = Fake::new(60, 12, &[b"r", b"", b"q"]);
    // Whether the load was stopped at each flush, once there is one.
    let stopped: std::rc::Rc<std::cell::RefCell<Vec<bool>>> = Default::default();
    let (coming, seen) = (
        std::rc::Rc::clone(&reload.coming),
        std::rc::Rc::clone(&stopped),
    );
    terminal.flushed = Some(Box::new(move || {
        if let Some((arrival, _)) = coming.borrow().first() {
            seen.borrow_mut().push(arrival.stopped());
        }
    }));
    let options = Options {
        reload: Some(&mut reload),
        ..Options::new(SearchCase::Smart)
    };
    run_document(&around(2, "a.png"), &mut inputs, options, &mut terminal);
    let stopped = stopped.borrow();
    // Running while it is shown; stopped by the flush that deletes the
    // images on quitting.
    let (last, before) = stopped.split_last().unwrap();
    assert!(*last, "{stopped:?}");
    assert!(before.iter().all(|stopped| !stopped), "{stopped:?}");
    let out = String::from_utf8(terminal.out).unwrap();
    assert!(until_quit(&out).1.contains("\x1b_Ga=d,"), "{out:?}");
}

#[test]
fn a_reload_that_fails_while_images_load_keeps_their_placeholders() {
    let mut reload = red_then(filled(20, 20, [0, 0, 255, 255]));
    reload.afterwards = Some("gone".to_owned());
    let red = filled(20, 20, [255, 0, 0, 255]);
    let terminal = loading_in(
        12,
        &around(2, "a.png"),
        vec![("a.png", red)],
        &mut reload,
        &[b"r", b"r", b""],
        &[],
        config::Follow::Off,
    );
    let frames = terminal.frames();
    // The first, reloading, reloaded, reloading, failed, and it drawn
    // anew as the band moves.
    assert_eq!(frames.len(), 6, "{frames:#?}");
    let placeholders = |frame: &str| frame.matches(crate::kitty::graphics::PLACEHOLDER).count();
    // The dimmed image, and the band below it.
    for frame in [&frames[2], &frames[4]] {
        assert_eq!(placeholders(frame), 2, "{frames:#?}");
        assert_eq!(frame.matches("\x1b_Ga=p,").count(), 1, "{frames:#?}");
    }
    let status = status_text(&frames[4], 12);
    assert!(
        status.contains("[rendering: 0/1]") && status.contains("gone"),
        "{status:?}"
    );
}

#[test]
fn an_image_that_fails_to_load_shows_as_a_missing_image_does() {
    let mut reload = red_then(filled(20, 20, [0, 0, 255, 255]));
    reload.fails = vec![ImageKey::from("b.png")];
    let red = filled(20, 20, [255, 0, 0, 255]);
    let terminal = loading_in(
        12,
        &around(2, "a.png"),
        vec![("a.png", red)],
        &mut reload,
        &[b"r", b"", b""],
        &[2],
        config::Follow::Off,
    );
    let frames = terminal.frames();
    let last = frames.last().unwrap();
    assert!(unstyled(last).contains("an image"), "{last:?}");
    assert!(
        !last.contains(crate::kitty::graphics::PLACEHOLDER),
        "{last:?}"
    );
    assert!(status_text(last, 12).contains("notes.md"), "{last:?}");
    // The same as a document whose image is missing shows at once.
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    inputs
        .images
        .insert("b.png", Err(ImageError::Missing("gone.png".into())));
    let mut fresh = Fake::new(60, 12, &[b"q"]);
    let options = Options::new(SearchCase::Smart);
    run_document(&around(2, "b.png"), &mut inputs, options, &mut fresh);
    // From the clearing of the screen, after the placeholder and the
    // band were deleted, to the status line.
    let shown = |frame: &str| {
        let frame = unstyled(frame);
        let start = frame.find("\x1b[H").unwrap();
        frame[start..]
            .split("\x1b[12;1H")
            .next()
            .unwrap()
            .to_owned()
    };
    assert_eq!(shown(last), shown(&fresh.frames()[0]));
}

#[test]
fn a_band_scrolled_out_of_view_does_not_move_on() {
    let mut reload = red_then(filled(20, 20, [0, 0, 255, 255]));
    let red = filled(20, 20, [255, 0, 0, 255]);
    let terminal = loading_in(
        4,
        &around(2, "a.png"),
        vec![("a.png", red)],
        &mut reload,
        // The band on the second row, then below the view, past it.
        &[b"jjj", b"r", b"", b"jjjj", b"", b""],
        &[],
        config::Follow::Off,
    );
    let waits = &terminal.waits;
    assert_eq!(waits[2], FRAME, "{waits:?}");
    assert_eq!(waits[4..6], [POLL, POLL], "{waits:?}");
    // Placed in the frame that showed it and the one after the read
    // below; not once it is out of view.
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    let (_, after) = out.rsplit_once("\x1b[?2026h").unwrap();
    assert_eq!(band_ids(&out).len(), 2, "{out:?}");
    assert!(band_ids(after).is_empty(), "{out:?}");
    assert!(!after.contains("\x1b_Ga=p,"), "{out:?}");
}

#[test]
fn quitting_leaves_the_programs_ending_to_the_caller() {
    let mut loads = Loads::new();
    loads.program(ImageKey::block("mermaid", "slow\n"), Duration::from_secs(2));
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    let mut terminal = Fake::new(60, 6, &[b"q"]);
    let options = Options {
        loads: Some(&mut loads),
        ..Options::new(SearchCase::Smart)
    };
    let start = Instant::now();
    run_document(&numbers(3), &mut inputs, options, &mut terminal);
    // Back before the program ends, with the loads stopped.
    assert!(
        start.elapsed() < Duration::from_millis(300),
        "{:?}",
        start.elapsed()
    );
    assert!(loads.is_empty());
    assert!(!loads.wait(Duration::ZERO));
    assert!(loads.wait(Duration::from_secs(5)));
}

/// Reloads the document as the first, with a program drawing a
/// diagram that ends the second after it is told to stop.
struct Drawing(Document, Duration);

impl Reload for Drawing {
    fn reload(&mut self, _: &mut Images, loads: &mut Loads, _: &Capabilities, _: bool) -> Reloaded {
        loads.program(ImageKey::block("mermaid", "slow\n"), self.1);
        Reloaded::Document(self.0.clone())
    }

    fn change(&mut self) -> Change {
        Change::Unchanged
    }
}

#[test]
fn quitting_with_loads_of_its_own_waits_for_them_after_deleting_the_images() {
    let ending = Duration::from_secs(1);
    let (document, image) = pictured([255, 0, 0, 255]);
    let mut drawing = Drawing(document.clone(), ending);
    let mut terminal = Fake::new(60, 6, &[b"r", b"q"]);
    // When each flush came.
    let flushes: std::rc::Rc<std::cell::RefCell<Vec<Instant>>> = Default::default();
    let noted = std::rc::Rc::clone(&flushes);
    terminal.flushed = Some(Box::new(move || noted.borrow_mut().push(Instant::now())));
    let options = Options {
        reload: Some(&mut drawing),
        ..Options::new(SearchCase::Smart)
    };
    let mut inputs = image;
    let start = Instant::now();
    run_document(&document, &mut inputs, options, &mut terminal);
    let end = Instant::now();
    // The program ended before `run` returned, and the images were
    // deleted before it did.
    assert!(end - start >= ending, "{:?}", end - start);
    let deleted = *flushes.borrow().last().unwrap();
    assert!(end - deleted >= ending / 2, "{:?}", end - deleted);
    let out = String::from_utf8(terminal.out).unwrap();
    assert!(until_quit(&out).1.contains("\x1b_Ga=d,"), "{out:?}");
}

#[test]
fn an_image_that_comes_at_its_placeholder_s_size_moves_nothing() {
    // As large as the red one that stands in for it.
    let mut reload = red_then(filled(20, 20, [0, 0, 255, 255]));
    let red = filled(20, 20, [255, 0, 0, 255]);
    let terminal = loading_in(
        12,
        &around(2, "a.png"),
        vec![("a.png", red)],
        &mut reload,
        &[b"r", b"", b""],
        &[2],
        config::Follow::Off,
    );
    let frames = terminal.frames();
    let (before, after) = (&frames[frames.len() - 2], frames.last().unwrap());
    assert!(status_text(before, 12).contains("rendering"), "{frames:#?}");
    assert!(!status_text(after, 12).contains("rendering"), "{frames:#?}");
    for text in ["After 1", "After 2", "After 3"] {
        assert_eq!(
            row_of(after, 12, text),
            row_of(before, 12, text),
            "{text}: {frames:#?}"
        );
        assert!(row_of(after, 12, text).is_some(), "{text}");
    }
}

#[test]
fn no_band_is_placed_under_the_help_panel() {
    let mut reload = red_then(filled(20, 20, [0, 0, 255, 255]));
    let red = filled(20, 20, [255, 0, 0, 255]);
    let terminal = loading_in(
        12,
        &around(2, "a.png"),
        vec![("a.png", red)],
        &mut reload,
        &[b"r", b"", b"?", b"", b""],
        &[],
        config::Follow::Off,
    );
    let frames = terminal.frames();
    let help = |frame: &str| frame.contains("down a line");
    // The band is placed before the help opens, and not under it.
    assert!(frames[2].contains("\x1b_Ga=p,"), "{frames:#?}");
    let last = frames.last().unwrap();
    assert!(help(last), "{frames:#?}");
    assert!(!last.contains("\x1b_Ga=p,"), "{last:?}");
    // Nothing moves on under the panel, so the pager waits a poll.
    let waits = &terminal.waits;
    assert_eq!(waits[waits.len() - 2..], [POLL, POLL], "{waits:?}");
}

/// Reloads the document as the first, with the Mermaid diagrams of
/// the second loading with the third, one at a time.
struct Gating(Document, Vec<&'static str>, crate::images::Gated);

impl Reload for Gating {
    fn reload(&mut self, _: &mut Images, loads: &mut Loads, _: &Capabilities, _: bool) -> Reloaded {
        loads.gated(&self.1, &self.2);
        Reloaded::Document(self.0.clone())
    }

    fn change(&mut self) -> Change {
        Change::Unchanged
    }
}

/// `numbers(40)` with the diagram `x` after line 1, `up` after line
/// 20 and `down` after line 37.
fn diagrammed() -> Document {
    let source: String = (1..=40)
        .map(|n| match n {
            1 => "x",
            20 => "up",
            37 => "down",
            _ => "",
        })
        .enumerate()
        .map(|(at, diagram)| match diagram {
            "" => format!("Line {}\n\n", at + 1),
            _ => format!("Line {}\n\n```mermaid\n{diagram}\n```\n\n", at + 1),
        })
        .collect();
    crate::parse::parse(&source)
}

/// The diagrams of [`diagrammed`] in the order they start when the
/// pager shows `numbers(40)` in a window of 10 rows, with `reads`,
/// and reloads it as `diagrammed()`. The read numbered `first` waits
/// for the first diagram to start, and the one numbered `open` lets
/// them all draw and waits for them.
fn started_in(reads: &[&[u8]], first: usize, open: usize) -> Vec<String> {
    let gated = crate::images::Gated::default();
    let mut gating = Gating(diagrammed(), vec!["x", "up", "down"], gated.clone());
    let mut terminal = Fake::new(60, 10, reads);
    let hooked = gated.clone();
    terminal.hook = Some(Box::new(move |read| {
        if read == first {
            hooked.until(1);
        } else if read == open {
            hooked.open.store(true, std::sync::atomic::Ordering::SeqCst);
            hooked.until(3);
        }
    }));
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    run_document(
        &numbers(40),
        &mut inputs,
        Options {
            reload: Some(&mut gating),
            ..Options::new(SearchCase::Smart)
        },
        &mut terminal,
    );
    gated.started()
}

#[test]
fn a_reload_loads_the_diagram_in_view_first() {
    // At the end, where `down` is after the reload.
    assert_eq!(started_in(&[b"G", b"r", b""], 2, 3), ["down", "up", "x"]);
}

#[test]
fn scrolling_loads_a_diagram_further_down_before_one_above_it() {
    // `x` starts first, in view at the top, and `down` comes into view.
    let started = started_in(&[b"r", b"", b"G", b""], 1, 3);
    assert_eq!(started, ["x", "down", "up"]);
}

#[test]
fn a_heading_image_comes_after_its_text_and_keeps_the_top_line() {
    let caps = Capabilities {
        text_sizing: false,
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(80)
    };
    let image = image::RgbaImage::from_pixel(100, 40, image::Rgba([200, 30, 30, 255]));
    let text = crate::images::Source::Text {
        image,
        breaks: Vec::new(),
        ends: vec![10],
    };
    let document = |heading: &str| {
        let mut source = format!("# {heading}\n\n");
        source.extend((1..=12).map(|n| format!("Line {n}\n\n")));
        crate::parse::parse(&source)
    };
    for (keys, top) in [(b"".as_slice(), "Kiri"), (b"jjjj", "Line 2")] {
        let mut reload = Background {
            document: document("Kiri"),
            wanted: vec![(
                crate::images::ImageKey::heading(1, "Kiri", (10, 20)),
                text.clone(),
            )],
            aside: Vec::new(),
            coming: Default::default(),
            fails: Vec::new(),
            afterwards: None,
            asked: 0,
        };
        let terminal = loading_with(
            caps.clone(),
            6,
            &document("Old"),
            Vec::new(),
            &mut reload,
            &[keys, b"r", b"", b""],
            &[3],
            config::Follow::Off,
        );
        let frames = terminal.frames();
        let (before, after) = (&frames[frames.len() - 2], frames.last().unwrap());
        // First as text, then as the image.
        assert!(!before.contains('\u{10EEEE}'), "{frames:#?}");
        assert_eq!(top_row(before), top, "{frames:#?}");
        if top == "Kiri" {
            assert!(after.contains('\u{10EEEE}'), "{frames:#?}");
        } else {
            assert_eq!(top_row(after), top, "{frames:#?}");
        }
    }
}
