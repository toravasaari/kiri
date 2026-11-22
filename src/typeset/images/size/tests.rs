use super::*;

/// An image of `w` × `h` pixels without transparent rows, in a row of
/// cells of 9 × 20.
fn image(w: u32, h: u32) -> Fit {
    fit_image((w, h), 0.0..1.0, (9, 20), 100, 30, true)
}

fn sized(fit: &Fit) -> (u32, u32, u16, u16) {
    (fit.width, fit.height, fit.columns, fit.rows)
}

#[test]
fn a_badge_an_icon_and_a_low_image_are_kept() {
    assert_eq!(sized(&image(89, 20)), (89, 20, 10, 1));
    assert_eq!(sized(&image(16, 16)), (16, 16, 2, 1));
    assert_eq!(image(16, 16).baseline, 16);
}

#[test]
fn an_image_that_is_too_high_for_a_row_is_shrunk_to_leave_little_of_its_last_cell_unused() {
    assert_eq!(sized(&image(41, 24)), (27, 16, 3, 1));
    assert_eq!(sized(&image(28, 27)), (18, 17, 2, 1));
    assert_eq!(sized(&image(54, 43)), (23, 18, 3, 1));
}

#[test]
fn transparent_rows_are_not_part_of_an_images_size() {
    let fit = fit_image((20, 24), 2.0 / 24.0..22.0 / 24.0, (9, 20), 100, 30, true);
    assert_eq!(fit.width, 18);
    assert_eq!(fit.opaque.end - fit.opaque.start, 18);
    assert_eq!(fit.height, 22);
    assert_eq!(fit.baseline, fit.opaque.end);
}

#[test]
fn a_tall_image_keeps_its_size_and_has_rows_to_the_text_cell_s_bottom() {
    assert_eq!(sized(&image(96, 64)), (96, 64, 11, 4));
    // Shrunk to the text's width first, then tall.
    let fit = fit_image((400, 300), 0.0..1.0, (9, 20), 20, 30, true);
    assert_eq!(sized(&fit), (180, 135, 20, 7));
    // At most as many rows as an image may cover.
    let fit = fit_image((90, 900), 0.0..1.0, (9, 20), 100, 10, true);
    assert_eq!(fit.rows, 10);
    assert!(fit.height <= 9 * 20 + 16);
}

#[test]
fn in_a_heading_an_image_is_shrunk_into_one_row() {
    let fit = fit_image((96, 64), 0.0..1.0, (9, 40), 100, 30, false);
    assert_eq!(sized(&fit), (60, 40, 7, 1));
}

fn formula(h: u32) -> Fit {
    fit_formula((30, h), 0.0..1.0, 0.25, (9, 20), 100, true)
}

#[test]
fn a_formula_stays_shrinks_into_the_row_or_makes_its_line_two_rows() {
    let stays = formula(16);
    assert_eq!((stays.height, stays.rows), (16, 1));
    assert_eq!(stays.baseline, 12);
    let shrunk = formula(22);
    assert!(shrunk.height <= 20 && shrunk.rows == 1);
    let two = formula(27);
    assert_eq!((two.height, two.rows), (27, 2));
    let shrunk_two = formula(44);
    assert!(shrunk_two.height <= 40 && shrunk_two.rows == 2);
    // In a heading everything fits one row.
    let heading = fit_formula((30, 44), 0.0..1.0, 0.0, (9, 40), 100, false);
    assert!(heading.height <= 40 && heading.rows == 1);
}

const CELL: (u16, u16) = (10, 20);

#[test]
fn an_image_is_shown_at_most_as_many_cells_as_it_can_cover() {
    let caps = Capabilities {
        graphics: crate::typeset::caps::Graphics::kitty_cells(300),
        cell_pixels: Some((9, 18)),
        ..Capabilities::kitty(80)
    };
    assert_eq!(largest(&caps), (2700, 5400));
    // The cell size assumed when the terminal tells none.
    let caps = Capabilities {
        cell_pixels: None,
        ..caps
    };
    assert_eq!(largest(&caps), (3000, 6000));
}
const NATURAL: Wanted = Wanted {
    width: None,
    height: None,
};

fn size_of(width: u32, height: u32, columns: u16, rows: u16) -> Option<Size> {
    Some(Size {
        width,
        height,
        columns,
        rows,
    })
}

#[test]
fn keeps_the_natural_size_in_css_pixels() {
    assert_eq!(
        size((320, 240), NATURAL, CELL, 100, 100),
        size_of(320, 240, 32, 12)
    );
}

#[test]
fn one_css_pixel_is_a_twentieth_of_the_cell_height() {
    assert_eq!(
        size((320, 240), NATURAL, (20, 40), 100, 100),
        size_of(640, 480, 32, 12)
    );
    assert_eq!(
        size((320, 240), NATURAL, (8, 16), 100, 100),
        size_of(256, 192, 32, 12)
    );
}

#[test]
fn keeps_one_image_pixel_per_terminal_pixel() {
    assert_eq!(
        size_in((320, 240), Unit::Pixel, (20, 40), 100, 100),
        size_of(320, 240, 16, 6)
    );
    assert_eq!(
        size_in((320, 240), Unit::Pixel, (20, 40), 10, 100),
        size_of(200, 150, 10, 4)
    );
}

#[test]
fn never_enlarges_a_small_image() {
    assert_eq!(
        size((16, 16), NATURAL, CELL, 100, 100),
        size_of(16, 16, 2, 1)
    );
}

#[test]
fn shrinks_a_wide_image_to_the_columns_keeping_its_shape() {
    assert_eq!(
        size((1600, 200), NATURAL, CELL, 44, 100),
        size_of(440, 55, 44, 3)
    );
}

#[test]
fn an_indented_image_shrinks_to_the_room_left() {
    assert_eq!(
        size((320, 240), NATURAL, CELL, 20, 100),
        size_of(200, 150, 20, 8)
    );
}

#[test]
fn shrinks_a_tall_image_to_the_rows() {
    assert_eq!(
        size((200, 800), NATURAL, CELL, 100, 10),
        size_of(50, 200, 5, 10)
    );
}

#[test]
fn rounds_to_whole_pixels_and_covers_every_partial_cell() {
    assert_eq!(
        size((25, 25), NATURAL, CELL, 100, 100),
        size_of(25, 25, 3, 2)
    );
    // 18.75 pixels round to 19, a little over two cells of 7 and one
    // of 15.
    assert_eq!(
        size((25, 25), NATURAL, (7, 15), 100, 100),
        size_of(19, 19, 3, 2)
    );
}

#[test]
fn a_sliver_keeps_at_least_one_pixel() {
    assert_eq!(
        size((10000, 1), NATURAL, CELL, 44, 100),
        size_of(440, 1, 44, 1)
    );
}

#[test]
fn pixel_sizes_are_those_of_cells_in_a_room_of_any_width() {
    // In a whole number of cells, the same as `size`.
    assert_eq!(
        pixel_size((1600, 200), NATURAL, css_pixel(CELL), (440.0, 5940.0)),
        Some((440, 55))
    );
    // In a room that is not, the image fills it exactly.
    assert_eq!(
        pixel_size((1600, 200), NATURAL, css_pixel(CELL), (672.0, 5940.0)),
        Some((672, 84))
    );
    // A small image keeps its size, and a tall one is cut to the height.
    assert_eq!(
        pixel_size((16, 16), NATURAL, css_pixel(CELL), (672.0, 5940.0)),
        Some((16, 16))
    );
    assert_eq!(
        pixel_size((200, 800), NATURAL, css_pixel(CELL), (672.0, 200.0)),
        Some((50, 200))
    );
    // A percentage is of the room.
    let half = Wanted {
        width: Some(Length::Percent(50)),
        height: None,
    };
    assert_eq!(
        pixel_size((100, 50), half, css_pixel(CELL), (672.0, 5940.0)),
        Some((336, 168))
    );
    assert_eq!(
        pixel_size((0, 50), NATURAL, css_pixel(CELL), (672.0, 5940.0)),
        None
    );
    assert_eq!(
        pixel_size((10, 10), NATURAL, css_pixel(CELL), (0.0, 5940.0)),
        None
    );
}

#[test]
fn an_image_without_pixels_or_room_has_no_size() {
    assert_eq!(size((0, 10), NATURAL, CELL, 44, 100), None);
    assert_eq!(size((10, 0), NATURAL, CELL, 44, 100), None);
    assert_eq!(size((10, 10), NATURAL, CELL, 0, 100), None);
    assert_eq!(size((10, 10), NATURAL, CELL, 44, 0), None);
}

fn wanted(width: Option<Length>, height: Option<Length>) -> Wanted {
    Wanted { width, height }
}

#[test]
fn a_wanted_width_keeps_the_shape() {
    let width = wanted(Some(Length::Pixels(160)), None);
    assert_eq!(
        size((320, 240), width, CELL, 100, 100),
        size_of(160, 120, 16, 6)
    );
}

#[test]
fn a_wanted_height_keeps_the_shape() {
    let height = wanted(None, Some(Length::Pixels(60)));
    assert_eq!(
        size((320, 240), height, CELL, 100, 100),
        size_of(80, 60, 8, 3)
    );
}

#[test]
fn a_wanted_width_and_height_change_the_shape() {
    let both = wanted(Some(Length::Pixels(100)), Some(Length::Pixels(100)));
    assert_eq!(
        size((320, 240), both, CELL, 100, 100),
        size_of(100, 100, 10, 5)
    );
}

#[test]
fn a_wanted_size_is_in_css_pixels() {
    let width = wanted(Some(Length::Pixels(160)), None);
    assert_eq!(
        size((320, 240), width, (20, 40), 100, 100),
        size_of(320, 240, 16, 6)
    );
}

#[test]
fn a_wanted_size_enlarges_a_small_image() {
    let width = wanted(Some(Length::Pixels(64)), None);
    assert_eq!(size((16, 16), width, CELL, 100, 100), size_of(64, 64, 7, 4));
}

#[test]
fn a_wanted_size_still_shrinks_to_the_room() {
    let both = wanted(Some(Length::Pixels(1000)), Some(Length::Pixels(100)));
    assert_eq!(size((10, 10), both, CELL, 40, 100), size_of(400, 40, 40, 2));
    let tall = wanted(None, Some(Length::Pixels(1000)));
    assert_eq!(
        size((10, 10), tall, CELL, 100, 10),
        size_of(200, 200, 20, 10)
    );
}

#[test]
fn a_wanted_percentage_is_of_the_width() {
    let half = wanted(Some(Length::Percent(50)), None);
    assert_eq!(
        size((320, 240), half, CELL, 40, 100),
        size_of(200, 150, 20, 8)
    );
    let double = wanted(Some(Length::Percent(200)), None);
    assert_eq!(
        size((32, 24), double, CELL, 40, 100),
        size_of(400, 300, 40, 15)
    );
}

#[test]
fn a_percentage_height_is_ignored() {
    let height = wanted(None, Some(Length::Percent(50)));
    assert_eq!(
        size((320, 240), height, CELL, 100, 100),
        size_of(320, 240, 32, 12)
    );
    let both = wanted(Some(Length::Pixels(160)), Some(Length::Percent(50)));
    assert_eq!(
        size((320, 240), both, CELL, 100, 100),
        size_of(160, 120, 16, 6)
    );
}

#[test]
fn assumes_ten_by_twenty_pixel_cells_when_unknown() {
    let mut caps = Capabilities::kitty(80);
    assert_eq!(cell_size(&caps), (10, 20));
    caps.cell_pixels = Some((9, 18));
    assert_eq!(cell_size(&caps), (9, 18));
    caps.cell_pixels = Some((0, 18));
    assert_eq!(cell_size(&caps), (10, 20));
}

#[test]
fn a_css_pixel_is_a_twentieth_of_the_cell_height_and_a_picture_takes_the_room_it_has() {
    assert!((css_pixel((9, 20)) - 1.0).abs() < f64::EPSILON);
    assert!((css_pixel((9, 30)) - 1.5).abs() < f64::EPSILON);
    let none = Wanted::default();
    assert_eq!(
        pixel_size((100, 50), none, 1.0, (500.0, 500.0)),
        Some((100, 50))
    );
    assert_eq!(
        pixel_size((100, 50), none, 1.5, (500.0, 500.0)),
        Some((150, 75))
    );
    // Shrunk to the room, keeping its shape.
    assert_eq!(
        pixel_size((100, 50), none, 1.0, (50.0, 500.0)),
        Some((50, 25))
    );
    assert_eq!(
        pixel_size((100, 50), none, 1.0, (500.0, 10.0)),
        Some((20, 10))
    );
    assert_eq!(pixel_size((100, 50), none, 0.0, (500.0, 500.0)), None);
    // At 200 % a CSS pixel is two terminal pixels, still within the room.
    let double = css_pixel((9, 20)) * 2.0;
    assert_eq!(
        pixel_size((100, 50), none, double, (500.0, 500.0)),
        Some((200, 100))
    );
    assert_eq!(
        pixel_size((100, 50), none, double, (150.0, 500.0)),
        Some((150, 75))
    );
}
