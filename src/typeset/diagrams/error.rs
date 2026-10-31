//! Why a diagram could not be drawn.

use std::fmt;
use std::time::Duration;

/// Why a diagram could not be drawn. Each variant is a failure that kiri
/// itself detects, apart from `Message`, which carries what a renderer
/// says. `Display` gives the text a warning and the line below the code
/// block show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagramError {
    /// What a renderer says of its own, such as RaTeX's or merman's
    /// message.
    Message(String),
    /// No program is set for the language; `language` is as the info
    /// string gives it.
    NoCommand(String),
    /// kiri has no renderer of its own for the language in this build.
    NotInBuild(String),
    /// No renderer is set at all, so no language is drawn.
    NoRenderer(String),
    /// The diagram was never drawn: no thread was left to draw it.
    NotDrawn,
    /// Kiri was told to stop before `program` started.
    NotStarted { program: String },
    /// The directory for the program's files could not be made; `error`
    /// is the system's reason.
    NoDirectory { program: String, error: String },
    /// The program's files could not be written; `error` is the system's
    /// reason.
    NoFiles { program: String, error: String },
    /// The program could not be started; `error` is the system's reason.
    CannotStart { program: String, error: String },
    /// The program did not finish within the time limit.
    TimedOut { program: String, limit: Duration },
    /// The program was stopped because kiri was told to stop.
    Interrupted { program: String },
    /// Waiting for the program failed; `error` is the system's reason.
    WaitFailed { program: String, error: String },
    /// The program ended with an exit status that is not success; `said`
    /// is the first lines it wrote on standard error, if any.
    Exited {
        program: String,
        code: i32,
        said: Option<String>,
    },
    /// A signal stopped the program; `said` is as for `Exited`.
    Signalled {
        program: String,
        said: Option<String>,
    },
    /// The program wrote an image larger than the limit, which `limit`
    /// gives as people read it, such as `10 MB`.
    TooLarge { program: String, limit: String },
    /// The program wrote no output file.
    NoOutput { program: String },
    /// The program's output file could not be read; `error` is the
    /// system's reason.
    Unreadable { program: String, error: String },
}

impl fmt::Display for DiagramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiagramError::Message(message) => f.write_str(message),
            DiagramError::NoCommand(language) => {
                write!(f, "{language} diagrams have no command")
            }
            DiagramError::NotInBuild(language) => {
                write!(f, "{language} diagrams are not drawn in this build")
            }
            DiagramError::NoRenderer(language) => {
                write!(f, "{language} diagrams are not drawn")
            }
            DiagramError::NotDrawn => f.write_str("not drawn"),
            DiagramError::NotStarted { program } => {
                write!(f, "{program} was not started: interrupted")
            }
            DiagramError::NoDirectory { program, error } => {
                write!(f, "cannot make a directory for {program}: {error}")
            }
            DiagramError::NoFiles { program, error } => {
                write!(f, "cannot write the files for {program}: {error}")
            }
            DiagramError::CannotStart { program, error }
            | DiagramError::WaitFailed { program, error } => write!(f, "{program}: {error}"),
            DiagramError::TimedOut { program, limit } => {
                write!(f, "{program} did not finish in {} s", limit.as_secs_f32())
            }
            DiagramError::Interrupted { program } => {
                write!(f, "{program} was stopped: interrupted")
            }
            DiagramError::Exited {
                program,
                code,
                said,
            } => {
                write!(f, "{program} exited with status {code}")?;
                said_after(f, said)
            }
            DiagramError::Signalled { program, said } => {
                write!(f, "{program} was stopped by a signal")?;
                said_after(f, said)
            }
            DiagramError::TooLarge { program, limit } => {
                write!(f, "{program} wrote an image larger than the {limit} limit")
            }
            DiagramError::NoOutput { program } => write!(f, "{program} wrote no output file"),
            DiagramError::Unreadable { program, error } => {
                write!(f, "cannot read what {program} wrote: {error}")
            }
        }
    }
}

/// What the program said, after how it ended.
fn said_after(f: &mut fmt::Formatter<'_>, said: &Option<String>) -> fmt::Result {
    match said {
        Some(said) => write!(f, ": {said}"),
        None => Ok(()),
    }
}

impl std::error::Error for DiagramError {}

impl From<String> for DiagramError {
    fn from(message: String) -> Self {
        DiagramError::Message(message)
    }
}

impl From<&str> for DiagramError {
    fn from(message: &str) -> Self {
        DiagramError::Message(message.to_owned())
    }
}
