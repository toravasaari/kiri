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

use std::fmt::Display;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::config::{Environment, Passthrough, Settings};
use crate::present::kitty::graphics::{Cells, ImageId};
use crate::session::settings::Follow;
use crate::session::state::Look;
use crate::term::{Mode, WindowSize};
use crate::typeset::caps::Capabilities;
use crate::typeset::diagnostic::Diagnostic;
use crate::typeset::input::Input;

mod given;
mod loads;
mod open;
mod pager;
mod print;
mod route;

pub use route::{show_document, show_files, shown};

/// What the front ends reach of the world outside kiri: files, standard
/// input and output, the environment and the terminal. The binary
/// implements it for the process; `sim.rs` in memory, for the tests.
pub trait Platform {
    // Files
    /// Whether `path` is a directory, following links.
    fn is_dir(&self, path: &Path) -> bool;
    /// Whether `path` is a regular file, following links.
    fn is_file(&self, path: &Path) -> bool;
    /// How the file at `path` looks now (mtime and length), looked up by
    /// its path, so that a file replaced by another, as an editor saves
    /// through a rename, counts as changed; why it cannot be looked at
    /// otherwise.
    fn look(&self, path: &Path) -> Result<Look, String>;
    /// The file at `path`, opened for reading from its start.
    fn open(&self, path: &Path) -> io::Result<Box<dyn Read>>;

    // Standard streams
    /// Standard input.
    fn stdin(&self) -> Box<dyn Read>;
    /// Standard output, for printing documents.
    fn stdout(&self) -> Box<dyn Write + '_>;
    /// Standard error, for what kiri tells and its progress lines. Not
    /// buffered: what is told is there at once, in order with standard
    /// output.
    fn stderr(&self) -> Box<dyn Write + Send + '_>;

    // Environment
    /// The environment variables kiri reads.
    fn environment(&self) -> &Environment;
    /// The fonts reading mode lays text out in.
    fn reading_fonts(&self) -> crate::typeset::reading::Fonts;

    // Terminal
    /// Whether standard output is a terminal.
    fn stdout_is_terminal(&self) -> bool;
    /// Whether standard error is a terminal.
    fn stderr_is_terminal(&self) -> bool;
    /// The terminal's capabilities, as `--mode`, `--width` and
    /// `--passthrough` ask for them.
    fn capabilities(
        &self,
        mode: Mode,
        width: Option<u16>,
        passthrough: Passthrough,
    ) -> Capabilities;
    /// The window's size, as standard output's terminal tells it.
    fn window(&self) -> Option<WindowSize>;
    /// Which of the images `sent` the terminal lost, as
    /// [`crate::term::lost_images`] asks it; `None` when it did not
    /// answer.
    fn lost_images(&self, sent: &[(ImageId, Cells)]) -> Option<Vec<ImageId>>;
    /// The pager's screen, `None` without a terminal to read keys from.
    /// `caught` runs once the screen catches the signals.
    fn screen(
        &self,
        caught: Box<dyn FnOnce() + '_>,
        wrap: bool,
        mouse: bool,
    ) -> Option<Box<dyn Screen + '_>>;
}

/// The pager's screen: a [`crate::pager::Terminal`], with its clock, that
/// is left at the end.
pub trait Screen: crate::pager::Terminal {
    /// Leaves the screen and restores the terminal, runs `after`, and then
    /// ends kiri with a signal that asked it to end, if one came.
    fn close_then(self: Box<Self>, after: Box<dyn FnOnce() + '_>);
}

impl<W: Write> Screen for crate::term::Screen<W> {
    fn close_then(self: Box<Self>, after: Box<dyn FnOnce() + '_>) {
        (*self).close_then(after);
    }
}

/// Tells `line` on standard error as kiri's own, after `kiri: `, as
/// `eprintln!` does, and as it, panics should the write fail.
fn tell(platform: &dyn Platform, line: impl Display) {
    let line = format!("kiri: {line}\n");
    (platform.stderr().write_all(line.as_bytes())).expect("failed printing to stderr");
}

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
