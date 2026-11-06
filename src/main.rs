//! The kiri binary: the platform's entry point. It reads the command line
//! (`cli.rs`) and resolves the settings from it, the environment and the
//! configuration file, and hands them to the frontends in the library
//! (`src/frontend/`), which show the files. It wires the parts of the
//! library together and does the I/O that they must not do.
//!
//! What does not belong here is a decision that needs no I/O, such as
//! whether a run prints, nor a rule about a document, a key or a setting.
//! A new function is a sign that the code has no place yet, not a place.
//!
//! Such decisions go to the state engine (`src/session/`), settings to
//! `src/config/`, and bytes for the terminal to the terminal layer
//! (`src/present/`). The placement map in docs/architecture.md says which.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{CommandFactory, FromArgMatches};
use kiri::config::{Environment, Passthrough};
use kiri::frontend::{Asked, Configured, Failure, Platform, Screen};
use kiri::present::kitty::graphics::{Cells, ImageId};
use kiri::session::plan::{self, Route};
use kiri::session::state::Look;
use kiri::term::{Mode, WindowSize};
use kiri::typeset::caps::Capabilities;

mod cli;
mod reports;

use cli::{Cli, completions, followed, options};
use reports::{doctor, list_emojis};

fn main() -> ExitCode {
    let real = Real {
        env: Environment::from_process(),
    };
    let mut command = Cli::command();
    let matches = command.get_matches_mut();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    // clap's `requires` takes --doctor's default value for the flag itself.
    if cli.raw && !cli.doctor {
        command
            .error(ErrorKind::MissingRequiredArgument, "--raw needs --doctor")
            .exit();
    }
    let result = if let Some(shell) = cli.completions {
        completions(&mut command, shell, &mut io::stdout().lock())
    } else if cli.doctor {
        doctor(&real, &cli)
    } else if cli.list_emojis {
        list_emojis(&real, &cli)
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
            route => run(&real, &cli, route),
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

/// Shows the files `route` names, as the command line asks, with the
/// settings it resolves.
fn run(real: &Real, cli: &Cli, route: Route) -> Result<(), Failure> {
    let resolved = kiri::config::resolve_for_run(&options(cli), real.environment());
    let configured = Configured {
        warnings: resolved.warnings,
        settings: resolved.settings,
        reading_named: resolved.reading_named,
    };
    let asked = Asked {
        files: &cli.files,
        format: cli.format,
        follow: followed(cli),
    };
    kiri::frontend::show_files(real, &asked, route, configured)
}

/// The process: its files, standard streams, environment and terminal.
struct Real {
    env: Environment,
}

impl Platform for Real {
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn look(&self, path: &Path) -> Result<Look, String> {
        let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
        Ok(Look {
            modified: metadata.modified().ok(),
            length: metadata.len(),
        })
    }

    fn open(&self, path: &Path) -> io::Result<Box<dyn Read>> {
        Ok(Box::new(File::open(path)?))
    }

    fn stdin(&self) -> Box<dyn Read> {
        Box::new(io::stdin().lock())
    }

    fn stdout(&self) -> Box<dyn Write + '_> {
        Box::new(io::stdout().lock())
    }

    fn stderr(&self) -> Box<dyn Write + Send + '_> {
        Box::new(io::stderr())
    }

    fn environment(&self) -> &Environment {
        &self.env
    }

    fn reading_fonts(&self) -> kiri::typeset::reading::Fonts {
        kiri::typeset::reading::Fonts::system()
    }

    fn stdout_is_terminal(&self) -> bool {
        kiri::term::stdout_is_terminal()
    }

    fn stderr_is_terminal(&self) -> bool {
        kiri::term::stderr_is_terminal()
    }

    fn capabilities(
        &self,
        mode: Mode,
        width: Option<u16>,
        passthrough: Passthrough,
    ) -> Capabilities {
        kiri::term::capabilities(mode, width, &self.env, passthrough)
    }

    fn window(&self) -> Option<WindowSize> {
        kiri::term::window_size(io::stdout())
    }

    fn lost_images(&self, sent: &[(ImageId, Cells)]) -> Option<Vec<ImageId>> {
        kiri::term::lost_images(sent)
    }

    fn screen(
        &self,
        (settle, caught): (Box<dyn FnOnce() + '_>, Box<dyn FnOnce() + '_>),
        wrap: bool,
        mouse: bool,
    ) -> Option<Box<dyn Screen + '_>> {
        let screen = kiri::term::Screen::open(settle, caught, wrap, mouse)?;
        Some(Box::new(screen))
    }
}
