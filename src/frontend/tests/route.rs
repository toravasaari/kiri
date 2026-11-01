//! The route, `show_files`, run in the simulated front end as `kiri
//! <files>` runs it: the files from the simulated disk, paged on its
//! terminal or printed to its standard output, with what it tells on its
//! standard error. The assertions look at the bytes, as the pager's
//! simulated tests do.

use super::{fresh, long};
use crate::frontend::Failure;
use crate::frontend::sim::{Sim, plain};
use crate::session::settings::Paging;

/// A window of 40 × 5 with `long.md` on its disk.
fn on_long() -> Sim {
    Sim::new(40, 5).file("long.md", &long())
}

#[test]
fn a_lone_file_longer_than_the_window_is_paged_and_g_goes_to_its_end() {
    let ran = on_long().keys(10, "G").kiri(&["long.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert_eq!(texts.len(), 2, "a frame at the start and one for G");
    assert!(texts[0].contains("Line 1") && !texts[0].contains("Line 10"));
    assert!(texts[1].contains("Line 10") && texts[1].contains("100%"));
    // The status line names the file, with no place among files.
    assert!(texts[0].contains("long.md") && texts[0].contains("21%"));
    assert_eq!(ran.err, "");
}

#[test]
fn a_file_that_fits_the_window_is_printed_with_paging_on_auto() {
    let ran = Sim::new(40, 20)
        .file("short.md", "Just this\n")
        .kiri(&["short.md"]);
    assert!(ran.frames().is_empty(), "nothing is paged");
    assert_eq!(plain(&ran.out).trim(), "Just this");
}

#[test]
fn a_pipe_gets_the_whole_file_printed_and_no_pager() {
    let ran = on_long().piped().kiri(&["long.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "no frame of the pager");
    let printed = plain(&ran.out);
    for n in 1..=10 {
        assert!(
            printed.contains(&format!("Line {n}")),
            "Line {n}: {printed}"
        );
    }
    // A file alone has no header.
    assert!(!printed.contains("long.md"));
    assert_eq!(ran.err, "");
}

#[test]
fn several_files_are_printed_each_under_a_header_that_names_it() {
    let ran = Sim::new(40, 5)
        .file("a.md", "# A\n\nalpha\n")
        .file("b.md", "# B\n\nbeta\n")
        .piped()
        .kiri(&["a.md", "b.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let printed = plain(&ran.out);
    let at = |text: &str| {
        printed
            .find(text)
            .unwrap_or_else(|| panic!("{text}: {printed}"))
    };
    assert!(at("a.md") < at("alpha") && at("alpha") < at("b.md") && at("b.md") < at("beta"));
    assert_eq!(ran.err, "");
}

#[test]
fn a_missing_file_among_two_is_told_the_other_is_printed_and_kiri_fails() {
    let ran = Sim::new(40, 5)
        .file("a.md", "# A\n\nalpha\n")
        .piped()
        .kiri(&["missing.md", "a.md"]);
    assert!(
        matches!(ran.result, Some(Err(Failure::Told))),
        "{:?}",
        ran.result
    );
    assert_eq!(
        ran.err,
        "kiri: missing.md: No such file or directory (os error 2)\n"
    );
    let printed = plain(&ran.out);
    assert!(
        printed.contains("alpha"),
        "the other file is shown: {printed}"
    );
}

#[test]
fn a_lone_file_that_cannot_be_opened_fails_before_anything_is_told() {
    let ran = Sim::new(40, 5).piped().kiri(&["missing.md"]);
    let Some(Err(Failure::Error(error))) = &ran.result else {
        panic!("not told by the caller: {:?}", ran.result);
    };
    assert_eq!(error, "missing.md: No such file or directory (os error 2)");
    assert_eq!((ran.out.as_str(), ran.err.as_str()), ("", ""));
}

#[test]
fn standard_input_is_paged_with_paging_always_as_a_file_named_stdin() {
    let ran = Sim::new(40, 5)
        .stdin(&long())
        .settings(|settings| settings.paging = Paging::Always)
        .kiri(&["-"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert!(texts[0].contains("Line 1") && !texts[0].contains("Line 10"));
    assert!(texts[0].contains("<stdin>"), "{}", texts[0]);
}

#[test]
fn standard_input_is_printed_with_paging_on_auto() {
    let ran = Sim::new(40, 5).stdin(&long()).kiri(&["-"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "not paged");
    assert!(plain(&ran.out).contains("Line 10"));
}

#[test]
fn r_reads_the_file_again_from_the_disk_through_the_platform() {
    let ran = on_long()
        .write(50, "long.md", &fresh())
        .keys(100, "r")
        .kiri(&["long.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    assert!(texts[0].contains("Line 1"));
    let last = texts.last().expect("a frame");
    assert!(
        last.contains("Fresh 1") && !last.contains("Line 1"),
        "{last}"
    );
}

#[test]
fn paging_always_without_keys_prints_and_tells_why() {
    let ran = on_long()
        .settings(|settings| settings.paging = Paging::Always)
        .no_keys()
        .kiri(&["long.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert_eq!(
        ran.err,
        "kiri: no terminal to read keys from, so the document is printed\n"
    );
    assert!(ran.frames().is_empty());
    assert!(plain(&ran.out).contains("Line 10"));
}

#[test]
fn paging_on_auto_without_keys_prints_and_tells_nothing() {
    let ran = on_long().no_keys().kiri(&["long.md"]);
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert_eq!(ran.err, "");
    assert!(plain(&ran.out).contains("Line 10"));
}

#[test]
#[should_panic(expected = ".show is for Sim::run")]
fn kiri_refuses_show() {
    Sim::new(40, 5).show("long.md").kiri(&["long.md"]);
}

#[test]
#[should_panic(expected = ".reading is for Sim::run")]
fn kiri_refuses_reading() {
    Sim::new(40, 5).reading().kiri(&["long.md"]);
}
