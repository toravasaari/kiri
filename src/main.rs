use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use kiri::config::{Environment, ThemeSource};
use kiri::images::Images;
use kiri::term::Capabilities;
use kiri::theme::Theme;

/// Render a Markdown document in the terminal.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Markdown file to render.
    file: PathBuf,

    /// Width in cells; defaults to the terminal width.
    #[arg(long)]
    width: Option<u16>,

    /// Which terminal features to use. Colours stay off when NO_COLOR is set.
    #[arg(long, value_enum, default_value_t = Mode::Auto)]
    mode: Mode,

    /// Theme file, instead of the one the configuration chooses.
    #[arg(long)]
    theme: Option<PathBuf>,

    /// No margins, heading indents or code block padding, for copying text.
    #[arg(short = 'M', long)]
    no_margins: bool,

    /// Show images as their alt text, without reading them.
    #[arg(short = 'I', long)]
    no_images: bool,

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

fn main() -> ExitCode {
    let cli = Cli::parse();
    let source = match std::fs::read_to_string(&cli.file) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("kiri: {}: {error}", cli.file.display());
            return ExitCode::FAILURE;
        }
    };
    let env = Environment::from_process();
    let config = match kiri::config::load(&env) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("kiri: {error}");
            return ExitCode::FAILURE;
        }
    };
    let theme_source = kiri::config::theme_source(cli.theme.as_deref(), &env, &config);
    let theme = match load_theme(theme_source) {
        Ok(theme) if cli.no_margins => theme.without_margins(),
        Ok(theme) => theme,
        Err(error) => {
            eprintln!("kiri: {error}");
            return ExitCode::FAILURE;
        }
    };
    let caps = capabilities(&cli);
    let document = kiri::parse::parse(&source);
    // Images are read only when the user wants them and the terminal can
    // show them; otherwise they are alt text, without warnings.
    let images_on = kiri::config::images(cli.no_images, &env, &config);
    let images = if images_on && caps.draws_images() {
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
    let mut out = io::stdout().lock();
    match kiri::render::render(&document, &images, &caps, &theme, &mut out)
        .and_then(|()| out.flush())
    {
        Ok(()) => ExitCode::SUCCESS,
        // A closed pipe (kiri file.md | head) is not an error.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kiri: {error}");
            ExitCode::FAILURE
        }
    }
}

fn load_theme(source: ThemeSource) -> Result<Theme, String> {
    match source {
        ThemeSource::File(path) => kiri::theme::load(&path),
        ThemeSource::DefaultFile(path) if path.exists() => kiri::theme::load(&path),
        ThemeSource::DefaultFile(_) | ThemeSource::BuiltIn => Ok(Theme::default()),
    }
}

fn capabilities(cli: &Cli) -> Capabilities {
    let width = || cli.width.or_else(kiri::term::terminal_width).unwrap_or(80);
    let mut caps = match cli.mode {
        Mode::Auto => kiri::term::detect(cli.width),
        Mode::Plain => Capabilities::plain(width()),
        Mode::Ansi => Capabilities::ansi(width()),
        // The cell size is not a probe: the window size reports it.
        Mode::Kitty => Capabilities {
            cell_pixels: kiri::term::cell_pixels(),
            ..Capabilities::kitty(width())
        },
    };
    // In every mode, including the forced ones.
    if kiri::term::no_color() {
        caps.color = false;
    }
    caps
}
