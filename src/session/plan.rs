//! The startup plan: the decisions a run makes before it shows anything,
//! such as whether a document is paged, and when its images load. They
//! need no I/O. The frontends in `src/frontend/` read the terminal and the
//! files through `Platform` (implemented for the process in `src/main.rs`)
//! and pass the answers in as values.

use std::path::{Path, PathBuf};

use crate::session::settings::{Animations, Follow, InitialLoad, Paging};
use crate::typeset::caps::{Capabilities, Graphics, Playback};
use crate::typeset::diagnostic::{Code, Diagnostic};
use crate::typeset::input::Input;
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
/// read from standard input, often the output of another command, the
/// `--doctor` report, and images shown on their own.
pub fn only_always(paging: Paging) -> Paging {
    match paging {
        Paging::Always => Paging::Always,
        Paging::Auto | Paging::Never => Paging::Never,
    }
}

/// `paging` for several files: when `alone`, each of them is shown
/// on its own as an image is, such as an image file or a diagram file, and
/// they are paged only when paging is `always`; with a document among
/// them, the files follow `paging` as one document does.
pub fn for_files(paging: Paging, alone: bool) -> Paging {
    match alone {
        true => only_always(paging),
        false => paging,
    }
}

/// Sets reading mode's wide code and tables, when reading mode is on, for
/// printing: they wrap, as nothing can scroll them.
pub fn printing(wide: Option<&mut Wide>) {
    if let Some(wide) = wide {
        *wide = Wide::Wrap;
    }
}

/// `caps` for printing the files, with the animated images playing as
/// `animations` says when it was `named`, by the flag, the variable or the
/// configuration file, and showing their first frame otherwise: kitty
/// plays them on after kiri has gone, and Ghostty cannot.
pub fn printing_animations(
    mut caps: Capabilities,
    animations: Animations,
    named: bool,
) -> Capabilities {
    let playback = match (animations, named, caps.playback()) {
        (Animations::Never, _, _) | (_, false, _) => Playback::None,
        (_, _, Playback::Terminal | Playback::Started { .. }) => Playback::Started {
            once: animations == Animations::Once,
        },
        (..) => Playback::None,
    };
    if let Graphics::Kitty(kitty) = &mut caps.graphics {
        kitty.playback = playback;
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

/// Whether a file shown on its own as an image is, an image file or a
/// diagram file, goes to the pager on a terminal: only when paging is
/// `always`, as for several such files. `terminal` tells whether the
/// output is one.
pub fn pages_alone(paging: Paging, terminal: bool, caps: &Capabilities) -> bool {
    for_files(paging, true) == Paging::Always && terminal && caps.styling
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
/// or a diagram, which are paged only as an image is, and read again by
/// nobody.
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
    /// Every file is shown on its own, as an image is: an image or a
    /// diagram.
    pub alone: bool,
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

/// `paging` for `files`, with `follow` for a file that is followed. Several
/// files follow [`for_files`]. Standard input is paged only with `always`.
/// For one file, an image is paged only with `always`, a diagram follows the
/// setting until it is drawn, and a document is paged as [`for_follow`] says.
pub fn paging(paging: Paging, files: &Files, follow: Follow) -> Paging {
    match files.route {
        Route::Several => for_files(paging, files.alone),
        Route::Stdin => for_standard_input(paging),
        _ if files.images_only => only_always(paging),
        _ if files.alone => paging,
        _ => for_follow(paging, follow, files.reloadable),
    }
}

/// Whether the files of a run stay `paged` once a file `alone` is `drawn`: a
/// drawn diagram alone is paged only when `paging` is `always`, as an image
/// is.
pub fn paged_when_drawn(paged: bool, alone: bool, drawn: bool, paging: Paging) -> bool {
    paged && !(alone && drawn && paging != Paging::Always)
}

/// Whether to tell that reading mode is not shown, when it is `asked` for:
/// not for a file `alone` that is `bare`, an image, a diagram or text written
/// as it is, unless it is paged as an image is (see [`pages_alone`]): such a
/// file is shown on its own, and reading mode is no part of showing it.
pub fn tells_reading(asked: bool, alone: bool, bare: bool, as_image: bool) -> bool {
    asked && !(alone && bare && !as_image)
}

/// Whether the warnings about the settings are told, given whether `files`
/// are `paged`: always, but for an image alone that is printed, which is
/// shown without them.
pub fn tells_warnings(files: &Files, paged: bool) -> bool {
    paged || !(files.route.is_alone() && files.images_only)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::caps::KittyGraphics;

    fn playback_when_printing(caps: Capabilities, setting: Animations, named: bool) -> Playback {
        printing_animations(caps, setting, named).playback()
    }

    fn ghostty() -> Capabilities {
        let mut caps = Capabilities::kitty(80);
        caps.graphics = Graphics::Kitty(KittyGraphics {
            playback: Playback::Redraw,
            terminal: Some("ghostty 1.1.3".into()),
            ..match caps.graphics {
                Graphics::Kitty(kitty) => kitty,
                Graphics::None => unreachable!(),
            }
        });
        caps
    }

    #[test]
    fn printed_animations_play_in_kitty_only_when_the_setting_was_named() {
        let kitty = Capabilities::kitty(80);
        let started = |once| Playback::Started { once };
        assert_eq!(
            playback_when_printing(kitty.clone(), Animations::Once, true),
            started(true)
        );
        assert_eq!(
            playback_when_printing(kitty.clone(), Animations::Always, true),
            started(false)
        );
        // Left to the default, or turned off, it is the first frame.
        for (setting, named) in [
            (Animations::Once, false),
            (Animations::Always, false),
            (Animations::Never, true),
            (Animations::Never, false),
        ] {
            assert_eq!(
                playback_when_printing(kitty.clone(), setting, named),
                Playback::None,
                "{setting:?} {named}"
            );
        }
    }

    #[test]
    fn printed_animations_stay_the_first_frame_in_ghostty_and_without_graphics() {
        for setting in [Animations::Once, Animations::Always] {
            assert_eq!(
                playback_when_printing(ghostty(), setting, true),
                Playback::None
            );
            assert_eq!(
                playback_when_printing(Capabilities::ansi(80), setting, true),
                Playback::None
            );
        }
    }

    #[test]
    fn auto_pages_a_document_longer_than_the_window() {
        assert!(pages(Paging::Auto, true, 25, Some(24)));
        assert!(!pages(Paging::Auto, true, 24, Some(24)));
        // Without a height, nothing tells that it is longer.
        assert!(!pages(Paging::Auto, true, 1000, None));
    }

    #[test]
    fn always_pages_even_a_short_document() {
        assert!(pages(Paging::Always, true, 1, Some(24)));
        assert!(pages(Paging::Always, true, 1, None));
    }

    #[test]
    fn nothing_is_paged_into_a_pipe_or_with_never() {
        for paging in [Paging::Always, Paging::Auto, Paging::Never] {
            assert!(!pages(paging, false, 100, Some(24)), "{paging:?}");
        }
        assert!(!pages(Paging::Never, true, 100, Some(24)));
    }

    #[test]
    fn standard_input_is_paged_only_when_paging_is_always() {
        assert_eq!(only_always(Paging::Always), Paging::Always);
        assert_eq!(only_always(Paging::Auto), Paging::Never);
        assert_eq!(only_always(Paging::Never), Paging::Never);
    }

    #[test]
    fn files_of_images_alone_are_paged_only_when_paging_is_always() {
        assert_eq!(for_files(Paging::Always, true), Paging::Always);
        assert_eq!(for_files(Paging::Auto, true), Paging::Never);
        assert_eq!(for_files(Paging::Never, true), Paging::Never);
    }

    #[test]
    fn files_with_a_document_among_them_follow_the_setting() {
        for paging in [Paging::Always, Paging::Auto, Paging::Never] {
            assert_eq!(for_files(paging, false), paging);
        }
    }

    #[test]
    fn printing_wraps_the_wide_code_and_tables_of_reading_mode() {
        let mut wide = Wide::Scroll;
        printing(Some(&mut wide));
        assert_eq!(wide, Wide::Wrap);
        // Without reading mode, there is nothing to change.
        printing(None);
    }

    #[test]
    fn paper_says_whether_the_background_is_dark() {
        let dark = on_page(Capabilities::kitty(80), Some(Paper::Dark));
        assert_eq!(dark.dark_background, Some(true));
        let light = on_page(Capabilities::kitty(80), Some(Paper::Light));
        assert_eq!(light.dark_background, Some(false));
    }

    #[test]
    fn without_paper_the_background_is_left_as_the_terminal_has_it() {
        let caps = Capabilities {
            dark_background: Some(true),
            ..Capabilities::kitty(80)
        };
        assert_eq!(on_page(caps.clone(), None).dark_background, Some(true));
        let unknown = Capabilities {
            dark_background: None,
            ..caps
        };
        assert_eq!(on_page(unknown, None).dark_background, None);
    }

    #[test]
    fn reading_mode_is_shown_when_asked_and_images_are_drawn() {
        assert!(reading(true, &Capabilities::kitty(80)));
        assert!(!reading(true, &Capabilities::ansi(80)));
        assert!(!reading(false, &Capabilities::kitty(80)));
    }

    #[test]
    fn reading_mode_is_left_off_for_tables_and_text_unless_it_is_named() {
        let plain = Files {
            plain: true,
            ..files(Route::One, false, false, false)
        };
        let document = files(Route::One, false, false, false);
        assert!(!reading_for(true, false, &plain));
        assert!(reading_for(true, true, &plain));
        assert!(!reading_for(false, true, &plain));
        assert!(reading_for(true, false, &document));
        assert!(!reading_for(false, false, &document));
    }

    #[test]
    fn reading_mode_that_is_asked_for_but_not_drawn_is_warned_of() {
        let warning = reading_warning(true, &Capabilities::ansi(80)).unwrap();
        assert_eq!(warning.code, Code::ReadingMode);
        let warning = warning.to_string();
        assert!(
            warning.starts_with("reading mode needs a terminal"),
            "{warning}"
        );
        assert_eq!(reading_warning(true, &Capabilities::kitty(80)), None);
        assert_eq!(reading_warning(false, &Capabilities::ansi(80)), None);
    }

    #[test]
    fn an_image_alone_is_paged_only_when_paging_is_always_on_a_terminal() {
        let styled = Capabilities::ansi(80);
        assert!(pages_alone(Paging::Always, true, &styled));
        assert!(!pages_alone(Paging::Auto, true, &styled));
        assert!(!pages_alone(Paging::Never, true, &styled));
        assert!(!pages_alone(Paging::Always, false, &styled));
        assert!(!pages_alone(Paging::Always, true, &Capabilities::plain(80)));
    }

    #[test]
    fn images_load_after_the_pager_opens_when_the_pager_may_page_them() {
        let kitty = Capabilities::kitty(80);
        for paging in [Paging::Always, Paging::Auto] {
            assert!(may_load_later(paging, true, true, &kitty), "{paging:?}");
        }
        assert!(!may_load_later(Paging::Never, true, true, &kitty));
        assert!(!may_load_later(Paging::Auto, false, true, &kitty));
        assert!(!may_load_later(Paging::Auto, true, false, &kitty));
        // Without styling, or without images a terminal can draw, there is
        // nothing to load later.
        assert!(!may_load_later(
            Paging::Auto,
            true,
            true,
            &Capabilities::plain(80)
        ));
        assert!(!may_load_later(
            Paging::Auto,
            true,
            true,
            &Capabilities::ansi(80)
        ));
    }

    #[test]
    fn always_pages_without_measuring() {
        assert!(surely_paged(Paging::Always, || panic!("measured")));
    }

    #[test]
    fn auto_pages_when_the_measured_document_is_longer_than_the_window() {
        let measured = |rows, height| move || Measured { rows, height };
        assert!(surely_paged(Paging::Auto, measured(25, Some(24))));
        assert!(!surely_paged(Paging::Auto, measured(24, Some(24))));
        assert!(!surely_paged(Paging::Auto, measured(1000, None)));
    }

    #[test]
    fn never_is_measured_as_auto() {
        // The callers pass no `never` here; this pins what it would do.
        let measured = |rows, height| move || Measured { rows, height };
        assert!(surely_paged(Paging::Never, measured(25, Some(24))));
        assert!(!surely_paged(Paging::Never, measured(24, Some(24))));
    }

    #[test]
    fn images_load_now_unless_the_document_surely_goes_to_the_pager() {
        for initial_load in [InitialLoad::Wait, InitialLoad::Background] {
            assert_eq!(load(false, initial_load), Load::Now, "{initial_load:?}");
        }
    }

    #[test]
    fn images_of_a_surely_paged_document_wait_or_load_later() {
        assert_eq!(load(true, InitialLoad::Wait), Load::Wait);
        assert_eq!(load(true, InitialLoad::Background), Load::Later);
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn one_file_is_shown_alone_and_standard_input_is_its_own_route() {
        assert_eq!(route(&paths(&["a.md"]), false), Route::One);
        assert_eq!(route(&paths(&["-"]), false), Route::Stdin);
        assert_eq!(route(&paths(&["-"]), true), Route::Stdin);
    }

    #[test]
    fn no_file_reads_standard_input_unless_it_is_a_terminal() {
        assert_eq!(route(&[], false), Route::Stdin);
        assert_eq!(route(&[], true), Route::Missing);
    }

    #[test]
    fn several_files_are_shown_one_after_another() {
        assert_eq!(route(&paths(&["a.md", "b.md"]), true), Route::Several);
        assert_eq!(route(&paths(&["a.md", "-"]), false), Route::Several);
    }

    #[test]
    fn standard_input_can_be_given_only_once() {
        assert_eq!(route(&paths(&["-", "-"]), false), Route::RepeatedStdin);
        assert_eq!(
            route(&paths(&["-", "a.md", "-"]), false),
            Route::RepeatedStdin
        );
    }

    #[test]
    fn a_followed_file_is_paged_when_paging_is_auto() {
        for follow in [Follow::On, Follow::Auto] {
            assert_eq!(for_follow(Paging::Auto, follow, true), Paging::Always);
        }
        assert_eq!(for_follow(Paging::Auto, Follow::Off, true), Paging::Auto);
        assert_eq!(for_follow(Paging::Auto, Follow::On, false), Paging::Auto);
    }

    #[test]
    fn following_leaves_other_paging_as_it_is() {
        for paging in [Paging::Always, Paging::Never] {
            assert_eq!(for_follow(paging, Follow::On, true), paging, "{paging:?}");
        }
    }

    fn files(route: Route, images_only: bool, alone: bool, reloadable: bool) -> Files {
        Files {
            route,
            images_only,
            alone,
            reloadable,
            text: false,
            plain: false,
        }
    }

    #[test]
    fn a_diagram_alone_that_is_drawn_is_paged_only_when_paging_is_always() {
        assert!(!paged_when_drawn(true, true, true, Paging::Auto));
        assert!(paged_when_drawn(true, true, true, Paging::Always));
        assert!(paged_when_drawn(true, true, false, Paging::Auto));
        assert!(paged_when_drawn(true, false, true, Paging::Auto));
        assert!(!paged_when_drawn(false, true, false, Paging::Always));
    }

    #[test]
    fn reading_mode_is_told_unless_a_bare_file_alone_is_printed() {
        assert!(tells_reading(true, true, false, false));
        assert!(tells_reading(true, true, true, true));
        assert!(tells_reading(true, false, true, false));
        assert!(!tells_reading(true, true, true, false));
        assert!(!tells_reading(false, true, false, true));
    }

    #[test]
    fn a_file_alone_shown_as_text_draws_no_diagrams() {
        let text = Files {
            text: true,
            ..files(Route::One, false, false, true)
        };
        assert!(!diagrams(true, &text));
        assert!(!diagrams(false, &text));
        // Among several files, there is nothing to load again.
        let several = Files {
            route: Route::Several,
            ..text
        };
        assert!(diagrams(true, &several));
        assert!(diagrams(true, &files(Route::One, false, false, true)));
    }

    #[test]
    fn several_files_are_paged_as_for_files() {
        let images = files(Route::Several, true, true, false);
        assert_eq!(paging(Paging::Auto, &images, Follow::Off), Paging::Never);
        assert_eq!(paging(Paging::Always, &images, Follow::Off), Paging::Always);
        let documents = files(Route::Several, false, false, false);
        assert_eq!(paging(Paging::Auto, &documents, Follow::On), Paging::Auto);
    }

    #[test]
    fn standard_input_and_an_image_alone_are_paged_only_when_paging_is_always() {
        let stdin = files(Route::Stdin, false, false, false);
        let image = files(Route::One, true, true, false);
        for files in [stdin, image] {
            assert_eq!(paging(Paging::Auto, &files, Follow::On), Paging::Never);
            assert_eq!(paging(Paging::Always, &files, Follow::On), Paging::Always);
        }
    }

    #[test]
    fn a_diagram_alone_is_paged_as_the_setting_says() {
        let diagram = files(Route::One, false, true, false);
        for setting in [Paging::Always, Paging::Auto, Paging::Never] {
            assert_eq!(paging(setting, &diagram, Follow::On), setting);
        }
    }

    #[test]
    fn a_document_alone_is_paged_when_it_is_followed() {
        let file = files(Route::One, false, false, true);
        assert_eq!(paging(Paging::Auto, &file, Follow::Auto), Paging::Always);
        assert_eq!(paging(Paging::Auto, &file, Follow::Off), Paging::Auto);
        let pipe = files(Route::One, false, false, false);
        assert_eq!(paging(Paging::Auto, &pipe, Follow::On), Paging::Auto);
    }

    #[test]
    fn the_settings_warnings_are_left_out_only_for_an_image_alone_that_is_printed() {
        let image = files(Route::One, true, true, false);
        assert!(!tells_warnings(&image, false));
        assert!(tells_warnings(&image, true));
        let images = files(Route::Several, true, true, false);
        assert!(tells_warnings(&images, false));
        let document = files(Route::One, false, false, true);
        assert!(tells_warnings(&document, false));
    }

    #[test]
    fn images_and_diagrams_are_shown_on_their_own() {
        assert!(shown_alone(Input::Diagram("mermaid")));
        assert!(!shown_alone(Input::Markdown));
        assert!(!shown_alone(Input::Text));
    }

    #[test]
    fn a_file_or_standard_input_is_alone_and_several_files_are_not() {
        assert!(Route::One.is_alone());
        assert!(Route::Stdin.is_alone());
        assert!(!Route::Several.is_alone());
    }
}
