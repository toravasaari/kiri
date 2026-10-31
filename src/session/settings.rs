//! The types of the settings that decide how the pager works: when it
//! opens, whether it waits for the images, how a search treats case, and
//! whether and where it follows a file. The platform's `src/config/`
//! reads them from the flags, the environment and the file; the state
//! engine only uses the values.

/// When the document is shown in the pager rather than printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Paging {
    /// Whenever the output is a terminal.
    Always,
    /// When the output is a terminal and the document is longer than the
    /// window, unless it comes from standard input or is the `--doctor`
    /// report.
    #[default]
    Auto,
    /// Never: the document is printed.
    Never,
}

/// How a search in the pager treats upper and lower case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchCase {
    /// A search term with no upper-case letter ignores case; one with an
    /// upper-case letter matches case exactly.
    #[default]
    Smart,
    /// Case matches exactly.
    Literal,
}

/// Whether the pager follows a lone file, reading it again whenever it
/// changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Follow {
    #[default]
    Off,
    /// Read again, with the view kept where it is.
    On,
    /// Read again, with the view moved to a change.
    Auto,
}

/// Which change auto-follow moves the view to, after a reload that
/// changed the document in more than one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FollowJump {
    /// The first change in the document.
    #[default]
    First,
    /// The last change in the document.
    Last,
    /// The change that takes the most rows.
    Most,
}

/// Whether the pager, when it surely opens, waits for the images and
/// diagrams of the document before it shows it, or shows the text at
/// once while they load in the background, as after a reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InitialLoad {
    /// Every image is loaded before the document shows.
    #[default]
    Wait,
    /// The text shows at once, and the images as they come.
    Background,
}
