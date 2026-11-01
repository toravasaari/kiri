//! Loading the images of a run's documents: the renderers that the
//! settings choose for their diagrams, a guard against the signals that
//! would end kiri while programs outside it draw them, and the images.
//! The caller tells how far drawing has got where it likes, such as on a
//! line of the terminal.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::plan;
use super::{
    Fonts, HeadingLook, ImageKey, Images, LoadOptions, Loads, Looks, Part, Placeholder, Stamps,
    Wanted, diagram_languages, diagram_look,
};
use crate::config::{self, Environment, Settings};
use crate::diagrams::find::{self, Search};
use crate::diagrams::{Command, ExternalSettings, FormulaColors, NoRenderer, Renderer, Theme};
use crate::load::queue::{Context, Job, Wanted as Entry, What};
use crate::load::{Renderers, load_parts};
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
    /// The colours formulas are drawn in.
    pub formula: FormulaColors,
    /// The most pixels, wide and high, an image can be shown at.
    pub largest: Option<(u32, u32)>,
    /// How headings are drawn as images; `None` draws none.
    pub headings: Option<HeadingLook>,
}

impl Loading<'_> {
    /// What the keys of the images loaded this way name of how they are
    /// drawn.
    fn looks(&self) -> Looks {
        Looks {
            diagram: diagram_look(
                self.theme,
                self.formula,
                self.settings.diagram_background_panel,
            ),
            headings: self.headings,
        }
    }
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
    let renderers = background_renderers(loading);
    let renderer: &dyn Renderer = match &renderers {
        Some(renderers) => renderers,
        None => &NoRenderer,
    };
    // Whether a failure came in the background stays with what is kept.
    let late = std::mem::take(&mut images.late);
    let looks = loading.looks();
    let (wanted, have) = plan(parts, renderer, &looks, Some(images));
    let shown: Vec<ImageKey> = (wanted.iter())
        .map(|(index, image)| image.key(&looks).in_document(parts[*index].number))
        .collect();
    let programs = programs(&wanted, renderers.as_ref());
    let context = Arc::new(context(loading, renderers));
    let mut kept = Images::new();
    let mut entries = Vec::new();
    for (((index, image), have), program) in wanted.into_iter().zip(have).zip(programs) {
        let part = &parts[index];
        let key = image.key(&looks).in_document(part.number);
        if let Some(have) = have {
            entries.push(Entry::Kept(key.clone()));
            match late.contains(&key) {
                true => kept.came(key, have),
                false => kept.insert(key, have),
            }
            continue;
        }
        let again = changed.iter().any(|(changed, _)| *changed == key);
        let job = job(part, key, image, program, &context, loading);
        entries.push(match again {
            true => Entry::Again(job),
            false => Entry::Missing(job),
        });
    }
    // What the documents no longer show, and what changed under them.
    let gone = std::mem::take(images).by_key.into_iter();
    let changed = (changed.into_iter()).filter_map(|(key, image)| Some((key, image?)));
    kept.replaced = (gone.chain(changed))
        .filter_map(|(key, image)| Some((key, image.ok()?)))
        .collect();
    loads.reconcile(entries);
    kept.shown = shown;
    *images = kept;
}

/// Starts loading every image of `parts` in the background in `loads`,
/// as a reload loads a new one, for the documents shown first: each has
/// nothing that tells its size standing in for it in `images` until it
/// comes. What else `images` holds, such as the images of the files
/// shown with `parts`, stays.
///
/// The signals are not caught, as for [`reload_in_background`].
pub fn load_in_background(
    parts: &[Part<'_>],
    loading: &Loading,
    images: &mut Images,
    loads: &mut Loads,
) {
    let renderers = background_renderers(loading);
    let renderer: &dyn Renderer = match &renderers {
        Some(renderers) => renderers,
        None => &NoRenderer,
    };
    let looks = loading.looks();
    let (wanted, _) = plan(parts, renderer, &looks, None);
    let programs = programs(&wanted, renderers.as_ref());
    let context = Arc::new(context(loading, renderers));
    let mut jobs = Vec::new();
    for ((index, image), program) in wanted.into_iter().zip(programs) {
        let part = &parts[index];
        let key = image.key(&looks).in_document(part.number);
        images.wait(key.clone(), Placeholder::Unsized);
        images.shown.push(key.clone());
        jobs.push(Entry::Again(job(
            part, key, image, program, &context, loading,
        )));
    }
    loads.reconcile(loads.adding(jobs));
}

/// Starts loading the heading images of `parts`, drawn as `look` says,
/// that `images` neither holds nor is loading, in the background in
/// `loads`, as after a change of the cell size: each heading shows as
/// text until its image comes. What else `images` holds stays.
pub fn load_headings_in_background(
    parts: &[Part<'_>],
    look: &HeadingLook,
    images: &mut Images,
    loads: &mut Loads,
) {
    let looks = Looks {
        headings: Some(*look),
        ..Looks::default()
    };
    let (wanted, _) = plan(parts, &NoRenderer, &looks, None);
    // A heading needs nothing of what draws diagrams.
    let context = Arc::new(Context {
        renderer: Box::new(NoRenderer),
        theme: Theme::Light,
        formula: FormulaColors::default(),
        panel: false,
        largest: None,
        max_file_size: 0,
    });
    let mut jobs = Vec::new();
    for (index, image) in wanted {
        let key = image.key(&looks).in_document(parts[index].number);
        let Wanted::Heading { level, words, cell } = image else {
            continue;
        };
        if images.get(&key).is_some() || loads.loading(&key) {
            continue;
        }
        images.wait(key.clone(), Placeholder::Unsized);
        let what = What::Heading {
            words,
            look: *look.level(level),
            cell,
        };
        let context = Arc::clone(&context);
        jobs.push(Entry::Missing(Job {
            key,
            what,
            program: false,
            context,
        }));
    }
    if jobs.is_empty() {
        return;
    }
    loads.reconcile(loads.adding(jobs));
}

/// Starts loading the images in the middle of the text of `parts`, which
/// reading mode shows and the other layouts do not, that `images` neither
/// holds nor is loading, in the background in `loads`: each shows as its
/// alt text, or its source for a formula, until it comes. What else
/// `images` holds stays.
///
/// The signals are not caught, as for [`reload_in_background`].
pub fn load_inline_in_background(
    parts: &[Part<'_>],
    loading: &Loading,
    images: &mut Images,
    loads: &mut Loads,
) {
    let parts: Vec<Part> = (parts.iter())
        .map(|part| Part {
            inline_images: true,
            ..*part
        })
        .collect();
    let renderers = background_renderers(loading);
    let renderer: &dyn Renderer = match &renderers {
        Some(renderers) => renderers,
        None => &NoRenderer,
    };
    let looks = loading.looks();
    let (wanted, _) = plan(&parts, renderer, &looks, None);
    let programs = programs(&wanted, renderers.as_ref());
    let context = Arc::new(context(loading, renderers));
    let mut jobs = Vec::new();
    for ((index, image), program) in wanted.into_iter().zip(programs) {
        let part = &parts[index];
        let key = image.key(&looks).in_document(part.number);
        if images.get(&key).is_some() || loads.loading(&key) {
            continue;
        }
        images.wait(key.clone(), Placeholder::Unsized);
        images.shown.push(key.clone());
        jobs.push(Entry::Missing(job(
            part, key, image, program, &context, loading,
        )));
    }
    loads.reconcile(loads.adding(jobs));
}

/// Stands in for every image of `parts` that [`load_in_background`] would
/// load with a box of `rows` rows in `images`, to tell how tall the
/// documents are before their images load, such as whether they are
/// longer than the window.
pub fn measure_in_background(
    parts: &[Part<'_>],
    loading: &Loading,
    images: &mut Images,
    rows: u16,
) {
    let renderers = background_renderers(loading);
    let renderer: &dyn Renderer = match &renderers {
        Some(renderers) => renderers,
        None => &NoRenderer,
    };
    let looks = loading.looks();
    let (wanted, _) = plan(parts, renderer, &looks, None);
    for (index, image) in wanted {
        let key = image.key(&looks).in_document(parts[index].number);
        images.wait(key, Placeholder::Rows(rows));
    }
}

/// The renderers the settings choose for drawing diagrams in the
/// background, without a flag to stop them; `None` with diagrams off.
fn background_renderers(loading: &Loading) -> Option<Renderers> {
    (loading.settings.diagrams).then(|| {
        let (commands, external) = chosen(loading);
        Renderers::new(commands, external)
    })
}

/// Whether a program outside kiri draws each of the images `wanted`, with
/// `renderers`; asked before they move into the context of the jobs.
fn programs(wanted: &[(usize, Wanted<'_>)], renderers: Option<&Renderers>) -> Vec<bool> {
    let program = |image: &Wanted<'_>| match image {
        Wanted::Diagram { language, .. } => {
            renderers.is_some_and(|renderers| renderers.runs_programs_for([*language]))
        }
        Wanted::Heading { .. } | Wanted::Url(_) => false,
    };
    wanted.iter().map(|(_, image)| program(image)).collect()
}

/// The job that loads `image` of `part`, keyed `key`, in the background,
/// as `context` says, with a heading drawn as `loading` says. `program`
/// tells whether a program outside kiri draws it.
fn job(
    part: &Part<'_>,
    key: ImageKey,
    image: Wanted<'_>,
    program: bool,
    context: &Arc<Context>,
    loading: &Loading,
) -> Job {
    let what = match image {
        Wanted::Heading { level, words, .. } => {
            let look = loading
                .headings
                .expect("headings are wanted only with a look");
            What::Heading {
                words,
                look: *look.level(level),
                cell: look.cell,
            }
        }
        Wanted::Url(url) => What::Url {
            url: url.to_owned(),
            base: part.base.to_path_buf(),
        },
        Wanted::Diagram {
            language,
            source,
            line,
        } => What::Diagram {
            language: language.to_owned(),
            source: source.into_owned(),
            line,
            by_line: part.by_line,
        },
    };
    let context = Arc::clone(context);
    Job {
        key,
        what,
        program,
        context,
    }
}

/// What the jobs of a load in the background need, as `loading` says,
/// with `renderers` for diagrams.
fn context(loading: &Loading, renderers: Option<Renderers>) -> Context {
    let renderer: Box<dyn Renderer + Send + Sync> = match renderers {
        Some(renderers) => Box::new(renderers),
        None => Box::new(NoRenderer),
    };
    Context {
        renderer,
        theme: loading.theme,
        formula: loading.formula,
        panel: loading.settings.diagram_background_panel,
        largest: loading.largest,
        max_file_size: loading.max_file_size,
    }
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
        formula: loading.formula,
        progress,
        panel: loading.settings.diagram_background_panel,
        largest: loading.largest,
        headings: loading.headings,
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
mod tests;
