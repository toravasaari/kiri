use std::fs::File;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser, ValueEnum};
use kiri::config::{Environment, Options, Origin, Setting, Settings};
use kiri::doctor::{Facts, Output};
use kiri::images::{Fonts, Format, Images};
use kiri::input::Input;
use kiri::model::Document;
use kiri::render::Inputs;
use kiri::term::Capabilities;
use kiri::theme::{Paper, Theme};

/// Render a Markdown document, or show an image, in the terminal.
#[derive(Parser)]
#[command(
    version,
    after_help = "Images are drawn only in terminals that support kitty's \
                  graphics protocol with Unicode placeholders, such as kitty \
                  0.28 or later; elsewhere they show their alt text."
)]
struct Cli {
    /// Markdown file to render, an image to show on its own, or a directory
    /// to show its README. Without one, or with -, kiri reads standard
    /// input unless it is a terminal.
    file: Option<PathBuf>,

    /// Show what kiri finds out about the terminal and which settings it
    /// uses, as Markdown when written to a file, for a bug report.
    #[arg(long, conflicts_with_all = ["file", "format"])]
    doctor: bool,

    /// With --doctor: the report as Markdown also in a terminal, without
    /// its sample, to copy into a bug report.
    #[arg(long)]
    raw: bool,

    /// What the file is: markdown, image, or an image format such as svg,
    /// png or jpg. By default its extension and content tell.
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

    /// Show images as their alt text, without reading them.
    #[arg(short = 'I', long)]
    no_images: bool,

    /// Show images, whatever KIRI_NO_IMAGES and the configuration file say.
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
        "give markdown, image, or an image format such as svg, png or jpg".to_owned()
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
        Err(error) => {
            eprintln!("kiri: {error}");
            ExitCode::FAILURE
        }
    }
}

/// What errors and warnings call standard input. Without an extension, so
/// that the content tells what standard input is, as for such a file.
const STDIN: &str = "<stdin>";

/// Shows the file at `path`, the README of a directory there, or standard
/// input without one.
fn run(cli: &Cli, file: Option<&Path>) -> Result<(), String> {
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
        return show_image(cli, path, format, Cursor::new(head).chain(reader), &env);
    }
    let mut bytes = head;
    reader.read_to_end(&mut bytes).map_err(failed)?;
    let source = String::from_utf8(bytes).map_err(|_| match cli.format {
        Some(_) => format!("{}: not UTF-8 text", path.display()),
        None => format!(
            "{}: neither Markdown nor an image kiri knows",
            path.display()
        ),
    })?;
    let config = kiri::config::load(&env)?;
    let choices = kiri::config::choose(&options(cli), &env, &config);
    for warning in &choices.warnings {
        eprintln!("kiri: {warning}");
    }
    let mut settings = choices.with_theme(kiri::config::load_theme(&choices.theme.value)?);
    if file.is_none() {
        settings.paging = only_always(settings.paging);
    }
    let caps = kiri::term::capabilities(cli.mode.into(), cli.width, &env);
    let document = kiri::parse::parse(&source);
    // Images in a piped document are relative to the current directory.
    let base = file.and_then(Path::parent).unwrap_or(Path::new(""));
    show_document(
        cli,
        &document,
        &path.to_string_lossy(),
        base,
        settings,
        caps,
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

/// Renders `document`, called `name`, whose images are relative to
/// `base`, on stdout.
fn show_document(
    cli: &Cli,
    document: &Document,
    name: &str,
    base: &Path,
    settings: Settings,
    mut caps: Capabilities,
) -> Result<(), String> {
    // The page decides, whatever the terminal's background is.
    if let Some(paper) = settings.paper {
        caps.dark_background = Some(paper == Paper::Dark);
    }
    // Images are read only when the user wants them and the terminal can
    // show them; otherwise they are alt text, without warnings.
    let images = if settings.images && caps.draws_images() {
        let max_file_size = cli.max_image_size * kiri::images::MB;
        let images = kiri::images::load(document, base, max_file_size);
        for warning in images.warnings() {
            eprintln!("kiri: {warning}");
        }
        images
    } else {
        Images::new()
    };
    let mut inputs = Inputs {
        images,
        theme: settings.theme,
        ..Inputs::new(caps)
    };
    if let Some(result) = page(
        cli,
        document,
        name,
        &mut inputs,
        settings.paging,
        settings.search_case,
    ) {
        return finish(result);
    }
    let mut out = io::stdout().lock();
    finish(kiri::render::render(document, &inputs, &mut out).and_then(|()| out.flush()))
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
/// printed instead. A search in the pager treats case as `case` says.
fn page(
    cli: &Cli,
    document: &Document,
    name: &str,
    inputs: &mut Inputs,
    paging: kiri::config::Paging,
    case: kiri::config::SearchCase,
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
    let Some(mut screen) = kiri::term::Screen::open() else {
        // Without a terminal to read keys from, as in the background, the
        // document is printed; only a reader who asked for the pager is
        // told.
        if paging == kiri::config::Paging::Always {
            eprintln!("kiri: no terminal to read keys from, so the document is printed");
        }
        return None;
    };
    let result = kiri::pager::run(document, name, inputs, cli.width, case, &mut screen);
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
fn doctor(cli: &Cli, matches: &ArgMatches) -> Result<(), String> {
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
        return finish(
            out.write_all(markdown.as_bytes())
                .and_then(|()| out.flush()),
        );
    }
    let document = kiri::parse::parse(&markdown);
    let mut settings = choices.with_theme(theme);
    settings.paging = only_always(settings.paging);
    show_document(
        cli,
        &document,
        "kiri --doctor",
        Path::new(""),
        settings,
        detection.caps,
    )
}

/// Where the value of the argument `id`, the flag `flag`, comes from.
fn origin(matches: &ArgMatches, id: &str, flag: &'static str) -> Origin {
    match matches.value_source(id) {
        Some(ValueSource::CommandLine) => Origin::Flag(flag),
        _ => Origin::Default,
    }
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
    }
}
