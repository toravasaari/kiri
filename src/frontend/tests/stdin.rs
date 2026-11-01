//! Standard input in the simulated frontend: printed unless paging is
//! `always`, and printed as plain text on a terminal that draws no styling.

use super::long;
use crate::frontend::sim::{Sim, plain};
use crate::session::settings::Paging;
use crate::typeset::caps::Capabilities;

#[test]
fn plain_mode_prints_without_the_pager() {
    let ran = Sim::new(40, 5)
        .caps(Capabilities::plain(40))
        .stdin(&long())
        .settings(|settings| settings.paging = Paging::Always)
        .kiri(&["-"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(!ran.out.contains('\x1b'), "{:?}", ran.out);
    assert!(plain(&ran.out).contains("Line 10"), "{:?}", ran.out);
}

#[test]
fn a_piped_document_is_printed_unless_paging_is_always() {
    let ran = Sim::new(40, 5).stdin(&long()).kiri(&["-"]).quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(ran.frames().is_empty(), "not paged: {:?}", ran.out);
    assert!(plain(&ran.out).contains("Line 10"), "{:?}", ran.out);

    let ran = Sim::new(40, 5)
        .stdin(&long())
        .settings(|settings| settings.paging = Paging::Always)
        .keys(10, "q")
        .kiri(&["-"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    assert!(!ran.frames().is_empty(), "paged: {:?}", ran.out);
}
