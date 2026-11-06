use super::*;

fn picture(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_pixel(width, height, Rgba([200, 30, 30, 255]))
}

#[test]
fn the_play_button_is_a_dark_circle_with_a_white_triangle_in_it() {
    let button = play_button(48);
    // Outside the circle it is clear, in the circle's edge it is dark and
    // partly transparent, and the triangle is white.
    assert_eq!(button.get_pixel(0, 0).0[3], 0);
    let edge = button.get_pixel(4, 24).0;
    assert!(edge[0] < 40 && (100..200).contains(&edge[3]), "{edge:?}");
    let middle = button.get_pixel(24, 24).0;
    assert!(middle[0] > 240 && middle[3] > 200, "{middle:?}");
}

#[test]
fn the_replay_icon_is_a_dark_circle_with_a_white_arc_in_it() {
    let icon = replay_icon(48);
    assert_eq!(icon.get_pixel(0, 0).0[3], 0);
    let dark = icon
        .pixels()
        .filter(|p| p.0[0] < 40 && p.0[3] > 100)
        .count();
    let white = icon
        .pixels()
        .filter(|p| p.0[0] > 240 && p.0[3] > 200)
        .count();
    assert!(dark > 500, "{dark} dark pixels");
    assert!((60..400).contains(&white), "{white} white pixels");
    // The middle of the circle is inside the arc, so it is the dark disc.
    assert!(icon.get_pixel(24, 24).0[0] < 40);
}

#[test]
fn the_play_button_goes_in_the_middle_of_the_picture_a_quarter_of_its_side() {
    let mut image = picture(200, 100);
    draw(&mut image, (0, 0, 200, 100), Mark::Play);
    // 25 % of 100 pixels: a button from 37 to 62 across and down the middle.
    assert_ne!(image.get_pixel(100, 50).0, [200, 30, 30, 255]);
    assert_eq!(image.get_pixel(100, 30).0, [200, 30, 30, 255]);
    assert_eq!(image.get_pixel(60, 50).0, [200, 30, 30, 255]);
    assert_ne!(image.get_pixel(88, 50).0, [200, 30, 30, 255]);
}

#[test]
fn the_replay_icon_goes_in_the_bottom_right_corner_and_the_middle_stays_clear() {
    let mut image = picture(200, 100);
    draw(&mut image, (0, 0, 200, 100), Mark::Replay);
    assert_eq!(image.get_pixel(100, 50).0, [200, 30, 30, 255]);
    assert_eq!(image.get_pixel(5, 5).0, [200, 30, 30, 255]);
    // 24 pixels with a margin of 6 from the corner.
    assert_ne!(
        image.get_pixel(200 - 6 - 12, 100 - 6 - 12).0,
        [200, 30, 30, 255]
    );
}

#[test]
fn a_mark_is_placed_by_the_box_of_the_picture_in_the_image() {
    let mut image = picture(300, 200);
    draw(&mut image, (100, 50, 100, 100), Mark::Play);
    assert_ne!(image.get_pixel(150, 100).0, [200, 30, 30, 255]);
    assert_eq!(image.get_pixel(50, 100).0, [200, 30, 30, 255]);
    assert_eq!(image.get_pixel(250, 100).0, [200, 30, 30, 255]);
}

#[test]
fn a_picture_under_48_pixels_on_a_side_gets_no_mark() {
    for (width, height) in [(47, 300), (300, 47), (30, 30)] {
        for mark in [Mark::Play, Mark::Replay] {
            let mut image = picture(width, height);
            let before = image.clone();
            draw(&mut image, (0, 0, width, height), mark);
            assert_eq!(image, before, "{width} × {height}");
        }
    }
    let mut image = picture(48, 48);
    let before = image.clone();
    draw(&mut image, (0, 0, 48, 48), Mark::Play);
    assert_ne!(image, before);
}

#[test]
fn a_stopped_picture_shows_its_first_frame_with_a_button_and_an_ended_one_its_last_with_an_icon() {
    assert_eq!(drawn(State::Stopped, 5, true), (0, Some(Mark::Play)));
    assert_eq!(drawn(State::Playing, 5, true), (0, None));
    assert_eq!(drawn(State::Ended, 5, true), (4, Some(Mark::Replay)));
    // Too small for a mark: the frames still go.
    assert_eq!(drawn(State::Stopped, 5, false), (0, None));
    assert_eq!(drawn(State::Ended, 5, false), (4, None));
}

#[test]
fn the_waiting_mark_is_a_half_circle_that_turns_a_step_at_a_time() {
    let steps: Vec<RgbaImage> = (0..WAIT_STEPS).map(|step| waiting_mark(48, step)).collect();
    for step in &steps {
        assert_eq!(step.get_pixel(0, 0).0[3], 0, "clear outside the circle");
        let white = step
            .pixels()
            .filter(|p| p.0[0] > 240 && p.0[3] > 200)
            .count();
        assert!((40..400).contains(&white), "{white} white pixels");
    }
    // Each step is another picture, and a whole round is back where it began.
    for (a, b) in [(0, 1), (0, 4), (2, 6)] {
        assert_ne!(steps[a], steps[b], "steps {a} and {b}");
    }
    assert_eq!(waiting_mark(48, 0), waiting_mark(48, WAIT_STEPS));
    // A half circle: the opposite side of the circle is clear of white.
    let half = |image: &RgbaImage, x: u32, y: u32| image.get_pixel(x, y).0[0] > 240;
    assert!(half(&steps[0], 24, 38) || half(&steps[0], 24, 10) || half(&steps[0], 38, 24));
    assert!(!(half(&steps[0], 10, 24) && half(&steps[0], 38, 24)));
}

#[test]
fn the_waiting_mark_goes_in_the_middle_of_a_big_picture_and_is_drawn_for_the_state() {
    let mut image = picture(200, 100);
    draw(&mut image, (0, 0, 200, 100), Mark::Wait(0));
    assert_ne!(image.get_pixel(100, 50).0, [200, 30, 30, 255]);
    assert_eq!(image.get_pixel(5, 5).0, [200, 30, 30, 255]);
    assert_eq!(drawn(State::Waiting(3), 5, true), (0, Some(Mark::Wait(3))));
    assert_eq!(drawn(State::Waiting(3), 5, false), (0, None));
}
