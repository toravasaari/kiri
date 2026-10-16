use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
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

    /// Which terminal features to use.
    #[arg(long, value_enum, default_value_t = Mode::Auto)]
    mode: Mode,
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
    let caps = capabilities(&cli);
    let document = kiri::parse::parse(&source);
    let mut out = io::stdout().lock();
    match kiri::render::render(&document, &caps, &Theme::default(), &mut out)
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

fn capabilities(cli: &Cli) -> Capabilities {
    let width = || cli.width.or_else(kiri::term::terminal_width).unwrap_or(80);
    match cli.mode {
        Mode::Auto => kiri::term::detect(cli.width),
        Mode::Plain => Capabilities::plain(width()),
        Mode::Ansi => Capabilities::ansi(width()),
        Mode::Kitty => Capabilities::kitty(width()),
    }
}
