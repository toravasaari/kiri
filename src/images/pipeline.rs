//! Loading the images of a run's documents: the renderers that the
//! settings choose for their diagrams, a guard against the signals that
//! would end kiri while programs outside it draw them, and the images.
//! The caller tells how far drawing has got where it likes, such as on a
//! line of the terminal.

use std::collections::{BTreeMap, HashSet};

use super::background::{Context, Job, What};
use super::{Fonts, ImageKey, Images, LoadOptions, Loads, Part, Stamps, Wanted, diagram_languages};
use super::{load_parts, plan};
use crate::config::{self, Environment, Settings};
use crate::diagrams::find::{self, Search};
use crate::diagrams::{Command, ExternalSettings, NoRenderer, Renderer, Renderers, Theme};
use crate::term::Interrupt;

/// What loading the documents' images needs to know, resolved for the
/// run.
#[derive(Clone, Copy)]
pub struct Loading<'a> {
    /// The environment, which tells where kiri looks for the programs
    /// that draw diagrams and where they keep their files.
    pub env: &'a Environment,
    /// The settings, which tell whether diagrams are drawn, and how.
    pub settings: &'a Settings,
    /// The largest image file read, in bytes.
    pub max_file_size: u64,
    /// The page diagrams are drawn for, unless they choose their own
    /// colours.
    pub theme: Theme,
    /// The most pixels, wide and high, an image can be shown at.
    pub largest: Option<(u32, u32)>,
}

/// A document's images, and the guard that caught the signals while
/// programs drew its diagrams, if any did.
#[derive(Debug, Default)]
pub struct Loaded {
    pub images: Images,
    /// Keeps the signals that the programs were stopped by from ending
    /// kiri by surprise later: it lives until something else catches them,
    /// such as the pager's screen, or until kiri ends.
    pub interrupt: Option<Interrupt>,
}

/// A signal, such as Ctrl-C's, came while programs drew diagrams. They
/// have stopped and their files are gone; kiri is to end by the signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interrupted(pub i32);

/// The images of the documents in `parts`, and their diagrams drawn
/// when the settings want them, as `loading` says, all in one map;
/// `progress` is told how many diagrams are drawn of how many. When
/// programs draw any of their diagrams, Ctrl-C, SIGTERM, SIGHUP and
/// Ctrl-\ stop them meanwhile, and are then the outcome.
pub fn load_documents(
    parts: &[Part<'_>],
    loading: &Loading,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> Result<Loaded, Interrupted> {
    let (images, interrupt) = load_kept(parts, loading, progress);
    if let Some(signal) = interrupt.as_ref().and_then(Interrupt::release) {
        return Err(Interrupted(signal));
    }
    Ok(Loaded { images, interrupt })
}

/// `load_documents` again, for documents that may have changed, into
/// `images`, which holds what was loaded before, with what it cannot
/// take from there loading in the background in `loads`: an image it
/// does not hold, a diagram that failed, and an image from a local file
/// whose modification time is not the one `stamps` hold. A load of the
/// same image that `loads` has under way carries on, unless the image's
/// file changed again; the loads of images the documents no longer show
/// stop. What `images` held that the documents no longer show, or that
/// loads anew, is set aside in it for [`Images::take_replaced`].
///
/// The signals are not caught: the caller, such as the pager, catches
/// them, and drops or stops `loads` when one comes.
pub fn reload_in_background(
    parts: &[Part<'_>],
    loading: &Loading,
    images: &mut Images,
    stamps: &mut Stamps,
    loads: &mut Loads,
) {
    let changed = stamps.refresh(parts, images);
    let renderers = (loading.settings.diagrams).then(|| {
        let (commands, external) = chosen(loading);
        Renderers::new(commands, external)
    });
    let renderer: &dyn Renderer = match &renderers {
        Some(renderers) => renderers,
        None => &NoRenderer,
    };
    // Whether a failure came in the background stays with what is kept.
    let late = std::mem::take(&mut images.late);
    let (wanted, have) = plan(parts, renderer, Some(images));
    let shown: Vec<ImageKey> = (wanted.iter())
        .map(|(index, image)| image.key().in_document(parts[*index].number))
        .collect();
    let keys: HashSet<&ImageKey> = shown.iter().collect();
    loads.keep(|key| keys.contains(key));
    let mut kept = Images::new();
    let mut jobs = Vec::new();
    for ((index, image), have) in wanted.into_iter().zip(have) {
        let part = &parts[index];
        let key = image.key().in_document(part.number);
        if let Some(have) = have {
            match late.contains(&key) {
                true => kept.came(key, have),
                false => kept.insert(key, have),
            }
            continue;
        }
        let again = changed.iter().any(|(changed, _)| *changed == key);
        if loads.loading(&key) && !again {
            continue;
        }
        let (what, program) = match image {
            Wanted::Url(url) => {
                let base = part.base.to_path_buf();
                (
                    What::Url {
                        url: url.to_owned(),
                        base,
                    },
                    false,
                )
            }
            Wanted::Diagram {
                language,
                source,
                line,
            } => {
                let program = (renderers.as_ref())
                    .is_some_and(|renderers| renderers.runs_programs_for([language]));
                let what = What::Diagram {
                    language: language.to_owned(),
                    source: source.to_owned(),
                    line,
                    by_line: part.by_line,
                };
                (what, program)
            }
        };
        jobs.push(Job { key, what, program });
    }
    // What the documents no longer show, and what changed under them.
    let gone = std::mem::take(images).by_key.into_iter();
    let changed = (changed.into_iter()).filter_map(|(key, image)| Some((key, image?)));
    kept.replaced = (gone.chain(changed))
        .filter_map(|(key, image)| Some((key, image.ok()?)))
        .collect();
    let renderer: Box<dyn Renderer + Send + Sync> = match renderers {
        Some(renderers) => Box::new(renderers),
        None => Box::new(NoRenderer),
    };
    let context = Context {
        renderer,
        theme: loading.theme,
        panel: loading.settings.diagram_panel,
        largest: loading.largest,
        max_file_size: loading.max_file_size,
    };
    loads.start(jobs, context);
    kept.shown = shown;
    *images = kept;
}

/// The images of `parts`, loaded as `load_documents` loads them, and the
/// guard that caught the signals while programs drew diagrams, if any
/// did.
fn load_kept(
    parts: &[Part<'_>],
    loading: &Loading,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> (Images, Option<Interrupt>) {
    let (renderers, interrupt) = match loading.settings.diagrams {
        true => {
            let (renderers, interrupt) = renderers(parts, loading);
            (Some(renderers), interrupt)
        }
        false => (None, None),
    };
    let options = LoadOptions {
        renderer: match &renderers {
            Some(renderers) => renderers,
            None => &NoRenderer,
        },
        theme: loading.theme,
        progress,
        panel: loading.settings.diagram_panel,
        largest: loading.largest,
        ..LoadOptions::default()
    };
    let (size, fonts) = (loading.max_file_size, &Fonts::system());
    (load_parts(parts, size, fonts, &options), interrupt)
}

/// The commands the settings choose for diagrams, and how they run,
/// without a flag to stop them.
fn chosen(loading: &Loading) -> (BTreeMap<String, Command>, ExternalSettings) {
    let (env, settings) = (loading.env, loading.settings);
    let search = Search::new(env);
    let commands = find::commands(
        &settings.diagram_commands,
        &settings.diagram_no_search,
        &search,
    );
    let external = ExternalSettings {
        time_limit: settings.diagram_timeout,
        work_dir: config::cache_directory(env).map(|cache| cache.join("diagrams")),
        ..ExternalSettings::new(config::temp_directory(env))
    };
    (commands, external)
}

/// The renderers the settings choose for the diagrams of the documents in
/// `parts`, and a guard that catches the signals while programs draw any
/// of them.
fn renderers(parts: &[Part<'_>], loading: &Loading) -> (Renderers, Option<Interrupt>) {
    let (commands, mut external) = chosen(loading);
    let renderers = Renderers::new(commands.clone(), external.clone());
    // Only while a program draws one of the documents' diagrams: they
    // otherwise end kiri at once, as without diagrams.
    let languages = (parts.iter()).flat_map(|part| diagram_languages(part.document, &renderers));
    if !renderers.runs_programs_for(languages) {
        return (renderers, None);
    }
    let interrupt = Interrupt::catch();
    external.stop = interrupt.as_ref().map(Interrupt::flag);
    (Renderers::new(commands, external), interrupt)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::images::{ImageError, Source};
    use crate::model::Document;
    use crate::testing::Scratch;

    /// `document` alone, the first, with images relative to `base`.
    fn alone<'a>(document: &'a Document, base: &'a Path) -> [Part<'a>; 1] {
        [Part {
            document,
            number: 0,
            base,
            by_line: true,
        }]
    }

    /// A run's settings with `command` for Mermaid.
    fn settings(command: &[&str]) -> Settings {
        let mut settings = crate::config::Config::default();
        let command = command.iter().map(|arg| (*arg).to_owned()).collect();
        settings
            .diagram_commands
            .insert("mermaid".to_owned(), command);
        let choices =
            crate::config::choose(&Default::default(), &Environment::default(), &settings);
        choices.with_theme(crate::theme::Theme::default())
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
        let document = crate::parse::parse("```mermaid\nflowchart TD\n```\n");
        let loading = Loading {
            env: &env,
            settings: &settings,
            max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
            theme: Theme::Light,
            largest: Some((15, 100)),
        };
        let loaded = load_documents(&alone(&document, &scratch), &loading, None).unwrap();
        assert!(loaded.interrupt.is_some());
        let key = crate::images::ImageKey::block("mermaid", "flowchart TD\n");
        match loaded.images.get(&key) {
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
        let document = crate::parse::parse("```mermaid\nflowchart TD\n```\n");
        let loading = Loading {
            env: &Environment::default(),
            settings: &settings,
            max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
            theme: Theme::Light,
            largest: None,
        };
        let parts = alone(&document, Path::new(""));
        let loaded = load_documents(&parts, &loading, None).unwrap();
        assert!(loaded.images.is_empty());
        assert!(loaded.interrupt.is_none());
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
            largest: None,
        };
        let document = crate::parse::parse("![x](a.png)\n");
        let parts = alone(&document, &scratch);
        let mut stamps = Stamps::of(&parts);
        let mut images = load_documents(&parts, &loading, None).unwrap().images;
        assert_eq!(size_of(&images, "a.png"), (4, 2));

        // Other content, the same time: the file is taken as it was.
        write_png(&scratch.join("a.png"), 6, 3, 1000);
        let mut loads = Loads::new();
        reload_in_background(&parts, &loading, &mut images, &mut stamps, &mut loads);
        assert_eq!(size_of(&images, "a.png"), (4, 2));
        assert!(loads.is_empty());

        write_png(&scratch.join("a.png"), 6, 3, 2000);
        reload_in_background(&parts, &loading, &mut images, &mut stamps, &mut loads);
        assert!(images.is_empty());
        let came = until(&mut loads, &"a.png".into());
        assert!(
            matches!(&came[..], [(_, Ok(image))] if image.size() == (6, 3)),
            "{came:?}"
        );
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
            largest: None,
        };
        let document = crate::parse::parse("![x](a.png)\n");
        let parts = alone(&document, &scratch);
        let mut stamps = Stamps::of(&parts);
        let mut images = load_documents(&parts, &loading, None).unwrap().images;
        assert!(matches!(images.get(&"a.png".into()), Some(Err(_))));
        write_png(&scratch.join("a.png"), 4, 2, 1000);
        let mut loads = Loads::new();
        reload_in_background(&parts, &loading, &mut images, &mut stamps, &mut loads);
        let came = until(&mut loads, &"a.png".into());
        assert!(
            matches!(&came[..], [(_, Ok(image))] if image.size() == (4, 2)),
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
            largest: None,
        };
        let document = crate::parse::parse("![x](https://example.com/a.png)\n");
        let parts = alone(&document, Path::new(""));
        let mut stamps = Stamps::of(&parts);
        let mut images = Images::new();
        images.insert(
            "https://example.com/a.png",
            Err(crate::images::ImageError::Remote),
        );
        let mut loads = Loads::new();
        reload_in_background(&parts, &loading, &mut images, &mut stamps, &mut loads);
        assert!(loads.is_empty());
        assert_eq!(images.len(), 1);
        assert!(matches!(
            images.get(&"https://example.com/a.png".into()),
            Some(Err(crate::images::ImageError::Remote))
        ));
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
    fn until(loads: &mut Loads, key: &ImageKey) -> Vec<(ImageKey, Result<Source, ImageError>)> {
        let start = std::time::Instant::now();
        let mut came = Vec::new();
        while start.elapsed() < std::time::Duration::from_secs(10) {
            came.extend(loads.poll());
            if came.iter().any(|(came, _)| came == key) {
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
            largest: None,
        };
        let before = crate::parse::parse("![x](a.png)\n");
        let parts = alone(&before, &scratch);
        let mut stamps = Stamps::of(&parts);
        let mut images = load_documents(&parts, &loading, None).unwrap().images;
        let document = crate::parse::parse("![x](a.png)\n\n```mermaid\none\n```\n");
        let parts = alone(&document, &scratch);
        let mut loads = Loads::new();
        reload_in_background(&parts, &loading, &mut images, &mut stamps, &mut loads);
        let diagram = ImageKey::block("mermaid", "one\n");
        assert_eq!(size_of(&images, "a.png"), (4, 2));
        assert!(images.get(&diagram).is_none());
        assert!(loads.loading(&diagram));
        assert_eq!(loads.progress(), Some((0, 1)));
        std::fs::write(scratch.join("one"), "").unwrap();
        let came = until(&mut loads, &diagram);
        assert!(matches!(came[..], [(_, Ok(Source::Svg(_)))]), "{came:?}");
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
            largest: None,
        };
        let both = crate::parse::parse("```mermaid\none\n```\n\n```mermaid\ntwo\n```\n");
        let parts = alone(&both, &scratch);
        let (mut images, mut stamps, mut loads) = (Images::new(), Stamps::of(&parts), Loads::new());
        reload_in_background(&parts, &loading, &mut images, &mut stamps, &mut loads);
        assert_eq!(loads.progress(), Some((0, 2)));
        // Both programs run before the newer reload.
        let started = || std::fs::read_to_string(scratch.join("started")).unwrap_or_default();
        let start = std::time::Instant::now();
        while started().lines().count() < 2 && start.elapsed() < std::time::Duration::from_secs(10)
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let one = crate::parse::parse("```mermaid\none\n```\n");
        let parts = alone(&one, &scratch);
        reload_in_background(&parts, &loading, &mut images, &mut stamps, &mut loads);
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
            largest: None,
        };
        let document = crate::parse::parse("```mermaid\none\n```\n");
        let parts = alone(&document, &scratch);
        let (mut images, mut stamps, mut loads) = (Images::new(), Stamps::of(&parts), Loads::new());
        reload_in_background(&parts, &loading, &mut images, &mut stamps, &mut loads);
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
            largest: None,
        };
        let before = crate::parse::parse("![x](a.png)\n\n![y](b.png)\n");
        let parts = alone(&before, &scratch);
        let mut stamps = Stamps::of(&parts);
        let mut images = load_documents(&parts, &loading, None).unwrap().images;
        write_png(&scratch.join("a.png"), 6, 3, 2000);
        let document = crate::parse::parse("![x](a.png)\n");
        let parts = alone(&document, &scratch);
        let mut loads = Loads::new();
        reload_in_background(&parts, &loading, &mut images, &mut stamps, &mut loads);
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
            matches!(&came[..], [(_, Ok(image))] if image.size() == (6, 3)),
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
            largest: None,
        };
        let diagrams = |names: &[&str]| {
            let blocks: Vec<String> = (names.iter())
                .map(|name| format!("```mermaid\n{name}\n```\n"))
                .collect();
            crate::parse::parse(&blocks.join("\n"))
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
        reload_in_background(&parts, &loading, &mut images, &mut stamps, &mut loads);
        wait_until(4);
        // Four more, while the first four, still wanted, are drawing.
        let both = diagrams(&["a1", "a2", "a3", "a4", "b1", "b2", "b3", "b4"]);
        let parts = alone(&both, &scratch);
        reload_in_background(&parts, &loading, &mut images, &mut stamps, &mut loads);
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
}
