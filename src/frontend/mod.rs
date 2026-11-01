//! The front ends: what shows the files that kiri is given, in the pager or
//! printed. They open the files and standard input, load the images and
//! diagrams the documents want, and run the pager's loop on the terminal.
//! The binary hands them what the command line asked for, `Asked`, and the
//! settings it resolved, `Configured`.
//!
//! What does not belong here is a decision that needs no I/O, such as
//! whether a run pages, which is in the state engine
//! (`src/session/plan.rs`), nor what is shown of a document, which the
//! typesetting engine and the terminal layer decide.
//!
//! A new thing that shows files goes in a file of its own here, not in
//! this one: the placement map in docs/architecture.md says which.

use std::path::PathBuf;

use crate::config::Settings;
use crate::session::settings::Follow;
use crate::typeset::diagnostic::Diagnostic;
use crate::typeset::input::Input;

mod given;
mod loads;
mod open;
mod pager;
mod print;
mod route;

pub use route::{show_document, show_files, shown};

/// Why kiri ends without doing what it was asked.
#[derive(Debug)]
pub enum Failure {
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

/// What the command line asks the front end for.
pub struct Asked<'a> {
    /// The files given; `-` is standard input.
    pub files: &'a [PathBuf],
    /// The input format `--format` names.
    pub format: Option<Input>,
    /// How the command line asks to follow, `Off` when it does not.
    pub follow: Follow,
}

/// The settings, as the binary resolved them from the command line, the
/// environment and the configuration file.
pub struct Configured {
    /// Warnings about them.
    pub warnings: Vec<Diagnostic>,
    /// The settings; or why the configuration file could not be read,
    /// with the settings without it.
    pub settings: Result<Settings, (String, Settings)>,
}
