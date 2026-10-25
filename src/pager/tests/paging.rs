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
