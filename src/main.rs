//! The kiri binary: the platform's entry point. It owns the command line,
//! opening files and standard input, the print and interactive shells,
//! and starting the image loads and the pager with the settings and the
//! terminal's capabilities. It wires the parts of the library together and
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

use clap::builder::{PossibleValue, PossibleValuesParser, TypedValueParser};
use clap::error::ErrorKind;
use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser};
use kiri::config::{
    Environment, FrontMatter, Named, Options, Origin, Passthrough, Setting, Settings,
};
use kiri::doctor::{Facts, Output};
use kiri::images::{
    Fonts, Format, ImageError, ImageKey, Images, Interrupted, Loaded, Loading, Loads, Part,
    Placeholder, Source, Stamps, reload_in_background,
};
use kiri::pager::{Change, Look, Reload, Reloaded, Warning};
use kiri::present::encoder::Encoder;
use kiri::present::output::Page;
use kiri::render::{Headings, Inputs, RenderErrors, TableRows};
use kiri::session::plan::{self, Load, Route};
use kiri::session::settings::{Follow, FollowJump, InitialLoad, Paging};
use kiri::term::{Interrupt, Mode};
use kiri::typeset::caps::Capabilities;
use kiri::typeset::input::Input;
use kiri::typeset::model::{Block, Document, FrontMatterError};
use kiri::typeset::reading::engine::Reading;
use kiri::typeset::reading::page::Wide;
use kiri::typeset::theme::{Paper, Theme};

/// Render a Markdown document, or show an image, in the terminal.
#[derive(Parser)]
#[command(
    version,
    // Longer lines are hard to read, also in a wide terminal.
    max_term_width = 100,
    after_help = "Images and diagrams are drawn only in terminals that support \
                  kitty's graphics protocol with Unicode placeholders, such as \
                  kitty 0.28 or later and Ghostty 1.0 or later; elsewhere images \
                  show their alt text and diagrams their code."
)]
struct Cli {
    /// Markdown, CSV or TSV, an image, a Mermaid diagram, or a directory
    ///
    /// A Markdown file is rendered, a CSV or TSV file shown as a table, an
    /// image or a Mermaid diagram (.mmd) shown on its own, and a directory
    /// shows its README.
    /// Several files are shown one after another, each under a header
    /// that names it; one that cannot be shown is reported, and the others
    /// are still shown.
    /// Without one, or with -, kiri reads standard input unless it is a
    /// terminal; - can be given only once.
    #[arg(value_name = "FILE")]
    files: Vec<PathBuf>,

    /// Show what kiri finds out about the terminal, for a bug report
    ///
    /// The report also tells which settings kiri uses. It is Markdown when
    /// written to a file.
    #[arg(long, conflicts_with_all = ["files", "format"])]
    doctor: bool,

    /// List the emoji shortcodes, such as :smile:
    ///
    /// A terminal shows them as a table; a pipe or a file gets one line per
    /// emoji.
    #[arg(long, conflicts_with_all = ["files", "format", "doctor"])]
    list_emojis: bool,

    /// With --doctor: the report as Markdown, also in a terminal
    ///
    /// The report then leaves out its sample, so it can be copied into a
    /// bug report.
    #[arg(long)]
    raw: bool,

    /// What the file is, such as markdown, csv or png
    ///
    /// The formats are markdown, csv, tsv, mermaid (a diagram shown on its
    /// own), text (the file as it is, without rendering it), image, or an
    /// image format such as svg, png or jpg. By default its extension and
    /// content tell.
    #[arg(long, value_parser = format)]
    format: Option<Input>,

    /// Width in cells; defaults to the terminal width.
    #[arg(long)]
    width: Option<u16>,

    /// Which terminal features to use
    ///
    /// Colours stay off when NO_COLOR is set.
    #[arg(long, value_parser = named_with_help(mode_help), default_value = Mode::Auto.name())]
    mode: Mode,

    /// Theme file
    ///
    /// It overrides --light, --dark, KIRI_THEME and the configuration file.
    #[arg(long)]
    theme: Option<PathBuf>,

    /// A light page in greys, whatever the terminal's colours
    ///
    /// Code gets colours for a light background.
    #[arg(long, conflicts_with = "dark")]
    light: bool,

    /// A dark page in greys, whatever the terminal's colours
    ///
    /// Code gets colours for a dark background.
    #[arg(long)]
    dark: bool,

    /// No margins, indents or padding, for copying text
    ///
    /// Leaves out the margins, heading indents and code block padding.
    #[arg(short = 'M', long)]
    no_margins: bool,

    /// Show images as their alt text and diagrams as code
    ///
    /// Images are not read. An image or diagram file given on its own is
    /// still shown.
    #[arg(short = 'I', long)]
    no_images: bool,

    /// Show images, whatever KIRI_NO_IMAGES and the configuration say
    ///
    /// Diagrams are shown too, unless they are off.
    #[arg(long, conflicts_with = "no_images")]
    images: bool,

    /// When to show the document in the pager
    ///
    /// With auto, the pager opens when the document is longer than the
    /// window and the output is a terminal.
    #[arg(long, value_parser = named::<Paging>(), value_name = "WHEN", conflicts_with_all = ["pager", "no_pager"])]
    paging: Option<Paging>,

    /// Show the document in the pager: --paging=always.
    #[arg(short = 'p', conflicts_with = "no_pager")]
    pager: bool,

    /// Print the document without the pager: --paging=never.
    #[arg(short = 'P')]
    no_pager: bool,

    /// Follow a lone file in the pager
    ///
    /// With on, kiri reads the file again whenever it changes, with the view
    /// kept where it is; with auto it also moves the view to a change; off
    /// unless set. Following pages the file, however short, unless paging is
    /// never. F goes from off to on to auto in the pager.
    #[arg(long, value_parser = named::<Follow>(), value_name = "WHEN", conflicts_with_all = ["follow_on", "follow_auto", "follow_off"])]
    follow: Option<Follow>,

    /// Follow the file, in the pager: --follow=on.
    #[arg(short = 'f', conflicts_with_all = ["follow_auto", "follow_off"])]
    follow_on: bool,

    /// Follow the file and move to a change: --follow=auto.
    #[arg(long = "fa", conflicts_with = "follow_off")]
    follow_auto: bool,

    /// Do not follow the file: --follow=off.
    #[arg(short = 'F')]
    follow_off: bool,

    /// Which change auto-follow moves the view to
    ///
    /// When a reload changed the document in more than one place: first,
    /// last or most (the one that takes the most rows); first unless set.
    #[arg(long, value_parser = named::<FollowJump>(), value_name = "WHICH")]
    follow_jump: Option<FollowJump>,

    /// Whether the pager waits for images before it shows the document
    ///
    /// With wait, the document is shown once its images and diagrams have
    /// loaded; with background, at once, with each image as it comes; wait
    /// unless set.
    /// Only where the pager surely opens: with -p or -f, or with paging on
    /// auto for a document longer than the window, each image counted at
    /// three rows; otherwise kiri waits.
    #[arg(long, value_parser = named::<InitialLoad>(), value_name = "HOW")]
    initial_load: Option<InitialLoad>,

    /// Whether kitty's images pass through tmux
    ///
    /// With detect, kiri wraps them for tmux when it runs inside tmux and
    /// turns on the pane's allow-passthrough, as `kitten icat` does; with
    /// tmux, it wraps them always; with none, never; detect unless set.
    #[arg(long, value_parser = named::<Passthrough>(), value_name = "HOW")]
    passthrough: Option<Passthrough>,

    /// How to show the front matter
    ///
    /// Front matter is the metadata block at the start of a Markdown file.
    /// The styles are table, code (a code block), hide, or off (read as
    /// Markdown); table unless set.
    #[arg(long, value_parser = named::<FrontMatter>(), value_name = "STYLE")]
    front_matter: Option<FrontMatter>,

    /// How headings show
    ///
    /// auto draws them large with text sizing, or as images where the
    /// terminal has no text sizing; sized only with text sizing; images
    /// always where images can be drawn; plain in one row; auto unless
    /// set.
    #[arg(long, value_parser = named::<Headings>(), value_name = "HOW")]
    headings: Option<Headings>,

    /// Table row style
    ///
    /// The styles are zebra (every second row shaded where the terminal's
    /// colours are known), lines, indent (the lines after the first of a
    /// wrapped value start two cells in), underline (each row's last line
    /// underlined), or off; zebra unless set.
    #[arg(long, value_parser = named::<TableRows>(), value_name = "STYLE")]
    table_rows: Option<TableRows>,

    /// Whether to draw diagrams and formulas
    ///
    /// Diagrams in code blocks, such as Mermaid, and formulas: on (drawn as
    /// images) or off (shown as code); on unless set.
    #[arg(long, value_parser = named::<bool>(), value_name = "SWITCH")]
    diagrams: Option<bool>,

    /// Where to tell why a diagram could not be drawn
    ///
    /// The places are inline (in the document, below its code), stderr (a
    /// warning) or both; both unless set.
    #[arg(long, value_parser = named::<RenderErrors>(), value_name = "WHERE")]
    render_errors: Option<RenderErrors>,

    /// Leave the mouse to the terminal in the pager
    ///
    /// The pager takes the mouse unless set, so that the wheel scrolls it,
    /// also inside tmux. While it has the mouse, kitty selects text with
    /// Shift and the mouse.
    #[arg(long)]
    no_mouse: bool,

    /// Let the pager take the mouse
    ///
    /// Whatever KIRI_MOUSE and the configuration say.
    #[arg(long, conflicts_with = "no_mouse")]
    mouse: bool,

    /// Experimental: the text in a proportional font, as a browser shows it
    ///
    /// Headings, paragraphs, lists and quotes are drawn as images, so their
    /// text cannot be selected or copied. It needs a terminal that draws
    /// images, such as kitty; elsewhere the document shows as text. The
    /// option's name may still change.
    #[arg(long)]
    reading: bool,

    /// Experimental: wide code and tables wrap or scroll in reading mode
    ///
    /// With wrap, a code block or table wider than reading mode's area
    /// wraps to fit it; with scroll, it keeps its lines and words whole and
    /// scrolls sideways in the pager, where W switches between the two.
    /// Printing always wraps. wrap unless set. The option's name may still
    /// change.
    #[arg(long, value_name = "HOW", value_parser = ["wrap", "scroll"])]
    wide: Option<String>,

    /// Largest image file to read, in megabytes
    ///
    /// At most 100; larger images show their alt text.
    #[arg(long, value_name = "MB", default_value_t = 10, value_parser = max_image_size)]
    max_image_size: u64,
}

/// The `--max-image-size` value: whole megabytes, from 1 to 100.
fn max_image_size(value: &str) -> Result<u64, String> {
    match value.parse() {
        Ok(megabytes @ 1..=100) => Ok(megabytes),
        _ => Err("give a whole number of MB from 1 to 100".to_owned()),
    }
}

/// The `--format` value.
fn format(value: &str) -> Result<Input, String> {
    kiri::typeset::input::format(value).ok_or_else(|| {
        "give markdown, csv, tsv, mermaid, text, image, or an image format such as svg, png \
         or jpg"
            .to_owned()
    })
}

/// A parser for the values of `T`, by their names.
fn named<T: Named + Send + Sync>() -> impl TypedValueParser<Value = T> {
    named_with_help(|_| None)
}

/// A parser for the values of `T`, by their names, with the help on each
/// value that `help` gives.
fn named_with_help<T: Named + Send + Sync>(
    help: fn(T) -> Option<&'static str>,
) -> impl TypedValueParser<Value = T> {
    let values = (T::ALL.iter()).map(|&value| PossibleValue::new(value.name()).help(help(value)));
    PossibleValuesParser::new(values).map(|name| T::named(&name).expect("a value's name"))
}

/// What `--help` tells of each mode.
fn mode_help(mode: Mode) -> Option<&'static str> {
    Some(match mode {
        Mode::Auto => "Ask the terminal; plain text in a pipe",
        Mode::Plain => "Text only, no escape sequences",
        Mode::Ansi => "Colours and attributes, no kitty extensions",
        Mode::Kitty => "All kitty features, without asking the terminal",
    })
}

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
        doctor(&cli, &matches)
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
    let context = Context::new(cli, &env);
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
fn settings(cli: &Cli, env: &Environment) -> (Vec<String>, Result<Settings, String>) {
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
    warn: impl FnOnce(&FrontMatterError),
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
                warn(error);
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
    let warn = |warning: &FrontMatterError| {
        // On standard error at once for a file alone, and in the pager too.
        if lone {
            eprintln!("kiri: {warning}");
        }
        warnings.push(warning.clone());
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
    let context = Context::new(cli, env);
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
    reading_warning: Option<String>,
    /// Where the pager reads the document again from.
    reload: Option<FileReload<'a>>,
}

/// `warning` about the file `name`, as told: named among several files.
fn told_line(lone: bool, name: &str, warning: &str) -> String {
    match lone {
        true => warning.to_owned(),
        false => format!("{name}: {warning}"),
    }
}

/// The warnings about `images` of `parts`, documents among `files`, under
/// each file's name, by the file's number among `files`.
fn told_files(
    images: &Images,
    parts: &[Part],
    files: &[kiri::typeset::model::File],
    show: &Show,
) -> Vec<Vec<String>> {
    let stderr = show.settings.render_errors.stderr();
    let mut told = vec![Vec::new(); files.len()];
    for part in parts {
        let name = &files[part.number].name;
        told[part.number] = (images.warnings_in(part.number, stderr).iter())
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
        Drawn::Failed(error) => told(failed, error.clone()),
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
                true => warned_of.push(Warning::from(warning)),
                false => warned.push(format!("{name}: {warning}")),
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
                    warned.push(told_line(lone, &name, &warning));
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
        warnings: (warned_of.into_iter())
            .chain(reading_warning.map(|message| Warning {
                line: None,
                message,
            }))
            .collect(),
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
            eprintln!("kiri: {name}: {warning}");
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
                    eprintln!("kiri: {}", told_line(lone, &name, &warning));
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
    warnings: Vec<FrontMatterError>,
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
    Failed(String),
    /// It could not be drawn, and the document tells why.
    Told,
    /// Nothing could draw it.
    Hint(String),
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
            let reason = match &error {
                ImageError::Diagram { reason, .. } => reason.clone(),
                error => error.to_string(),
            };
            // Back in, so that the document tells it below the source.
            images.insert(key, Err(error));
            Some(match settings.render_errors.stderr() {
                true => Drawn::Failed(format!("{path}: {reason}")),
                false => Drawn::Told,
            })
        }
        // Not drawn, though it could have been shown: no renderer for it.
        None if settings.diagrams && caps.draws_images() => Some(Drawn::Hint(format!(
            "{path}: no program to draw {} diagrams; see --doctor",
            kiri::diagrams::display_name(language)
        ))),
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
    fn new(cli: &Cli, env: &'a Environment) -> Self {
        Context {
            env,
            mode: cli.mode,
            width: cli.width,
            max_file_size: cli.max_image_size * kiri::images::MB,
            follow: follow(cli).map_or(Follow::Off, |follow| follow.value),
            reading: cli.reading,
            wide: match cli.wide.as_deref() {
                Some("scroll") => Wide::Scroll,
                _ => Wide::Wrap,
            },
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

/// The rows an image or diagram that has not loaded yet counts as, to
/// tell whether a document is longer than the window before it loads.
const ROWS_UNLOADED: u16 = 3;

/// Whether `content`, whose `parts` are to load their images in the
/// background, surely goes to the pager with `settings`: always with
/// paging `always`; with paging on `auto`, when it is longer than the
/// window laid out with `inputs` and each image of `parts` counted at
/// [`ROWS_UNLOADED`] rows, which then stand in for them in `inputs`
/// until they start loading.
fn surely_paged(
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
struct Waited {
    /// The guard that caught the signals meanwhile, to go once the
    /// pager's screen catches them.
    interrupt: Option<Interrupt>,
    /// The loads still under way, when the reader opened the pager
    /// before all had come.
    loads: Option<Loads>,
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
fn wait_or_skip(
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
fn finish_loading(
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
fn load_in_background(
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
fn load_parts(
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
fn loading<'a>(context: &'a Context, settings: &'a Settings, caps: &Capabilities) -> Loading<'a> {
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
fn part<'a>(document: &'a Document, file: Option<&'a Path>, inline_images: bool) -> Part<'a> {
    Part {
        document,
        number: 0,
        base: file.and_then(Path::parent).unwrap_or(Path::new("")),
        by_line: file.is_some(),
        inline_images,
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
        match content {
            kiri::typeset::layout::Content::Files(files) => {
                eprintln!("kiri: {}: {warning}", files[number].name)
            }
            kiri::typeset::layout::Content::Document(_) => eprintln!("kiri: {warning}"),
        }
    }
    Some(result)
}

/// Waits for the programs that drew `loads`, stopped, to end, with a line
/// on standard error, a terminal, while that takes more than a second.
fn close_programs(loads: &Loads) {
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

/// Reads the document from its file again for the pager, and loads the
/// images it does not have yet.
struct FileReload<'a> {
    context: &'a Context<'a>,
    settings: &'a Settings,
    /// The regular file the document was read from, as `input`.
    path: PathBuf,
    input: Input,
    /// When the files behind the images were last changed.
    stamps: Stamps,
    /// How the file looked when it was last read, when it could be
    /// looked at.
    read: Option<Look>,
}

impl Reload for FileReload<'_> {
    fn reload(
        &mut self,
        images: &mut Images,
        loads: &mut Loads,
        caps: &Capabilities,
        reading: bool,
    ) -> Reloaded {
        let (path, input) = (self.path.as_path(), self.input);
        // Before the file is read, so that a change meanwhile counts.
        self.read = look(path).ok();
        // The status line names the file already, so the reasons do not.
        let source = match read_text(path) {
            Ok(source) => source,
            Err(error) => return Reloaded::Failed(error),
        };
        let mut warnings = Vec::new();
        let warn = |error: &FrontMatterError| warnings.push(Warning::from(error));
        let document = match document(input, &source, path, self.settings, caps, warn) {
            Ok(Some(document)) => document,
            Ok(None) => return Reloaded::Failed("cannot be shown".to_owned()),
            Err(error) => return Reloaded::Failed(without_path(error, path)),
        };
        if self.settings.images && caps.draws_images() {
            let parts = [part(&document, Some(path), reading)];
            let loading = loading(self.context, self.settings, caps);
            reload_in_background(&parts, &loading, images, &mut self.stamps, loads);
        }
        match warnings.is_empty() {
            true => Reloaded::Document(document),
            false => Reloaded::Warned(document, warnings),
        }
    }

    fn change(&mut self) -> Change {
        match look(&self.path) {
            Ok(look) if Some(look) == self.read => Change::Unchanged,
            Ok(look) => Change::Changed(look),
            Err(why) => Change::Gone(why),
        }
    }
}

/// How the file at `path` looks now, looked up by its path, so that a file
/// replaced by another, as an editor saves through a rename, counts as
/// changed; why it cannot be looked at otherwise.
fn look(path: &Path) -> Result<Look, String> {
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    Ok(Look {
        modified: metadata.modified().ok(),
        length: metadata.len(),
    })
}

/// The text of the file at `path`, read whole; why it cannot be read
/// otherwise.
fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_owned())
}

/// `error` without the `path: ` it starts with, if it does.
fn without_path(error: String, path: &Path) -> String {
    let prefix = format!("{}: ", path.display());
    match error.strip_prefix(&prefix) {
        Some(reason) => reason.to_owned(),
        None => error,
    }
}

/// The image file `path`, in `format`, read from `reader` up to the size
/// limit, for a terminal with `caps`, which must draw images.
fn read_image_file(
    context: &Context,
    caps: &Capabilities,
    path: &Path,
    format: Format,
    reader: impl Read,
) -> Result<Source, String> {
    if !caps.draws_images() {
        return Err(format!(
            "{}: the terminal cannot show images",
            path.display()
        ));
    }
    let max_file_size = context.max_file_size;
    kiri::images::read_image(reader, path, format, max_file_size, &Fonts::system()).map_err(
        |error| match error {
            error if error.names_file() => error.to_string(),
            error => format!("{}: {error}", path.display()),
        },
    )
}

/// Writes the `--doctor` report: rendered for a terminal, as Markdown
/// otherwise or with `--raw`.
/// Problems with the configuration file and the theme are part of the
/// report instead of errors, and kiri goes on without them.
fn doctor(cli: &Cli, matches: &ArgMatches) -> Result<(), Failure> {
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
    let detection = kiri::term::examine(cli.mode, cli.width, &env, passthrough);
    let output = match (cli.raw, kiri::term::stdout_is_terminal()) {
        (true, _) => Output::Raw,
        (false, true) => Output::Rendered,
        (false, false) => Output::Markdown,
    };
    let markdown = kiri::doctor::report(&Facts {
        env: &env,
        mode: or_default(matches, "mode", "--mode", cli.mode),
        width: flag(cli.width, "--width"),
        max_image_size: or_default(
            matches,
            "max_image_size",
            "--max-image-size",
            cli.max_image_size,
        ),
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
    for warning in &warnings {
        eprintln!("kiri: {warning}");
    }
    let settings = settings?;
    let caps = kiri::term::capabilities(cli.mode, cli.width, &env, settings.passthrough);
    let document = kiri::typeset::parse::parse(&kiri::list::emoji_document());
    show_report(cli, &env, "kiri --list-emojis", document, settings, caps)
}

/// What the command line asks for, each with the flag it comes from.
fn options(cli: &Cli) -> Options {
    Options {
        theme: flag(cli.theme.clone(), "--theme"),
        paper: (flag(cli.light.then_some(Paper::Light), "--light"))
            .or(flag(cli.dark.then_some(Paper::Dark), "--dark")),
        margins: flag(cli.no_margins.then_some(false), "--no-margins"),
        images: (flag(cli.images.then_some(true), "--images"))
            .or(flag(cli.no_images.then_some(false), "--no-images")),
        paging: (flag(cli.paging, "--paging"))
            .or(flag(cli.pager.then_some(Paging::Always), "-p"))
            .or(flag(cli.no_pager.then_some(Paging::Never), "-P")),
        follow: follow(cli),
        follow_jump: flag(cli.follow_jump, "--follow-jump"),
        initial_load: flag(cli.initial_load, "--initial-load"),
        passthrough: flag(cli.passthrough, "--passthrough"),
        front_matter: flag(cli.front_matter, "--front-matter"),
        headings: flag(cli.headings, "--headings"),
        table_rows: flag(cli.table_rows, "--table-rows"),
        diagrams: flag(cli.diagrams, "--diagrams"),
        render_errors: flag(cli.render_errors, "--render-errors"),
        mouse: (flag(cli.mouse.then_some(true), "--mouse"))
            .or(flag(cli.no_mouse.then_some(false), "--no-mouse")),
        ..Options::default()
    }
}

/// How the command line asks to follow the file, with the flag it comes
/// from.
fn follow(cli: &Cli) -> Option<Setting<Follow>> {
    (flag(cli.follow, "--follow"))
        .or(flag(cli.follow_on.then_some(Follow::On), "-f"))
        .or(flag(cli.follow_auto.then_some(Follow::Auto), "--fa"))
        .or(flag(cli.follow_off.then_some(Follow::Off), "-F"))
}

/// `value`, when the command line gives it, from the flag `name`: the
/// flag's long name, or its short one where it has no long one, as clap
/// does not tell which of them the user gave.
fn flag<T>(value: Option<T>, name: &'static str) -> Option<Setting<T>> {
    value.map(|value| Setting::new(value, Origin::Flag(name)))
}

/// `value` of the argument `id`: from the flag `name`, else clap's default.
fn or_default<T>(matches: &ArgMatches, id: &str, name: &'static str, value: T) -> Setting<T> {
    let given = matches.value_source(id) == Some(ValueSource::CommandLine);
    match given {
        true => Setting::new(value, Origin::Flag(name)),
        false => Setting::new(value, Origin::Default),
    }
}
