use std::cell::Cell;
use std::fs::File;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::builder::{PossibleValue, PossibleValuesParser, TypedValueParser};
use clap::error::ErrorKind;
use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser};
use kiri::caps::Capabilities;
use kiri::config::{Environment, FrontMatter, Named, Options, Origin, Paging, Setting, Settings};
use kiri::doctor::{Facts, Output};
use kiri::images::{
    Fonts, Format, ImageError, ImageKey, Images, Interrupted, Loaded, Loading, Part, Source,
    Stamps, reload_documents,
};
use kiri::input::Input;
use kiri::model::{Block, Document};
use kiri::pager::{Change, Look, Reload, Reloaded};
use kiri::render::output::Page;
use kiri::render::{Inputs, RenderErrors, TableRows};
use kiri::term::Mode;
use kiri::theme::{Paper, Theme};

/// Render a Markdown document, or show an image, in the terminal.
#[derive(Parser)]
#[command(
    version,
    // Longer lines are hard to read, also in a wide terminal.
    max_term_width = 100,
    after_help = "Images and diagrams are drawn only in terminals that support \
                  kitty's graphics protocol with Unicode placeholders, such as \
                  kitty 0.28 or later; elsewhere images show their alt text \
                  and diagrams their code."
)]
struct Cli {
    /// Markdown file to render, a CSV or TSV file to show as a table, an
    /// image or a Mermaid diagram (.mmd) to show on its own, or a directory
    /// to show its README.
    /// Several files are shown one after another, each under a header
    /// that names it; one that cannot be shown is reported, and the others
    /// are still shown.
    /// Without one, or with -, kiri reads standard input unless it is a
    /// terminal; - can be given only once.
    #[arg(value_name = "FILE")]
    files: Vec<PathBuf>,

    /// Show what kiri finds out about the terminal and which settings it
    /// uses, as Markdown when written to a file, for a bug report.
    #[arg(long, conflicts_with_all = ["files", "format"])]
    doctor: bool,

    /// List the emoji shortcodes, such as :smile:. A terminal shows them
    /// as a table; a pipe or a file gets one line per emoji.
    #[arg(long, conflicts_with_all = ["files", "format", "doctor"])]
    list_emojis: bool,

    /// With --doctor: the report as Markdown also in a terminal, without
    /// its sample, to copy into a bug report.
    #[arg(long)]
    raw: bool,

    /// What the file is: markdown, csv, tsv, mermaid (a diagram shown on
    /// its own), text (the file as it is, without rendering it), image, or
    /// an image format such as svg, png or jpg. By default its extension and
    /// content tell.
    #[arg(long, value_parser = format)]
    format: Option<Input>,

    /// Width in cells; defaults to the terminal width.
    #[arg(long)]
    width: Option<u16>,

    /// Which terminal features to use. Colours stay off when NO_COLOR is set.
    #[arg(long, value_parser = named_with_help(mode_help), default_value = Mode::Auto.name())]
    mode: Mode,

    /// Theme file; overrides --light, --dark, KIRI_THEME and the
    /// configuration file.
    #[arg(long)]
    theme: Option<PathBuf>,

    /// A light page in greys, whatever the terminal's colours; code colours
    /// for a light background.
    #[arg(long, conflicts_with = "dark")]
    light: bool,

    /// A dark page in greys, whatever the terminal's colours; code colours
    /// for a dark background.
    #[arg(long)]
    dark: bool,

    /// No margins, heading indents or code block padding, for copying text.
    #[arg(short = 'M', long)]
    no_margins: bool,

    /// Show images as their alt text, without reading them, and diagrams
    /// as code. An image or diagram file given on its own is still shown.
    #[arg(short = 'I', long)]
    no_images: bool,

    /// Show images, and diagrams unless they are off, whatever
    /// KIRI_NO_IMAGES and the configuration file say.
    #[arg(long, conflicts_with = "no_images")]
    images: bool,

    /// When to show the document in the pager: always, auto (when it is
    /// longer than the window and the output is a terminal) or never.
    #[arg(long, value_parser = named::<Paging>(), value_name = "WHEN", conflicts_with_all = ["pager", "no_pager"])]
    paging: Option<Paging>,

    /// Show the document in the pager: --paging=always.
    #[arg(short = 'p', conflicts_with = "no_pager")]
    pager: bool,

    /// Print the document without the pager: --paging=never.
    #[arg(short = 'P')]
    no_pager: bool,

    /// Follow a lone file in the pager: on (read it again whenever it
    /// changes, with the view kept where it is) or off; off unless set.
    /// F turns it on and off in the pager.
    #[arg(long, value_parser = named::<bool>(), value_name = "SWITCH", conflicts_with_all = ["follow_on", "follow_off"])]
    follow: Option<bool>,

    /// Follow the file: --follow=on.
    #[arg(short = 'f', conflicts_with = "follow_off")]
    follow_on: bool,

    /// Do not follow the file: --follow=off.
    #[arg(short = 'F')]
    follow_off: bool,

    /// Front matter render style: table, code (a code block), hide, or off
    /// (read as Markdown); table unless set. Front matter is the metadata
    /// block at the start of a Markdown file.
    #[arg(long, value_parser = named::<FrontMatter>(), value_name = "STYLE")]
    front_matter: Option<FrontMatter>,

    /// Table row style: zebra (every second row shaded where the
    /// terminal's colours are known), lines, indent (the lines after the
    /// first of a wrapped value start two cells in), underline (each row's
    /// last line underlined), or off; zebra unless set.
    #[arg(long, value_parser = named::<TableRows>(), value_name = "STYLE")]
    table_rows: Option<TableRows>,

    /// Diagrams in code blocks, such as Mermaid: on (drawn as images) or
    /// off (shown as code); on unless set.
    #[arg(long, value_parser = named::<bool>(), value_name = "SWITCH")]
    diagrams: Option<bool>,

    /// Where to tell why a diagram could not be drawn: inline (in the
    /// document, below its code), stderr (a warning) or both; both unless
    /// set.
    #[arg(long, value_parser = named::<RenderErrors>(), value_name = "WHERE")]
    render_errors: Option<RenderErrors>,

    /// Largest image file to read, in megabytes (at most 100); larger
    /// images show their alt text.
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
    kiri::input::format(value).ok_or_else(|| {
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
        let stdin = Path::new("-");
        match cli.files.as_slice() {
            [path] if path != stdin => run(&cli, Some(path)),
            [_] => run(&cli, None),
            [] if !kiri::term::stdin_is_terminal() => run(&cli, None),
            // clap cannot require the file only when stdin is a terminal,
            // so this is its own error for a missing argument.
            [] => command
                .error(
                    ErrorKind::MissingRequiredArgument,
                    "the following required arguments were not provided:\n  <FILE>...",
                )
                .exit(),
            paths if paths.iter().filter(|path| *path == stdin).count() > 1 => command
                .error(
                    ErrorKind::ArgumentConflict,
                    "- (standard input) can be given only once",
                )
                .exit(),
            paths => run_files(&cli, paths),
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

/// Shows the file at `path`, the README of a directory there, or standard
/// input without one.
fn run(cli: &Cli, file: Option<&Path>) -> Result<(), Failure> {
    let Opened {
        path,
        file,
        look,
        input,
        content,
    } = open(cli, file)?;
    let env = Environment::from_process();
    let context = Context::new(cli, &env);
    let source = match content {
        Content::Image { format, head, rest } => {
            let caps = kiri::term::capabilities(context.mode, context.width, &env);
            let (warnings, settings) = settings(cli, &env);
            let documents = settings.is_ok();
            // As for several image files, a broken configuration file does
            // not stop the image.
            let settings = settings.unwrap_or_else(|_| without_config(cli, &env));
            if pages_alone(&settings, &caps) {
                for warning in &warnings {
                    eprintln!("kiri: {warning}");
                }
                let opened = Opened {
                    path,
                    file,
                    look,
                    input,
                    content: Content::Image { format, head, rest },
                };
                let caps = on_page(caps, &settings);
                let failed = Cell::new(false);
                return show_files(
                    &context,
                    vec![Ok(opened)],
                    settings,
                    documents,
                    caps,
                    &failed,
                );
            }
            let reader = image_reader(file.as_deref(), head, rest)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            return Ok(show_image(&context, &caps, &path, format, reader)?);
        }
        Content::Text(source) => source,
    };
    let (warnings, settings) = settings(cli, &env);
    for warning in &warnings {
        eprintln!("kiri: {warning}");
    }
    let mut settings = settings?;
    if file.is_none() {
        settings.paging = kiri::pager::only_always(settings.paging);
    }
    let caps = kiri::term::capabilities(context.mode, context.width, &env);
    if matches!(input, Input::Diagram(_)) && pages_alone(&settings, &caps) {
        let opened = Opened {
            path,
            file,
            look,
            input,
            content: Content::Text(source),
        };
        let caps = on_page(caps, &settings);
        let failed = Cell::new(false);
        return show_files(&context, vec![Ok(opened)], settings, true, caps, &failed);
    }
    let (path, file) = (path.as_path(), file.as_deref());
    if let Input::Diagram(language) = input {
        return show_diagram(&context, language, &source, path, file, settings, caps);
    }
    // The source of a diagram in text stays source.
    settings.diagrams &= input != Input::Text;
    let warn = |warning| eprintln!("kiri: {warning}");
    let Some(document) = document(input, &source, path, &settings, &caps, warn)? else {
        return Ok(write_as_is(&source)?);
    };
    // Only a document read from a regular file can be read again: a pipe
    // given by its path, such as <(cmd), is read to its end already.
    let from = (file.filter(|path| path.is_file())).map(|path| FromFile { path, input, look });
    // Following pages the file however short it is, as -p does, unless
    // paging is never.
    if from.is_some() && settings.follow && settings.paging == Paging::Auto {
        settings.paging = Paging::Always;
    }
    show_document(
        &context,
        &document,
        &path.to_string_lossy(),
        from,
        settings,
        caps,
    )
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
        .take(kiri::input::HEAD as u64)
        .read_to_end(&mut head)
        .map_err(failed)?;
    let input = cli
        .format
        .unwrap_or_else(|| kiri::input::detect(&path, &head));
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
    warn: impl FnOnce(String),
) -> Result<Option<Document>, String> {
    Ok(Some(match input {
        Input::Table(separator) => kiri::parse::csv::parse(source, separator)
            .map_err(|error| format!("{}: {error}", path.display()))?,
        Input::Text if !caps.styling => return Ok(None),
        Input::Text => kiri::parse::code(source, kiri::input::language(path).as_deref()),
        _ => {
            let style = settings.front_matter.style();
            let document = kiri::parse::parse_with(source, style.is_some());
            let warning =
                style.and_then(|style| kiri::render::front_matter_warning(&document, style));
            if let Some(warning) = warning {
                warn(warning);
            }
            document
        }
    }))
}

/// Shows the files at `paths`, `-` for standard input, one after another,
/// each under a header that names it, as `run` shows each of them. A file
/// that cannot be shown is told about at its place among the others, which
/// are still shown; kiri then fails.
fn run_files(cli: &Cli, paths: &[PathBuf]) -> Result<(), Failure> {
    let failed = Cell::new(false);
    let tell = |error: String| told(&failed, error);
    let mut opened = Vec::new();
    for path in paths {
        let path = Some(path.as_path()).filter(|path| *path != Path::new("-"));
        opened.push(match open(cli, path) {
            Ok(mut opened_file) => {
                // Many image files may be given, more than kiri can keep
                // open, so a regular file is closed until it is shown.
                if let Content::Image { rest, .. } = &mut opened_file.content
                    && (opened_file.file.as_deref()).is_some_and(Path::is_file)
                {
                    *rest = None;
                }
                Ok(opened_file)
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
    let images_only = shown().all(|file| matches!(file.content, Content::Image { .. }));
    // Each of them shown on its own, as an image is.
    let alone = shown().all(|file| matches!(file.input, Input::Image(_) | Input::Diagram(_)));
    let (warnings, settings) = self::settings(cli, &env);
    for warning in &warnings {
        eprintln!("kiri: {warning}");
    }
    // As for an image on its own, a broken configuration file does not
    // stop images; the documents are not shown without it.
    let settings = match settings {
        Ok(settings) => Some(settings),
        Err(_) if images_only => None,
        Err(error) => {
            tell(error);
            None
        }
    };
    let documents = settings.is_some();
    let mut settings = settings.unwrap_or_else(|| without_config(cli, &env));
    settings.paging = kiri::pager::for_files(settings.paging, alone);
    let caps = on_page(
        kiri::term::capabilities(context.mode, context.width, &env),
        &settings,
    );
    show_files(&context, opened, settings, documents, caps, &failed)
}

/// The settings from the command line and the environment alone, for a
/// configuration file that cannot be read, with the default theme.
fn without_config(cli: &Cli, env: &Environment) -> Settings {
    let choices = kiri::config::choose(&options(cli), env, &Default::default());
    choices.with_theme(Theme::default())
}

/// Whether a file shown on its own as an image is, an image file or a
/// diagram file, goes to the pager for `settings` on a terminal with
/// `caps`: only when paging is `always`, as for several such files.
fn pages_alone(settings: &Settings, caps: &Capabilities) -> bool {
    kiri::pager::for_files(settings.paging, true) == Paging::Always
        && kiri::term::stdout_is_terminal()
        && caps.styling
}

/// Shows the files `opened`, or why each could not be opened, as
/// `run_files` shows them, with `settings`, whose paging is that for these
/// files, and for a terminal with `caps`; documents are shown only when
/// `documents`, as the configuration file could be read. One file alone
/// is shown in the pager as a document on its own is, without a header.
/// A file that cannot be shown is noted in `failed`, and kiri then fails.
fn show_files(
    context: &Context,
    opened: Vec<Result<Opened, (PathBuf, String)>>,
    settings: Settings,
    documents: bool,
    caps: Capabilities,
    failed: &Cell<bool>,
) -> Result<(), Failure> {
    let tell = |error: String| told(failed, error);
    let mut given = Vec::new();
    for opened in opened {
        let opened = match opened {
            Ok(opened) => opened,
            Err((path, error)) => {
                given.push(Given::failed(path, error));
                continue;
            }
        };
        let name = opened.path.display().to_string();
        let mut warnings = Vec::new();
        let warn = |warning| warnings.push(warning);
        let shown = match opened.content {
            Content::Image { format, head, rest } => Shown::Image { format, head, rest },
            // The configuration file's error tells why.
            Content::Text(_) if !documents => continue,
            Content::Text(source) => match opened.input {
                Input::Diagram(_) if !caps.styling => Shown::AsIs(source),
                Input::Diagram(language) => Shown::Diagram {
                    document: kiri::parse::code(&source, Some(language)),
                    language,
                    source,
                },
                input => match document(input, &source, &opened.path, &settings, &caps, warn) {
                    Ok(Some(document)) => Shown::Document {
                        document,
                        images: input != Input::Text,
                    },
                    Ok(None) => Shown::AsIs(source),
                    Err(error) => Shown::Failed(error),
                },
            },
        };
        given.push(Given {
            name,
            path: opened.path,
            file: opened.file,
            shown,
            warnings,
            drawn: None,
        });
    }
    let terminal = kiri::term::stdout_is_terminal() && caps.styling;
    // In the pager, the files are one stream, read whole before it opens.
    let paged = terminal && settings.paging != Paging::Never;
    if paged {
        given = (given.into_iter())
            .map(|given| read_whole(context, &caps, given))
            .collect();
    }
    // Every file failed; an empty pager would only wait.
    if given.iter().all(Given::is_failed) {
        for given in &given {
            if let Shown::Failed(error) = &given.shown {
                tell(error.clone());
            }
        }
        if failed.get() {
            return Err(Failure::Told);
        }
    }
    let Loaded {
        mut images,
        mut interrupt,
    } = load_given(context, &given, &settings, &caps)?;
    for (number, file) in given.iter_mut().enumerate() {
        file.drawn = self::drawn(number, file, &mut images, &settings, &caps);
    }
    let mut inputs = Inputs {
        images,
        theme: settings.theme.clone(),
        front_matter: settings.front_matter.style().unwrap_or_default(),
        table_rows: settings.table_rows,
        render_errors: settings.render_errors,
        ..Inputs::new(caps)
    };
    let shown = if paged {
        page_files(context, given, &mut inputs, &settings, failed, || {
            drop(interrupt.take())
        })
    } else {
        print_files(context, given, &mut inputs, &settings, failed)
    };
    match shown {
        // A closed pipe (kiri *.md | head) takes no more.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
        Err(error) => return Err(Failure::Error(error.to_string())),
        Ok(()) => {}
    }
    match failed.get() {
        true => Err(Failure::Told),
        false => Ok(()),
    }
}

/// `given`, an image file read whole, to be shown in a stream of files
/// that its reader cannot wait for, such as the pager's, or failed with why
/// it cannot be read; any other file as it is.
fn read_whole(context: &Context, caps: &Capabilities, given: Given) -> Given {
    let Given {
        name,
        path,
        file,
        shown,
        warnings,
        drawn,
    } = given;
    let shown = match shown {
        Shown::Image { format, head, rest } => {
            let image = image_reader(file.as_deref(), head, rest)
                .map_err(|error| format!("{}: {error}", path.display()))
                .and_then(|reader| read_image_file(context, caps, &path, format, reader));
            match image {
                Ok(image) => Shown::Read(image),
                Err(error) => Shown::Failed(error),
            }
        }
        shown => shown,
    };
    Given {
        name,
        path,
        file,
        shown,
        warnings,
        drawn,
    }
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
    }
}

/// Shows `given`, the files read whole, in the pager as one stream when
/// the paging of `settings` asks for it, and otherwise prints them as
/// `print_files` does, with the images `inputs` holds and those of the
/// image files among them. One file alone is shown as a document on its
/// own is, without a header, and the status line names it without its
/// place among files.
/// A file that failed is in the pager's stream under its header, with why.
/// Why a file failed, and a warning about a file, are told before the
/// pager opens, and why the diagram of a diagram file was not drawn after
/// it closes; all of them are noted in `failed`. `caught` runs once the
/// pager's screen catches the signals.
fn page_files(
    context: &Context,
    given: Vec<Given>,
    inputs: &mut Inputs,
    settings: &Settings,
    failed: &Cell<bool>,
    caught: impl FnOnce(),
) -> io::Result<()> {
    let mut files = Vec::new();
    // What is told about each file before it is shown.
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    // What is told about each file after it is shown.
    let mut drawn = Vec::new();
    for (number, given) in given.into_iter().enumerate() {
        drawn.push(given.drawn);
        let name = given.name;
        let mut error = None;
        let mut warned: Vec<_> = (given.warnings.iter())
            .map(|warning| format!("{name}: {warning}"))
            .collect();
        let document = match given.shown {
            Shown::Failed(why) => {
                // The header names the file already.
                let prefix = format!("{}: ", given.path.display());
                let reason = why.strip_prefix(&prefix).unwrap_or(&why).to_owned();
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
                    warned.push(format!("{name}: {warning}"));
                }
                document
            }
            Shown::Diagram { document, .. } => document,
            // Plain output, the only one to get text as it is, has no
            // pager, nor do images read as they are shown.
            Shown::AsIs(text) => kiri::parse::code(&text, None),
            Shown::Image { .. } => unreachable!("an image file in the pager is read whole"),
        };
        errors.push(error);
        warnings.push(warned);
        files.push(kiri::model::File { name, document });
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
    };
    let lone = files.len() == 1;
    let (content, name) = match lone {
        true => (
            kiri::render::layout::Content::Document(&files[0].document),
            files[0].name.as_str(),
        ),
        // The pager's status line names each file after itself.
        false => (kiri::render::layout::Content::Files(&files), ""),
    };
    // Nothing to reload: an image file is not read again. Nothing to
    // follow either, which the pager says only when the command line asked
    // to follow.
    let options = kiri::pager::Options {
        follow: context.follow,
        ..kiri::pager::Options::new(settings.search_case)
    };
    let paging = settings.paging;
    if let Some(result) = page(context, content, name, inputs, paging, options, opening) {
        // What the pager would have hidden.
        (drawn.iter().flatten()).for_each(|drawn| tell_drawn(drawn, failed));
        return result;
    }
    // Printed as `print_files` prints them: a failed file is not shown, and
    // what is told about each file is told at its place, unless it was
    // told before a pager that then could not open.
    let mut out = io::stdout().lock();
    let mut apart = false;
    for (number, file) in files.iter().enumerate() {
        if !told_before.get() {
            tell_before(number);
        }
        if errors[number].is_some() {
            continue;
        }
        let shown = match lone {
            true => kiri::render::render(&file.document, inputs, &mut out),
            false => kiri::render::render_file(file, number, apart, inputs, &mut out),
        };
        shown.and_then(|()| out.flush())?;
        apart = true;
        if let Some(drawn) = &drawn[number] {
            tell_drawn(drawn, failed);
        }
    }
    Ok(())
}

/// Prints `given`, each file under its header, as it comes: an image file
/// is read only then, so that no more than one is held at once. A warning
/// about a file is told before it, and why the diagram of a diagram file
/// was not drawn after it; a file that cannot be shown is told about at its
/// place and noted in `failed`.
fn print_files(
    context: &Context,
    given: Vec<Given>,
    inputs: &mut Inputs,
    settings: &Settings,
    failed: &Cell<bool>,
) -> io::Result<()> {
    let mut out = io::stdout().lock();
    let mut apart = false;
    for (number, given) in given.into_iter().enumerate() {
        let name = given.name;
        let drawn = given.drawn;
        for warning in &given.warnings {
            eprintln!("kiri: {name}: {warning}");
        }
        let shown = match given.shown {
            Shown::Failed(error) => {
                told(failed, error);
                continue;
            }
            Shown::Image { format, head, rest } => {
                let image = image_reader(given.file.as_deref(), head, rest)
                    .map_err(|error| format!("{name}: {error}"))
                    .and_then(|reader| {
                        read_image_file(context, &inputs.caps, &given.path, format, reader)
                    });
                match image {
                    Ok(image) => print_image(image, name, number, apart, inputs, &mut out),
                    Err(error) => {
                        told(failed, error);
                        continue;
                    }
                }
            }
            Shown::Read(image) => print_image(image, name, number, apart, inputs, &mut out),
            Shown::Document { document, .. } => {
                let stderr = settings.render_errors.stderr();
                for warning in inputs.images.warnings_in(number, stderr) {
                    eprintln!("kiri: {name}: {warning}");
                }
                let file = kiri::model::File { name, document };
                kiri::render::render_file(&file, number, apart, inputs, &mut out)
            }
            // What could not be drawn is told after it.
            Shown::Diagram { document, .. } => {
                let file = kiri::model::File { name, document };
                kiri::render::render_file(&file, number, apart, inputs, &mut out)
            }
            Shown::AsIs(text) => {
                let file = kiri::model::File {
                    name,
                    document: Document::default(),
                };
                kiri::render::render_file(&file, number, apart, inputs, &mut out)
                    .and_then(|()| out.write_all(text.as_bytes()))
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

/// Prints `image`, the image file `name` numbered `number` among the files
/// shown, under its header, as `render_file` prints a file, with the
/// images `inputs` holds.
fn print_image(
    image: Source,
    name: String,
    number: usize,
    apart: bool,
    inputs: &mut Inputs,
    out: &mut impl Write,
) -> io::Result<()> {
    let key = ImageKey::from(name.as_str()).in_document(number);
    inputs.images.insert(key.clone(), Ok(image));
    let shown = kiri::render::render_file(&image_file(name), number, apart, inputs, out);
    inputs.images.remove(&key);
    shown
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
    /// What is told about it before it is shown, besides its images.
    warnings: Vec<String>,
    /// What is told after it is shown: why its diagram was not drawn, for
    /// a diagram file.
    drawn: Option<Drawn>,
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
    /// The file at `path`, which cannot be shown for `error`.
    fn failed(path: PathBuf, error: String) -> Self {
        Given {
            name: path.display().to_string(),
            path,
            file: None,
            shown: Shown::Failed(error),
            warnings: Vec::new(),
            drawn: None,
        }
    }

    /// Whether the file cannot be shown.
    fn is_failed(&self) -> bool {
        matches!(self.shown, Shown::Failed(_))
    }
}

/// What is told after a diagram file is shown.
enum Drawn {
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
) -> Result<Loaded, Failure> {
    if !caps.draws_images() {
        return Ok(Loaded::default());
    }
    let parts: Vec<Part> = (given.iter().enumerate())
        .filter_map(|(number, given)| {
            let document = match &given.shown {
                Shown::Document { document, images } if *images && settings.images => document,
                Shown::Diagram { document, .. } => document,
                _ => return None,
            };
            let file = given.file.as_deref();
            Some(Part {
                number,
                ..part(document, file)
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
            None
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
fn image_file(name: String) -> kiri::model::File {
    let document = Document::new(vec![Block::ImageFile(name.clone())]);
    kiri::model::File { name, document }
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
    /// Whether the command line asks to follow the file, so that the pager
    /// says when there is nothing to follow.
    follow: bool,
}

impl<'a> Context<'a> {
    fn new(cli: &Cli, env: &'a Environment) -> Self {
        Context {
            env,
            mode: cli.mode,
            width: cli.width,
            max_file_size: cli.max_image_size * kiri::images::MB,
            follow: cli.follow.unwrap_or(cli.follow_on),
        }
    }
}

/// A document read from a regular file, as `input`, which the pager can read
/// again.
#[derive(Clone, Copy)]
struct FromFile<'a> {
    path: &'a Path,
    input: Input,
    /// How the file looked before it was read.
    look: Option<Look>,
}

/// Renders `document`, called `name`, on stdout. When it comes from a
/// file, its images are relative to the file's directory, warnings name a
/// diagram by its line and the pager reloads it with `r`; otherwise images
/// are relative to the current directory, a diagram is named by its first
/// line and there is nothing to reload.
fn show_document(
    context: &Context,
    document: &Document,
    name: &str,
    from: Option<FromFile>,
    settings: Settings,
    caps: Capabilities,
) -> Result<(), Failure> {
    let caps = on_page(caps, &settings);
    let parts = [part(document, from.map(|from| from.path))];
    // Before the images are read, so that a file changed meanwhile counts
    // as changed.
    let stamps = Stamps::of(&parts);
    let loaded = load_images(context, &parts, &settings, &caps)?;
    for warning in loaded.images.warnings_with(settings.render_errors.stderr()) {
        eprintln!("kiri: {warning}");
    }
    let reading = from.map(|from| (from, stamps));
    print_document(context, document, name, loaded, settings, caps, reading)
}

/// Shows the diagram `source`, in `language`, from the file at `path` or
/// from standard input, on its own and as large as fits the window, as an
/// image file is shown: drawn as in a document, also with images off.
/// Where it is not drawn, its source is shown as `--format text` shows
/// it, and a diagram that fails to draw is an error after that, told
/// below the source, on standard error or both, as the settings say. One
/// that nothing could draw, where diagrams are on and the terminal shows
/// them, gets a hint after it.
fn show_diagram(
    context: &Context,
    language: &str,
    source: &str,
    path: &Path,
    file: Option<&Path>,
    mut settings: Settings,
    caps: Capabilities,
) -> Result<(), Failure> {
    let caps = on_page(caps, &settings);
    let document = kiri::parse::code(source, Some(language));
    settings.images = true;
    let mut loaded = load_images(context, &[part(&document, file)], &settings, &caps)?;
    let key = ImageKey::block(language, source);
    let error = match loaded.images.remove(&key) {
        Some(Ok(image)) => {
            let page = Page {
                background: settings.theme.background,
                foreground: settings.theme.foreground,
            };
            let mut out = io::stdout().lock();
            let shown = kiri::render::render_image_on(image, &caps, page, &mut out);
            return Ok(finish(shown.and_then(|()| out.flush()))?);
        }
        Some(Err(error)) => {
            let reason = match &error {
                ImageError::Diagram { reason, .. } => reason.clone(),
                error => error.to_string(),
            };
            // Back in, so that the document tells it below the source.
            loaded.images.insert(key, Err(error));
            Some(reason)
        }
        None => None,
    };
    let stderr = settings.render_errors.stderr();
    // Not drawn, though it could have been shown: no renderer for it.
    let no_renderer = settings.diagrams && caps.draws_images() && error.is_none();
    if caps.styling {
        let name = path.to_string_lossy();
        print_document(context, &document, &name, loaded, settings, caps, None)?;
    } else {
        write_as_is(source)?;
    }
    if no_renderer {
        eprintln!(
            "kiri: {}: no program to draw {} diagrams; see --doctor",
            path.display(),
            kiri::diagrams::display_name(language)
        );
    }
    match error {
        Some(error) if stderr => Err(Failure::Error(format!("{}: {error}", path.display()))),
        Some(_) => Err(Failure::Told),
        None => Ok(()),
    }
}

/// `caps` for the page `settings` choose: a light or dark page decides
/// whatever the terminal's background is.
fn on_page(mut caps: Capabilities, settings: &Settings) -> Capabilities {
    if let Some(paper) = settings.paper {
        caps.dark_background = Some(paper == Paper::Dark);
    }
    caps
}

/// The images of the documents in `parts`, loaded and drawn when
/// `settings` want them and `caps` can show them; otherwise none, as they
/// are alt text, without warnings. While programs draw any of their
/// diagrams, Ctrl-C, SIGTERM, SIGHUP and Ctrl-\ stop them; such a
/// signal is the failure. On a terminal, a line tells how far drawing has
/// got when it takes long.
fn load_images(
    context: &Context,
    parts: &[Part],
    settings: &Settings,
    caps: &Capabilities,
) -> Result<Loaded, Failure> {
    if !settings.images || !caps.draws_images() {
        return Ok(Loaded::default());
    }
    load_parts(context, parts, settings, caps)
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
        largest: Some(kiri::render::image::largest(caps)),
    }
}

/// `document`, the first and only one shown, from `file` or not as for
/// `show_document`, as a part whose images are loaded.
fn part<'a>(document: &'a Document, file: Option<&'a Path>) -> Part<'a> {
    Part {
        document,
        number: 0,
        base: file.and_then(Path::parent).unwrap_or(Path::new("")),
        by_line: file.is_some(),
    }
}

/// Renders `document`, called `name`, with the images `loaded` on
/// stdout: in the pager when `settings` ask for it, and otherwise
/// printed. The pager catches the signals itself, so the guard against
/// them that came with the images goes once its screen catches them. With
/// `reading`, the file the document is from and when its images' files
/// were last changed, the pager can read it again.
fn print_document(
    context: &Context,
    document: &Document,
    name: &str,
    loaded: Loaded,
    settings: Settings,
    caps: Capabilities,
    reading: Option<(FromFile, Stamps)>,
) -> Result<(), Failure> {
    let Loaded {
        images,
        mut interrupt,
    } = loaded;
    let mut inputs = Inputs {
        images,
        theme: settings.theme.clone(),
        front_matter: settings.front_matter.style().unwrap_or_default(),
        table_rows: settings.table_rows,
        render_errors: settings.render_errors,
        ..Inputs::new(caps)
    };
    let mut reload = reading.map(|(from, stamps)| FileReload {
        context,
        settings: &settings,
        from,
        stamps,
        read: from.look,
    });
    if let Some(result) = page(
        context,
        kiri::render::layout::Content::Document(document),
        name,
        &mut inputs,
        settings.paging,
        kiri::pager::Options {
            // Without a file to follow, the pager says so only when the
            // command line asked to follow.
            follow: match reload {
                Some(_) => settings.follow,
                None => context.follow,
            },
            reload: reload.as_mut().map(|reload| reload as &mut dyn Reload),
            ..kiri::pager::Options::new(settings.search_case)
        },
        || drop(interrupt.take()),
    ) {
        return Ok(finish(result)?);
    }
    let mut out = io::stdout().lock();
    Ok(finish(
        kiri::render::render(document, &inputs, &mut out).and_then(|()| out.flush()),
    )?)
}

/// Shows `content` in the pager when `paging` asks for it and the output
/// is a terminal that takes escape sequences; `None` when it is to be
/// printed instead. `options` tell the pager how a search treats case and
/// where `r` gets the document again from, if anywhere. The pager catches
/// Ctrl-C itself; `caught` runs once its screen catches the signals.
fn page(
    context: &Context,
    content: kiri::render::layout::Content,
    name: &str,
    inputs: &mut Inputs,
    paging: kiri::config::Paging,
    options: kiri::pager::Options,
    caught: impl FnOnce(),
) -> Option<io::Result<()>> {
    let terminal = kiri::term::stdout_is_terminal() && inputs.caps.styling;
    if !terminal || paging == kiri::config::Paging::Never {
        return None;
    }
    let stream = kiri::render::layout::content(content, inputs);
    let rows = kiri::pager::rows(&stream.lines, &inputs.caps);
    let height = kiri::term::window_size(io::stdout()).map(|window| window.rows);
    if !kiri::pager::pages(paging, terminal, rows, height) {
        return None;
    }
    // The screen catches the signals before the terminal is raw; the
    // guard, which would otherwise end kiri with the terminal raw, goes
    // then.
    let Some(mut screen) = kiri::term::Screen::open(caught) else {
        // Without a terminal to read keys from, as in the background, the
        // document is printed, and the signals still end kiri; only a
        // reader who asked for the pager is told.
        if paging == kiri::config::Paging::Always {
            eprintln!("kiri: no terminal to read keys from, so the document is printed");
        }
        return None;
    };
    let result = kiri::pager::run(
        content,
        Some(stream),
        name,
        inputs,
        kiri::pager::Options {
            width: context.width,
            ..options
        },
        &mut screen,
    );
    screen.close();
    Some(result)
}

/// Reads the document from its file again for the pager, and loads the
/// images it does not have yet.
struct FileReload<'a> {
    context: &'a Context<'a>,
    settings: &'a Settings,
    from: FromFile<'a>,
    /// When the files behind the images were last changed.
    stamps: Stamps,
    /// How the file looked when it was last read, when it could be
    /// looked at.
    read: Option<Look>,
}

impl Reload for FileReload<'_> {
    fn reload(&mut self, images: &mut Images, caps: &Capabilities) -> Reloaded {
        let FromFile { path, input, .. } = self.from;
        // Before the file is read, so that a change meanwhile counts.
        self.read = look(path).ok();
        // The status line names the file already, so the reasons do not.
        let source = match read_text(path) {
            Ok(source) => source,
            Err(error) => return Reloaded::Failed(error),
        };
        // A warning about the front matter has no place in the pager.
        let document = match document(input, &source, path, self.settings, caps, drop) {
            Ok(Some(document)) => document,
            Ok(None) => return Reloaded::Failed("cannot be shown".to_owned()),
            Err(error) => return Reloaded::Failed(without_path(error, path)),
        };
        if self.settings.images && caps.draws_images() {
            let parts = [part(&document, Some(path))];
            let loading = loading(self.context, self.settings, caps);
            // The pager says it is reloading, so the progress line is not
            // for the terminal's alternate screen.
            let stamps = &mut self.stamps;
            if reload_documents(&parts, &loading, None, images, stamps).is_err() {
                return Reloaded::Failed("stopped by a signal".to_owned());
            }
        }
        Reloaded::Document(document)
    }

    fn change(&mut self) -> Change {
        match look(self.from.path) {
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

/// Prints the image file `path`, in `format`, read from `reader`, for a
/// terminal with `caps`. It is shown whatever the settings say about
/// images in documents, but only up to the size limit.
fn show_image(
    context: &Context,
    caps: &Capabilities,
    path: &Path,
    format: Format,
    reader: impl Read,
) -> Result<(), String> {
    let image = read_image_file(context, caps, path, format, reader)?;
    let mut out = io::stdout().lock();
    finish(kiri::render::render_image(image, caps, &mut out).and_then(|()| out.flush()))
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
    let detection = kiri::term::examine(cli.mode, cli.width, &env);
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
        config_error,
        theme_error,
        output,
    });
    if output != Output::Rendered {
        let mut out = io::stdout().lock();
        return Ok(finish(
            out.write_all(markdown.as_bytes())
                .and_then(|()| out.flush()),
        )?);
    }
    let document = kiri::parse::parse(&markdown);
    let mut settings = choices.with_theme(theme);
    settings.paging = kiri::pager::only_always(settings.paging);
    show_document(
        &Context::new(cli, &env),
        &document,
        "kiri --doctor",
        None,
        settings,
        detection.caps,
    )
}

/// Writes the emoji shortcodes: a rendered document for a terminal, one
/// line per emoji otherwise.
fn list_emojis(cli: &Cli) -> Result<(), Failure> {
    if !kiri::term::stdout_is_terminal() {
        let mut out = io::stdout().lock();
        return Ok(finish(
            out.write_all(kiri::list::emoji_lines().as_bytes())
                .and_then(|()| out.flush()),
        )?);
    }
    let env = Environment::from_process();
    let config = kiri::config::load(&env)?;
    let (warnings, settings) = kiri::config::settings(&options(cli), &env, &config);
    for warning in &warnings {
        eprintln!("kiri: {warning}");
    }
    let settings = settings?;
    let caps = kiri::term::capabilities(cli.mode, cli.width, &env);
    let document = kiri::parse::parse(&kiri::list::emoji_document());
    show_document(
        &Context::new(cli, &env),
        &document,
        "kiri --list-emojis",
        None,
        settings,
        caps,
    )
}

/// Writes `text` on stdout as it is, as cat would, without rendering it:
/// for `--format text` in plain output.
fn write_as_is(text: &str) -> Result<(), String> {
    let mut out = io::stdout().lock();
    finish(out.write_all(text.as_bytes()).and_then(|()| out.flush()))
}

/// The outcome of writing the output.
fn finish(result: io::Result<()>) -> Result<(), String> {
    match result {
        // A closed pipe (kiri file.md | head) is not an error.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result.map_err(|error| error.to_string()),
    }
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
        follow: (flag(cli.follow, "--follow"))
            .or(flag(cli.follow_on.then_some(true), "-f"))
            .or(flag(cli.follow_off.then_some(false), "-F")),
        front_matter: flag(cli.front_matter, "--front-matter"),
        table_rows: flag(cli.table_rows, "--table-rows"),
        diagrams: flag(cli.diagrams, "--diagrams"),
        render_errors: flag(cli.render_errors, "--render-errors"),
    }
}

/// `value`, when the command line gives it, from the flag `name`: the
/// flag's long name, or its short one where it has no long one, as clap
/// does not tell which of them the user gave.
fn flag<T>(value: Option<T>, name: &'static str) -> Option<Setting<T>> {
    value.map(|value| Setting::new(value, Origin::Flag(name)))
}

/// The value of the argument `id`, `value`, from the flag `name` when the
/// command line gives it, and otherwise clap's default.
fn or_default<T>(matches: &ArgMatches, id: &str, name: &'static str, value: T) -> Setting<T> {
    let given = matches.value_source(id) == Some(ValueSource::CommandLine);
    match given {
        true => Setting::new(value, Origin::Flag(name)),
        false => Setting::new(value, Origin::Default),
    }
}
