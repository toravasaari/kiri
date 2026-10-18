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
    /// Markdown file to render, or an image to show on its own.
    #[arg(required_unless_present_any = ["doctor", "raw"])]
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
    let result = match &cli.file {
        Some(path) => run(&cli, path),
        None => doctor(&cli, &matches),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kiri: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli, path: &Path) -> Result<(), String> {
    let failed = |error: io::Error| format!("{}: {error}", path.display());
    let mut file = File::open(path).map_err(failed)?;
    let mut head = Vec::new();
    (&mut file)
        .take(kiri::input::HEAD as u64)
        .read_to_end(&mut head)
        .map_err(failed)?;
    let env = Environment::from_process();
    let input = cli
        .format
        .unwrap_or_else(|| kiri::input::detect(path, &head));
    if let Input::Image(format) = input {
        return show_image(cli, path, format, Cursor::new(head).chain(file), &env);
    }
    let mut bytes = head;
    file.read_to_end(&mut bytes).map_err(failed)?;
    let source = String::from_utf8(bytes).map_err(|_| match cli.format {
        Some(_) => format!("{}: not UTF-8 text", path.display()),
        None => format!(
            "{}: neither Markdown nor an image kiri knows",
            path.display()
        ),
    })?;
    let config = kiri::config::load(&env)?;
    let settings = kiri::config::settings(&options(cli), &env, &config)?;
    let caps = kiri::term::capabilities(cli.mode.into(), cli.width, &env);
    let document = kiri::parse::parse(&source);
    let base = path.parent().unwrap_or(Path::new(""));
    show_document(cli, &document, base, settings, caps)
}

/// Renders `document`, whose images are relative to `base`, on stdout.
fn show_document(
    cli: &Cli,
    document: &Document,
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
    let inputs = Inputs {
        images,
        theme: settings.theme,
        ..Inputs::new(caps)
    };
    let mut out = io::stdout().lock();
    finish(kiri::render::render(document, &inputs, &mut out).and_then(|()| out.flush()))
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
    let settings = Settings {
        theme: if choices.margins.value {
            theme
        } else {
            theme.without_margins()
        },
        paper: choices.paper,
        images: choices.images.value,
    };
    let document = kiri::parse::parse(&markdown);
    show_document(cli, &document, Path::new(""), settings, detection.caps)
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
    }
}
