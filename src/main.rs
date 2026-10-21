use std::fs::File;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser, ValueEnum};
use kiri::caps::Capabilities;
use kiri::config::{Environment, Options, Origin, Setting, Settings};
use kiri::diagrams::{ExternalSettings, NoRenderer, Renderers};
use kiri::doctor::{Facts, Output};
use kiri::images::{Fonts, Format, ImageError, ImageKey, Images, LoadOptions};
use kiri::input::Input;
use kiri::model::Document;
use kiri::render::Inputs;
use kiri::render::output::Page;
use kiri::term::Interrupt;
use kiri::theme::{Paper, Theme};

/// Render a Markdown document, or show an image, in the terminal.
#[derive(Parser)]
#[command(
    version,
    after_help = "Images and diagrams are drawn only in terminals that support \
                  kitty's graphics protocol with Unicode placeholders, such as \
                  kitty 0.28 or later; elsewhere images show their alt text \
                  and diagrams their code."
)]
struct Cli {
    /// Markdown file to render, a CSV or TSV file to show as a table, an
    /// image or a Mermaid diagram (.mmd) to show on its own, or a directory
    /// to show its README.
    /// Without one, or with -, kiri reads standard input unless it is a
    /// terminal.
    file: Option<PathBuf>,

    /// Show what kiri finds out about the terminal and which settings it
    /// uses, as Markdown when written to a file, for a bug report.
    #[arg(long, conflicts_with_all = ["file", "format"])]
    doctor: bool,

    /// List the emoji shortcodes, such as :smile:. A terminal shows them
    /// as a table; a pipe or a file gets one line per emoji.
    #[arg(long, conflicts_with_all = ["file", "format", "doctor"])]
    list_emojis: bool,

    /// With --doctor: the report as Markdown also in a terminal, without
    /// its sample, to copy into a bug report.
    #[arg(long)]
    raw: bool,

    /// What the file is: markdown, csv, tsv, mermaid (a diagram shown on
    /// its own), text (the file as it is, without rendering it), image, or
    /// an image format such as svg, png or jpg. By default its extension and
    /// content tell.
    #[arg(short, long, value_parser = format)]
    format: Option<Input>,

    /// Width in cells; defaults to the terminal width.
    #[arg(long)]
    width: Option<u16>,

    /// Which terminal features to use. Colours stay off when NO_COLOR is set.
    #[arg(long, value_enum, default_value_t = Mode::Auto)]
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
    #[arg(long, value_enum, value_name = "WHEN", conflicts_with_all = ["pager", "no_pager"])]
    paging: Option<Paging>,

    /// Show the document in the pager: --paging=always.
    #[arg(short = 'p', conflicts_with = "no_pager")]
    pager: bool,

    /// Print the document without the pager: --paging=never.
    #[arg(short = 'P')]
    no_pager: bool,

    /// Front matter render style: table, code (a code block), hide, or off
    /// (read as Markdown); table unless set. Front matter is the metadata
    /// block at the start of a Markdown file.
    #[arg(long, value_enum, value_name = "STYLE")]
    front_matter: Option<FrontMatter>,

    /// Table row style: zebra (every second row shaded where the
    /// terminal's colours are known), lines, indent (the lines after the
    /// first of a wrapped value start two cells in), underline (each row's
    /// last line underlined), or off; zebra unless set.
    #[arg(long, value_enum, value_name = "STYLE")]
    table_rows: Option<TableRows>,

    /// Diagrams in code blocks, such as Mermaid: on (drawn as images) or
    /// off (shown as code); on unless set.
    #[arg(long, value_enum, value_name = "SWITCH")]
    diagrams: Option<Switch>,

    /// Where to tell why a diagram could not be drawn: inline (in the
    /// document, below its code), stderr (a warning) or both; both unless
    /// set.
    #[arg(long, value_enum, value_name = "WHERE")]
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

/// `term::Mode` for clap, which takes its help from these comments.
#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    /// Ask the terminal; plain text in a pipe.
    Auto,
    /// Text only, no escape sequences.
    Plain,
    /// Colours and attributes, no kitty extensions.
    Ansi,
    /// All kitty features, without asking the terminal.
    Kitty,
}

impl From<Mode> for kiri::term::Mode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Auto => Self::Auto,
            Mode::Plain => Self::Plain,
            Mode::Ansi => Self::Ansi,
            Mode::Kitty => Self::Kitty,
        }
    }
}

/// `config::Paging` for clap.
#[derive(Clone, Copy, ValueEnum)]
enum Paging {
    Always,
    Auto,
    Never,
}

impl From<Paging> for kiri::config::Paging {
    fn from(paging: Paging) -> Self {
        match paging {
            Paging::Always => Self::Always,
            Paging::Auto => Self::Auto,
            Paging::Never => Self::Never,
        }
    }
}

/// `config::FrontMatter` for clap.
#[derive(Clone, Copy, ValueEnum)]
enum FrontMatter {
    Table,
    Code,
    Hide,
    Off,
}

impl From<FrontMatter> for kiri::config::FrontMatter {
    fn from(front_matter: FrontMatter) -> Self {
        match front_matter {
            FrontMatter::Table => Self::Table,
            FrontMatter::Code => Self::Code,
            FrontMatter::Hide => Self::Hide,
            FrontMatter::Off => Self::Off,
        }
    }
}

/// `render::TableRows` for clap.
#[derive(Clone, Copy, ValueEnum)]
enum TableRows {
    Off,
    Zebra,
    Lines,
    Indent,
    Underline,
}

impl From<TableRows> for kiri::render::TableRows {
    fn from(rows: TableRows) -> Self {
        match rows {
            TableRows::Off => Self::Off,
            TableRows::Zebra => Self::Zebra,
            TableRows::Lines => Self::Lines,
            TableRows::Indent => Self::Indent,
            TableRows::Underline => Self::Underline,
        }
    }
}

/// `render::RenderErrors` for clap.
#[derive(Clone, Copy, ValueEnum)]
enum RenderErrors {
    Inline,
    Stderr,
    Both,
}

impl From<RenderErrors> for kiri::render::RenderErrors {
    fn from(errors: RenderErrors) -> Self {
        match errors {
            RenderErrors::Inline => Self::Inline,
            RenderErrors::Stderr => Self::Stderr,
            RenderErrors::Both => Self::Both,
        }
    }
}

/// An on or off setting for clap.
#[derive(Clone, Copy, ValueEnum)]
enum Switch {
    On,
    Off,
}

impl From<Switch> for bool {
    fn from(switch: Switch) -> Self {
        matches!(switch, Switch::On)
    }
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
        match cli.file.as_deref() {
            Some(path) if path != Path::new("-") => run(&cli, Some(path)),
            Some(_) => run(&cli, None),
            None if !kiri::term::stdin_is_terminal() => run(&cli, None),
            // clap cannot require the file only when stdin is a terminal,
            // so this is its own error for a missing argument.
            None => command
                .error(
                    ErrorKind::MissingRequiredArgument,
                    "the following required arguments were not provided:\n  <FILE>",
                )
                .exit(),
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
    let readme;
    let file = match file {
        Some(dir) if dir.is_dir() => {
            readme = self::readme(dir)?;
            Some(readme.as_path())
        }
        file => file,
    };
    let path = file.unwrap_or(Path::new(STDIN));
    let failed = |error: io::Error| format!("{}: {error}", path.display());
    let mut reader: Box<dyn Read> = match file {
        Some(path) => Box::new(File::open(path).map_err(failed)?),
        None => Box::new(io::stdin().lock()),
    };
    let mut head = Vec::new();
    (&mut reader)
        .take(kiri::input::HEAD as u64)
        .read_to_end(&mut head)
        .map_err(failed)?;
    let env = Environment::from_process();
    let input = cli
        .format
        .unwrap_or_else(|| kiri::input::detect(path, &head));
    if let Input::Image(format) = input {
        return Ok(show_image(
            cli,
            path,
            format,
            Cursor::new(head).chain(reader),
            &env,
        )?);
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
    let config = kiri::config::load(&env)?;
    let (warnings, settings) = kiri::config::settings(&options(cli), &env, &config);
    for warning in &warnings {
        eprintln!("kiri: {warning}");
    }
    let mut settings = settings?;
    if file.is_none() {
        settings.paging = only_always(settings.paging);
    }
    let caps = kiri::term::capabilities(cli.mode.into(), cli.width, &env);
    if let Input::Diagram(language) = input {
        return show_diagram(cli, language, &source, path, file, settings, caps, &env);
    }
    let document = match input {
        Input::Table(separator) => kiri::parse::csv::parse(&source, separator)
            .map_err(|error| format!("{}: {error}", path.display()))?,
        // Plain output gets the text as it is, as from cat.
        Input::Text if !caps.styling => return Ok(write_as_is(&source)?),
        Input::Text => {
            // The source of a diagram stays source.
            settings.diagrams = false;
            kiri::parse::code(&source, kiri::input::language(path).as_deref())
        }
        _ => {
            let style = settings.front_matter.style();
            let document = kiri::parse::parse_with(&source, style.is_some());
            let warning =
                style.and_then(|style| kiri::render::front_matter_warning(&document, style));
            if let Some(warning) = warning {
                eprintln!("kiri: {warning}");
            }
            document
        }
    };
    show_document(
        cli,
        &document,
        &path.to_string_lossy(),
        file,
        settings,
        caps,
        &env,
    )
}

/// The README the directory `dir` shows: `README.md`, or else `README`.
fn readme(dir: &Path) -> Result<PathBuf, String> {
    ["README.md", "README"]
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| format!("{}: no README.md or README", dir.display()))
}

/// Renders `document`, called `name`, on stdout. When it comes from
/// `file`, its images are relative to the file's directory and warnings
/// name a diagram by its line; otherwise images are relative to the
/// current directory and a diagram is named by its first line. `env`
/// tells where to look for a program that draws diagrams.
fn show_document(
    cli: &Cli,
    document: &Document,
    name: &str,
    file: Option<&Path>,
    settings: Settings,
    caps: Capabilities,
    env: &Environment,
) -> Result<(), Failure> {
    let caps = on_page(caps, &settings);
    let mut interrupt = None;
    let images = load_images(cli, document, file, &settings, &caps, env, &mut interrupt)?;
    for warning in images.warnings_with(settings.render_errors.stderr()) {
        eprintln!("kiri: {warning}");
    }
    print_document(cli, document, name, images, settings, caps, &mut interrupt)
}

/// Shows the diagram `source`, in `language`, from the file at `path` or
/// from standard input, on its own and as large as fits the window, as an
/// image file is shown: drawn as in a document, also with images off.
/// Where it is not drawn, its source is shown as `--format text` shows
/// it, and a diagram that fails to draw is an error after that, told
/// below the source, on standard error or both, as the settings say. One
/// that nothing could draw, where diagrams are on and the terminal shows
/// them, gets a hint after it.
#[allow(clippy::too_many_arguments)]
fn show_diagram(
    cli: &Cli,
    language: &str,
    source: &str,
    path: &Path,
    file: Option<&Path>,
    mut settings: Settings,
    caps: Capabilities,
    env: &Environment,
) -> Result<(), Failure> {
    let caps = on_page(caps, &settings);
    let document = kiri::parse::code(source, Some(language));
    settings.images = true;
    let mut interrupt = None;
    let mut images = load_images(cli, &document, file, &settings, &caps, env, &mut interrupt)?;
    let key = ImageKey::block(language, source);
    let error = match images.remove(&key) {
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
            images.insert(key, Err(error));
            Some(reason)
        }
        None => None,
    };
    let stderr = settings.render_errors.stderr();
    // Not drawn, though it could have been shown: no renderer for it.
    let no_renderer = settings.diagrams && caps.draws_images() && error.is_none();
    if caps.styling {
        let name = path.to_string_lossy();
        print_document(
            cli,
            &document,
            &name,
            images,
            settings,
            caps,
            &mut interrupt,
        )?;
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

/// The images of `document`, from `file` or not as for `show_document`,
/// loaded and drawn when `settings` want them and `caps` can show them;
/// otherwise none, as they are alt text, without warnings. When programs
/// draw any of its diagrams, `interrupt` catches Ctrl-C, SIGTERM and
/// SIGHUP while the images load, which stop the programs; such a signal
/// is the failure.
fn load_images(
    cli: &Cli,
    document: &Document,
    file: Option<&Path>,
    settings: &Settings,
    caps: &Capabilities,
    env: &Environment,
    interrupt: &mut Option<Interrupt>,
) -> Result<Images, Failure> {
    if !settings.images || !caps.draws_images() {
        return Ok(Images::new());
    }
    let base = file.and_then(Path::parent).unwrap_or(Path::new(""));
    let max_file_size = cli.max_image_size * kiri::images::MB;
    let mut renderers = None;
    if settings.diagrams {
        let search = kiri::diagrams::find::Search::new(env);
        let configured = &settings.diagram_commands;
        let no_search = &settings.diagram_no_search;
        let mut external = ExternalSettings {
            time_limit: settings.diagram_timeout,
            max_size: max_file_size,
            work_dir: kiri::config::cache_directory(env).map(|cache| cache.join("diagrams")),
            ..ExternalSettings::new(kiri::config::temp_directory(env))
        };
        let chosen = Renderers::chosen(configured, no_search, &search, external.clone());
        // Only while a program draws one of the document's diagrams: they
        // otherwise end kiri at once, as without diagrams.
        if chosen.runs_programs_for(kiri::images::diagram_languages(document, &chosen)) {
            *interrupt = Interrupt::catch();
        }
        external.stop = interrupt.as_ref().map(Interrupt::flag);
        renderers = Some(Renderers::chosen(configured, no_search, &search, external));
    }
    let options = LoadOptions {
        renderer: match &renderers {
            Some(renderers) => renderers,
            None => &NoRenderer,
        },
        theme: kiri::diagrams::Theme::for_page(caps.dark_background),
        by_line: file.is_some(),
        progress: None,
        panel: settings.diagram_panel,
        largest: Some(kiri::render::image::largest(caps)),
    };
    let load = |options: &LoadOptions| kiri::images::load(document, base, max_file_size, options);
    // Drawing diagrams can take seconds with an external program; a
    // terminal tells how far it has got.
    let images = if kiri::term::stderr_is_terminal() {
        use kiri::term::progress::{DELAY, while_working};
        while_working(io::stderr(), DELAY, |progress| {
            load(&LoadOptions {
                progress: Some(&|done, total| progress.update(done, total)),
                ..options
            })
        })
    } else {
        load(&options)
    };
    if let Some(signal) = interrupt.as_ref().and_then(Interrupt::release) {
        return Err(Failure::Signal(signal));
    }
    Ok(images)
}

/// Renders `document`, called `name`, with `images` on stdout: in the
/// pager when `settings` ask for it, and otherwise printed. The pager
/// catches the signals itself, so `interrupt` goes once its screen
/// catches them.
fn print_document(
    cli: &Cli,
    document: &Document,
    name: &str,
    images: Images,
    settings: Settings,
    caps: Capabilities,
    interrupt: &mut Option<Interrupt>,
) -> Result<(), Failure> {
    let mut inputs = Inputs {
        images,
        theme: settings.theme,
        front_matter: settings.front_matter.style().unwrap_or_default(),
        table_rows: settings.table_rows,
        render_errors: settings.render_errors,
        ..Inputs::new(caps)
    };
    if let Some(result) = page(
        cli,
        document,
        name,
        &mut inputs,
        settings.paging,
        settings.search_case,
        interrupt,
    ) {
        return Ok(finish(result)?);
    }
    let mut out = io::stdout().lock();
    Ok(finish(
        kiri::render::render(document, &inputs, &mut out).and_then(|()| out.flush()),
    )?)
}

/// `paging` for a document that is paged only when paging is `always`:
/// one read from standard input, often the output of another command, and
/// the `--doctor` report.
fn only_always(paging: kiri::config::Paging) -> kiri::config::Paging {
    use kiri::config::Paging;
    match paging {
        Paging::Always => Paging::Always,
        Paging::Auto | Paging::Never => Paging::Never,
    }
}

/// Shows `document` in the pager when `paging` asks for it and the output
/// is a terminal that takes escape sequences; `None` when it is to be
/// printed instead. A search in the pager treats case as `case` says. The
/// pager catches Ctrl-C itself, so `interrupt` goes once its screen
/// catches the signals.
fn page(
    cli: &Cli,
    document: &Document,
    name: &str,
    inputs: &mut Inputs,
    paging: kiri::config::Paging,
    case: kiri::config::SearchCase,
    interrupt: &mut Option<Interrupt>,
) -> Option<io::Result<()>> {
    let terminal = kiri::term::stdout_is_terminal() && inputs.caps.styling;
    if !terminal || paging == kiri::config::Paging::Never {
        return None;
    }
    let lines = kiri::render::layout::layout(document, inputs);
    let rows = kiri::pager::rows(&lines, &inputs.caps);
    let height = kiri::term::window_size(io::stdout()).map(|window| window.rows);
    if !kiri::pager::pages(paging, terminal, rows, height) {
        return None;
    }
    // The screen catches the signals before the terminal is raw; the
    // guard, which would otherwise end kiri with the terminal raw, goes
    // then.
    let Some(mut screen) = kiri::term::Screen::open(|| drop(interrupt.take())) else {
        // Without a terminal to read keys from, as in the background, the
        // document is printed, and the signals still end kiri; only a
        // reader who asked for the pager is told.
        if paging == kiri::config::Paging::Always {
            eprintln!("kiri: no terminal to read keys from, so the document is printed");
        }
        return None;
    };
    let result = kiri::pager::run(
        document,
        Some(lines),
        name,
        inputs,
        cli.width,
        case,
        &mut screen,
    );
    screen.close();
    Some(result)
}

/// Shows the image file `path`, in `format`, read from `reader`. It
/// is shown whatever the settings say about images in documents, but only
/// up to the size limit. The configuration file is not read: everything in
/// it is about documents, so a broken one does not stop an image.
fn show_image(
    cli: &Cli,
    path: &Path,
    format: Format,
    reader: impl Read,
    env: &Environment,
) -> Result<(), String> {
    let caps = kiri::term::capabilities(cli.mode.into(), cli.width, env);
    if !caps.draws_images() {
        return Err(format!(
            "{}: the terminal cannot show images",
            path.display()
        ));
    }
    let max_file_size = cli.max_image_size * kiri::images::MB;
    let image = kiri::images::read_image(reader, path, format, max_file_size, &Fonts::system())
        .map_err(|error| match error {
            error if error.names_file() => error.to_string(),
            error => format!("{}: {error}", path.display()),
        })?;
    let mut out = io::stdout().lock();
    finish(kiri::render::render_image(image, &caps, &mut out).and_then(|()| out.flush()))
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
    let detection = kiri::term::examine(cli.mode.into(), cli.width, &env);
    let output = match (cli.raw, kiri::term::stdout_is_terminal()) {
        (true, _) => Output::Raw,
        (false, true) => Output::Rendered,
        (false, false) => Output::Markdown,
    };
    let markdown = kiri::doctor::report(&Facts {
        env: &env,
        mode: Setting::new(cli.mode.into(), origin(matches, "mode", "--mode")),
        width: cli.width,
        max_image_size: Setting::new(
            cli.max_image_size,
            origin(matches, "max_image_size", "--max-image-size"),
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
    settings.paging = only_always(settings.paging);
    show_document(
        cli,
        &document,
        "kiri --doctor",
        None,
        settings,
        detection.caps,
        &env,
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
    let caps = kiri::term::capabilities(cli.mode.into(), cli.width, &env);
    let document = kiri::parse::parse(&kiri::list::emoji_document());
    show_document(
        cli,
        &document,
        "kiri --list-emojis",
        None,
        settings,
        caps,
        &env,
    )
}

/// Where the value of the argument `id`, the flag `flag`, comes from.
fn origin(matches: &ArgMatches, id: &str, flag: &'static str) -> Origin {
    match matches.value_source(id) {
        Some(ValueSource::CommandLine) => Origin::Flag(flag),
        _ => Origin::Default,
    }
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

/// What the command line asks for.
fn options(cli: &Cli) -> Options {
    Options {
        theme: cli.theme.clone(),
        paper: match (cli.light, cli.dark) {
            (true, _) => Some(Paper::Light),
            (_, true) => Some(Paper::Dark),
            _ => None,
        },
        no_margins: cli.no_margins,
        images: match (cli.images, cli.no_images) {
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        },
        paging: match (cli.paging, cli.pager, cli.no_pager) {
            (Some(paging), _, _) => Some(Setting::new(paging.into(), Origin::Flag("--paging"))),
            (_, true, _) => Some(Setting::new(Paging::Always.into(), Origin::Flag("-p"))),
            (_, _, true) => Some(Setting::new(Paging::Never.into(), Origin::Flag("-P"))),
            _ => None,
        },
        front_matter: cli.front_matter.map(Into::into),
        table_rows: cli.table_rows.map(Into::into),
        diagrams: cli.diagrams.map(Into::into),
        render_errors: cli.render_errors.map(Into::into),
    }
}
