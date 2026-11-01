use std::path::Path;

use super::*;
use crate::images::{Arrived, ImageError, Source};
use crate::session::loading::Need;
use crate::testing::Scratch;
use crate::typeset::model::Document;

/// `document` alone, the first, with images relative to `base`.
fn alone<'a>(document: &'a Document, base: &'a Path) -> [Part<'a>; 1] {
    [Part {
        document,
        number: 0,
        base,
        by_line: true,
        inline_images: false,
    }]
}

/// A run's settings with `command` for Mermaid.
fn settings(command: &[&str]) -> Settings {
    let mut settings = crate::config::Config::default();
    let command = command.iter().map(|arg| (*arg).to_owned()).collect();
    settings
        .diagrams
        .commands
        .insert("mermaid".to_owned(), command);
    let choices = crate::config::choose(&Default::default(), &Environment::default(), &settings);
    choices.with_theme(crate::typeset::theme::Theme::default())
}

/// Has the loads load the images `parts` want, as the pager does after
/// each layout: those `images` does not hold, in the order of the
/// documents, and stopping the loads of the others.
fn want(parts: &[Part<'_>], loading: &Loading, images: &Images, loads: &mut Loads) {
    let jobs = jobs(parts, loading);
    let keys: Vec<ImageKey> = jobs.iter().map(|job| job.key.clone()).collect();
    let held = |key: &ImageKey| images.get(key).is_some();
    loads.want(&jobs, crate::session::loading::wanted(&keys, &[], held));
}

/// The images of `parts` as the foreground load before the pager loads
/// them: the jobs run through `Loads::want`, waited for until nothing
/// loads, and inserted in the order of the documents. It mirrors
/// `frontend::loads::load_parts`.
fn loaded(parts: &[Part<'_>], loading: &Loading) -> Images {
    let jobs = jobs(parts, loading);
    let mut loads = Loads::new();
    loads.want(
        &jobs,
        (0..jobs.len()).map(|at| (at, Need::Missing)).collect(),
    );
    let mut came = std::collections::HashMap::new();
    while !loads.is_empty() {
        loads.wait_by(std::time::Instant::now() + std::time::Duration::from_secs(1));
        for Arrived { key, image, .. } in loads.poll() {
            came.insert(key, image);
        }
    }
    let mut images = Images::new();
    for job in jobs {
        if let Some(image) = came.remove(&job.key) {
            images.insert(job.key, image);
        }
    }
    images
}

/// What the pager does for a reload: [`reload_in_background`] brings
/// `images` up to date, and the loads then load what is missing.
fn reconciled(
    parts: &[Part<'_>],
    loading: &Loading,
    images: &mut Images,
    stamps: &mut Stamps,
    loads: &mut Loads,
) {
    reload_in_background(parts, loading, images, stamps, loads);
    want(parts, loading, images, loads);
}

/// `parts`, all of them reading the images in the middle of text.
fn reading<'a>(parts: &[Part<'a>]) -> Vec<Part<'a>> {
    (parts.iter())
        .map(|part| Part {
            inline_images: true,
            ..*part
        })
        .collect()
}

#[test]
fn a_program_s_diagram_is_reduced_to_the_largest_size_it_can_be_shown_at() {
    // A 60 × 30 pixel PNG at three pixels to the CSS pixel.
    let scratch = Scratch::new("pipeline-largest");
    let mut png = std::io::Cursor::new(Vec::new());
    let image = image::RgbaImage::from_pixel(60, 30, image::Rgba([255, 0, 0, 255]));
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    std::fs::write(scratch.join("drawn.png"), png.into_inner()).unwrap();
    let copy = format!("cp '{}' \"$2\"", scratch.join("drawn.png").display());
    let settings = settings(&[
        "/bin/sh", "-c", &copy, "sh", "{input}", "{output}", "{scale}",
    ]);
    let env = Environment {
        tmpdir: Some(scratch.as_os_str().to_owned()),
        ..Environment::default()
    };
    let document = crate::typeset::parse::parse("```mermaid\nflowchart TD\n```\n");
    let loading = Loading {
        env: &env,
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: Some((15, 100)),
        headings: None,
    };
    let parts = alone(&document, &scratch);
    // A program draws a diagram, so the foreground load catches the signals.
    assert!(jobs(&parts, &loading).iter().any(|job| job.program));
    let images = loaded(&parts, &loading);
    let key = crate::images::ImageKey::block("mermaid", "flowchart TD\n")
        .with_look(loading.looks().diagram);
    match images.get(&key) {
        Some(Ok(Source::Dense { image, css })) => {
            assert_eq!((image.dimensions(), *css), ((15, 8), (20, 10)));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn no_diagrams_are_drawn_when_they_are_off() {
    let mut settings = settings(&["/bin/false", "{input}", "{output}"]);
    settings.diagrams = false;
    let document = crate::typeset::parse::parse("```mermaid\nflowchart TD\n```\n");
    let loading = Loading {
        env: &Environment::default(),
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    let parts = alone(&document, Path::new(""));
    let images = loaded(&parts, &loading);
    assert!(images.is_empty());
    assert!(!jobs(&parts, &loading).iter().any(|job| job.program));
}

/// A PNG of `width` × `height` pixels at `path`, last modified
/// `modified` seconds after the start of the epoch.
fn write_png(path: &Path, width: u32, height: u32, modified: u64) {
    let mut png = std::io::Cursor::new(Vec::new());
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 0, 0, 255]));
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    std::fs::write(path, png.into_inner()).unwrap();
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(modified);
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(time).unwrap();
}

fn size_of(images: &Images, url: &str) -> (u32, u32) {
    match images.get(&url.into()) {
        Some(Ok(image)) => image.size(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_reload_loads_an_image_file_again_only_when_its_modification_time_changed() {
    let scratch = Scratch::new("pipeline-stamps");
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    let settings = settings(&["/bin/false"]);
    let loading = Loading {
        env: &Environment::default(),
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    let document = crate::typeset::parse::parse("![x](a.png)\n");
    let parts = alone(&document, &scratch);
    let mut stamps = Stamps::of(&parts);
    let mut images = loaded(&parts, &loading);
    assert_eq!(size_of(&images, "a.png"), (4, 2));

    // Other content, the same time: the file is taken as it was.
    write_png(&scratch.join("a.png"), 6, 3, 1000);
    let mut loads = Loads::new();
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    assert_eq!(size_of(&images, "a.png"), (4, 2));
    assert!(loads.is_empty());

    write_png(&scratch.join("a.png"), 6, 3, 2000);
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    assert!(images.is_empty());
    let came = until(&mut loads, &"a.png".into());
    assert!(
        matches!(&came[..], [Arrived { image: Ok(image), .. }] if image.size() == (6, 3)),
        "{came:?}"
    );
}

#[test]
fn a_reload_in_reading_mode_loads_an_image_in_the_text_again_when_its_file_changed() {
    let scratch = Scratch::new("pipeline-stamps-inline");
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    let settings = settings(&["/bin/false"]);
    let loading = Loading {
        env: &Environment::default(),
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    let document = crate::typeset::parse::parse("Words ![x](a.png) in a line.\n");
    let parts = [Part {
        inline_images: true,
        ..alone(&document, &scratch)[0]
    }];
    let mut stamps = Stamps::of(&parts);
    let mut images = loaded(&parts, &loading);
    assert_eq!(size_of(&images, "a.png"), (4, 2));

    // The same time: the file is taken as it was.
    write_png(&scratch.join("a.png"), 6, 3, 1000);
    let mut loads = Loads::new();
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    assert_eq!(size_of(&images, "a.png"), (4, 2));
    assert!(loads.is_empty());

    write_png(&scratch.join("a.png"), 6, 3, 2000);
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    assert!(images.is_empty());
    let came = until(&mut loads, &"a.png".into());
    assert!(
        matches!(&came[..], [Arrived { image: Ok(image), .. }] if image.size() == (6, 3)),
        "{came:?}"
    );
}

#[test]
fn a_reload_after_reading_mode_came_on_keeps_an_image_in_the_text_whose_file_did_not_change() {
    let scratch = Scratch::new("pipeline-stamps-inline-m");
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    let settings = settings(&["/bin/false"]);
    let loading = Loading {
        env: &Environment::default(),
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    let document = crate::typeset::parse::parse("Words ![x](a.png) in a line.\n");
    // Opened as text, then `M` loads what reading mode shows in the text.
    let text = alone(&document, &scratch);
    let mut stamps = Stamps::of(&text);
    let mut images = loaded(&text, &loading);
    let mut loads = Loads::new();
    want(&reading(&text), &loading, &images, &mut loads);
    let came = until(&mut loads, &"a.png".into());
    images.came(
        "a.png".into(),
        Ok(came.into_iter().next().unwrap().image.unwrap()),
    );

    // `r` in reading mode: the file is as it was, so nothing loads again.
    let reading = reading(&text);
    reconciled(&reading, &loading, &mut images, &mut stamps, &mut loads);
    assert!(loads.is_empty(), "{:?}", loads.keys().collect::<Vec<_>>());
    assert_eq!(size_of(&images, "a.png"), (4, 2));
}

#[test]
fn a_reload_loads_an_image_whose_file_was_missing_when_it_appears() {
    let scratch = Scratch::new("pipeline-stamps-missing");
    let settings = settings(&["/bin/false"]);
    let loading = Loading {
        env: &Environment::default(),
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    let document = crate::typeset::parse::parse("![x](a.png)\n");
    let parts = alone(&document, &scratch);
    let mut stamps = Stamps::of(&parts);
    let mut images = loaded(&parts, &loading);
    assert!(matches!(images.get(&"a.png".into()), Some(Err(_))));
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    let mut loads = Loads::new();
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    let came = until(&mut loads, &"a.png".into());
    assert!(
        matches!(&came[..], [Arrived { image: Ok(image), .. }] if image.size() == (4, 2)),
        "{came:?}"
    );
}

#[test]
fn a_reload_keeps_an_image_that_has_no_file() {
    let settings = settings(&["/bin/false"]);
    let loading = Loading {
        env: &Environment::default(),
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    let document = crate::typeset::parse::parse("![x](https://example.com/a.png)\n");
    let parts = alone(&document, Path::new(""));
    let mut stamps = Stamps::of(&parts);
    let mut images = Images::new();
    images.insert(
        "https://example.com/a.png",
        Err(crate::images::ImageError::Remote),
    );
    let mut loads = Loads::new();
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    assert!(loads.is_empty());
    assert_eq!(images.len(), 1);
    assert!(matches!(
        images.get(&"https://example.com/a.png".into()),
        Some(Err(crate::images::ImageError::Remote))
    ));
}

#[test]
fn a_reload_loads_a_diagram_that_failed_again_and_an_image_file_that_failed_it_does_not() {
    let scratch = Scratch::new("pipeline-failed-diagram");
    let settings = settings(&["/bin/false"]);
    let loading = Loading {
        env: &Environment::default(),
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    let document =
        crate::typeset::parse::parse("![x](missing.png)\n\n```mermaid\nflowchart TD\n```\n");
    let parts = alone(&document, &scratch);
    let mut stamps = Stamps::of(&parts);
    let mut images = loaded(&parts, &loading);
    let diagram = ImageKey::block("mermaid", "flowchart TD\n").with_look(loading.looks().diagram);
    let missing = ImageKey::from("missing.png");
    assert!(matches!(images.get(&diagram), Some(Err(_))), "{images:?}");
    assert!(matches!(images.get(&missing), Some(Err(_))), "{images:?}");
    let mut loads = Loads::new();
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    // The diagram is drawn anew, and the failed image file, which did not
    // change, is not read again.
    assert!(loads.loading(&diagram));
    assert!(images.get(&diagram).is_none());
    assert!(!loads.loading(&missing));
    assert!(matches!(images.get(&missing), Some(Err(_))), "{images:?}");
}

/// The settings and environment for a run whose Mermaid diagrams the
/// shell script `script` draws, with its files in `scratch`.
fn scripted(scratch: &Scratch, script: &str) -> (Settings, Environment) {
    let path = scratch.join("draw.sh");
    std::fs::write(&path, script).unwrap();
    let path = path.to_string_lossy().into_owned();
    let settings = settings(&["/bin/sh", &path, "{input}", "{output}"]);
    let env = Environment {
        tmpdir: Some(scratch.as_os_str().to_owned()),
        ..Environment::default()
    };
    (settings, env)
}

/// Draws a diagram as an SVG square, once a file named as its first
/// line is in `scratch`; notes that it started, in `started` with its
/// source, and that it was stopped, in a file named as its first line
/// with `.stopped` after it.
fn waiting_script(scratch: &Scratch) -> String {
    let dir = scratch.to_string_lossy();
    format!(
        "name=$(head -n 1 \"$1\")\n\
             trap \"touch '{dir}/$name.stopped'; exit 130\" INT\n\
             echo \"$name\" >> '{dir}/started'\n\
             while [ ! -e \"{dir}/$name\" ]; do sleep 0.02; done\n\
             printf '<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8\" height=\"8\"/>' > \"$2\"\n"
    )
}

/// What comes of `loads` until `key` does, for at most ten seconds.
fn until(loads: &mut Loads, key: &ImageKey) -> Vec<Arrived> {
    let start = std::time::Instant::now();
    let mut came = Vec::new();
    while start.elapsed() < std::time::Duration::from_secs(10) {
        came.extend(loads.poll());
        if came.iter().any(|came| came.key == *key) {
            return came;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("{key:?} did not come: {came:?}");
}

/// Waits up to ten seconds for `path` to be there.
fn wait_for(path: &Path) {
    let start = std::time::Instant::now();
    while !path.exists() && start.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(path.exists(), "{path:?}");
}

#[test]
fn a_reload_in_the_background_keeps_what_it_has_and_loads_the_rest() {
    let scratch = Scratch::new("pipeline-background");
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    let (settings, env) = scripted(&scratch, &waiting_script(&scratch));
    let loading = Loading {
        env: &env,
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    let before = crate::typeset::parse::parse("![x](a.png)\n");
    let parts = alone(&before, &scratch);
    let mut stamps = Stamps::of(&parts);
    let mut images = loaded(&parts, &loading);
    let document = crate::typeset::parse::parse("![x](a.png)\n\n```mermaid\none\n```\n");
    let parts = alone(&document, &scratch);
    let mut loads = Loads::new();
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    let diagram = ImageKey::block("mermaid", "one\n");
    assert_eq!(size_of(&images, "a.png"), (4, 2));
    assert!(images.get(&diagram).is_none());
    assert!(loads.loading(&diagram));
    assert_eq!(loads.progress(), Some((0, 1)));
    std::fs::write(scratch.join("one"), "").unwrap();
    let came = until(&mut loads, &diagram);
    assert!(
        matches!(
            came[..],
            [Arrived {
                image: Ok(Source::Svg(_)),
                ..
            }]
        ),
        "{came:?}"
    );
    assert!(loads.is_empty());
    assert_eq!(loads.progress(), None);
}

#[test]
fn a_newer_reload_stops_the_loads_it_no_longer_wants_and_keeps_the_others() {
    let scratch = Scratch::new("pipeline-newer");
    let (settings, env) = scripted(&scratch, &waiting_script(&scratch));
    let loading = Loading {
        env: &env,
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    let both = crate::typeset::parse::parse("```mermaid\none\n```\n\n```mermaid\ntwo\n```\n");
    let parts = alone(&both, &scratch);
    let (mut images, mut stamps, mut loads) = (Images::new(), Stamps::of(&parts), Loads::new());
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    assert_eq!(loads.progress(), Some((0, 2)));
    // Both programs run before the newer reload.
    let started = || std::fs::read_to_string(scratch.join("started")).unwrap_or_default();
    let start = std::time::Instant::now();
    while started().lines().count() < 2 && start.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let one = crate::typeset::parse::parse("```mermaid\none\n```\n");
    let parts = alone(&one, &scratch);
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    wait_for(&scratch.join("two.stopped"));
    assert!(!scratch.join("one.stopped").exists());
    assert_eq!(
        loads.keys().collect::<Vec<_>>(),
        [&ImageKey::block("mermaid", "one\n")]
    );
    assert_eq!(loads.progress(), Some((0, 1)));
    std::fs::write(scratch.join("one"), "").unwrap();
    std::fs::write(scratch.join("two"), "").unwrap();
    let key = ImageKey::block("mermaid", "one\n");
    let came = until(&mut loads, &key);
    assert_eq!(came.len(), 1, "{came:?}");
    // Drawn once: the load the first reload started carried on.
    let mut started = started();
    started.retain(|c| c != '\n');
    assert_eq!(started.matches("one").count(), 1, "{started}");
}

#[test]
fn stopping_the_loads_ends_their_programs_before_it_returns() {
    let scratch = Scratch::new("pipeline-stop");
    let (settings, env) = scripted(&scratch, &waiting_script(&scratch));
    let loading = Loading {
        env: &env,
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    let document = crate::typeset::parse::parse("```mermaid\none\n```\n");
    let parts = alone(&document, &scratch);
    let (mut images, mut stamps, mut loads) = (Images::new(), Stamps::of(&parts), Loads::new());
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    wait_for(&scratch.join("started"));
    let start = std::time::Instant::now();
    loads.stop();
    assert!(scratch.join("one.stopped").exists());
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    assert!(loads.is_empty());
    assert!(loads.poll().is_empty());
}

#[test]
fn a_reload_in_the_background_loads_a_changed_file_again_and_sets_the_old_image_aside() {
    let scratch = Scratch::new("pipeline-background-stamps");
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    write_png(&scratch.join("b.png"), 2, 2, 1000);
    let settings = settings(&["/bin/false"]);
    let loading = Loading {
        env: &Environment::default(),
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    let before = crate::typeset::parse::parse("![x](a.png)\n\n![y](b.png)\n");
    let parts = alone(&before, &scratch);
    let mut stamps = Stamps::of(&parts);
    let mut images = loaded(&parts, &loading);
    write_png(&scratch.join("a.png"), 6, 3, 2000);
    let document = crate::typeset::parse::parse("![x](a.png)\n");
    let parts = alone(&document, &scratch);
    let mut loads = Loads::new();
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    assert!(images.is_empty(), "{images:?}");
    let mut replaced: Vec<_> = (images.take_replaced().into_iter())
        .map(|(key, image)| (key.to_string(), image.size()))
        .collect();
    replaced.sort();
    assert_eq!(
        replaced,
        [("a.png".to_owned(), (4, 2)), ("b.png".to_owned(), (2, 2))]
    );
    let came = until(&mut loads, &"a.png".into());
    assert!(
        matches!(&came[..], [Arrived { image: Ok(image), .. }] if image.size() == (6, 3)),
        "{came:?}"
    );
}

#[test]
fn overlapping_reloads_run_no_more_programs_at_once_than_one_reload() {
    let scratch = Scratch::new("pipeline-at-once");
    let (settings, env) = scripted(&scratch, &waiting_script(&scratch));
    let loading = Loading {
        env: &env,
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    let diagrams = |names: &[&str]| {
        let blocks: Vec<String> = (names.iter())
            .map(|name| format!("```mermaid\n{name}\n```\n"))
            .collect();
        crate::typeset::parse::parse(&blocks.join("\n"))
    };
    let started = || {
        let started = std::fs::read_to_string(scratch.join("started")).unwrap_or_default();
        started.lines().count()
    };
    let wait_until = |count| {
        let start = std::time::Instant::now();
        while started() < count && start.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    let first = diagrams(&["a1", "a2", "a3", "a4"]);
    let parts = alone(&first, &scratch);
    let (mut images, mut stamps, mut loads) = (Images::new(), Stamps::of(&parts), Loads::new());
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    wait_until(4);
    // Four more, while the first four, still wanted, are drawing.
    let both = diagrams(&["a1", "a2", "a3", "a4", "b1", "b2", "b3", "b4"]);
    let parts = alone(&both, &scratch);
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(started(), crate::diagrams::external::AT_ONCE);
    // One ends, and one more starts.
    std::fs::write(scratch.join("a1"), "").unwrap();
    wait_until(5);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(started(), 5);
    for name in ["a2", "a3", "a4", "b1", "b2", "b3", "b4"] {
        std::fs::write(scratch.join(name), "").unwrap();
    }
    let mut came = 0;
    let start = std::time::Instant::now();
    while came < 8 && start.elapsed() < std::time::Duration::from_secs(10) {
        came += loads.poll().len();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(came, 8);
}

#[test]
fn the_inline_pictures_load_when_asked_for_and_only_those_that_are_missing() {
    let scratch = Scratch::new("pipeline-inline");
    let settings = settings(&["/bin/false"]);
    let loading = Loading {
        env: &Environment::default(),
        settings: &settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    };
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    write_png(&scratch.join("f.png"), 3, 3, 1000);
    let document = crate::typeset::parse::parse("![alone](f.png)\n\nText ![a](a.png) in a line.\n");
    let parts = alone(&document, &scratch);
    // Without reading mode the lone image is all there is to load.
    let mut images = loaded(&parts, &loading);
    assert!(images.get(&"f.png".into()).is_some());
    assert!(images.get(&"a.png".into()).is_none());
    let mut loads = Loads::new();
    let parts = reading(&parts);
    want(&parts, &loading, &images, &mut loads);
    // Only the one in the line loads; the other is held, untouched.
    assert_eq!(loads.keys().collect::<Vec<_>>(), [&ImageKey::from("a.png")]);
    assert!(images.get(&"f.png".into()).is_some());
    let came = until(&mut loads, &"a.png".into());
    assert!(
        matches!(&came[..], [Arrived { image: Ok(image), .. }] if image.size() == (4, 2)),
        "{came:?}"
    );
    // Asked again, while it is loading or held, it starts nothing.
    images.came(
        "a.png".into(),
        Ok(came.into_iter().next().unwrap().image.unwrap()),
    );
    want(&parts, &loading, &images, &mut loads);
    assert!(loads.is_empty());
}

/// What the loads of the entry points below are given: no program draws
/// anything, and a heading is drawn in cells of 10 × 20 pixels.
fn plain_loading<'a>(env: &'a Environment, settings: &'a Settings) -> Loading<'a> {
    Loading {
        env,
        settings,
        max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
        theme: Theme::Light,
        formula: FormulaColors::default(),
        largest: None,
        headings: None,
    }
}

fn heading_look() -> HeadingLook {
    let level = crate::images::LevelLook {
        rows: 2,
        font_px: 30.0,
        bold: true,
        italic: false,
        color: [0x20, 0x60, 0xd0],
        code: crate::images::CodeLook {
            color: [0xe0, 0x20, 0xe0],
            background: None,
            padding: true,
        },
    };
    HeadingLook {
        cell: (10, 20),
        levels: [level; 6],
    }
}

fn square() -> Result<Source, ImageError> {
    Ok(Source::Raster(image::RgbaImage::new(2, 2)))
}

/// The key of the heading `# Kiri`, drawn as `heading_look` says.
fn heading_key() -> ImageKey {
    let document = crate::typeset::parse::parse("# Kiri\n");
    let parts = alone(&document, Path::new("."));
    let (settings, env) = (settings(&["/bin/false"]), Environment::default());
    let loading = Loading {
        headings: Some(heading_look()),
        ..plain_loading(&env, &settings)
    };
    let [job] = &jobs(&parts, &loading)[..] else {
        panic!("one heading")
    };
    job.key.clone()
}

#[test]
fn loading_in_the_background_leaves_the_loads_of_other_images_under_way() {
    let scratch = Scratch::new("pipeline-leaves-others");
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    let settings = settings(&["/bin/false"]);
    let env = Environment::default();
    let loading = plain_loading(&env, &settings);
    let document = crate::typeset::parse::parse("![x](a.png)\n");
    let parts = alone(&document, &scratch);
    let (mut images, mut loads) = (Images::new(), Loads::new());
    let other = loads.expect("other.png");
    load_in_background(&parts, &loading, &mut images, &mut loads);
    assert!(loads.loading(&"other.png".into()));
    assert!(!other.stopped());
    assert!(loads.loading(&"a.png".into()));
    assert_eq!(loads.progress(), Some((0, 2)));
    until(&mut loads, &"a.png".into());
    // The other load, still under way, comes as it was started.
    other.come(square());
    assert_eq!(loads.poll().len(), 1);
    assert!(loads.is_empty());
}

#[test]
fn loading_in_the_background_starts_an_image_under_way_anew_and_drops_the_old_result() {
    let scratch = Scratch::new("pipeline-restarts");
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    let settings = settings(&["/bin/false"]);
    let env = Environment::default();
    let loading = plain_loading(&env, &settings);
    let document = crate::typeset::parse::parse("![x](a.png)\n");
    let parts = alone(&document, &scratch);
    let (mut images, mut loads) = (Images::new(), Loads::new());
    let old = loads.expect("a.png");
    load_in_background(&parts, &loading, &mut images, &mut loads);
    assert!(old.stopped());
    assert!(loads.loading(&"a.png".into()));
    assert_eq!(loads.progress(), Some((0, 1)));
    // What the stopped load brings is dropped; the new one's is not.
    old.come(Ok(Source::Raster(image::RgbaImage::new(9, 9))));
    let came = until(&mut loads, &"a.png".into());
    assert!(
        matches!(&came[..], [Arrived { image: Ok(image), .. }] if image.size() == (4, 2)),
        "{came:?}"
    );
}

#[test]
fn wanting_a_heading_carries_on_one_under_way_and_loads_one_that_is_not_held() {
    let document = crate::typeset::parse::parse("# Kiri\n");
    let parts = alone(&document, Path::new("."));
    let (settings, env) = (settings(&["/bin/false"]), Environment::default());
    let loading = Loading {
        headings: Some(heading_look()),
        ..plain_loading(&env, &settings)
    };
    let key = heading_key();
    let (mut images, mut loads) = (Images::new(), Loads::new());
    let heading = loads.expect(key.clone());
    want(&parts, &loading, &images, &mut loads);
    // The one under way is not stopped, nor started again.
    assert!(!heading.stopped());
    assert_eq!(loads.progress(), Some((0, 1)));
    heading.come(square());
    assert_eq!(loads.poll().len(), 1);
    assert!(!loads.loading(&key));
    // Wanted with nothing held for it, it starts.
    want(&parts, &loading, &images, &mut loads);
    assert!(loads.loading(&key));
    assert_eq!(loads.progress(), Some((0, 1)));
    let came = until(&mut loads, &key);
    assert!(
        matches!(&came[..], [Arrived { image: Ok(_), .. }]),
        "{came:?}"
    );
    // Held, it is not wanted again.
    images.came(key.clone(), square());
    want(&parts, &loading, &images, &mut loads);
    assert!(loads.is_empty());
}

#[test]
fn wanting_the_inline_pictures_carries_on_a_load_under_way_and_loads_one_that_is_not() {
    let scratch = Scratch::new("pipeline-inline-leaves");
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    let settings = settings(&["/bin/false"]);
    let env = Environment::default();
    let loading = plain_loading(&env, &settings);
    let document = crate::typeset::parse::parse("Text ![a](a.png) in a line.\n");
    let parts = reading(&alone(&document, &scratch));
    let (images, mut loads) = (Images::new(), Loads::new());
    let under_way = loads.expect("a.png");
    want(&parts, &loading, &images, &mut loads);
    assert!(!under_way.stopped());
    assert_eq!(loads.progress(), Some((0, 1)));
    // The load under way is the one that comes.
    under_way.come(Ok(Source::Raster(image::RgbaImage::new(5, 5))));
    let came = loads.poll();
    assert!(
        matches!(&came[..], [Arrived { image: Ok(image), .. }] if image.size() == (5, 5)),
        "{came:?}"
    );
    // Not loaded, nor loading: it starts.
    want(&parts, &loading, &images, &mut loads);
    assert!(loads.loading(&"a.png".into()));
    until(&mut loads, &"a.png".into());
}

#[test]
fn a_reload_carries_on_a_load_under_way_of_an_image_still_shown_and_stops_the_rest() {
    let scratch = Scratch::new("pipeline-reload-expect");
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    let settings = settings(&["/bin/false"]);
    let env = Environment::default();
    let loading = plain_loading(&env, &settings);
    let document = crate::typeset::parse::parse("![x](a.png)\n");
    let parts = alone(&document, &scratch);
    let (mut images, mut stamps, mut loads) = (Images::new(), Stamps::of(&parts), Loads::new());
    let shown = loads.expect("a.png");
    let gone = loads.expect("gone.png");
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    assert!(!shown.stopped());
    assert!(gone.stopped());
    assert_eq!(loads.keys().collect::<Vec<_>>(), [&ImageKey::from("a.png")]);
    assert_eq!(loads.progress(), Some((0, 1)));
    // The load that was under way is the one that comes.
    shown.come(Ok(Source::Raster(image::RgbaImage::new(7, 7))));
    let came = loads.poll();
    assert!(
        matches!(&came[..], [Arrived { image: Ok(image), .. }] if image.size() == (7, 7)),
        "{came:?}"
    );
    assert!(loads.is_empty());
}

#[test]
fn a_reload_starts_a_load_under_way_anew_when_its_file_changed_again() {
    let scratch = Scratch::new("pipeline-reload-again");
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    let settings = settings(&["/bin/false"]);
    let env = Environment::default();
    let loading = plain_loading(&env, &settings);
    let document = crate::typeset::parse::parse("![x](a.png)\n");
    let parts = alone(&document, &scratch);
    let mut stamps = Stamps::of(&parts);
    let mut images = loaded(&parts, &loading);
    write_png(&scratch.join("a.png"), 6, 3, 2000);
    let mut loads = Loads::new();
    let old = loads.expect("a.png");
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    assert!(old.stopped());
    assert_eq!(loads.progress(), Some((0, 1)));
    old.come(Ok(Source::Raster(image::RgbaImage::new(7, 7))));
    let came = until(&mut loads, &"a.png".into());
    assert!(
        matches!(&came[..], [Arrived { image: Ok(image), .. }] if image.size() == (6, 3)),
        "{came:?}"
    );
}

/// A document with a heading, an image file and a diagram, and a loading
/// that draws the heading as an image.
const MEASURED: &str = "# Kiri\n\n![x](a.png)\n\n```mermaid\none\n```\n";

#[test]
fn the_images_planned_for_paging_are_those_loading_in_the_background_waits_for() {
    let scratch = Scratch::new("pipeline-measured");
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    let settings = settings(&["/bin/false"]);
    let env = Environment::default();
    let loading = Loading {
        headings: Some(heading_look()),
        ..plain_loading(&env, &settings)
    };
    let document = crate::typeset::parse::parse(MEASURED);
    let parts = alone(&document, &scratch);
    let planned = planned_keys(&parts, &loading);
    let mut waited = Images::new();
    let mut loads = Loads::new();
    load_in_background(&parts, &loading, &mut waited, &mut loads);
    loads.halt();
    assert_eq!(waited.shown.len(), 3, "a heading, an image and a diagram");
    assert_eq!(planned, waited.shown);
}

#[test]
fn a_reload_carries_on_a_load_under_way_of_an_image_it_holds() {
    let scratch = Scratch::new("pipeline-reload-held");
    write_png(&scratch.join("a.png"), 4, 2, 1000);
    let settings = settings(&["/bin/false"]);
    let env = Environment::default();
    let loading = plain_loading(&env, &settings);
    let document = crate::typeset::parse::parse("![x](a.png)\n");
    let parts = alone(&document, &scratch);
    let mut stamps = Stamps::of(&parts);
    let mut images = loaded(&parts, &loading);
    let mut loads = Loads::new();
    let under_way = loads.expect("a.png");
    reconciled(&parts, &loading, &mut images, &mut stamps, &mut loads);
    assert_eq!(size_of(&images, "a.png"), (4, 2));
    assert!(!under_way.stopped());
    assert!(loads.loading(&"a.png".into()));
}

#[test]
fn the_jobs_of_the_parts_are_their_heading_image_and_diagram_in_the_order_of_the_documents() {
    let settings = settings(&["/bin/false"]);
    let env = Environment::default();
    let loading = Loading {
        headings: Some(heading_look()),
        ..plain_loading(&env, &settings)
    };
    let document = crate::typeset::parse::parse(MEASURED);
    let parts = alone(&document, Path::new("."));
    let jobs = jobs(&parts, &loading);
    let keys: Vec<ImageKey> = jobs.iter().map(|job| job.key.clone()).collect();
    assert_eq!(keys, planned_keys(&parts, &loading));
    assert_eq!(keys.len(), 3, "{keys:?}");
    assert!(matches!(jobs[0].compute, What::Heading { .. }), "{jobs:?}");
    assert!(jobs[0].fetch.is_none());
    assert!(matches!(jobs[1].compute, What::Decode), "{jobs:?}");
    assert!(jobs[1].fetch.is_some());
    assert!(matches!(jobs[2].compute, What::Diagram { .. }), "{jobs:?}");
    // Nothing started, and no heading without a look.
    let without = plain_loading(&env, &settings);
    assert_eq!(super::jobs(&parts, &without).len(), 2);
}

#[test]
fn the_jobs_of_a_part_include_the_pictures_in_text_only_when_it_reads_them() {
    let settings = settings(&["/bin/false"]);
    let env = Environment::default();
    let loading = plain_loading(&env, &settings);
    let document = crate::typeset::parse::parse("![alone](f.png)\n\nText ![a](a.png) in a line.\n");
    let mut parts = alone(&document, Path::new("."));
    let keys = |parts: &[Part]| -> Vec<String> {
        (jobs(parts, &loading).iter())
            .map(|job| job.key.to_string())
            .collect()
    };
    assert_eq!(keys(&parts), ["f.png"]);
    parts[0].inline_images = true;
    assert_eq!(keys(&parts), ["f.png", "a.png"]);
}
