//! The startup plan: the decisions a run makes before it shows anything,
//! such as whether a document is paged, and when its images load. They
//! need no I/O. The platform, `src/main.rs`, reads the terminal and the
//! files and passes the answers in as values.

use crate::config::Paging;

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
}
