//! Diagnostics: what kiri found wrong while it read, loaded or drew a
//! document, and tells the reader without stopping.
//!
//! A bad setting, front matter that does not parse, an image that cannot
//! be read, a diagram that does not draw and reading mode that cannot be
//! shown are all [`Diagnostic`]s. Whoever finds the trouble makes one;
//! whoever shows it turns it into text: [`Display`](std::fmt::Display) is
//! the line that goes to standard error, and [`Diagnostic::pager_message`]
//! the text of the pager's warning line.

use std::fmt;

use crate::typeset::model::FrontMatterError;

/// What kind of trouble a diagnostic is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// A setting that was left out, such as `KIRI_PAGING=joo`.
    Config,
    /// Front matter that could not be read.
    FrontMatter,
    /// Reading mode was asked for and cannot be shown.
    ReadingMode,
    /// An image that could not be loaded.
    Image,
    /// A diagram that could not be drawn.
    Diagram,
    /// A diagram that no program is there to draw.
    NoDiagramProgram,
    /// A Markdown element that kiri has no way to show.
    Markdown,
}

/// One thing kiri found wrong, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    /// The document it belongs to: set when it is told among several
    /// files, and for a diagram file shown on its own; `None` for a
    /// document alone and for trouble that is not in a file.
    pub file: Option<String>,
    /// The image or diagram it is about, as the text names it: a path, a
    /// URL, `mermaid diagram on line 3`; `None` when there is none.
    pub subject: Option<String>,
    /// The line of the document it is at, counting from 1, when known.
    pub line: Option<usize>,
    /// What is wrong, as one line of text, without the file's name and
    /// the subject. For front matter it is the parser's message, without
    /// a position.
    pub message: String,
}

impl Diagnostic {
    /// A diagnostic of `code` that says `message`, about no file in
    /// particular.
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            file: None,
            subject: None,
            line: None,
            message: message.into(),
        }
    }

    /// The same diagnostic, about the image or diagram `subject`.
    #[must_use]
    pub fn about(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// The same diagnostic, about the file `file`.
    #[must_use]
    pub fn in_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    /// The same diagnostic, told as it is for a file on its own: without
    /// the file's name.
    #[must_use]
    pub fn unnamed(mut self) -> Self {
        self.file = None;
        self
    }

    /// The text of the pager's warning line, which puts the line before
    /// it: front matter says what it is, and the rest is its subject and
    /// message. It leaves the file out; the pager names the file itself.
    pub fn pager_message(&self) -> String {
        match self.code {
            Code::FrontMatter => format!("front matter: {}", self.message),
            _ => self.about_text(),
        }
    }

    /// The subject and the message, without the file's name.
    fn about_text(&self) -> String {
        match &self.subject {
            Some(subject) => format!("{subject}: {}", self.message),
            None => self.message.clone(),
        }
    }

    /// The text without the file's name, as standard error tells it.
    fn text(&self) -> String {
        match (self.code, self.line) {
            (Code::FrontMatter, Some(line)) => {
                format!("front matter, line {line}: {}", self.message)
            }
            (Code::FrontMatter, None) => format!("front matter: {}", self.message),
            (Code::Markdown, Some(line)) => format!("{} (line {line})", self.message),
            _ => self.about_text(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.file {
            Some(file) => write!(f, "{file}: {}", self.text()),
            None => f.write_str(&self.text()),
        }
    }
}

impl From<&FrontMatterError> for Diagnostic {
    fn from(error: &FrontMatterError) -> Self {
        Self {
            line: error.line,
            ..Self::new(Code::FrontMatter, error.message.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn front_matter(line: Option<usize>) -> Diagnostic {
        Diagnostic::from(&FrontMatterError {
            line,
            message: "bad".to_owned(),
        })
    }

    #[test]
    fn front_matter_tells_its_line_on_standard_error_and_not_in_the_pager_text() {
        let error = front_matter(Some(4));
        assert_eq!(error.to_string(), "front matter, line 4: bad");
        assert_eq!(error.pager_message(), "front matter: bad");
        assert_eq!(error.line, Some(4));
        let error = front_matter(None);
        assert_eq!(error.to_string(), "front matter: bad");
        assert_eq!(error.pager_message(), "front matter: bad");
    }

    #[test]
    fn other_diagnostics_say_their_message_as_it_is() {
        let error = Diagnostic::new(Code::Image, "a.png: file not found");
        assert_eq!(error.to_string(), "a.png: file not found");
        assert_eq!(error.pager_message(), "a.png: file not found");
    }

    #[test]
    fn a_subject_comes_before_the_message_and_after_the_file() {
        let error = Diagnostic::new(Code::Image, "file not found").about("a.png");
        assert_eq!(error.to_string(), "a.png: file not found");
        assert_eq!(error.pager_message(), "a.png: file not found");
        let error = error.in_file("c.md");
        assert_eq!(error.to_string(), "c.md: a.png: file not found");
        // The pager names the file itself, so its text has none.
        assert_eq!(error.pager_message(), "a.png: file not found");
    }

    #[test]
    fn a_file_is_named_before_the_text() {
        let error = front_matter(Some(2)).in_file("c.md");
        assert_eq!(error.to_string(), "c.md: front matter, line 2: bad");
        assert_eq!(error.unnamed().to_string(), "front matter, line 2: bad");
        let error = Diagnostic::new(Code::Diagram, "sh failed").in_file("d.mmd");
        assert_eq!(error.to_string(), "d.mmd: sh failed");
    }
}
