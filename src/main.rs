use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// Render a Markdown document in the terminal.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Markdown file to render.
    file: PathBuf,
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
    let document = kiri::parse::parse(&source);
    let mut out = io::stdout().lock();
    match kiri::render::plain::render(&document, &mut out).and_then(|()| out.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        // A closed pipe (kiri file.md | head) is not an error.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kiri: {error}");
            ExitCode::FAILURE
        }
    }
}
