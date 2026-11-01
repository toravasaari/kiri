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
        (std::io::sink(), DELAY),
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
        (std::io::sink(), DELAY),
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
        (std::io::sink(), DELAY),
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
    let over = wait(
        (std::io::sink(), DELAY),
        &mut loads,
        |_, _| {},
        Line::Quiet,
        || None,
    );
    assert!(matches!(over, Over::Done));
}

#[test]
fn the_line_of_a_wait_for_diagrams_counts_only_them() {
    let (mut loads, mut arrivals) = expecting(&["picture.png", "diagram"]);
    let diagram = arrivals.pop().unwrap();
    let picture = arrivals.pop().unwrap();
    picture.come(square());
    let mut diagram = Some(diagram);
    let mut out = Vec::new();
    let line = Line::Diagrams(HashSet::from([ImageKey::from("diagram")]));
    let over = wait(
        (&mut out, Duration::ZERO),
        &mut loads,
        // The diagram comes after the picture has been given, so that the
        // line shows the picture's arrival, and not the diagram's.
        |key, _| {
            if key == ImageKey::from("picture.png") {
                diagram.take().unwrap().come(square());
            }
        },
        line,
        || None,
    );
    assert!(matches!(over, Over::Done));
    let line = crate::term::progress::line(0, 1);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        format!("\r\x1b[2K{line}\r\x1b[2K")
    );
}

#[test]
fn stopping_a_guard_stops_the_loads_and_gives_the_signal_as_the_failure() {
    let (mut loads, arrivals) = expecting(&["a.png", "b.png"]);
    let guard = Guard::default();
    let failure = guard.stop(&mut loads, 2);
    assert!(matches!(failure, Failure::Signal(2)), "{failure:?}");
    assert!(arrivals.iter().all(|arrival| arrival.stopped()));
    assert!(loads.is_empty());
}

#[test]
fn a_guard_that_catches_nothing_has_no_signal_to_give_and_nothing_to_release() {
    let mut guard = Guard::default();
    assert_eq!(guard.signal(), None);
    assert_eq!(guard.release(), None);
    // The screen's catching it is no harm either.
    guard.caught();
    assert_eq!(guard.signal(), None);
}

/// Run on its own by [`a_signal_after_a_finished_wait_ends_kiri`]: a wait
/// that finished releases the guard, so a later SIGHUP ends the process.
#[test]
#[ignore = "run in a process of its own"]
fn sighup_after_finish_loading() {
    let (loads, arrivals) = expecting(&["a.png"]);
    let mut guard = Guard::default();
    guard.catch();
    for arrival in arrivals {
        arrival.come(square());
    }
    let mut images = Images::new();
    assert!(finish_loading(loads, &mut images, &mut guard).is_ok());
    signal_hook::low_level::raise(signal_hook::consts::SIGHUP).unwrap();
}

#[test]
fn a_signal_after_a_finished_wait_ends_kiri() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};

    // A process of its own, as the signals reach the whole test binary.
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "frontend::loads::tests::sighup_after_finish_loading",
        ])
        .args(["--ignored", "--test-threads", "1"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.signal(), Some(libc::SIGHUP), "{output:?}");
    // A name that finds no test is not killed by anything.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("running 1 test"), "{stdout}");
}

/// Run on its own by [`a_signal_while_the_wait_runs_is_the_failure_and_the_next_ends_kiri`].
#[test]
#[ignore = "run in a process of its own"]
fn sighup_during_finish_loading() {
    let (loads, _arrivals) = expecting(&["a.png"]);
    let mut guard = Guard::default();
    guard.catch();
    signal_hook::low_level::raise(signal_hook::consts::SIGHUP).unwrap();
    let mut images = Images::new();
    let failure = finish_loading(loads, &mut images, &mut guard).unwrap_err();
    assert!(matches!(failure, Failure::Signal(1)), "{failure:?}");
    // Released by the stop, so the next one ends the process.
    signal_hook::low_level::raise(signal_hook::consts::SIGHUP).unwrap();
}

#[test]
fn a_signal_while_the_wait_runs_is_the_failure_and_the_next_ends_kiri() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};

    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "frontend::loads::tests::sighup_during_finish_loading",
        ])
        .args(["--ignored", "--test-threads", "1"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.signal(), Some(libc::SIGHUP), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("running 1 test"), "{stdout}");
}
