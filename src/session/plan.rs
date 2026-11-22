//! The startup plan: the decisions a run makes before it shows anything,
//! such as whether a document is paged, and when its images load. They
//! need no I/O. The frontends in `src/frontend/` read the terminal and the
//! files through `Platform` (implemented for the process in `src/main.rs`)
//! and pass the answers in as values.

use std::path::{Path, PathBuf};

use crate::session::settings::{Follow, InitialLoad, Paging};
use crate::typeset::caps::{Capabilities, Graphics, Playback};
use crate::typeset::diagnostic::{Code, Diagnostic};
use crate::typeset::input::Input;
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::page::Wide;
use crate::typeset::theme::Paper;

/// Whether a document of `rows` rows is paged: never when the output is not
/// a terminal, and with `Auto` only when it is longer than the window's
/// `height`, when that is known.
pub fn pages(paging: Paging, terminal: bool, rows: usize, height: Option<u16>) -> bool {
    match paging {
        _ if !terminal => false,
        Paging::Always => true,
        Paging::Auto => height.is_some_and(|height| rows > usize::from(height)),
        Paging::Never => false,
    }
}

/// `paging` for what is paged only when paging is `always`: a document
/// read from standard input, often the output of another command, and the
/// `--doctor` report. Everything else follows one rule: with `auto`, what
/// the run prints is paged when it is longer than the window (see
/// [`pages`]).
pub fn only_always(paging: Paging) -> Paging {
    match paging {
        Paging::Always => Paging::Always,
        Paging::Auto | Paging::Never => Paging::Never,
    }
}

/// Sets reading mode, when it is on, for printing: wide code and tables
/// wrap, as nothing can scroll them, and headings keep their place, as
/// nothing jumps to them.
pub fn printing(reading: Option<&mut Reading>) {
    if let Some(reading) = reading {
        reading.wide = Wide::Wrap;
        reading.snap = false;
    }
}

/// `caps` for printing the files: an animated image shows its first
/// frame, as kitty would play it on after kiri has gone and Ghostty cannot.
pub fn printing_still(mut caps: Capabilities) -> Capabilities {
    if let Graphics::Kitty(kitty) = &mut caps.graphics {
        kitty.playback = Playback::None;
    }
    caps
}

/// `caps` for a page: the paper theme, when the theme is one, says whether
/// the background is dark.
pub fn on_page(mut caps: Capabilities, paper: Option<Paper>) -> Capabilities {
    if let Some(paper) = paper {
        caps.dark_background = Some(paper == Paper::Dark);
    }
    caps
}

/// Whether reading mode is shown: when it is asked for and the terminal
/// draws images.
pub fn reading(asked: bool, caps: &Capabilities) -> bool {
    asked && caps.draws_images()
}

/// Why reading mode is not shown, when it is asked for and the terminal
/// cannot draw images.
pub fn reading_warning(asked: bool, caps: &Capabilities) -> Option<Diagnostic> {
    (asked && !caps.draws_images()).then(|| {
        Diagnostic::new(
            Code::ReadingMode,
            "reading mode needs a terminal that draws images; showing the document as text",
        )
    })
}

/// Whether the pager opens whatever the files are, which is known before
/// they are laid out: paging is `always` and the output is a terminal that
/// takes escape sequences. With `auto` it depends on how long the files
/// are. `terminal` tells whether the output is one.
pub fn opens_surely(paging: Paging, terminal: bool, caps: &Capabilities) -> bool {
    paging == Paging::Always && terminal && caps.styling
}

/// Whether the images of a document may still load once the pager has
/// opened, rather than before: in the background, or when the reader
/// skips the wait for them. That is when the settings show images that
/// `caps` can show and may page the document, as paging is `always` or
/// `auto` and the output a terminal that takes escape sequences;
/// [`surely_paged`] tells for `auto`.
pub fn may_load_later(paging: Paging, images: bool, terminal: bool, caps: &Capabilities) -> bool {
    matches!(paging, Paging::Always | Paging::Auto)
        && terminal
        && caps.styling
        && images
        && caps.draws_images()
}

/// What the layout of a document comes to, for [`surely_paged`]: its rows,
/// and the window's height when it is known.
pub struct Measured {
    pub rows: usize,
    pub height: Option<u16>,
}

/// Whether a document surely goes to the pager with `paging`: always with
/// `always`, and with `auto` when it is longer than the window. `measure`
/// is called only for `auto`, as measuring takes time. The callers pass
/// no other paging here, so `never` counts as `auto`.
pub fn surely_paged(paging: Paging, measure: impl FnOnce() -> Measured) -> bool {
    match paging {
        Paging::Always => true,
        Paging::Auto | Paging::Never => {
            let Measured { rows, height } = measure();
            pages(Paging::Auto, true, rows, height)
        }
    }
}

/// When the images of a document load, against the pager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Load {
    /// Before the document is shown: the pager is not sure to open, so the
    /// images load first.
    Now,
    /// Before the pager opens, while the reader may skip the wait and let
    /// the rest load in the background. Where no keys can be read, the
    /// images load now, as with `Now`.
    Wait,
    /// In the background, once the pager has opened.
    Later,
}

/// When the images load, for a document that `surely` goes to the pager
/// or not, as `initial_load` says.
pub fn load(surely: bool, initial_load: InitialLoad) -> Load {
    match (surely, initial_load) {
        (false, _) => Load::Now,
        (true, InitialLoad::Wait) => Load::Wait,
        (true, InitialLoad::Background) => Load::Later,
    }
}

/// `paging` for a document read from standard input, which is often the
/// output of another command: only `always` pages it.
pub fn for_standard_input(paging: Paging) -> Paging {
    only_always(paging)
}

/// `paging` for a file that is followed, which is read again when it
/// changes. Following pages a file, however short, as `-p` does, unless
/// paging is never; a file that cannot be read again is not followed.
pub fn for_follow(paging: Paging, follow: Follow, reloadable: bool) -> Paging {
    match paging {
        Paging::Auto if reloadable && follow != Follow::Off => Paging::Always,
        _ => paging,
    }
}

/// What the files on the command line make a run show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// One file, shown on its own.
    One,
    /// Standard input: the one file `-`, or none given while standard input
    /// is not a terminal.
    Stdin,
    /// Several files, one after another.
    Several,
    /// None given, while standard input is a terminal.
    Missing,
    /// Standard input, `-`, given more than once.
    RepeatedStdin,
}

/// The route for `paths`, the files given on the command line, with
/// `stdin_is_terminal` telling whether standard input is one.
pub fn route(paths: &[PathBuf], stdin_is_terminal: bool) -> Route {
    let stdin = Path::new("-");
    match paths {
        [path] if path != stdin => Route::One,
        [_] => Route::Stdin,
        [] if !stdin_is_terminal => Route::Stdin,
        [] => Route::Missing,
        paths if paths.iter().filter(|path| *path == stdin).count() > 1 => Route::RepeatedStdin,
        _ => Route::Several,
    }
}

/// Whether a file of `input` is shown on its own, as an image is: an image
/// or a diagram, which nobody reads again.
pub fn shown_alone(input: Input) -> bool {
    matches!(input, Input::Image(_) | Input::Diagram(_))
}

impl Route {
    /// Whether the run shows one file, or standard input, on its own,
    /// without a header.
    pub fn is_alone(self) -> bool {
        matches!(self, Route::One | Route::Stdin)
    }
}

/// What the files of a run are, for choosing their paging and what is told.
#[derive(Debug, Clone, Copy)]
pub struct Files {
    pub route: Route,
    /// Every file is an image.
    pub images_only: bool,
    /// The one file is a document read from a regular file, which can be
    /// read again.
    pub reloadable: bool,
    /// The one file is shown as text, whatever its extension says.
    pub text: bool,
    /// Every file is a table (CSV or TSV) or shown as text, none a document.
    pub plain: bool,
}

/// Whether reading mode is on for `files`, given the setting `reading` and
/// whether it was `named`: not for files that are only tables or text, which
/// keep to the text layout, as it is the better one for copying, unless the
/// reader named reading mode. A document among the files keeps it on for all
/// of them, as it is one setting of the run.
pub fn reading_for(reading: bool, named: bool, files: &Files) -> bool {
    reading && (named || !files.plain)
}

/// Whether diagrams are drawn for `files`, given the setting `diagrams`: not
/// in a file alone that is shown as text, whose diagram stays source, though
/// it is reloaded or its pictures loaded later.
pub fn diagrams(diagrams: bool, files: &Files) -> bool {
    diagrams && !(files.route.is_alone() && files.text)
}

/// `paging` for `files`, with `follow` for a file that is followed. One rule
/// covers every file and every mix of them, images and diagrams too: the
/// setting, and with `auto` the rows decide (see [`pages`]). Standard input
/// is paged only with `always`, and one file is paged as [`for_follow`]
/// says.
pub fn paging(paging: Paging, files: &Files, follow: Follow) -> Paging {
    match files.route {
        Route::Stdin => for_standard_input(paging),
        Route::Several => paging,
        _ => for_follow(paging, follow, files.reloadable),
    }
}

/// Whether to tell that reading mode is not shown, when it is `asked` for:
/// not for a file `alone` that is `bare`, an image, a diagram or text written
/// as it is, unless the pager surely `opens` (see [`opens_surely`]): such a
/// file is shown on its own, and reading mode is no part of showing it.
pub fn tells_reading(asked: bool, alone: bool, bare: bool, opens: bool) -> bool {
    asked && !(alone && bare && !opens)
}

/// Whether the warnings about the settings are told, given whether the pager
/// surely `opens` for `files` (see [`opens_surely`]): always, but for an
/// image alone that is printed, which is shown without them.
pub fn tells_warnings(files: &Files, opens: bool) -> bool {
    opens || !(files.route.is_alone() && files.images_only)
}

#[cfg(test)]
mod tests;
