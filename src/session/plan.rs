//! The startup plan: the decisions a run makes before it shows anything,
//! such as whether a document is paged, and when its images load. They
//! need no I/O. The platform, `src/main.rs`, reads the terminal and the
//! files and passes the answers in as values.

use crate::config::Paging;
use crate::typeset::caps::Capabilities;
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

/// `paging` for several files: when `images_only`, each of them is shown
/// on its own as an image is, such as an image file or a diagram file, and
/// they are paged only when paging is `always`; with a document among
/// them, the files follow `paging` as one document does.
pub fn for_files(paging: Paging, images_only: bool) -> Paging {
    match images_only {
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

/// `caps` for a page: the paper from `--light` or `--dark` says whether
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
pub fn reading_warning(asked: bool, caps: &Capabilities) -> Option<String> {
    (asked && !caps.draws_images()).then(|| {
        "reading mode needs a terminal that draws images; showing the document as text".to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn reading_mode_that_is_asked_for_but_not_drawn_is_warned_of() {
        let warning = reading_warning(true, &Capabilities::ansi(80)).unwrap();
        assert!(
            warning.starts_with("reading mode needs a terminal"),
            "{warning}"
        );
        assert_eq!(reading_warning(true, &Capabilities::kitty(80)), None);
        assert_eq!(reading_warning(false, &Capabilities::ansi(80)), None);
    }
}
