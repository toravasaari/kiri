//! The jobs that load the images of a run's documents, with the renderers
//! that the settings choose for their diagrams, and the bringing of the
//! images up to date for a reload. Nothing here loads: the jobs run in the
//! queue of `src/load/queue.rs`, which the frontends start and wait for,
//! and which stops them when a signal that the frontend caught asks it to.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::plan;
use super::{
    HeadingLook, ImageKey, Images, Job, Loads, Looks, Part, Placeholder, Stamps, Wanted,
    diagram_look,
};
use crate::config::{self, Environment, Settings};
use crate::diagrams::find::{self, Search};
use crate::diagrams::{Command, ExternalSettings, FormulaColors, NoRenderer, Renderer, Theme};
use crate::load::Renderers;
use crate::load::queue::{Context, Fetch, Wanted as Entry, What};

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

/// Brings `images`, which holds what was loaded before, up to date for
/// documents that may have changed: it keeps what it can take from there
/// and sets the rest aside, but starts no load. What it cannot take is an
/// image it does not hold, a diagram that failed, and an image
/// from a local file whose modification time is not the one `stamps` hold;
/// the caller loads those, as [`jobs`] and [`Loads::want`] have it. A load
/// of an image whose file changed again stops here, so that it starts anew
/// there; the other loads under way are left to the caller to keep or
/// stop. What `images` held that the documents no longer show, or that
/// loads anew, is set aside in it for [`Images::take_replaced`].
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
    let mut kept = Images::new();
    for ((index, image), have) in wanted.into_iter().zip(have) {
        let key = image.key(&looks).in_document(parts[index].number);
        let Some(have) = have else {
            continue;
        };
        match late.contains(&key) {
            true => kept.came(key, have),
            false => kept.insert(key, have),
        }
    }
    // What the documents no longer show, and what changed under them.
    let gone = std::mem::take(images).by_key.into_iter();
    let changed_keys: Vec<ImageKey> = changed.iter().map(|(key, _)| key.clone()).collect();
    let changed = (changed.into_iter()).filter_map(|(key, image)| Some((key, image?)));
    kept.replaced = (gone.chain(changed))
        .filter_map(|(key, image)| Some((key, image.ok()?)))
        .collect();
    let under_way = loads.keys().filter(|key| !changed_keys.contains(key));
    loads.reconcile(under_way.cloned().map(Entry::Kept).collect());
    *images = kept;
}

/// The jobs that load the images of `parts`, as a load in the background
/// does them, in the order of the documents: their images, diagrams and
/// headings, and those in the middle of text for a part that reads them.
/// Nothing starts: the caller says which of them load, as
/// [`Loads::want`] does.
pub fn jobs(parts: &[Part<'_>], loading: &Loading) -> Vec<Job> {
    let renderers = background_renderers(loading);
    let renderer: &dyn Renderer = match &renderers {
        Some(renderers) => renderers,
        None => &NoRenderer,
    };
    let looks = loading.looks();
    let (wanted, _) = plan(parts, renderer, &looks, None);
    let programs = programs(&wanted, renderers.as_ref());
    let context = Arc::new(context(loading, renderers));
    let jobs = wanted
        .into_iter()
        .zip(programs)
        .map(|((index, image), program)| {
            let part = &parts[index];
            let key = image.key(&looks).in_document(part.number);
            job(part, key, image, program, &context, loading)
        });
    jobs.collect()
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
    let jobs = jobs(parts, loading);
    for job in &jobs {
        images.wait(job.key.clone(), Placeholder::Unsized);
        images.shown.push(job.key.clone());
    }
    loads.reconcile(loads.adding(jobs.into_iter().map(Entry::Again).collect()));
}

/// The keys of the images of `parts` that [`load_in_background`] would
/// load, in the order of the documents, to tell how tall the documents are
/// before their images load, such as whether they are longer than the
/// window: the caller stands a box in for each.
pub fn planned_keys(parts: &[Part<'_>], loading: &Loading) -> Vec<ImageKey> {
    let renderers = background_renderers(loading);
    let renderer: &dyn Renderer = match &renderers {
        Some(renderers) => renderers,
        None => &NoRenderer,
    };
    crate::typeset::images::plan::keys(parts, renderer, &loading.looks())
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
    let (fetch, compute) = match image {
        Wanted::Heading { level, words, .. } => {
            let look = loading
                .headings
                .expect("headings are wanted only with a look");
            let what = What::Heading {
                words,
                look: *look.level(level),
                cell: look.cell,
            };
            (None, what)
        }
        Wanted::Url(url) => {
            let fetch = Fetch::Url {
                url: url.to_owned(),
                base: part.base.to_path_buf(),
            };
            (Some(fetch), What::Decode)
        }
        Wanted::Diagram {
            language,
            source,
            line,
        } => {
            let what = What::Diagram {
                language: language.to_owned(),
                source: source.into_owned(),
                line,
                by_line: part.by_line,
            };
            (None, what)
        }
    };
    let context = Arc::clone(context);
    Job {
        key,
        fetch,
        compute,
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

#[cfg(test)]
mod tests;
