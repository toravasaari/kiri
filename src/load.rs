use std::io;
use std::path::Path;

use kiri::config::Settings;
use kiri::images::{Images, Interrupted, Loaded, Loading, Loads, Part};
use kiri::render::Inputs;
use kiri::session::plan;
use kiri::term::Interrupt;
use kiri::typeset::caps::Capabilities;
use kiri::typeset::model::Document;
use kiri::typeset::theme::Theme;

use crate::{Context, Failure};

/// The rows an image or diagram that has not loaded yet counts as, to
/// tell whether a document is longer than the window before it loads.
pub(crate) const ROWS_UNLOADED: u16 = 3;

/// Whether `content`, whose `parts` are to load their images in the
/// background, surely goes to the pager with `settings`: always with
/// paging `always`; with paging on `auto`, when it is longer than the
/// window laid out with `inputs` and each image of `parts` counted at
/// [`ROWS_UNLOADED`] rows, which then stand in for them in `inputs`
/// until they start loading.
pub(crate) fn surely_paged(
    context: &Context,
    content: kiri::typeset::layout::Content,
    parts: &[Part],
    settings: &Settings,
    inputs: &mut Inputs,
) -> bool {
    let longer = plan::surely_paged(settings.paging, || {
        let loading = loading(context, settings, &inputs.caps);
        kiri::images::measure_in_background(parts, &loading, &mut inputs.images, ROWS_UNLOADED);
        let stream = kiri::typeset::layout::content(content, inputs);
        let rows = kiri::pager::rows(&stream.lines, &inputs.caps);
        let height = kiri::term::window_size(io::stdout()).map(|window| window.rows);
        plan::Measured { rows, height }
    });
    if !longer {
        inputs.images.take_placeholders();
    }
    longer
}

/// What waiting for the images before the pager opens came to.
pub(crate) struct Waited {
    /// The guard that caught the signals meanwhile, to go once the
    /// pager's screen catches them.
    pub(crate) interrupt: Option<Interrupt>,
    /// The loads still under way, when the reader opened the pager
    /// before all had come.
    pub(crate) loads: Option<Loads>,
}

/// Loads the images of `parts` into `inputs` while the reader waits for
/// the pager, with the progress line on standard error, a terminal, and
/// the hint that Space opens the pager now: until they have all come, or
/// until the reader presses Space or Enter, which leaves the rest loading
/// in the background. Other keys go unheeded. Ctrl-C, SIGTERM, SIGHUP and
/// Ctrl-\ stop the programs meanwhile, as when the images load first,
/// and are then the failure. `None` without a terminal to read keys
/// from, or without the line to tell of them, standard error not being
/// a terminal, where nothing has started.
pub(crate) fn wait_or_skip(
    context: &Context,
    parts: &[Part],
    settings: &Settings,
    inputs: &mut Inputs,
) -> Option<Result<Waited, Failure>> {
    if !kiri::term::stderr_is_terminal() {
        return None;
    }
    let mut keys = kiri::term::WaitKeys::open()?;
    let interrupt = Interrupt::catch();
    let signal = || interrupt.as_ref().and_then(Interrupt::signal);
    let mut loads = Loads::new();
    let loading = loading(context, settings, &inputs.caps);
    kiri::images::load_in_background(parts, &loading, &mut inputs.images, &mut loads);
    let images = &mut inputs.images;
    let poll = || {
        for (key, image) in loads.poll() {
            images.insert(key, image);
        }
        loads.progress().filter(|_| signal().is_none())
    };
    let skip = |timeout| keys.skip(timeout);
    let delay = kiri::term::progress::DELAY;
    let skipped = kiri::term::progress::until_done_or_skipped(io::stderr(), delay, poll, skip);
    // The terminal as it was, before the pager or the shell takes it.
    drop(keys);
    if signal().is_some() {
        loads.halt();
        loads.wait(kiri::images::STOPPING);
        let signal = interrupt.as_ref().and_then(Interrupt::release);
        return signal.map(|signal| Err(Failure::Signal(signal)));
    }
    let loads = skipped.then_some(loads);
    Some(Ok(Waited { interrupt, loads }))
}

/// Waits for what `loads` load to come into `images`, and gives what
/// came. A signal that `interrupt` caught meanwhile stops them, and is
/// the failure.
pub(crate) fn finish_loading(
    mut loads: Loads,
    images: &mut Images,
    interrupt: Option<&Interrupt>,
) -> Result<Images, Failure> {
    let mut came = Images::new();
    loop {
        for (key, image) in loads.poll() {
            images.insert(key.clone(), image.clone());
            came.insert(key, image);
        }
        if loads.is_empty() {
            return Ok(came);
        }
        if let Some(signal) = interrupt.and_then(Interrupt::signal) {
            loads.halt();
            loads.wait(kiri::images::STOPPING);
            let signal = interrupt.and_then(Interrupt::release).unwrap_or(signal);
            return Err(Failure::Signal(signal));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Starts loading the images of `parts` in `loads`, as a reload loads a
/// new image: each shows as it does without its image, its code or alt
/// text, with nothing to tell its size, until it comes.
pub(crate) fn load_in_background(
    context: &Context,
    parts: &[Part],
    settings: &Settings,
    inputs: &mut Inputs,
    loads: &mut Loads,
) {
    let loading = loading(context, settings, &inputs.caps);
    kiri::images::load_in_background(parts, &loading, &mut inputs.images, loads);
}

/// The images of the documents in `parts`, loaded and drawn as
/// `load_images` loads them, whatever the settings say about images.
pub(crate) fn load_parts(
    context: &Context,
    parts: &[Part],
    settings: &Settings,
    caps: &Capabilities,
) -> Result<Loaded, Failure> {
    let loading = loading(context, settings, caps);
    // Drawing diagrams can take seconds with an external program; a
    // terminal tells how far it has got.
    let loaded = if kiri::term::stderr_is_terminal() {
        use kiri::term::progress::{DELAY, while_working};
        while_working(io::stderr(), DELAY, |progress| {
            let tell = |done, total| progress.update(done, total);
            kiri::images::load_documents(parts, &loading, Some(&tell))
        })
    } else {
        kiri::images::load_documents(parts, &loading, None)
    };
    loaded.map_err(|Interrupted(signal)| Failure::Signal(signal))
}

/// What loading images needs to know, for a window with `caps`.
pub(crate) fn loading<'a>(
    context: &'a Context,
    settings: &'a Settings,
    caps: &Capabilities,
) -> Loading<'a> {
    Loading {
        env: context.env,
        settings,
        max_file_size: context.max_file_size,
        theme: kiri::diagrams::Theme::for_page(caps.dark_background),
        formula: formula_colors(&settings.theme, caps),
        largest: Some(kiri::typeset::images::size::largest(caps)),
        headings: kiri::render::heading_look(
            settings.headings,
            !settings.images,
            &settings.theme,
            caps,
        ),
    }
}

/// The colours formulas are drawn in, as RGB: the theme's for formulas,
/// and otherwise the text's, the theme's or else the terminal's; the
/// first of them known as RGB.
fn formula_colors(theme: &Theme, caps: &Capabilities) -> kiri::diagrams::FormulaColors {
    let rgb = |color: Option<kiri::typeset::color::Color>| match color?.rgb(&caps.palette)? {
        kiri::typeset::color::Color::Rgb(red, green, blue) => Some([red, green, blue]),
        kiri::typeset::color::Color::Palette(_) => None,
    };
    let text = [theme.formula.text, theme.foreground, caps.foreground];
    kiri::diagrams::FormulaColors {
        text: text.into_iter().find_map(rgb),
        background: rgb(theme.formula.background),
    }
}

/// `document`, from `file` or not, whose images are looked for beside the
/// file, as a part whose images are loaded.
pub(crate) fn part<'a>(
    document: &'a Document,
    file: Option<&'a Path>,
    inline_images: bool,
) -> Part<'a> {
    Part {
        document,
        number: 0,
        base: file.and_then(Path::parent).unwrap_or(Path::new("")),
        by_line: file.is_some(),
        inline_images,
    }
}

/// Waits for the programs that drew `loads`, stopped, to end, with a line
/// on standard error, a terminal, while that takes more than a second.
pub(crate) fn close_programs(loads: &Loads) {
    let limit = kiri::images::STOPPING;
    let wait = |most| loads.wait(most);
    if !kiri::term::stderr_is_terminal() {
        wait(limit);
        return;
    }
    let text = "kiri: closing background programs…";
    let second = std::time::Duration::from_secs(1);
    kiri::term::progress::while_waiting(io::stderr(), second, limit, text, wait);
}
