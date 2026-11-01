use std::collections::HashSet;

use super::*;
use crate::images::Expecting;
use crate::load::queue::Wanted;

/// `loads`, running nothing, with the images `names` loading, and what
/// brings each of them.
fn expecting(names: &[&str]) -> (Loads, Vec<crate::images::Arrival>) {
    let runner = Expecting::default();
    let mut loads = Loads::with(Box::new(runner.clone()));
    loads.reconcile(
        (names.iter())
            .map(|name| Wanted::Missing(crate::images::Job::named(*name)))
            .collect(),
    );
    let arrivals = names.iter().map(|_| runner.take().1).collect();
    (loads, arrivals)
}

fn square() -> Result<Source, ImageError> {
    Ok(Source::Raster(image::RgbaImage::new(2, 2)))
}

#[test]
fn a_quiet_wait_gives_each_image_as_it_comes_and_ends_when_nothing_loads() {
    let (mut loads, arrivals) = expecting(&["a.png", "b.png"]);
    let producer = std::thread::spawn(move || {
        for arrival in arrivals {
            arrival.come(square());
        }
    });
    let mut came = HashSet::new();
    let over = wait(
        std::io::sink(),
        &mut loads,
        |key, image| {
            assert!(image.is_ok());
            came.insert(key);
        },
        Line::Quiet,
        || None,
    );
    producer.join().unwrap();
    assert!(matches!(over, Over::Done));
    let names: HashSet<ImageKey> = ["a.png", "b.png"].into_iter().map(Into::into).collect();
    assert_eq!(came, names);
    assert!(loads.is_empty());
}

#[test]
fn a_wait_ends_by_the_signal_and_stops_nothing() {
    let (mut loads, arrivals) = expecting(&["a.png"]);
    let over = wait(
        std::io::sink(),
        &mut loads,
        |_, _| {},
        Line::Quiet,
        || Some(2),
    );
    assert!(matches!(over, Over::Signal(2)));
    assert!(!loads.is_empty());
    assert!(!arrivals[0].stopped());
}

#[test]
fn what_came_before_the_signal_is_given_before_the_wait_ends() {
    let (mut loads, mut arrivals) = expecting(&["a.png", "b.png"]);
    arrivals.remove(0).come(square());
    let mut came = Vec::new();
    let over = wait(
        std::io::sink(),
        &mut loads,
        |key, _| came.push(key),
        Line::Quiet,
        || Some(15),
    );
    assert!(matches!(over, Over::Signal(15)));
    assert_eq!(came, [ImageKey::from("a.png")]);
    assert!(loads.loading(&"b.png".into()));
}

#[test]
fn a_wait_with_nothing_loading_ends_at_once() {
    let mut loads = Loads::new();
    let over = wait(std::io::sink(), &mut loads, |_, _| {}, Line::Quiet, || None);
    assert!(matches!(over, Over::Done));
}
