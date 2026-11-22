use super::*;
use crate::typeset::caps::KittyGraphics;

fn ghostty() -> Capabilities {
    let mut caps = Capabilities::kitty(80);
    caps.graphics = Graphics::Kitty(KittyGraphics {
        playback: Playback::Redraw,
        terminal: Some("ghostty 1.1.3".into()),
        ..match caps.graphics {
            Graphics::Kitty(kitty) => kitty,
            Graphics::None | Graphics::Sixel(_) => unreachable!(),
        }
    });
    caps
}

#[test]
fn printed_animations_show_their_first_frame_in_every_terminal() {
    for caps in [Capabilities::kitty(80), ghostty(), Capabilities::ansi(80)] {
        assert_eq!(printing_still(caps).playback(), Playback::None);
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
fn standard_input_and_the_doctor_report_are_paged_only_when_paging_is_always() {
    assert_eq!(only_always(Paging::Always), Paging::Always);
    assert_eq!(only_always(Paging::Auto), Paging::Never);
    assert_eq!(only_always(Paging::Never), Paging::Never);
}

#[test]
fn printing_wraps_wide_blocks_and_leaves_headings_where_they_are() {
    let mut reading = Reading {
        fonts: std::rc::Rc::new(crate::typeset::reading::Fonts::fixture()),
        wide: Wide::Scroll,
        snap: true,
        zoom: 100,
        pictures: Default::default(),
        glyphs: Default::default(),
    };
    printing(Some(&mut reading));
    assert_eq!(reading.wide, Wide::Wrap);
    assert!(!reading.snap);
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
        ..files(Route::One, false, false)
    };
    let document = files(Route::One, false, false);
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
fn paging_is_always_only_with_paging_always_on_a_terminal_that_styles() {
    let styled = Capabilities::ansi(80);
    assert!(opens_surely(Paging::Always, true, &styled));
    assert!(!opens_surely(Paging::Auto, true, &styled));
    assert!(!opens_surely(Paging::Never, true, &styled));
    assert!(!opens_surely(Paging::Always, false, &styled));
    assert!(!opens_surely(
        Paging::Always,
        true,
        &Capabilities::plain(80)
    ));
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

fn files(route: Route, images_only: bool, reloadable: bool) -> Files {
    Files {
        route,
        images_only,
        reloadable,
        text: false,
        plain: false,
    }
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
        ..files(Route::One, false, true)
    };
    assert!(!diagrams(true, &text));
    assert!(!diagrams(false, &text));
    // Among several files, there is nothing to load again.
    let several = Files {
        route: Route::Several,
        ..text
    };
    assert!(diagrams(true, &several));
    assert!(diagrams(true, &files(Route::One, false, true)));
}

#[test]
fn several_files_follow_the_setting_whatever_they_are() {
    // Images, diagrams, documents and a mix: the rows decide with auto.
    for images_only in [true, false] {
        let several = files(Route::Several, images_only, false);
        for setting in [Paging::Always, Paging::Auto, Paging::Never] {
            assert_eq!(paging(setting, &several, Follow::On), setting);
        }
    }
}

#[test]
fn standard_input_is_paged_only_when_paging_is_always() {
    let stdin = files(Route::Stdin, false, false);
    assert_eq!(paging(Paging::Auto, &stdin, Follow::On), Paging::Never);
    assert_eq!(paging(Paging::Always, &stdin, Follow::On), Paging::Always);
    assert_eq!(paging(Paging::Never, &stdin, Follow::On), Paging::Never);
}

#[test]
fn an_image_or_a_diagram_alone_is_paged_as_the_setting_says() {
    // An image is taller than the window only with others, so with
    // auto the rows decide.
    for images_only in [true, false] {
        let file = files(Route::One, images_only, false);
        for setting in [Paging::Always, Paging::Auto, Paging::Never] {
            assert_eq!(paging(setting, &file, Follow::On), setting);
        }
    }
}

#[test]
fn a_document_alone_is_paged_when_it_is_followed() {
    let file = files(Route::One, false, true);
    assert_eq!(paging(Paging::Auto, &file, Follow::Auto), Paging::Always);
    assert_eq!(paging(Paging::Auto, &file, Follow::Off), Paging::Auto);
    let pipe = files(Route::One, false, false);
    assert_eq!(paging(Paging::Auto, &pipe, Follow::On), Paging::Auto);
}

#[test]
fn the_settings_warnings_are_left_out_only_for_an_image_alone_that_is_printed() {
    let image = files(Route::One, true, false);
    assert!(!tells_warnings(&image, false));
    assert!(tells_warnings(&image, true));
    let images = files(Route::Several, true, false);
    assert!(tells_warnings(&images, false));
    let document = files(Route::One, false, true);
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
