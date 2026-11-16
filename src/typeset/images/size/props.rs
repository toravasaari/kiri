//! Property tests of the size arithmetic: whatever the image and the room,
//! it takes no longer than a second, and what it draws fits the columns and
//! the rows it was given and keeps the image's proportions.

use std::time::{Duration, Instant};

use proptest::prelude::*;

use super::*;
use crate::testing::props_config;

/// A size in pixels, log-uniform from 1 to 100 000, so that small and huge
/// images are as likely as middling ones.
fn pixels() -> impl Strategy<Value = u32> {
    (0.0f64..=5.0).prop_map(|exponent| (10f64.powf(exponent) as u32).max(1))
}

/// The opaque rows of an image: all of them, or a part.
fn opaque() -> impl Strategy<Value = Range<f64>> {
    prop_oneof![
        Just(0.0..1.0),
        (0.0f64..=1.0, 0.0f64..=1.0).prop_map(|(a, b)| a.min(b)..a.max(b)),
    ]
}

/// A cell of `cw` × `ch` pixels.
fn room() -> impl Strategy<Value = (u16, u16)> {
    (1u16..=40, 1u16..=100)
}

/// `fit.width` × `fit.height` is `natural` scaled, up to the rounding of both
/// to whole pixels. A side that was rounded up to the one pixel it needs at
/// least is left out.
fn keeps_proportions(fit: &Fit, (nw, nh): (u32, u32)) -> bool {
    let (w, h) = (u64::from(fit.width), u64::from(fit.height));
    let (nw, nh) = (u64::from(nw), u64::from(nh));
    if w * nh < nw || h * nw < nh {
        return true;
    }
    (w * nh).abs_diff(h * nw) <= nw + nh
}

proptest! {
    #![proptest_config(props_config())]

    #[test]
    fn an_image_fits_its_room_and_keeps_its_proportions(
        natural in (pixels(), pixels()),
        room in room(),
        max_columns in 1u16..=300,
        max_cells in 1u16..=200,
        grow in any::<bool>(),
    ) {
        let started = Instant::now();
        let fit = fit_image(natural, 0.0..1.0, room, max_columns, max_cells, grow);
        prop_assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        prop_assert!(fit.columns <= max_columns, "{} columns of {max_columns}", fit.columns);
        prop_assert!(fit.rows >= 1 && fit.rows <= max_cells, "{} rows of {max_cells}", fit.rows);
        if !grow {
            prop_assert_eq!(fit.rows, 1);
        }
        prop_assert!(keeps_proportions(&fit, natural), "{fit:?}");
    }

    #[test]
    fn an_image_with_transparent_rows_does_not_panic_or_take_long(
        natural in (pixels(), pixels()),
        opaque in opaque(),
        room in room(),
        max_columns in 1u16..=300,
        max_cells in 1u16..=200,
        grow in any::<bool>(),
    ) {
        let started = Instant::now();
        let fit = fit_image(natural, opaque, room, max_columns, max_cells, grow);
        prop_assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        prop_assert!(fit.columns <= max_columns);
        prop_assert!(fit.rows >= 1 && fit.rows <= max_cells);
    }

    #[test]
    fn a_formula_fits_its_columns_and_keeps_its_proportions(
        natural in (pixels(), pixels()),
        opaque in opaque(),
        depth in -0.5f64..=1.5,
        room in room(),
        max_columns in 1u16..=300,
        grow in any::<bool>(),
    ) {
        let started = Instant::now();
        let fit = fit_formula(natural, opaque.clone(), depth, room, max_columns, grow);
        prop_assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        prop_assert!(fit.columns <= max_columns, "{} columns of {max_columns}", fit.columns);
        prop_assert!(fit.rows == 1 || fit.rows == 2);
        if !grow {
            prop_assert_eq!(fit.rows, 1);
        }
        if opaque == (0.0..1.0) {
            prop_assert!(keeps_proportions(&fit, natural), "{fit:?}");
        }
    }
}

/// The minimal case that the property found: an image more than a thousand
/// times higher than the room it is fitted to was given up at a scale of
/// 1/1000, which left it taller than `max_cells` rows.
#[test]
fn a_very_tall_image_still_fits_the_rows_it_may_take() {
    let fit = fit_image(
        (1, 27119),
        0.14436508682782503..0.9364219063340354,
        (1, 1),
        1,
        2,
        true,
    );
    assert!(fit.rows <= 2, "{fit:?}");
    let fit = fit_image((9, 100_000), 0.0..1.0, (9, 20), 100, 1, true);
    assert_eq!(fit.rows, 1, "{fit:?}");
}
