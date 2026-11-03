use super::*;

/// The images a test has loading: the keys the planner wants once the
/// document was read, what each comes as, and the loads the runner has
/// started, which the test brings to come.
#[derive(Clone, Default)]
struct Coming {
    planned: std::rc::Rc<std::cell::RefCell<Vec<ImageKey>>>,
    outcomes: std::rc::Rc<
        std::cell::RefCell<
            std::collections::HashMap<ImageKey, Result<crate::images::Source, ImageError>>,
        >,
    >,
    started: crate::images::Expecting,
}

impl Coming {
    /// Brings the first load that has started: it comes as what its key
    /// was to come as.
    fn bring(&self) {
        let (key, arrival) = self.started.take();
        arrival.come(self.outcomes.borrow_mut().remove(&key).expect("an outcome"));
    }

    /// The pager's loads, which this runs, and its planner, which wants
    /// the keys planned.
    fn options<'a>(&self, options: Options<'a>) -> (Options<'a>, Loads) {
        let planned = std::rc::Rc::clone(&self.planned);
        let planner: crate::pager::Planner = Box::new(move |_, _, _| {
            (planned.borrow().iter())
                .map(|key| crate::images::Job::named(key.clone()))
                .collect()
        });
        let options = Options {
            planner: Some(planner),
            ..options
        };
        (options, Loads::with(Box::new(self.started.clone())))
    }
}

/// [`run_document`] with the loads of `coming`.
fn run_coming(
    document: &Document,
    inputs: &mut Inputs,
    options: Options<'_>,
    coming: &Coming,
    terminal: &mut Fake,
) {
    let (options, mut loads) = coming.options(options);
    run_document_in(document, inputs, options, &mut loads, terminal);
    loads.halt();
}

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

impl SourceFiles for Background {
    fn reread(
        &mut self,
        _: &Reread,
        images: &mut Images,
        _: &mut Loads,
        _: &Capabilities,
        _: bool,
    ) -> (Option<Look>, Reloaded) {
        self.asked += 1;
        if let Some(why) = self.afterwards.clone().filter(|_| self.asked > 1) {
            return (Some(READ), Reloaded::Failed(why));
        }
        for key in &self.aside {
            images.set_aside(key);
        }
        for (key, image) in self.wanted.drain(..) {
            let image = match self.fails.contains(&key) {
                true => Err(ImageError::Missing("gone.png".into())),
                false => Ok(image),
            };
            self.coming.planned.borrow_mut().push(key.clone());
            self.coming.outcomes.borrow_mut().insert(key, image);
        }
        (Some(READ), Reloaded::Document(self.document.clone()))
    }

    fn look(&mut self, _: &Path) -> Result<Look, String> {
        Ok(READ)
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
    follow: settings::Follow,
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
    follow: settings::Follow,
) -> Fake {
    let mut inputs = Inputs::new(caps);
    for (url, image) in images {
        inputs.images.insert(url, Ok(image));
    }
    let mut terminal = Fake::new(60, rows, reads);
    let coming = reload.coming.clone();
    let comes = comes.to_vec();
    let bringing = coming.clone();
    terminal.hook = Some(Box::new(move |read| {
        if comes.contains(&read) {
            bringing.bring();
        }
    }));
    let options = Options {
        files: Some(reload),
        follow,
        ..Options::new(SearchCase::Smart)
    };
    run_coming(before, &mut inputs, options, &coming, &mut terminal);
    terminal
}

/// A document with `above` lines above the image `url` and three
/// below.
fn around(above: usize, url: &str) -> Document {
    let mut source: String = (1..=above).map(|n| format!("Line {n}\n\n")).collect();
    source.push_str(&format!(
        "![an image]({url})\n\nAfter 1\n\nAfter 2\n\nAfter 3\n"
    ));
    crate::typeset::parse::parse(&source)
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
        settings::Follow::Off,
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
        settings::Follow::Off,
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
        settings::Follow::Off,
    );
    let frames = terminal.frames();
    let reloaded = unstyled(&frames[2]);
    assert!(reloaded.contains("an image"), "{reloaded:?}");
    // Placed on the blank line below the alt text, two cells in.
    let (_, below) = reloaded.split_once("an image").unwrap();
    let (_, blank) = below.split_once("\x1b[6;1H").unwrap();
    let (blank, _) = blank.split_once("\x1b[7;1H").unwrap();
    assert!(blank.contains("\x1b7\x1b[2C\x1b_Ga=p,"), "{reloaded:?}");
    assert!(!reloaded.contains(crate::present::kitty::graphics::PLACEHOLDER));
}

#[test]
fn an_image_that_comes_keeps_the_top_line_in_place() {
    // Ten rows for one: below the top line, and above it.
    let short =
        crate::typeset::parse::parse("Line 1\n\nLine 2\n\nLine 3\n\n![an image](b.png)\n\nAfter\n");
    for (document, rows, keys, top) in [
        // At the end, which the view would follow down otherwise.
        (short, 8, b"G".as_slice(), "Line 3"),
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
            settings::Follow::Off,
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
        document: crate::typeset::parse::parse(&changed),
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
        settings::Follow::Auto,
    );
    let frames = terminal.frames();
    let top = |frame: &str| top_row(frame);
    let jumped = top(&frames[2]);
    assert_ne!(jumped, top(&frames[0]), "{frames:#?}");
    assert_eq!(top(frames.last().unwrap()), jumped, "{frames:#?}");
}

#[test]
fn auto_following_reads_once_when_the_file_never_changes() {
    let before = numbers(30);
    let mut changed: String = (1..=30).map(|n| format!("Line {n}\n\n")).collect();
    changed = changed.replace("Line 20\n", "![changed](b.png)\n");
    let mut reload = Background {
        document: crate::typeset::parse::parse(&changed),
        wanted: vec![(ImageKey::from("b.png"), filled(20, 200, [0, 0, 255, 255]))],
        aside: Vec::new(),
        coming: Default::default(),
        fails: Vec::new(),
        afterwards: None,
        asked: 0,
    };
    let keys: [&[u8]; 8] = [b"r", b"", b"", b"", b"", b"", b"", b""];
    loading_in(
        8,
        &before,
        Vec::new(),
        &mut reload,
        &keys,
        &[2],
        settings::Follow::Auto,
    );
    assert_eq!(reload.asked, 1);
}

#[test]
fn the_status_line_counts_the_images_that_came_until_all_have() {
    let mut reload = Background {
        document: crate::typeset::parse::parse("![a](a.png)\n\n![b](b.png)\n"),
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
        settings::Follow::Off,
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
        crate::typeset::parse::parse(&source)
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
    caps.max_image_cells = crate::present::kitty::graphics::MAX_CELLS;
    let terminal = loading_with(
        caps,
        21,
        &source(false),
        vec![("a.png", filled(20, 20, [255, 0, 0, 255]))],
        &mut reload,
        &[b"r", b""],
        &[1],
        settings::Follow::Auto,
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
    let coming = reload.coming.clone();
    let seen = std::rc::Rc::clone(&stopped);
    let started = coming.started.clone();
    terminal.flushed = Some(Box::new(move || {
        if let Some((_, arrival)) = started.0.borrow().first() {
            seen.borrow_mut().push(arrival.stopped());
        }
    }));
    let options = Options {
        files: Some(&mut reload),
        ..Options::new(SearchCase::Smart)
    };
    let document = around(2, "a.png");
    run_coming(&document, &mut inputs, options, &coming, &mut terminal);
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
        settings::Follow::Off,
    );
    let frames = terminal.frames();
    // The first, reloading, reloaded, reloading, failed, and it drawn
    // anew as the band moves.
    assert_eq!(frames.len(), 6, "{frames:#?}");
    let placeholders = |frame: &str| {
        frame
            .matches(crate::present::kitty::graphics::PLACEHOLDER)
            .count()
    };
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
        settings::Follow::Off,
    );
    let frames = terminal.frames();
    let last = frames.last().unwrap();
    assert!(unstyled(last).contains("an image"), "{last:?}");
    assert!(
        !last.contains(crate::present::kitty::graphics::PLACEHOLDER),
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
        settings::Follow::Off,
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
    loads.program(
        ImageKey::rendered("mermaid", "slow\n"),
        Duration::from_secs(2),
    );
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    let mut terminal = Fake::new(60, 6, &[b"q"]);
    let start = Instant::now();
    // The program is wanted, so that it runs until the reader quits.
    let key = ImageKey::rendered("mermaid", "slow\n");
    let options = Options {
        planner: Some(Box::new(move |_, _, _| {
            vec![crate::images::Job::named(key.clone())]
        })),
        ..Options::new(SearchCase::Smart)
    };
    run_document_in(&numbers(3), &mut inputs, options, &mut loads, &mut terminal);
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
        settings::Follow::Off,
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
        settings::Follow::Off,
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

/// Reloads the document as the first, and the Mermaid diagrams of the
/// second, which the planner wants once it was, load with the third, one
/// at a time.
struct Gating(Document, std::rc::Rc<std::cell::Cell<bool>>);

impl SourceFiles for Gating {
    fn reread(
        &mut self,
        _: &Reread,
        _: &mut Images,
        _: &mut Loads,
        _: &Capabilities,
        _: bool,
    ) -> (Option<Look>, Reloaded) {
        self.1.set(true);
        (Some(READ), Reloaded::Document(self.0.clone()))
    }

    fn look(&mut self, _: &Path) -> Result<Look, String> {
        Ok(READ)
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
    crate::typeset::parse::parse(&source)
}

/// The diagrams of [`diagrammed`] in the order they start when the
/// pager shows `numbers(40)` in a window of 10 rows, with `reads`,
/// and reloads it as `diagrammed()`. The read numbered `first` waits
/// for the first diagram to start, and the one numbered `open` lets
/// them all draw and waits for them.
fn started_in(reads: &[&[u8]], first: usize, open: usize) -> Vec<String> {
    let gated = crate::images::Gated::default();
    let reloaded: std::rc::Rc<std::cell::Cell<bool>> = Default::default();
    let mut gating = Gating(diagrammed(), std::rc::Rc::clone(&reloaded));
    let mut terminal = Fake::new(60, 10, reads);
    let hooked = gated.clone();
    terminal.hook = Some(Box::new(move |read| {
        if read == first {
            // The jobs are told their order before the frame is drawn, not
            // only at the first poll after the wait for keys.
            let started = hooked.until(1);
            assert!(!started.is_empty(), "no load started by read {first}");
        } else if read == open {
            hooked.open.store(true, std::sync::atomic::Ordering::SeqCst);
            hooked.until(3);
        }
    }));
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    let planning = gated.clone();
    let planner: crate::pager::Planner = Box::new(move |_, caps, _| {
        let look = crate::typeset::images::diagram_look_for(
            &crate::typeset::theme::Theme::default(),
            caps,
            true,
        );
        match reloaded.get() {
            true => planning.jobs(&["x", "up", "down"], look),
            false => Vec::new(),
        }
    });
    run_document(
        &numbers(40),
        &mut inputs,
        Options {
            files: Some(&mut gating),
            planner: Some(planner),
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
        crate::typeset::parse::parse(&source)
    };
    for (keys, top) in [(b"".as_slice(), "Kiri"), (b"jjjj", "Line 2")] {
        let mut reload = Background {
            document: document("Kiri"),
            wanted: vec![(
                Inputs::new(caps.clone()).drawn(crate::images::ImageKey::heading(
                    1,
                    "Kiri",
                    (10, 20),
                )),
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
            settings::Follow::Off,
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

#[test]
fn a_changed_diagram_loads_with_its_old_drawing_dimmed_under_the_layouts_look() {
    let caps = Capabilities::kitty(60);
    let key = |code: &str| Inputs::new(caps.clone()).drawn(ImageKey::rendered("mermaid", code));
    let diagram = |code: &str| format!("Before\n\n```mermaid\n{code}```\n\nAfter\n");
    let (old, new) = (key("flowchart TD\n"), key("flowchart LR\n"));
    let mut inputs = Inputs::new(caps.clone());
    inputs
        .images
        .insert(old.clone(), Ok(filled(40, 20, [255, 0, 0, 255])));
    let mut reload = Background {
        document: crate::typeset::parse::parse(&diagram("flowchart LR\n")),
        wanted: vec![(new.clone(), filled(40, 20, [0, 0, 255, 255]))],
        aside: vec![old],
        coming: Default::default(),
        fails: Vec::new(),
        afterwards: None,
        asked: 0,
    };
    let mut terminal = Fake::new(60, 12, &[b"r", b"", b"q"]);
    let coming = reload.coming.clone();
    let options = Options {
        files: Some(&mut reload),
        ..Options::new(SearchCase::Smart)
    };
    let before = crate::typeset::parse::parse(&diagram("flowchart TD\n"));
    run_coming(&before, &mut inputs, options, &coming, &mut terminal);
    let shown = inputs.images.placeholder(&new);
    assert!(
        matches!(shown, Some(crate::images::Placeholder::Dimmed(_))),
        "{shown:?}"
    );
}

/// Reloads the document as the first, as a reload that plans the diagram
/// `key` does: a diagram that failed is left out of what is kept, so the
/// planner's key is missing and loads anew; the planner wants the key once
/// the document was read. Each load comes as a failure.
struct Retrying {
    document: Document,
    key: ImageKey,
    coming: Coming,
}

impl SourceFiles for Retrying {
    fn reread(
        &mut self,
        _: &Reread,
        images: &mut Images,
        _: &mut Loads,
        _: &Capabilities,
        _: bool,
    ) -> (Option<Look>, Reloaded) {
        if matches!(images.get(&self.key), Some(Err(_))) {
            images.set_aside(&self.key);
        }
        let mut planned = self.coming.planned.borrow_mut();
        if planned.is_empty() {
            planned.push(self.key.clone());
        }
        let failed = Err(ImageError::Missing("x.mmd".into()));
        self.coming
            .outcomes
            .borrow_mut()
            .insert(self.key.clone(), failed);
        (Some(READ), Reloaded::Document(self.document.clone()))
    }

    fn look(&mut self, _: &Path) -> Result<Look, String> {
        Ok(READ)
    }
}

#[test]
fn a_diagram_that_failed_in_the_background_loads_again_only_when_the_document_is_read_again() {
    let source = "```mermaid\nx\n```\n\nAfter\n";
    let document = crate::typeset::parse::parse(source);
    let mut reload = Retrying {
        document: document.clone(),
        key: ImageKey::rendered("mermaid", "x\n"),
        coming: Default::default(),
    };
    // The first load comes, failed, at read 1; then the reader scrolls,
    // and at read 6 the document is read again.
    let reads: &[&[u8]] = &[b"r", b"", b"G", b"gg", b"j", b"", b"r", b"", b""];
    let mut terminal = Fake::new(60, 10, reads);
    let coming = reload.coming.clone();
    let counted: std::rc::Rc<std::cell::RefCell<Vec<usize>>> = Default::default();
    let (note, taken) = (std::rc::Rc::clone(&counted), std::cell::Cell::new(0));
    let watching = coming.clone();
    terminal.hook = Some(Box::new(move |read| {
        if read == 1 {
            watching.bring();
            taken.set(taken.get() + 1);
        }
        // The loads that started: those brought and those still waiting.
        note.borrow_mut()
            .push(taken.get() + watching.started.0.borrow().len());
    }));
    let mut inputs = Inputs::new(Capabilities::kitty(80));
    let options = Options {
        files: Some(&mut reload),
        ..Options::new(SearchCase::Smart)
    };
    run_coming(&document, &mut inputs, options, &coming, &mut terminal);
    // Run once by the first reload and not again while the document stays
    // as it was read; twice once it is read again.
    let counted = counted.borrow();
    assert_eq!(counted[0], 0, "{counted:?}");
    assert!(counted[1..=6].iter().all(|&runs| runs == 1), "{counted:?}");
    assert!(counted[7..].iter().all(|&runs| runs == 2), "{counted:?}");
}

/// Runs the pager on `document` on a terminal that gives `keys`, wanting
/// the jobs `plan` gives for the caps and whether reading mode is on, on a
/// runner that only notes the loads. What it noted, in the order they
/// started, each with whether it was stopped at the read numbered `last`,
/// which is the one that quits; the images the pager says it shows.
fn noted(
    document: &Document,
    mut terminal: Fake,
    mut inputs: Inputs,
    last: usize,
    plan: impl Fn(&Capabilities, bool) -> Vec<crate::images::Job> + 'static,
) -> (Vec<(ImageKey, bool)>, Vec<ImageKey>) {
    let started = crate::images::Expecting::default();
    let (watching, flags) = (started.clone(), Rc::new(RefCell::new(Vec::new())));
    let seen = Rc::clone(&flags);
    terminal.hook = Some(Box::new(move |read| {
        if read == last {
            let noted = watching.0.borrow();
            *seen.borrow_mut() = (noted.iter())
                .map(|(key, arrival)| (key.clone(), arrival.stopped()))
                .collect();
        }
    }));
    let options = Options {
        reading: Some(super::reading::reading()),
        planner: Some(Box::new(move |_, caps, reading| plan(caps, reading))),
        ..Options::new(SearchCase::Smart)
    };
    let mut loads = Loads::with(Box::new(started));
    run_document_in(document, &mut inputs, options, &mut loads, &mut terminal);
    loads.halt();
    (flags.take(), inputs.images.shown.clone())
}

use std::cell::RefCell;
use std::rc::Rc;

fn named(urls: &[&str]) -> Vec<crate::images::Job> {
    urls.iter()
        .map(|url| crate::images::Job::named(*url))
        .collect()
}

#[test]
fn m_off_stops_the_loads_of_the_pictures_in_text_and_m_on_starts_them_anew() {
    let plan = |_: &Capabilities, reading: bool| match reading {
        true => named(&["text.png", "p.png"]),
        false => named(&["text.png"]),
    };
    let flags = |keys: &[&[u8]]| {
        let terminal = Fake::new(80, 10, keys);
        let inputs = Inputs::new(Capabilities::kitty(80));
        let flags = noted(&numbers(3), terminal, inputs, keys.len(), plan).0;
        (flags.into_iter())
            .map(|(key, stopped)| (key.to_string(), stopped))
            .collect::<Vec<_>>()
    };
    let (p, t) = ("p.png".to_owned(), "text.png".to_owned());
    // Text alone loads its image once and carries on through the modes.
    assert_eq!(
        flags(&[b"M", b"M"]),
        [(t.clone(), false), (p.clone(), true)]
    );
    // On again, the pictures start anew: the old load was stopped.
    assert_eq!(
        flags(&[b"M", b"M", b"M"]),
        [(t, false), (p.clone(), true), (p, false)]
    );
}

#[test]
fn a_new_cell_size_stops_the_loads_of_the_headings_of_the_old_cells() {
    let caps = Capabilities {
        text_sizing: false,
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(60)
    };
    let pixels = |columns: u16, cell: (u16, u16)| WindowSize {
        columns,
        rows: 6,
        width: columns * cell.0,
        height: 6 * cell.1,
    };
    let mut terminal = Fake::new(60, 6, &[b"", b"", b""]);
    terminal.size = pixels(60, (10, 20));
    terminal.sizes = [pixels(60, (10, 20)), pixels(70, (8, 16))].into();
    let plan = |caps: &Capabilities, _: bool| {
        let cell = crate::typeset::images::size::cell_size(caps);
        vec![crate::images::Job::named(ImageKey::heading(
            1, "Kiri", cell,
        ))]
    };
    let flags = noted(&numbers(3), terminal, Inputs::new(caps), 2, plan).0;
    // The heading for the old cells was stopped; that for the new one runs.
    let (old, new) = (
        ImageKey::heading(1, "Kiri", (10, 20)),
        ImageKey::heading(1, "Kiri", (8, 16)),
    );
    assert_eq!(flags, [(old, true), (new, false)]);
}

#[test]
fn the_loads_not_near_the_view_start_in_plan_order_with_headings_and_pictures_among_them() {
    let caps = Capabilities::kitty(80);
    let drawn = |key: ImageKey| Inputs::new(caps.clone()).drawn(key);
    let diagram = |name: &str| drawn(ImageKey::rendered("mermaid", &format!("{name}\n")));
    let heading = ImageKey::heading(1, "Kiri", (10, 20));
    let (x, up, down) = (diagram("x"), diagram("up"), diagram("down"));
    let planned = [
        heading.clone(),
        down.clone(),
        up.clone(),
        ImageKey::from("p.png"),
        x.clone(),
    ];
    let plan = move |_: &Capabilities, _: bool| -> Vec<crate::images::Job> {
        let jobs = planned.iter().cloned();
        jobs.map(crate::images::Job::named).collect()
    };
    let terminal = Fake::new(60, 10, &[b""]);
    let flags = noted(&diagrammed(), terminal, Inputs::new(caps.clone()), 1, plan).0;
    let order: Vec<ImageKey> = flags.into_iter().map(|(key, _)| key).collect();
    // The diagrams by how far they are from the view at the top, which are
    // the images that stand somewhere in the lines; then the rest, as
    // planned: the heading and the picture in the text.
    assert_eq!(order, [x, up, down, heading, ImageKey::from("p.png")]);
}

#[test]
fn the_images_shown_are_those_planned_in_their_order_and_follow_reading_mode() {
    let plan = |_: &Capabilities, reading: bool| match reading {
        true => named(&["a.png", "p.png", "b.png"]),
        false => named(&["a.png", "b.png"]),
    };
    let shown = |keys: &[&[u8]]| {
        let terminal = Fake::new(80, 10, keys);
        let inputs = Inputs::new(Capabilities::kitty(80));
        let shown = noted(&numbers(3), terminal, inputs, keys.len(), plan).1;
        shown
            .into_iter()
            .map(|key| key.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(shown(&[]), ["a.png", "b.png"]);
    // The picture in the text between them while reading mode is on, and
    // not told of once it is off again.
    assert_eq!(shown(&[b"M"]), ["a.png", "p.png", "b.png"]);
    assert_eq!(shown(&[b"M", b"M"]), ["a.png", "b.png"]);
}

#[test]
fn what_the_planner_wants_is_what_the_layout_draws() {
    let caps = Capabilities {
        text_sizing: false,
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(80)
    };
    let env = crate::config::Environment::default();
    let settings = crate::config::Settings {
        headings: crate::render::Headings::Auto,
        images: true,
        ..plain_settings()
    };
    let document =
        crate::typeset::parse::parse("# Kiri\n\n![x](a.png)\n\n```mermaid\nflowchart TD\n```\n");
    // As the frontend's planner asks for it, from the settings and the caps:
    // this mirrors `frontend::loads::loading`, which it does not call.
    let loading = crate::images::Loading {
        env: &env,
        settings: &settings,
        max_file_size: crate::images::DEFAULT_MAX_FILE_SIZE,
        theme: crate::diagrams::Theme::for_page(caps.dark_background),
        formula: crate::typeset::images::formula_colors(&settings.theme, &caps),
        largest: Some(crate::typeset::images::size::largest(&caps)),
        headings: crate::render::heading_look(
            settings.headings,
            !settings.images,
            &settings.theme,
            &caps,
        ),
    };
    let part = crate::images::Part {
        document: &document,
        number: 0,
        base: std::path::Path::new(""),
        by_line: true,
        inline_images: false,
    };
    let keys: Vec<ImageKey> = (crate::images::jobs(&[part], &loading).into_iter())
        .map(|job| job.key)
        .collect();
    let mut inputs = Inputs::new(caps);
    inputs.theme = settings.theme.clone();
    inputs.headings = settings.headings.with_images(settings.images);
    inputs.diagram_panel = settings.diagram_background_panel;
    let mut drawn = vec![
        inputs.drawn(ImageKey::heading(1, "Kiri", (10, 20))),
        inputs.drawn(ImageKey::from("a.png")),
    ];
    // Without the built-in renderer nothing draws the diagram.
    if cfg!(feature = "mermaid") {
        drawn.push(inputs.drawn(ImageKey::rendered("mermaid", "flowchart TD\n")));
    }
    assert_eq!(keys, drawn);
}
