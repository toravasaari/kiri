//! Loading a document's images for a run of kiri: the renderers that the
//! settings choose for its diagrams, a guard against the signals that
//! would end kiri while programs outside it draw them, and the images.
//! The caller tells how far drawing has got where it likes, such as on a
//! line of the terminal.

use std::path::Path;

use super::{Images, LoadOptions, diagram_languages, load};
use crate::config::{self, Environment, Settings};
use crate::diagrams::find::{self, Search};
use crate::diagrams::{ExternalSettings, NoRenderer, Renderers, Theme};
use crate::term::Interrupt;

/// What loading a document's images needs to know, resolved for the run.
#[derive(Clone, Copy)]
pub struct Loading<'a> {
    /// The environment, which tells where kiri looks for the programs
    /// that draw diagrams and where they keep their files.
    pub env: &'a Environment,
    /// The settings, which tell whether diagrams are drawn, and how.
    pub settings: &'a Settings,
    /// The directory that relative image paths start from.
    pub base: &'a Path,
    /// The largest image file read, in bytes.
    pub max_file_size: u64,
    /// The page diagrams are drawn for, unless they choose their own
    /// colours.
    pub theme: Theme,
    /// Whether a warning names a diagram by its line in the document, or
    /// by its own first line.
    pub by_line: bool,
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

/// The images of `document`, and its diagrams drawn when the settings
/// want them, as `loading` says; `progress` is told how many diagrams are
/// drawn of how many. When programs draw any of its diagrams, Ctrl-C,
/// SIGTERM and SIGHUP stop them meanwhile, and are then the outcome.
pub fn load_document(
    document: &crate::model::Document,
    loading: &Loading,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> Result<Loaded, Interrupted> {
    let (renderers, interrupt) = match loading.settings.diagrams {
        true => {
            let (renderers, interrupt) = renderers(document, loading);
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
        by_line: loading.by_line,
        progress,
        panel: loading.settings.diagram_panel,
        largest: loading.largest,
    };
    let images = load(document, loading.base, loading.max_file_size, &options);
    if let Some(signal) = interrupt.as_ref().and_then(Interrupt::release) {
        return Err(Interrupted(signal));
    }
    Ok(Loaded { images, interrupt })
}

/// The renderers the settings choose for `document`'s diagrams, and a
/// guard that catches the signals while programs draw any of them.
fn renderers(
    document: &crate::model::Document,
    loading: &Loading,
) -> (Renderers, Option<Interrupt>) {
    let (env, settings) = (loading.env, loading.settings);
    let search = Search::new(env);
    let commands = find::commands(
        &settings.diagram_commands,
        &settings.diagram_no_search,
        &search,
    );
    let mut external = ExternalSettings {
        time_limit: settings.diagram_timeout,
        work_dir: config::cache_directory(env).map(|cache| cache.join("diagrams")),
        ..ExternalSettings::new(config::temp_directory(env))
    };
    let renderers = Renderers::new(commands.clone(), external.clone());
    // Only while a program draws one of the document's diagrams: they
    // otherwise end kiri at once, as without diagrams.
    let languages = diagram_languages(document, &renderers);
    if !renderers.runs_programs_for(languages) {
        return (renderers, None);
    }
    let interrupt = Interrupt::catch();
    external.stop = interrupt.as_ref().map(Interrupt::flag);
    (Renderers::new(commands, external), interrupt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::Source;
    use crate::testing::Scratch;

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
            base: &scratch,
            max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
            theme: Theme::Light,
            by_line: true,
            largest: Some((15, 100)),
        };
        let loaded = load_document(&document, &loading, None).unwrap();
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
            base: Path::new(""),
            max_file_size: super::super::DEFAULT_MAX_FILE_SIZE,
            theme: Theme::Light,
            by_line: true,
            largest: None,
        };
        let loaded = load_document(&document, &loading, None).unwrap();
        assert!(loaded.images.is_empty());
        assert!(loaded.interrupt.is_none());
    }
}
