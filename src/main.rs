use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use kiri::config::{Environment, Options};
use kiri::images::Images;
use kiri::render::Inputs;
use kiri::theme::Paper;

/// Render a Markdown document in the terminal.
#[derive(Parser)]
#[command(
    version,
    after_help = "Images are drawn only in terminals that support kitty's \
                  graphics protocol with Unicode placeholders, such as kitty \
                  0.28 or later; elsewhere they show their alt text."
)]
struct Cli {
    /// Markdown file to render.
    file: PathBuf,

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
    match run(&Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kiri: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), String> {
    let source = std::fs::read_to_string(&cli.file)
        .map_err(|error| format!("{}: {error}", cli.file.display()))?;
    let env = Environment::from_process();
    let config = kiri::config::load(&env)?;
    let settings = kiri::config::settings(&options(cli), &env, &config)?;
    let mut caps = kiri::term::capabilities(cli.mode.into(), cli.width, &env);
    // The page decides, whatever the terminal's background is.
    if let Some(paper) = settings.paper {
        caps.dark_background = Some(paper == Paper::Dark);
    }
    let document = kiri::parse::parse(&source);
    // Images are read only when the user wants them and the terminal can
    // show them; otherwise they are alt text, without warnings.
    let images = if settings.images && caps.draws_images() {
        let base = cli.file.parent().unwrap_or(Path::new(""));
        let max_file_size = cli.max_image_size * kiri::images::MB;
        let images = kiri::images::load(&document, base, max_file_size);
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
    match kiri::render::render(&document, &inputs, &mut out).and_then(|()| out.flush()) {
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
