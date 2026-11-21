//! The budget of image bytes: a long page in reading mode, past it, lets
//! go of the parts far from the view.

use super::reading::reading;
use super::*;

/// A page of reading mode that has many parts: a hundred paragraphs.
fn long() -> Document {
    let text: Vec<String> = (1..=100).map(|n| format!("Paragraph {n}.")).collect();
    crate::typeset::parse::parse(&text.join("\n\n"))
}

/// The updates drawn for the long page in a window of 80 × 10 cells with
/// `budget` bytes for images, when the reader gives `keys`.
fn frames_with_budget(budget: Option<usize>, keys: &[&[u8]]) -> Vec<String> {
    let mut terminal = Fake::new(80, 10, keys);
    let mut inputs = Inputs {
        reading: Some(reading()),
        ..Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..screen_caps(80)
        })
    };
    let document = long();
    let options = Options::new(SearchCase::Smart);
    let mut loads = Loads::inline_builds();
    let mut driver = Driver::new(
        Content::Document(&document),
        None,
        session_for(&options),
        &mut inputs,
        options,
        &mut loads,
        &mut terminal,
    );
    driver.tools.encoder.set_budget(budget);
    while driver.round().unwrap() {}
    drop(driver);
    loads.halt();
    loads.wait(crate::images::STOPPING);
    terminal.updates()
}

/// Whether `frame` deletes an image before it sends or draws anything.
fn deletes_first(frame: &str) -> bool {
    let delete = frame.find("a=d,d=I");
    let send = frame.find("\x1b_Ga=T,");
    delete.is_some() && delete < send.or(Some(usize::MAX))
}

#[test]
fn a_long_page_with_a_small_budget_deletes_parts_at_the_start_of_a_frame_and_sends_them_again() {
    // The reader pages down 40 times and goes back to the top.
    let mut keys: Vec<&[u8]> = vec![b" "; 40];
    keys.extend([b"g".as_slice(), b"q".as_slice()]);
    let frames = frames_with_budget(Some(1), &keys);
    let deleting: Vec<usize> = (frames.iter().enumerate())
        .filter(|(_, frame)| frame.contains("a=d,d=I"))
        .map(|(at, _)| at)
        .collect();
    assert!(!deleting.is_empty(), "no frame deletes");
    for &at in &deleting {
        assert!(deletes_first(&frames[at]), "frame {at}: {:?}", frames[at]);
    }
    // Back at the top, the parts the terminal no longer holds are sent.
    let back = frames.len() - 1;
    let sent = |frame: &str| frame.matches("\x1b_Ga=T,").count();
    assert!(sent(&frames[back]) >= 1, "{:?}", frames[back]);
}

/// The ids `update` deletes, and those it sends after the first delete.
fn deleted_and_sent(update: &str) -> (Vec<String>, Vec<String>) {
    let ids = |marker: &str, from: &str| -> Vec<String> {
        (from.split(marker).skip(1))
            .filter_map(|rest| {
                let rest = rest.split("i=").nth(1)?;
                Some(rest.chars().take_while(char::is_ascii_digit).collect())
            })
            .collect()
    };
    let first = update.find("a=d,d=I").unwrap_or(update.len());
    let deleted = ids("a=d,d=I", &update[first..]);
    let sent = ids("\x1b_Ga=T,", &update[first..]);
    (deleted, sent)
}

#[test]
fn a_part_the_frame_draws_is_not_deleted_by_it_when_the_reader_jumps_back() {
    // With a budget of one byte every part that the last frame did not ask
    // for goes. The top, left for the end of the page and called back, is
    // in view in the frame that comes back: it must not be deleted and then
    // sent in the same update.
    let updates = frames_with_budget(Some(1), &[b"G", b"g", b"q"]);
    for (at, update) in updates.iter().enumerate() {
        let (deleted, sent) = deleted_and_sent(update);
        let again: Vec<_> = sent.iter().filter(|id| deleted.contains(id)).collect();
        assert!(again.is_empty(), "update {at} deletes and sends {again:?}");
    }
}

#[test]
fn with_no_budget_the_same_pages_delete_nothing_before_the_pager_quits() {
    let mut keys: Vec<&[u8]> = vec![b" "; 40];
    keys.extend([b"g".as_slice(), b"q".as_slice()]);
    let frames = frames_with_budget(None, &keys);
    assert!(frames.iter().all(|frame| !frame.contains("a=d,d=I")));
}

/// The budget that the pager gives the encoder it makes, for `options`.
fn budget_of(options: Options) -> Option<usize> {
    let mut terminal = Fake::new(80, 10, &[]);
    let mut inputs = Inputs {
        reading: Some(reading()),
        ..Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..screen_caps(80)
        })
    };
    let document = long();
    let mut loads = Loads::inline_builds();
    let driver = Driver::new(
        Content::Document(&document),
        None,
        session_for(&options),
        &mut inputs,
        options,
        &mut loads,
        &mut terminal,
    );
    let budget = driver.tools.encoder.budget();
    drop(driver);
    loads.halt();
    loads.wait(crate::images::STOPPING);
    budget
}

#[test]
fn the_pager_gives_its_encoder_the_budget_its_options_give() {
    let options = Options {
        image_memory: 1_000,
        ..Options::new(SearchCase::Smart)
    };
    assert_eq!(budget_of(options), Some(1_000));
}

#[test]
fn with_no_setting_the_pager_budgets_128_megabytes_of_images() {
    assert_eq!(
        budget_of(Options::new(SearchCase::Smart)),
        Some(128_000_000)
    );
}
