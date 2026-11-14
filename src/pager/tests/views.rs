//! A view opened over the document and the way back to the one below:
//! each view has its own file, images, search and place, and what the one
//! below had is as it was when the view is closed. Nothing opens a view
//! with a key yet, so the tests ask the loop to.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::session::deck::Sheet;

type Rig<'a, 'r, 'l> = Driver<'a, 'r, 'l, Fake>;

/// The loop over `base` with `options`, on `terminal`: `steps` are
/// taken, and then the loop runs on until the reader quits, which an
/// empty terminal does.
fn visit<'a, 'r>(
    base: &'a Document,
    inputs: &'a mut Inputs,
    terminal: &'a mut Fake,
    options: Options<'r>,
    steps: impl FnOnce(&mut Rig<'a, 'r, '_>),
) {
    let mut loads = Loads::inline_builds();
    let session = session_for(&options);
    let content = Content::Document(base);
    let mut rig = Driver::new(
        content, None, session, inputs, options, &mut loads, terminal,
    );
    steps(&mut rig);
    while rig.round().unwrap() {}
    drop(rig);
    loads.halt();
    loads.wait(crate::images::STOPPING);
}

/// Gives `keys` and then a `?`, which ends the layout with the same lines
/// and opens the help; closes the help, and calls `then`.
fn between(rig: &mut Rig<'_, '_, '_>, keys: &[&[u8]], then: impl FnOnce(&mut Rig<'_, '_, '_>)) {
    let reads = keys.iter().map(|keys| keys.to_vec());
    rig.tools.terminal.keys.extend(reads);
    rig.tools.terminal.keys.push_back(b"?".to_vec());
    assert!(rig.round().unwrap(), "the round ended without quitting");
    rig.state.dismiss();
    then(rig);
}

fn open(rig: &mut Rig<'_, '_, '_>, path: &str) -> ViewId {
    let view = rig.open_view(Path::new(path), Input::Markdown);
    view.expect("the file opens")
}

fn base_of(rig: &Rig<'_, '_, '_>) -> ViewId {
    rig.session.base().id()
}

fn plain(then: &mut Then) -> Options<'_> {
    Options {
        files: Some(then),
        ..Options::new(SearchCase::Smart)
    }
}

fn narrow() -> Inputs {
    Inputs::new(Capabilities::ansi(80))
}

#[test]
fn an_opened_view_fills_the_screen_and_the_status_line_names_it() {
    let mut then = Then::new([]).opening([Reloaded::Document(rows(3))]);
    let (base, mut inputs) = (numbers(30), narrow());
    let mut terminal = Fake::new(40, 8, &[]);
    let options = plain(&mut then);
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        between(rig, &[], |rig| {
            let view = open(rig, "docs/other.md");
            assert_eq!(rig.top(), (Sheet::View(view), view));
            let sheets: Vec<Sheet> = rig.state.deck.sheets().collect();
            assert_eq!(sheets, [Sheet::Base, Sheet::View(view)]);
        });
    });
    let frames = terminal.frames();
    let last = frames.last().unwrap();
    assert!(status_row(last, 8).contains("docs/other.md"), "{last:?}");
    let screens = terminal.screens();
    let screen = screen_text(screens.last().unwrap());
    assert!(screen.contains("Row 1"), "{screen}");
    assert!(!screen.contains("Line"), "the base shows through: {screen}");
}

#[test]
fn an_opened_view_reads_again_its_own_file() {
    let again = Reloaded::Document(rows(4));
    let mut then = Then::new([again]).opening([Reloaded::Document(rows(3))]);
    let (base, mut inputs) = (numbers(30), narrow());
    let mut terminal = Fake::new(40, 8, &[]);
    let options = plain(&mut then);
    let mut seen = None;
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        between(rig, &[], |rig| {
            let view = open(rig, "docs/other.md");
            seen = Some(rig.session.sources_of(view)[0].id());
        });
        let before = rig.session.sources()[0].version();
        rig.tools.terminal.keys.push_back(b"r".to_vec());
        assert!(rig.round().unwrap());
        let base = base_of(rig);
        rig.back_to_view(base);
        let after = rig.session.sources()[0].version();
        assert_eq!(after, before, "the base's version did not move");
    });
    assert_eq!(then.jobs.len(), 1, "only the view's file was read");
    let job = &then.jobs[0];
    assert_eq!(job.path, PathBuf::from("docs/other.md"));
    assert_eq!((Some(job.source), job.document), (seen, 1));
}

#[test]
fn an_opened_view_plans_its_images_by_its_own_file() {
    type Calls = Rc<RefCell<Vec<(usize, Vec<Option<PathBuf>>)>>>;
    let calls = Calls::default();
    let record = Rc::clone(&calls);
    let planner: Planner<'_> = Box::new(move |planned, _, _| {
        let paths = planned.paths.iter().map(|path| path.map(Path::to_path_buf));
        record.borrow_mut().push((planned.first, paths.collect()));
        Vec::new()
    });
    let mut then = Then::new([]).opening([Reloaded::Document(rows(3))]);
    let (base, mut inputs) = (numbers(30), narrow());
    let mut terminal = Fake::new(40, 8, &[]);
    let options = Options {
        planner: Some(planner),
        ..plain(&mut then)
    };
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        between(rig, &[], |rig| {
            open(rig, "docs/other.md");
        });
    });
    let calls = calls.borrow();
    let notes = (0, vec![Some(PathBuf::from("notes.md"))]);
    let other = (1, vec![Some(PathBuf::from("docs/other.md"))]);
    assert_eq!(calls.first(), Some(&notes), "{calls:?}");
    assert!(calls.contains(&other), "{calls:?}");
}

#[test]
fn the_base_s_images_keep_their_ids_under_an_opened_view_and_are_not_sent_again() {
    let (base, mut inputs) = pictured([255, 0, 0, 255]);
    let mut then = Then::new([]).opening([Reloaded::Document(rows(3))]);
    let mut terminal = Fake::new(60, 8, &[]);
    let options = plain(&mut then);
    let mut marks = Vec::new();
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        between(rig, &[], |rig| {
            marks.push(rig.tools.terminal.out.len());
            open(rig, "docs/other.md");
        });
        let base = base_of(rig);
        between(rig, &[], |rig| {
            marks.push(rig.tools.terminal.out.len());
            rig.back_to_view(base);
        });
        // The base is drawn again in the next round, which ends on a `?`.
        between(rig, &[], |rig| marks.push(rig.tools.terminal.out.len()));
    });
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    assert_eq!(sent_ids(&out[..marks[0]]).len(), 1, "sent once, at first");
    let (under, back) = (&out[marks[0]..marks[1]], &out[marks[1]..marks[2]]);
    assert!(sent_ids(under).is_empty(), "{under:?}");
    assert!(deleted_ids(under).is_empty(), "{under:?}");
    assert!(sent_ids(back).is_empty(), "{back:?}");
    assert!(deleted_ids(back).is_empty(), "{back:?}");
    assert!(back.contains(crate::present::kitty::graphics::PLACEHOLDER));
}

#[test]
fn a_search_in_an_opened_view_leaves_the_base_s_search() {
    let mut then = Then::new([]).opening([Reloaded::Document(rows(9))]);
    let (base, mut inputs) = (numbers(30), narrow());
    let mut terminal = Fake::new(40, 8, &[]);
    let options = plain(&mut then);
    let term = |rig: &Rig<'_, '_, '_>| {
        let search = rig.state.document().search.as_ref();
        search.map(|search| search.term().to_owned())
    };
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        between(rig, &[b"/Line 5\r"], |rig| {
            open(rig, "docs/other.md");
            assert_eq!(term(rig), None, "the view starts with no search");
        });
        let base = base_of(rig);
        between(rig, &[b"/Row 2\r"], |rig| {
            assert_eq!(term(rig).as_deref(), Some("Row 2"));
            rig.back_to_view(base);
            assert_eq!(term(rig).as_deref(), Some("Line 5"), "the base's own");
        });
    });
}

#[test]
fn back_to_the_base_shows_it_where_it_was() {
    let top_after = |keys: &[&[u8]]| {
        let frames = shown(&numbers(40), 40, 8, keys);
        top_row_of(replay(&frames).last().unwrap())
    };
    let expected = top_after(&[b"d", b"d"]);
    assert_ne!(expected, top_after(&[]), "the keys moved the view");
    let mut then = Then::new([]).opening([Reloaded::Document(rows(30))]);
    let (base, mut inputs) = (numbers(40), narrow());
    let mut terminal = Fake::new(40, 8, &[]);
    let options = plain(&mut then);
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        between(rig, &[b"d", b"d"], |rig| {
            open(rig, "docs/other.md");
        });
        let base = base_of(rig);
        between(rig, &[b"d"], |rig| rig.back_to_view(base));
    });
    let screens = terminal.screens();
    // The `d` moved the view of the other document, not the base's.
    assert_eq!(top_row_of(screens.last().unwrap()), expected);
    let tops: Vec<String> = screens.iter().map(|screen| top_row_of(screen)).collect();
    assert!(tops.iter().any(|top| top == "Row 1"), "{tops:?}");
}

#[test]
fn following_polls_only_the_top_view_s_file() {
    let mut then = Then::new([]).opening([Reloaded::Document(rows(3))]);
    let (base, mut inputs) = (numbers(30), narrow());
    let mut terminal = Fake::new(40, 8, &[]);
    let options = Options {
        follow: settings::Follow::On,
        ..plain(&mut then)
    };
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        between(rig, &[], |rig| {
            open(rig, "docs/other.md");
        });
        let empty = std::iter::repeat_n(Vec::new(), 4);
        rig.tools.terminal.keys.extend(empty);
    });
    let paths: Vec<_> = then.looked_at.iter().collect();
    assert!(!paths.is_empty(), "the view's file is looked at");
    assert!(
        paths.iter().all(|path| *path == Path::new("docs/other.md")),
        "{paths:?}"
    );
}

#[test]
fn a_failed_open_pushes_nothing_and_closes_its_view() {
    let mut then = Then::new([]).opening([Reloaded::Failed("gone".to_owned())]);
    let (base, mut inputs) = (numbers(30), narrow());
    let mut terminal = Fake::new(40, 8, &[]);
    let options = plain(&mut then);
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        between(rig, &[], |rig| {
            let failed = rig.open_view(Path::new("docs/none.md"), Input::Markdown);
            assert_eq!(failed, Err("gone".to_owned()));
            assert_eq!(rig.state.deck.document_sheet(), Sheet::Base);
            assert_eq!(rig.session.sources().len(), 1, "its source is gone too");
            assert_eq!(rig.session.sources_of(base_of(rig)).len(), 1);
        });
    });
    assert_eq!(then.opened, [(PathBuf::from("docs/none.md"), 1)]);
    let screens = terminal.screens();
    assert!(screen_text(screens.last().unwrap()).contains("Line 1"));
}

#[test]
fn a_resize_under_an_opened_view_lays_the_base_out_anew_on_return() {
    let words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda";
    let base = crate::typeset::parse::parse(words);
    let mut then = Then::new([]).opening([Reloaded::Document(rows(3))]);
    let mut inputs = narrow();
    let mut terminal = Fake::new(40, 8, &[]);
    let options = plain(&mut then);
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        between(rig, &[], |rig| {
            open(rig, "docs/other.md");
        });
        rig.tools.terminal.sizes.push_back(window(20, 8));
        rig.tools.terminal.keys.push_back(Vec::new());
        assert!(rig.round().unwrap(), "the resize ended the layout");
        let base = base_of(rig);
        rig.back_to_view(base);
    });
    let screens = terminal.screens();
    let last = screens.last().unwrap();
    let widest = (last.iter())
        .map(|row| unstyled(row).trim_end().chars().count())
        .max();
    assert!(widest.is_some_and(|width| width <= 20), "{last:?}");
    assert!(screen_text(last).contains("alpha"), "{last:?}");
}

#[test]
fn returning_in_reading_mode_draws_the_base_s_pages_again() {
    let caps = Capabilities {
        cell_pixels: Some((10, 20)),
        ..Capabilities::kitty(80)
    };
    let mut inputs = Inputs::new(caps);
    inputs.reading = Some(reading::reading());
    let mut then = Then::new([]).opening([Reloaded::Document(rows(3))]);
    let base = numbers(3);
    let mut terminal = Fake::new(80, 10, &[]);
    let options = plain(&mut then);
    let mut marks = Vec::new();
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        between(rig, &[], |rig| {
            marks.push(rig.tools.terminal.out.len());
            open(rig, "docs/other.md");
        });
        let base = base_of(rig);
        between(rig, &[], |rig| rig.back_to_view(base));
        between(rig, &[], |rig| marks.push(rig.tools.terminal.out.len()));
    });
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    let pages = sent_ids(&out[..marks[0]]);
    assert!(!pages.is_empty(), "the base's pages were sent at first");
    let later = &out[marks[0]..marks[1]];
    for id in deleted_ids(later).into_iter().chain(sent_ids(later)) {
        assert!(!pages.contains(&id), "{id} of the base was sent or deleted");
    }
    let seen = terminal.seen();
    assert!(reading::shows_page(seen.last().unwrap()));
}

#[test]
fn closing_a_view_forgets_its_images_its_layout_and_tells_the_files() {
    let mut then = Then::new([]).opening([Reloaded::Document(rows(3))]);
    let (base, mut inputs) = (numbers(30), narrow());
    let held = |document| ImageKey::from("x.png").in_document(document);
    let picture = || Ok(crate::images::Source::Raster(image::RgbaImage::new(1, 1)));
    inputs.images.insert(held(0), picture());
    let mut terminal = Fake::new(40, 8, &[]);
    let options = plain(&mut then);
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        let mut view = None;
        between(rig, &[], |rig| {
            view = Some(open(rig, "docs/other.md"));
            rig.tools.inputs.images.insert(held(1), picture());
        });
        let sheet = Sheet::View(view.unwrap());
        between(rig, &[], |rig| {
            assert!(
                rig.layouts.below(Sheet::Base).len() == 1,
                "the view's layout"
            );
            let base = base_of(rig);
            rig.back_to_view(base);
            let images = &rig.tools.inputs.images;
            assert!(images.get(&held(1)).is_none(), "the view's image");
            assert!(images.get(&held(0)).is_some(), "the base's image stays");
            assert!(rig.layouts.below(Sheet::Base).is_empty());
            let sheets: Vec<Sheet> = rig.state.deck.sheets().collect();
            assert!(!sheets.contains(&sheet));
            assert_eq!(rig.session.sources().len(), 1, "its source is closed");
        });
    });
    assert_eq!(then.forgotten, [1]);
}

#[test]
fn the_base_keeps_its_notes_across_a_view() {
    use crate::typeset::images::animation::{Animation, Frame, Plays};
    // A picture over the budget in reading mode says once that only its
    // first frame shows; that note is the base's.
    let text: String = (0..14).map(|n| format!("Paragraph {n}\n\n")).collect();
    let base = crate::typeset::parse::parse(&format!("{text}![wide](wide.gif)\n"));
    let frame = Frame {
        image: image::RgbaImage::from_pixel(300, 180, image::Rgba([255, 0, 0, 255])),
        gap: Duration::from_millis(100),
    };
    let animation = Animation {
        frames: vec![frame; 31],
        plays: Plays::Forever,
        over_budget: false,
    };
    let mut inputs = Inputs::new(Capabilities::kitty(60));
    inputs.caps.cell_pixels = Some((32, 64));
    inputs.reading = Some(reading::reading());
    let picture = crate::images::Source::Animated(std::sync::Arc::new(animation));
    inputs.images.insert("wide.gif", Ok(picture));
    let mut then = Then::new([]).opening([Reloaded::Document(rows(3))]);
    let mut terminal = Fake::new(60, 40, &[]);
    let options = plain(&mut then);
    let mut counts = Vec::new();
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        between(rig, &[], |rig| {
            counts.push(rig.state.document().all_warnings().len());
            open(rig, "docs/other.md");
        });
        let base = base_of(rig);
        between(rig, &[], |rig| rig.back_to_view(base));
        between(rig, &[], |rig| {
            counts.push(rig.state.document().all_warnings().len());
        });
    });
    assert_eq!(counts, [1, 1], "the base's note before and after a view");
}

#[test]
fn a_playing_animation_of_the_base_is_stopped_when_a_view_opens_and_stays_so() {
    use crate::typeset::images::animation::Plays;
    let (base, mut inputs) = animations::animated(0, Plays::Forever);
    let mut then = Then::new([]).opening([Reloaded::Document(rows(3))]);
    let mut terminal = Fake::new(60, 8, &[]);
    let options = plain(&mut then);
    let mut marks = Vec::new();
    visit(&base, &mut inputs, &mut terminal, options, |rig| {
        // The picture is started by a click, which selects none, and sent
        // with its frames. The `?` that
        // ends the round stops it, so it is started again in the state,
        // as a round that ends otherwise would leave it.
        let start: [&[u8]; 5] = [b"", b"\x1b[<0;3;1M\x1b[<0;3;1m", b"", b"", b""];
        between(rig, &start, |rig| {
            let key = crate::images::ImageKey::from("anim.gif");
            let now = rig.tools.terminal.now();
            let animation = &mut rig.state.document_mut().animation;
            animation.start(&key, now - Duration::from_secs(1));
            animation.built(&key, true, now);
            assert!(animation.playing().is_some(), "it plays");
            marks.push(rig.tools.terminal.out.len());
            open(rig, "docs/other.md");
        });
        let base = base_of(rig);
        between(rig, &[], |rig| {
            marks.push(rig.tools.terminal.out.len());
            rig.back_to_view(base);
        });
        between(rig, &[], |rig| marks.push(rig.tools.terminal.out.len()));
    });
    let out = String::from_utf8(terminal.out.clone()).unwrap();
    let started = animations::controlled(&out[..marks[0]]);
    assert!(started.iter().any(|(_, keys)| keys.starts_with("c=1,s=3")));
    let id = started[0].0;
    let under = animations::controlled(&out[marks[0]..marks[1]]);
    assert!(
        under.contains(&(id, "s=1".to_owned())),
        "the stop is in the view's first round: {under:?}"
    );
    let back = animations::controlled(&out[marks[1]..marks[2]]);
    let restarts = back.iter().filter(|(_, keys)| keys.starts_with("c=1,s=3"));
    assert_eq!(restarts.count(), 0, "it stays stopped: {back:?}");
}
