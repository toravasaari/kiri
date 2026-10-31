//! The kiri binary: the platform's entry point. It opens files and standard
//! input, the print and interactive shells, and starts the image loads and
//! the pager with the settings and the terminal's capabilities. The command
//! line is in `cli.rs`. It wires the parts of the library together and
//! does the I/O that they must not do.
//!
//! What does not belong here is a decision that needs no I/O, such as
//! whether a run prints, nor a rule about a document, a key or a setting.
//! A new function is a sign that the code has no place yet, not a place.
//!
//! Such decisions go to the state engine (`src/session/`), settings to
//! `src/config/`, and bytes for the terminal to the terminal layer
//! (`src/present/`). The placement map in docs/architecture.md says which.

use std::cell::Cell;
use std::fs::File;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use clap::error::ErrorKind;
use clap::{CommandFactory, FromArgMatches};
use kiri::config::{Environment, Settings};
use kiri::doctor::{Facts, Output};
use kiri::images::{
    Format, ImageError, ImageKey, Images, Loaded, Loads, Part, Placeholder, Source, Stamps,
};
use kiri::pager::Reload;
use kiri::present::encoder::Encoder;
use kiri::present::output::Page;
use kiri::render::Inputs;
use kiri::session::plan::{self, Load, Route};
use kiri::session::settings::{Follow, Paging};
use kiri::session::state::Look;
use kiri::typeset::caps::Capabilities;
use kiri::typeset::diagnostic::{Code, Diagnostic};
use kiri::typeset::input::Input;
use kiri::typeset::model::{Block, Document};
use kiri::typeset::reading::engine::Reading;
use kiri::typeset::reading::page::Wide;
use kiri::typeset::theme::Theme;

mod cli;
mod load;

use cli::{Cli, follow, options};
use load::{
    FileReload, ROWS_UNLOADED, close_programs, finish_loading, load_in_background, load_parts,
    loading, look, part, read_image_file, surely_paged, wait_or_skip, without_path,
};

fn main() -> ExitCode {
    let mut command = Cli::command();
    let matches = command.get_matches_mut();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    // clap's `requires` takes --doctor's default value for the flag itself.
    if cli.raw && !cli.doctor {
        command
            .error(ErrorKind::MissingRequiredArgument, "--raw needs --doctor")
            .exit();
    }
    let result = if cli.doctor {
        doctor(&cli)
    } else if cli.list_emojis {
        list_emojis(&cli)
    } else {
        match plan::route(&cli.files, kiri::term::stdin_is_terminal()) {
            // clap cannot require the file only when stdin is a terminal,
            // so this is its own error for a missing argument.
            Route::Missing => command
                .error(
                    ErrorKind::MissingRequiredArgument,
                    "the following required arguments were not provided:\n  <FILE>...",
                )
                .exit(),
            Route::RepeatedStdin => command
                .error(
                    ErrorKind::ArgumentConflict,
                    "- (standard input) can be given only once",
                )
                .exit(),
            route => run(&cli, route),
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Error(error)) => {
            eprintln!("kiri: {error}");
            ExitCode::FAILURE
        }
        Err(Failure::Told) => ExitCode::FAILURE,
        Err(Failure::Signal(signal)) => {
            // As the signal would have ended kiri, now that the programs
            // it started have stopped and their files are gone.
            kiri::term::end_by(signal);
            // As a shell tells that a signal ended a command, should the
            // signal not end kiri.
            ExitCode::from(128u8.saturating_add(signal as u8))
        }
    }
}

/// Why kiri ends without doing what it was asked.
enum Failure {
    /// An error, which kiri tells.
    Error(String),
    /// An error that the output has told already.
    Told,
    /// A signal, such as Ctrl-C's, stopped it.
    Signal(i32),
}

impl From<String> for Failure {
    fn from(error: String) -> Self {
        Failure::Error(error)
    }
}

/// What errors and warnings call standard input. Without an extension, so
/// that the content tells what standard input is, as for such a file.
const STDIN: &str = "<stdin>";

/// Shows the files that `route` names: one file, the README of a directory,
/// standard input, or several files, one after another, each under a header
/// that names it. A file that cannot be shown is told about at its place
/// among the others, which are still shown; kiri then fails. One file has
/// no header, and the pager reads it again when it changes.
fn run(cli: &Cli, route: Route) -> Result<(), Failure> {
    let lone = route.is_alone();
    let paths: Vec<Option<&Path>> = match route {
        Route::Stdin => vec![None],
        _ => (cli.files.iter())
            .map(|path| Some(path.as_path()).filter(|path| *path != Path::new("-")))
            .collect(),
    };
    let mut opened = Vec::new();
    for path in paths {
        opened.push(match open(cli, path) {
            // Nothing else is shown, so nothing is told before it.
            Err(error) if lone => return Err(error.into()),
            Ok(mut file) => {
                // Many image files may be given, more than kiri can keep
                // open, so a regular file is closed until it is shown.
                if let Content::Image { rest, .. } = &mut file.content
                    && !lone
                    && (file.file.as_deref()).is_some_and(Path::is_file)
                {
                    *rest = None;
                }
                Ok(file)
            }
            Err(error) => Err((
                path.map_or_else(|| PathBuf::from(STDIN), Path::to_owned),
                error,
            )),
        });
    }
    let env = Environment::from_process();
    let shown = || opened.iter().flatten();
    let files = plan::Files {
        route,
        images_only: shown().all(|file| matches!(file.content, Content::Image { .. })),
        alone: shown().all(|file| plan::shown_alone(file.input)),
        reloadable: shown().any(Opened::reloadable),
        text: shown().any(|file| file.input == Input::Text),
    };
    let (warnings, settings) = self::settings(cli, &env);
    // A broken configuration file does not stop images; the documents are
    // not shown without it.
    let (documents, mut settings, broken) = match settings {
        Ok(settings) => (true, settings, None),
        Err(error) => (false, without_config(cli, &env), Some(error)),
    };
    settings.paging = plan::paging(settings.paging, &files, settings.follow);
    settings.diagrams = plan::diagrams(settings.diagrams, &files);
    let context = Context::new(cli, &env, &settings);
    let caps = kiri::term::capabilities(context.mode, context.width, &env, settings.passthrough);
    let caps = plan::on_page(caps, settings.paper);
    let would_page = plan::pages_alone(settings.paging, kiri::term::stdout_is_terminal(), &caps);
    if plan::tells_warnings(&files, would_page) {
        for warning in &warnings {
            eprintln!("kiri: {warning}");
        }
    }
    let failed = Cell::new(false);
    if let Some(error) = broken.filter(|_| !files.images_only) {
        told(&failed, error);
    }
    let run = Run {
        context: &context,
        settings,
        caps,
        documents,
        lone,
        reading: true,
        failed: &failed,
    };
    let given = (opened.into_iter())
        .filter_map(|opened| match opened {
            Ok(opened) => given_from(&run, opened),
            Err((path, error)) => Some(Given::failed(path, error)),
        })
        .collect();
    show_given(run, given)
}

/// A file opened and read far enough to tell what it is.
struct Opened {
    /// What errors and warnings call it: its path, the README's for a
    /// directory, or `<stdin>`.
    path: PathBuf,
    /// The file, or `None` for standard input.
    file: Option<PathBuf>,
    /// How the file looked before it was read, when it could be looked at.
    look: Option<Look>,
    input: Input,
    content: Content,
}

impl Opened {
    /// Whether it is a document read from a regular file, which can be read
    /// again: not an image or a diagram, nor a pipe given by its path.
    fn reloadable(&self) -> bool {
        !plan::shown_alone(self.input) && (self.file.as_deref()).is_some_and(Path::is_file)
    }
}

/// What a file opened holds.
enum Content {
    /// An image in `format`, which starts with `head` and goes on in
    /// `rest`; `None` when the file is closed, to be opened again.
    Image {
        format: Format,
        head: Vec<u8>,
        rest: Option<Box<dyn Read>>,
    },
    /// Text, read whole.
    Text(String),
}

/// Opens the file at `path`, the README of a directory there, or standard
/// input without one, and tells what it is: an image is read only as far
/// as that takes, and any other file whole, as text.
fn open(cli: &Cli, path: Option<&Path>) -> Result<Opened, String> {
    let file = match path {
        Some(dir) if dir.is_dir() => Some(readme(dir)?),
        path => path.map(Path::to_owned),
    };
    let path = file.clone().unwrap_or_else(|| PathBuf::from(STDIN));
    // Before the file is read, so that a change meanwhile counts.
    let look = file.as_deref().and_then(|file| look(file).ok());
    let failed = |error: io::Error| format!("{}: {error}", path.display());
    let mut reader: Box<dyn Read> = match &file {
        Some(file) => Box::new(File::open(file).map_err(failed)?),
        None => Box::new(io::stdin().lock()),
    };
    let mut head = Vec::new();
    (&mut reader)
        .take(kiri::typeset::input::HEAD as u64)
        .read_to_end(&mut head)
        .map_err(failed)?;
    let input = cli
        .format
        .unwrap_or_else(|| kiri::typeset::input::detect(&path, &head));
    if let Input::Image(format) = input {
        let content = Content::Image {
            format,
            head,
            rest: Some(reader),
        };
        return Ok(Opened {
            path,
            file,
            look,
            input,
            content,
        });
    }
    let mut bytes = head;
    reader.read_to_end(&mut bytes).map_err(failed)?;
    // Text named by `--format` or by its extension is not an image either.
    let named = cli.format.is_some() || matches!(input, Input::Table(_) | Input::Diagram(_));
    let source = String::from_utf8(bytes).map_err(|_| {
        if named {
            format!("{}: not UTF-8 text", path.display())
        } else {
            format!(
                "{}: neither Markdown nor an image kiri knows",
                path.display()
            )
        }
    })?;
    Ok(Opened {
        path,
        file,
        look,
        input,
        content: Content::Text(source),
    })
}

/// The image that starts with `head` and goes on in `rest`, or, once
/// `rest` is closed, in `file` opened again from its start.
fn image_reader(
    file: Option<&Path>,
    head: Vec<u8>,
    rest: Option<Box<dyn Read>>,
) -> io::Result<Box<dyn Read>> {
    Ok(match (rest, file) {
        (Some(rest), _) => Box::new(Cursor::new(head).chain(rest)),
        (None, Some(file)) => Box::new(File::open(file)?),
        (None, None) => unreachable!("standard input is never closed"),
    })
}

/// The settings for documents, from the command line, the environment and
/// the configuration file, and warnings about them.
fn settings(cli: &Cli, env: &Environment) -> (Vec<Diagnostic>, Result<Settings, String>) {
    match kiri::config::load(env) {
        Ok(config) => kiri::config::settings(&options(cli), env, &config),
        Err(error) => (Vec::new(), Err(error)),
    }
}

/// The document that `source`, from the file at `path`, is as `input`,
/// anything but a diagram or an image, for `settings` and `caps`; `None`
/// for text that plain output gets as it is, as from cat. A warning
/// about its front matter goes to `warn`.
fn document(
    input: Input,
    source: &str,
    path: &Path,
    settings: &Settings,
    caps: &Capabilities,
    warn: impl FnOnce(Diagnostic),
) -> Result<Option<Document>, String> {
    Ok(Some(match input {
        Input::Table(separator) => kiri::typeset::parse::csv::parse(source, separator)
            .map_err(|error| format!("{}: {error}", path.display()))?,
        Input::Text if !caps.styling => return Ok(None),
        Input::Text => {
            kiri::typeset::parse::code(source, kiri::typeset::input::language(path).as_deref())
        }
        _ => {
            let style = settings.front_matter.style();
            let document = kiri::typeset::parse::parse_with(source, style.is_some());
            let error = style.and_then(|style| kiri::render::front_matter_error(&document, style));
            if let Some(error) = error {
                warn(Diagnostic::from(error));
            }
            document
        }
    }))
}

/// The settings from the command line and the environment alone, for a
/// configuration file that cannot be read, with the default theme.
fn without_config(cli: &Cli, env: &Environment) -> Settings {
    let choices = kiri::config::choose(&options(cli), env, &Default::default());
    choices.with_theme(Theme::default())
}

/// What shows the files, as `run` decides it.
struct Run<'a> {
    context: &'a Context<'a>,
    /// With the paging for these files.
    settings: Settings,
    /// For the terminal, with the paper that was asked for.
    caps: Capabilities,
    /// Whether documents are shown: the configuration file could be read.
    documents: bool,
    /// Whether it is one file, shown on its own.
    lone: bool,
    /// Whether to tell that reading mode is not shown.
    reading: bool,
    /// A file that cannot be shown is noted here, and kiri then fails.
    failed: &'a Cell<bool>,
}

/// How the files are shown, once their paging is set.
#[derive(Clone, Copy)]
struct Show<'a> {
    context: &'a Context<'a>,
    settings: &'a Settings,
    lone: bool,
    failed: &'a Cell<bool>,
}

/// The file `opened`, ready to show; `None` for text that is not shown, as
/// the configuration file could not be read.
fn given_from(run: &Run, opened: Opened) -> Option<Given> {
    let (settings, caps, lone) = (&run.settings, &run.caps, run.lone);
    let reload = (lone && opened.reloadable()).then_some((opened.input, opened.look));
    // A pipe given by its path is read to its end already, so the images
    // of a document alone from one are relative to the current directory.
    let alone = plan::shown_alone(opened.input);
    let file = (opened.file).filter(|file| alone || !lone || file.is_file());
    let name = opened.path.display().to_string();
    let mut warnings = Vec::new();
    let warn = |warning: Diagnostic| {
        // On standard error at once for a file alone, and in the pager too.
        if lone {
            eprintln!("kiri: {warning}");
        }
        warnings.push(warning);
    };
    let shown = match opened.content {
        Content::Image { format, head, rest } => Shown::Image { format, head, rest },
        // The configuration file's error tells why.
        Content::Text(_) if !run.documents => return None,
        Content::Text(source) => match opened.input {
            Input::Diagram(_) if !caps.styling => Shown::AsIs(source),
            Input::Diagram(language) => Shown::Diagram {
                document: kiri::typeset::parse::code(&source, Some(language)),
                language,
                source,
            },
            input => match document(input, &source, &opened.path, settings, caps, warn) {
                Ok(Some(document)) => Shown::Document {
                    document,
                    images: input != Input::Text,
                },
                Ok(None) => Shown::AsIs(source),
                Err(error) => Shown::Failed(error),
            },
        },
    };
    Some(Given {
        name,
        path: opened.path,
        file,
        shown,
        warnings,
        drawn: None,
        reload,
    })
}

/// Shows `given` as `run` says: in the pager as one stream when the paging
/// asks for it, and otherwise printed.
fn show_given(run: Run, mut given: Vec<Given>) -> Result<(), Failure> {
    let Run {
        context,
        settings,
        caps,
        lone,
        reading,
        failed,
        ..
    } = run;
    let terminal = kiri::term::stdout_is_terminal() && caps.styling;
    // In the pager, the files are one stream, read whole before it opens.
    let mut paged = terminal && settings.paging != Paging::Never;
    if paged {
        given = (given.into_iter())
            .map(|given| read_whole(context, &caps, given))
            .collect();
    }
    // Every file failed; an empty pager would only wait.
    if given.iter().all(Given::is_failed) {
        for given in &given {
            if let Shown::Failed(error) = &given.shown {
                told(failed, error.clone());
            }
        }
        if failed.get() {
            return Err(Failure::Told);
        }
    }
    // Before the images are read, so that a file changed meanwhile counts
    // as changed.
    let reload =
        (given.first().filter(|_| lone)).and_then(|given| reloading(given, context, &settings));
    // The documents' images load once the pager has opened.
    let later = paged && plan::may_load_later(settings.paging, settings.images, terminal, &caps);
    let Loaded {
        mut images,
        mut interrupt,
    } = load_given(context, &given, &settings, &caps, !later)?;
    for (number, file) in given.iter_mut().enumerate() {
        file.drawn = self::drawn(number, file, &mut images, &settings, &caps);
    }
    let drawn = given
        .first()
        .is_some_and(|given| matches!(given.drawn, Some(Drawn::Done)));
    paged = plan::paged_when_drawn(paged, lone, drawn, settings.paging);
    // Only the shape of what is shown decides this, so it stays a mapping.
    // A diagram drawn is a document by now, and not bare; as it needs a
    // terminal that draws images, nothing is told for it either way.
    let bare = given.first().is_some_and(|given| {
        matches!(
            given.shown,
            Shown::AsIs(_) | Shown::Image { .. } | Shown::Diagram { .. }
        )
    });
    let as_image = plan::pages_alone(settings.paging, terminal, &caps);
    let asked = plan::tells_reading(context.reading && reading, lone, bare, as_image);
    let reading_warning = plan::reading_warning(asked, &caps);
    if let Some(warning) = &reading_warning {
        eprintln!("kiri: {warning}");
    }
    let mut inputs = Inputs {
        images,
        theme: settings.theme.clone(),
        front_matter: settings.front_matter.style().unwrap_or_default(),
        headings: settings.headings.with_images(settings.images),
        table_rows: settings.table_rows,
        render_errors: settings.render_errors,
        reading: plan::reading(context.reading, &caps).then(|| context.reading_mode()),
        ..Inputs::new(caps)
    };
    let show = Show {
        context,
        settings: &settings,
        lone,
        failed,
    };
    if paged {
        let caught = || drop(interrupt.take());
        let opening = Opening {
            later,
            reading_warning,
            reload,
        };
        page_files(&show, given, &mut inputs, opening, caught)?;
    } else {
        shown(print_files(&show, given, &mut inputs))?;
    }
    match failed.get() {
        true => Err(Failure::Told),
        false => Ok(()),
    }
}

/// Shows `document`, called `name`, as a report that kiri writes itself
/// is shown, not a file: on its own, with nothing to read again.
fn show_report(
    cli: &Cli,
    env: &Environment,
    name: &str,
    document: Document,
    settings: Settings,
    caps: Capabilities,
) -> Result<(), Failure> {
    let shown = Shown::Document {
        document,
        images: true,
    };
    let given = Given::new(name.to_owned(), PathBuf::from(name), shown);
    let context = Context::new(cli, env, &settings);
    let failed = Cell::new(false);
    let run = Run {
        context: &context,
        caps: plan::on_page(caps, settings.paper),
        settings,
        documents: true,
        lone: true,
        reading: false,
        failed: &failed,
    };
    show_given(run, vec![given])
}

/// The reading of `given` again from its file, when it is a document alone
/// read from a regular file; its images' stamps are taken now.
fn reloading<'a>(
    given: &Given,
    context: &'a Context<'a>,
    settings: &'a Settings,
) -> Option<FileReload<'a>> {
    let (input, look) = given.reload?;
    let (Shown::Document { document, .. }, Some(path)) = (&given.shown, given.file.clone()) else {
        return None;
    };
    let stamps = Stamps::of(&[part(document, Some(&path), context.reading)]);
    Some(FileReload {
        context,
        settings,
        path,
        input,
        stamps,
        read: look,
    })
}

/// What is left to decide when the pager opens.
struct Opening<'a> {
    /// Whether the documents' images load once the pager has opened.
    later: bool,
    reading_warning: Option<Diagnostic>,
    /// Where the pager reads the document again from.
    reload: Option<FileReload<'a>>,
}

/// `warning` about the file `name`, as told: named among several files.
fn told_line(lone: bool, name: &str, warning: Diagnostic) -> Diagnostic {
    match lone {
        true => warning,
        false => warning.in_file(name),
    }
}

/// The warnings about `images` of `parts`, documents among `files`, under
/// each file's name, by the file's number among `files`.
fn told_files(
    images: &Images,
    parts: &[Part],
    files: &[kiri::typeset::model::File],
    show: &Show,
) -> Vec<Vec<Diagnostic>> {
    let stderr = show.settings.render_errors.stderr();
    let mut told = vec![Vec::new(); files.len()];
    for part in parts {
        let name = &files[part.number].name;
        told[part.number] = (images.warnings_in(part.number, stderr).into_iter())
            .map(|warning| told_line(show.lone, name, warning))
            .collect();
    }
    told
}

/// What writing the output came to: a closed pipe (kiri *.md | head) takes
/// no more, which is no failure.
fn shown(result: io::Result<()>) -> Result<(), Failure> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(Failure::Error(error.to_string())),
        Ok(()) => Ok(()),
    }
}

/// `given`, an image file read whole, to be shown in a stream of files
/// that its reader cannot wait for, such as the pager's, or failed with why
/// it cannot be read; any other file as it is.
fn read_whole(context: &Context, caps: &Capabilities, mut given: Given) -> Given {
    given.shown = match std::mem::replace(&mut given.shown, Shown::AsIs(String::new())) {
        Shown::Image { format, head, rest } => {
            let image = image_reader(given.file.as_deref(), head, rest)
                .map_err(|error| format!("{}: {error}", given.path.display()))
                .and_then(|reader| read_image_file(context, caps, &given.path, format, reader));
            match image {
                Ok(image) => Shown::Read(image),
                Err(error) => Shown::Failed(error),
            }
        }
        shown => shown,
    };
    given
}

/// Tells `error`, with which kiri is to fail, as `failed` then holds.
fn told(failed: &Cell<bool>, error: String) {
    eprintln!("kiri: {error}");
    failed.set(true);
}

/// Tells `drawn`, after its diagram file is shown, and notes in `failed`
/// whether kiri is to fail for it.
fn tell_drawn(drawn: &Drawn, failed: &Cell<bool>) {
    match drawn {
        Drawn::Failed(error) => told(failed, error.to_string()),
        Drawn::Told => failed.set(true),
        Drawn::Hint(hint) => eprintln!("kiri: {hint}"),
        Drawn::Done => {}
    }
}

/// Shows `given`, the files read whole, in the pager as one stream, and
/// prints them as `print_files` does should it not open, with the images
/// `inputs` holds and those of the image files among them. One file alone
/// is shown as a document on its own is, without a header, and the status
/// line names it without its place among files.
/// A file that failed is in the pager's stream under its header, with why.
/// Why a file failed, and a warning about a file, are told before the
/// pager opens, and why the diagram of a diagram file was not drawn after
/// it closes; all of them are noted in `failed`. `caught` runs once the
/// pager's screen catches the signals. With `opening.later`, the images of
/// the documents load in the background once the pager has opened; should
/// it not open, they load before the files are printed.
#[expect(clippy::too_many_lines)]
fn page_files(
    show: &Show,
    given: Vec<Given>,
    inputs: &mut Inputs,
    opening: Opening,
    caught: impl FnOnce(),
) -> Result<(), Failure> {
    let Show {
        context,
        settings,
        lone,
        failed,
    } = *show;
    let Opening {
        later,
        reading_warning,
        reload: mut reloader,
    } = opening;
    let reloaded = reloader.as_ref().map(|reload| reload.path.clone());
    let mut files = Vec::new();
    // What is told about each file before it is shown.
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    // What the pager tells of a file alone, as it was read.
    let mut warned_of = Vec::new();
    // What is told about each file after it is shown.
    let mut drawn = Vec::new();
    // The documents whose images load later, by number, and their files.
    let mut documents = Vec::new();
    for (number, given) in given.into_iter().enumerate() {
        drawn.push(given.drawn);
        if let Shown::Document { images: true, .. } = given.shown
            && later
            && settings.images
        {
            documents.push((number, given.file.clone()));
        }
        let name = given.name;
        let mut error = None;
        let mut warned = Vec::new();
        for warning in &given.warnings {
            match lone {
                true => warned_of.push(warning.clone()),
                false => warned.push(warning.clone().in_file(&name)),
            }
        }
        let document = match given.shown {
            Shown::Failed(why) => {
                // The header names the file already.
                let reason = without_path(why.clone(), &given.path);
                error = Some(why);
                Document::new(vec![Block::Error(reason)])
            }
            Shown::Read(image) => {
                let key = ImageKey::from(name.as_str()).in_document(number);
                inputs.images.insert(key, Ok(image));
                image_file(name.clone()).document
            }
            Shown::Document { document, .. } => {
                let stderr = settings.render_errors.stderr();
                for warning in inputs.images.warnings_in(number, stderr) {
                    warned.push(told_line(lone, &name, warning));
                }
                document
            }
            Shown::Diagram { document, .. } => document,
            // Plain output, the only one to get text as it is, has no
            // pager, nor do images read as they are shown.
            Shown::AsIs(text) => kiri::typeset::parse::code(&text, None),
            Shown::Image { .. } => unreachable!("an image file in the pager is read whole"),
        };
        errors.push(error);
        warnings.push(warned);
        files.push(kiri::typeset::model::File { name, document });
    }
    let one = files.len() == 1;
    let (content, name) = match one {
        true => (
            kiri::typeset::layout::Content::Document(&files[0].document),
            files[0].name.as_str(),
        ),
        // The pager's status line names each file after itself.
        false => (kiri::typeset::layout::Content::Files(&files), ""),
    };
    // The documents whose images load later, as parts that read images as
    // `inline_images` says.
    let parts_of = |inline_images| -> Vec<Part> {
        (documents.iter())
            .map(|(number, file)| Part {
                number: *number,
                ..part(&files[*number].document, file.as_deref(), inline_images)
            })
            .collect()
    };
    let mut parts = parts_of(context.reading);
    // With paging on auto, the images load first unless the files are
    // longer than the window without them; the guard then goes once the
    // pager's screen catches the signals.
    let mut guard = None;
    // Waiting, the reader can open the pager before they have all come,
    // and the rest then load in the background.
    let mut still = None;
    let load = (!parts.is_empty()).then(|| {
        plan::load(
            surely_paged(context, content, &parts, settings, inputs),
            settings.initial_load,
        )
    });
    let waited =
        match (load == Some(Load::Wait)).then(|| wait_or_skip(context, &parts, settings, inputs)) {
            Some(Some(waited)) => Some(waited?),
            _ => None,
        };
    if let Some(waited) = waited {
        let told = told_files(&inputs.images, &parts, &files, show);
        for (warned, told) in warnings.iter_mut().zip(told) {
            warned.extend(told);
        }
        guard = waited.interrupt;
        // The pager decides as it was told: with three rows for each
        // image that has not come.
        for key in waited.loads.iter().flat_map(Loads::keys) {
            inputs
                .images
                .wait(key.clone(), Placeholder::Rows(ROWS_UNLOADED));
        }
        still = waited.loads;
        parts.clear();
    } else if matches!(load, Some(Load::Now | Load::Wait)) {
        let loaded = load_parts(context, &parts, settings, &inputs.caps)?;
        let told = told_files(&loaded.images, &parts, &files, show);
        for (warned, told) in warnings.iter_mut().zip(told) {
            warned.extend(told);
        }
        inputs.images.extend(loaded.images);
        guard = loaded.interrupt;
        parts.clear();
    }
    let tell_before = |number: usize| {
        for warning in &warnings[number] {
            eprintln!("kiri: {warning}");
        }
        if let Some(error) = &errors[number] {
            told(failed, error.clone());
        }
    };
    let told_before = Cell::new(false);
    let opening = || {
        (0..files.len()).for_each(tell_before);
        told_before.set(true);
        caught();
        drop(guard.take());
    };
    // Several files, and an image file, are not read again, and there is
    // nothing to follow, which the pager says only when the command line
    // asked to follow.
    let options = kiri::pager::Options {
        follow: match reloader {
            Some(_) => settings.follow,
            None => context.follow,
        },
        jump: settings.follow_jump,
        warnings: (warned_of.into_iter()).chain(reading_warning).collect(),
        reload: reloader.as_mut().map(|reload| reload as &mut dyn Reload),
        reading: Some(context.reading_mode()),
        // What reading mode shows in the middle of text loads when `M`
        // turns it on, unless `--reading` had it loaded at the start. A
        // file alone loads that of the document as the last reload read it.
        inline: Some(Box::new(|shown, images, loads, caps| {
            if !(settings.images && caps.draws_images()) {
                return;
            }
            let parts: Vec<Part> = match shown {
                kiri::typeset::layout::Content::Document(document) if lone => {
                    vec![part(document, reloaded.as_deref(), true)]
                }
                _ => parts_of(true),
            };
            let loading = loading(context, settings, caps);
            kiri::images::load_inline_in_background(&parts, &loading, images, loads);
        })),
        background_offset: settings.background_offset,
        ..kiri::pager::Options::new(settings.search_case)
    };
    let start = |inputs: &mut Inputs, loads: &mut Loads| {
        if !parts.is_empty() {
            load_in_background(context, &parts, settings, inputs, loads);
        }
        // Each image shows as one that loads in the background does.
        if let Some(still) = still.take() {
            *loads = still;
            for key in loads.keys() {
                inputs.images.wait(key.clone(), Placeholder::Unsized);
            }
        }
    };
    let opening = (opening, start);
    if let Some(result) = page(context, content, name, inputs, settings, options, opening) {
        // What the pager would have hidden.
        (drawn.iter().flatten()).for_each(|drawn| tell_drawn(drawn, failed));
        return shown(result);
    }
    // Printed after all, so the images load first, and what they tell is
    // told at each document's place; the guard lives on as it does when
    // they loaded before.
    let (images_told, _interrupt) = match (parts.is_empty(), still.take()) {
        (false, _) => {
            let loaded = load_parts(context, &parts, settings, &inputs.caps)?;
            let told = told_files(&loaded.images, &parts, &files, show);
            inputs.images.extend(loaded.images);
            (told, loaded.interrupt)
        }
        // What the reader skipped comes first.
        (true, Some(still)) => {
            let came = finish_loading(still, &mut inputs.images, guard.as_ref())?;
            (
                told_files(&came, &parts_of(context.reading), &files, show),
                None,
            )
        }
        (true, None) => (vec![Vec::new(); files.len()], None),
    };
    plan::printing(inputs.reading.as_mut().map(|reading| &mut reading.wide));
    let mut out = kiri::term::output(&inputs.caps);
    let mut apart = false;
    // Whether kitty did not answer the check of a file's images.
    let silent = Cell::new(false);
    for (number, file) in files.iter().enumerate() {
        if !told_before.get() {
            tell_before(number);
        }
        for warning in &images_told[number] {
            eprintln!("kiri: {warning}");
        }
        if errors[number].is_some() {
            continue;
        }
        let rendered = match one {
            true => kiri::render::render(&file.document, inputs, &mut out),
            false => kiri::render::render_file(file, number, apart, inputs, &mut out),
        };
        shown(
            rendered.and_then(|sent| held(sent, &inputs.images, &inputs.caps, &silent, &mut out)),
        )?;
        apart = true;
        if let Some(drawn) = &drawn[number] {
            tell_drawn(drawn, failed);
        }
    }
    Ok(())
}

/// Prints `given`, each file under its header, as it comes: an image file
/// is read only then, so that no more than one is held at once. A file
/// alone has no header. A warning about a file is told before it, and why
/// the diagram of a diagram file was not drawn after it; a file that cannot
/// be shown is told about at its place and noted in `failed`.
fn print_files(show: &Show, given: Vec<Given>, inputs: &mut Inputs) -> io::Result<()> {
    let Show {
        context,
        settings,
        lone,
        failed,
    } = *show;
    plan::printing(inputs.reading.as_mut().map(|reading| &mut reading.wide));
    let mut out = kiri::term::output(&inputs.caps);
    let mut apart = false;
    // Whether kitty did not answer the check of a file's images.
    let silent = Cell::new(false);
    for (number, given) in given.into_iter().enumerate() {
        let name = given.name;
        let drawn = given.drawn;
        for warning in given.warnings.iter().filter(|_| !lone) {
            eprintln!("kiri: {}", warning.clone().in_file(&name));
        }
        let image = |image: Source, name: String, inputs: &mut Inputs, out: &mut _| match lone {
            true => print_alone(image, Page::default(), inputs, &silent, out),
            false => print_image(image, name, number, apart, inputs, &silent, out),
        };
        let shown = match given.shown {
            Shown::Failed(error) => {
                told(failed, error);
                continue;
            }
            Shown::Image { format, head, rest } => {
                let reader = image_reader(given.file.as_deref(), head, rest)
                    .map_err(|error| format!("{name}: {error}"));
                match reader.and_then(|reader| {
                    read_image_file(context, &inputs.caps, &given.path, format, reader)
                }) {
                    Ok(read) => image(read, name, inputs, &mut out),
                    Err(error) => {
                        told(failed, error);
                        continue;
                    }
                }
            }
            Shown::Read(read) => image(read, name, inputs, &mut out),
            // The diagram that was drawn, on the page of the theme.
            Shown::Document { .. } if lone && matches!(drawn, Some(Drawn::Done)) => {
                let key = ImageKey::from(name.as_str()).in_document(number);
                let drawn = inputs.images.remove(&key);
                let (background, foreground) = (inputs.theme.background, inputs.theme.foreground);
                let page = Page {
                    background,
                    foreground,
                };
                match drawn {
                    Some(Ok(image)) => print_alone(image, page, inputs, &silent, &mut out),
                    _ => Ok(()),
                }
            }
            Shown::Document { document, .. } => {
                let stderr = settings.render_errors.stderr();
                for warning in inputs.images.warnings_in(number, stderr) {
                    eprintln!("kiri: {}", told_line(lone, &name, warning));
                }
                let file = kiri::typeset::model::File { name, document };
                print_file(&file, number, (apart, lone), inputs, &silent, &mut out)
            }
            // What could not be drawn is told after it.
            Shown::Diagram { document, .. } => {
                let file = kiri::typeset::model::File { name, document };
                print_file(&file, number, (apart, lone), inputs, &silent, &mut out)
            }
            Shown::AsIs(text) => {
                let file = kiri::typeset::model::File {
                    name,
                    document: Document::default(),
                };
                match lone {
                    true => io::stdout().lock().write_all(text.as_bytes()),
                    false => kiri::render::render_file(&file, number, apart, inputs, &mut out)
                        .and_then(|_| out.write_all(text.as_bytes())),
                }
            }
        };
        apart = true;
        shown.and_then(|()| out.flush())?;
        if let Some(drawn) = &drawn {
            tell_drawn(drawn, failed);
        }
    }
    Ok(())
}

/// Prints `file`, numbered `number` among the files shown, under its
/// header, or without one when it is `lone`, after the blank line that
/// comes between two files when `apart`, and checks its images as [`held`]
/// does with `silent`.
fn print_file(
    file: &kiri::typeset::model::File,
    number: usize,
    (apart, lone): (bool, bool),
    inputs: &Inputs,
    silent: &Cell<bool>,
    out: &mut impl Write,
) -> io::Result<()> {
    let sent = match lone {
        true => kiri::render::render(&file.document, inputs, out),
        false => kiri::render::render_file(file, number, apart, inputs, out),
    }?;
    held(sent, &inputs.images, &inputs.caps, silent, out)
}

/// Prints `image`, a file alone, as large as fits the window and without a
/// header, on `page`, and checks it as [`held`] does with `silent`.
fn print_alone(
    image: Source,
    page: Page,
    inputs: &Inputs,
    silent: &Cell<bool>,
    out: &mut impl Write,
) -> io::Result<()> {
    kiri::render::render_image_on(image, &inputs.caps, page, out)
        .and_then(|(sent, images)| held(sent, &images, &inputs.caps, silent, out))
}

/// Prints `image`, the image file `name` numbered `number` among the files
/// shown, under its header, as `render_file` prints a file, with the
/// images `inputs` holds, and checks it as [`held`] does with `silent`.
fn print_image(
    image: Source,
    name: String,
    number: usize,
    apart: bool,
    inputs: &mut Inputs,
    silent: &Cell<bool>,
    out: &mut impl Write,
) -> io::Result<()> {
    let key = ImageKey::from(name.as_str()).in_document(number);
    inputs.images.insert(key.clone(), Ok(image));
    let shown = kiri::render::render_file(&image_file(name), number, apart, inputs, out)
        .and_then(|sent| held(sent, &inputs.images, &inputs.caps, silent, out));
    inputs.images.remove(&key);
    shown
}

/// Flushes `out` and makes sure kitty holds the images `encoder` sent
/// there, from `images`, to a terminal with `caps`. tmux drops output for
/// a client that cannot keep up, and the images in it with it, so through
/// tmux kiri asks kitty for them and sends again what it lost, a few
/// times at most. Without tmux, nothing is asked. Once kitty did not
/// answer at all, as from a pane tmux does not show, `silent` is set and
/// nothing more is asked, so that each file does not wait for it.
fn held(
    mut encoder: Encoder,
    images: &kiri::images::Images,
    caps: &Capabilities,
    silent: &Cell<bool>,
    out: &mut impl Write,
) -> io::Result<()> {
    out.flush()?;
    for _ in 0..kiri::present::encoder::AGAIN {
        let sent = encoder.take_sent();
        if sent.is_empty() || silent.get() {
            break;
        }
        let Some(lost) = kiri::term::lost_images(&sent) else {
            silent.set(true);
            break;
        };
        for id in lost {
            encoder.send_again(id, images, caps, out)?;
        }
        out.flush()?;
    }
    Ok(())
}

/// A file among several, ready to show.
struct Given {
    /// What its header calls it.
    name: String,
    /// What errors and warnings call it.
    path: PathBuf,
    /// The file, or `None` for standard input.
    file: Option<PathBuf>,
    shown: Shown,
    /// What is told about it before it is shown, besides its images. A
    /// file alone has told them already, and the pager tells them too.
    warnings: Vec<Diagnostic>,
    /// What is told after it is shown: why its diagram was not drawn, for
    /// a diagram file.
    drawn: Option<Drawn>,
    /// For a document alone read from a regular file: how it was read, so
    /// that the pager can read it again.
    reload: Option<(Input, Option<Look>)>,
}

/// What a file among several is shown as.
enum Shown {
    /// An image in `format`, read when it is shown, as `image_reader`
    /// reads it.
    Image {
        format: Format,
        head: Vec<u8>,
        rest: Option<Box<dyn Read>>,
    },
    /// An image file read whole.
    Read(Source),
    /// A document, with its images unless it is text.
    Document { document: Document, images: bool },
    /// A diagram file in `language`: `source`, and the document of its
    /// code, shown where it is not drawn.
    Diagram {
        language: &'static str,
        source: String,
        document: Document,
    },
    /// Text written as it is.
    AsIs(String),
    /// Nothing, as the file cannot be shown: why, told as an error.
    Failed(String),
}

impl Given {
    /// The file called `name` that is `shown`, with nothing told about it.
    fn new(name: String, path: PathBuf, shown: Shown) -> Self {
        Given {
            name,
            path,
            file: None,
            shown,
            warnings: Vec::new(),
            drawn: None,
            reload: None,
        }
    }

    /// The file at `path`, which cannot be shown for `error`.
    fn failed(path: PathBuf, error: String) -> Self {
        Self::new(path.display().to_string(), path, Shown::Failed(error))
    }

    /// Whether the file cannot be shown.
    fn is_failed(&self) -> bool {
        matches!(self.shown, Shown::Failed(_))
    }
}

/// What is told after a diagram file is shown.
enum Drawn {
    /// It was drawn: the file is its image, and there is nothing to tell.
    Done,
    /// It could not be drawn: why, told as an error.
    Failed(Diagnostic),
    /// It could not be drawn, and the document tells why.
    Told,
    /// Nothing could draw it.
    Hint(Diagnostic),
}

/// The images of the documents among `given`, loaded and drawn as
/// `load_images` loads them, keyed by the number of each among them. The
/// diagram of a diagram file is drawn also with images off, as it is on
/// its own, and text has none.
fn load_given(
    context: &Context,
    given: &[Given],
    settings: &Settings,
    caps: &Capabilities,
    documents: bool,
) -> Result<Loaded, Failure> {
    if !caps.draws_images() {
        return Ok(Loaded::default());
    }
    let parts: Vec<Part> = (given.iter().enumerate())
        .filter_map(|(number, given)| {
            let document = match &given.shown {
                Shown::Document { document, images } if *images && settings.images && documents => {
                    document
                }
                Shown::Diagram { document, .. } => document,
                _ => return None,
            };
            let file = given.file.as_deref();
            Some(Part {
                number,
                ..part(document, file, context.reading)
            })
        })
        .collect();
    load_parts(context, &parts, settings, caps)
}

/// The diagram file `given`, numbered `number` among the files shown,
/// made an image file where its diagram in `images` was drawn: the image
/// is then keyed by the file's name. Otherwise what is told after it is
/// shown, as for a diagram file on its own.
fn drawn(
    number: usize,
    given: &mut Given,
    images: &mut kiri::images::Images,
    settings: &Settings,
    caps: &Capabilities,
) -> Option<Drawn> {
    let Shown::Diagram {
        language, source, ..
    } = &given.shown
    else {
        return None;
    };
    let language = *language;
    let key = ImageKey::block(language, source).in_document(number);
    let path = given.path.display();
    match images.remove(&key) {
        Some(Ok(image)) => {
            let name = given.name.clone();
            images.insert(ImageKey::from(name.as_str()).in_document(number), Ok(image));
            given.shown = Shown::Document {
                document: Document::new(vec![Block::ImageFile(name)]),
                images: true,
            };
            Some(Drawn::Done)
        }
        Some(Err(error)) => {
            let (code, reason) = match &error {
                ImageError::Diagram { reason, .. } => (Code::Diagram, reason.clone()),
                error => (Code::Image, error.to_string()),
            };
            // Back in, so that the document tells it below the source.
            images.insert(key, Err(error));
            Some(match settings.render_errors.stderr() {
                true => Drawn::Failed(Diagnostic::new(code, reason).in_file(path.to_string())),
                false => Drawn::Told,
            })
        }
        // Not drawn, though it could have been shown: no renderer for it.
        None if settings.diagrams && caps.draws_images() => {
            let hint = format!(
                "no program to draw {} diagrams; see --doctor",
                kiri::diagrams::display_name(language)
            );
            Some(Drawn::Hint(
                Diagnostic::new(Code::NoDiagramProgram, hint).in_file(path.to_string()),
            ))
        }
        None => None,
    }
}

/// The image file `name` among several, as a file to render.
fn image_file(name: String) -> kiri::typeset::model::File {
    let document = Document::new(vec![Block::ImageFile(name.clone())]);
    kiri::typeset::model::File { name, document }
}

/// The README the directory `dir` shows: `README.md`, or else `README`.
fn readme(dir: &Path) -> Result<PathBuf, String> {
    ["README.md", "README"]
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| format!("{}: no README.md or README", dir.display()))
}

/// What a run of kiri takes from its command line and its environment
/// below `run`, resolved.
struct Context<'a> {
    /// The environment, which tells where to look for a program that
    /// draws diagrams, among others.
    env: &'a Environment,
    mode: kiri::term::Mode,
    /// The width in cells that `--width` asks for.
    width: Option<u16>,
    /// The largest image file to read, in bytes.
    max_file_size: u64,
    /// How the command line asks to follow the file, so that the pager
    /// says when there is nothing to follow.
    follow: Follow,
    /// Whether `--reading` asks for reading mode.
    reading: bool,
    /// What becomes of code blocks and tables wider than reading mode's
    /// area in the pager, as `--wide` says.
    wide: Wide,
}

impl<'a> Context<'a> {
    fn new(cli: &Cli, env: &'a Environment, settings: &Settings) -> Self {
        Context {
            env,
            mode: settings.mode,
            width: settings.width,
            max_file_size: settings.max_image_size.0 * kiri::images::MB,
            follow: follow(cli).map_or(Follow::Off, |follow| follow.value),
            reading: settings.reading,
            wide: settings.wide,
        }
    }

    /// Reading mode in the system's fonts, which load when text is first
    /// laid out in them, as `M` in the pager turns it on.
    fn reading_mode(&self) -> Reading {
        Reading {
            pictures: Default::default(),
            fonts: Rc::new(kiri::typeset::reading::Fonts::system()),
            wide: self.wide,
        }
    }
}

/// Shows `content` in the pager when the paging of `settings` asks for it
/// and the output is a terminal that takes escape sequences, with the
/// mouse taken if they say so; `None` when it is to be printed instead.
/// `options` tell the pager how a search treats case and where `r` gets
/// the document again from, if anywhere. The pager catches Ctrl-C itself;
/// of `opening`, `caught` runs once its screen catches the signals, and
/// then `start`, which may start loading images in the background.
fn page(
    context: &Context,
    content: kiri::typeset::layout::Content,
    name: &str,
    inputs: &mut Inputs,
    settings: &Settings,
    options: kiri::pager::Options,
    (caught, start): (impl FnOnce(), impl FnOnce(&mut Inputs, &mut Loads)),
) -> Option<io::Result<()>> {
    let paging = settings.paging;
    let terminal = kiri::term::stdout_is_terminal() && inputs.caps.styling;
    if !terminal || paging == Paging::Never {
        return None;
    }
    let stream = kiri::typeset::layout::content(content, inputs);
    let rows = kiri::pager::rows(&stream.lines, &inputs.caps);
    let height = kiri::term::window_size(io::stdout()).map(|window| window.rows);
    if !plan::pages(paging, terminal, rows, height) {
        return None;
    }
    // The screen catches the signals before the terminal is raw; the
    // guard, which would otherwise end kiri with the terminal raw, goes
    // then.
    let Some(mut screen) =
        kiri::term::Screen::open(caught, inputs.caps.tmux_passthrough, settings.mouse)
    else {
        // Without a terminal to read keys from, as in the background, the
        // document is printed, and the signals still end kiri; only a
        // reader who asked for the pager is told.
        if paging == Paging::Always {
            eprintln!("kiri: no terminal to read keys from, so the document is printed");
        }
        return None;
    };
    // The images that still load when the pager quits, which it stops;
    // their programs may take a moment to end.
    let mut loads = Loads::new();
    start(inputs, &mut loads);
    // The layout before knew nothing of the images that load.
    let laid_out = (!inputs.images.loading()).then_some(stream);
    let result = kiri::pager::run(
        content,
        laid_out,
        name,
        inputs,
        kiri::pager::Options {
            width: context.width,
            loads: Some(&mut loads),
            ..options
        },
        &mut screen,
    );
    loads.halt();
    screen.close_then(|| close_programs(&loads));
    // What the pager showed failed, as told before it opens when the
    // images load first: under each file's name among several.
    let stderr = inputs.render_errors.stderr();
    for (number, warning) in inputs.images.late_warnings(stderr) {
        let warning = match content {
            kiri::typeset::layout::Content::Files(files) => warning.in_file(&files[number].name),
            kiri::typeset::layout::Content::Document(_) => warning,
        };
        eprintln!("kiri: {warning}");
    }
    Some(result)
}

/// Writes the `--doctor` report: rendered for a terminal, as Markdown
/// otherwise or with `--raw`.
/// Problems with the configuration file and the theme are part of the
/// report instead of errors, and kiri goes on without them.
fn doctor(cli: &Cli) -> Result<(), Failure> {
    let env = Environment::from_process();
    let (config, config_error) = match kiri::config::load(&env) {
        Ok(config) => (config, None),
        Err(error) => (Default::default(), Some(error)),
    };
    let choices = kiri::config::choose(&options(cli), &env, &config);
    let (theme, theme_error) = match kiri::config::load_theme(&choices.theme.value) {
        Ok(theme) => (theme, None),
        Err(error) => (Theme::default(), Some(error)),
    };
    let passthrough = choices.passthrough.value;
    let detection = kiri::term::examine(choices.mode.value, choices.width.value, &env, passthrough);
    let output = match (cli.raw, kiri::term::stdout_is_terminal()) {
        (true, _) => Output::Raw,
        (false, true) => Output::Rendered,
        (false, false) => Output::Markdown,
    };
    let markdown = kiri::doctor::report(&Facts {
        env: &env,
        detection: &detection,
        choices: &choices,
        theme: &theme,
        config_error,
        theme_error,
        output,
    });
    if output != Output::Rendered {
        let mut out = io::stdout().lock();
        return shown(
            out.write_all(markdown.as_bytes())
                .and_then(|()| out.flush()),
        );
    }
    let document = kiri::typeset::parse::parse(&markdown);
    let mut settings = choices.with_theme(theme);
    settings.paging = plan::only_always(settings.paging);
    show_report(
        cli,
        &env,
        "kiri --doctor",
        document,
        settings,
        detection.caps,
    )
}

/// Writes the emoji shortcodes: a rendered document for a terminal, one
/// line per emoji otherwise.
fn list_emojis(cli: &Cli) -> Result<(), Failure> {
    if !kiri::term::stdout_is_terminal() {
        let mut out = io::stdout().lock();
        let lines = kiri::list::emoji_lines();
        return shown(out.write_all(lines.as_bytes()).and_then(|()| out.flush()));
    }
    let env = Environment::from_process();
    let (warnings, settings) = settings(cli, &env);
    (warnings.iter()).for_each(|warning| eprintln!("kiri: {warning}"));
    let settings = settings?;
    let caps = kiri::term::capabilities(settings.mode, settings.width, &env, settings.passthrough);
    let document = kiri::typeset::parse::parse(&kiri::list::emoji_document());
    show_report(cli, &env, "kiri --list-emojis", document, settings, caps)
}
