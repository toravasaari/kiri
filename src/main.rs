//! The kiri binary: the platform's entry point. It reads the command line
//! (`cli.rs`) and resolves the settings from it, the environment and the
//! configuration file, and hands them to the front ends in the library
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

use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{CommandFactory, FromArgMatches};
use kiri::config::{Environment, Settings};
use kiri::frontend::{Asked, Configured, Failure};
use kiri::session::plan::{self, Route};
use kiri::typeset::diagnostic::Diagnostic;
use kiri::typeset::theme::Theme;

mod cli;
mod reports;

use cli::{Cli, followed, options};
use reports::{doctor, list_emojis};

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
        doctor(&cli)
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

/// Shows the files `route` names, as the command line asks, with the
/// settings it resolves.
fn run(cli: &Cli, route: Route) -> Result<(), Failure> {
    let env = Environment::from_process();
    let (warnings, settings) = settings(cli, &env);
    let configured = Configured {
        warnings,
        settings: settings.map_err(|error| (error, without_config(cli, &env))),
    };
    let asked = Asked {
        files: &cli.files,
        format: cli.format,
        follow: followed(cli),
    };
    kiri::frontend::show_files(&asked, route, configured)
}

/// The settings for documents, from the command line, the environment and
/// the configuration file, and warnings about them.
fn settings(cli: &Cli, env: &Environment) -> (Vec<Diagnostic>, Result<Settings, String>) {
    match kiri::config::load(env) {
        Ok(config) => kiri::config::settings(&options(cli), env, &config),
        Err(error) => (Vec::new(), Err(error)),
    }
}

/// The settings from the command line and the environment alone, for a
/// configuration file that cannot be read, with the default theme.
fn without_config(cli: &Cli, env: &Environment) -> Settings {
    let choices = kiri::config::choose(&options(cli), env, &Default::default());
    choices.with_theme(Theme::default())
}
