use std::time::Duration;

use image::Rgba;

use super::*;
use crate::typeset::images::animation::{Frame, Plays};

fn animation(frames: usize) -> Arc<Animation> {
    let colours = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
    let frames = (0..frames)
        .map(|n| Frame {
            image: RgbaImage::from_pixel(10, 10, Rgba(colours[n % 3])),
            gap: Duration::from_millis(100),
        })
        .collect();
    Arc::new(Animation {
        frames,
        plays: Plays::Forever,
        over_budget: false,
    })
}

#[test]
fn a_picture_shown_as_an_image_gets_each_frame_on_its_canvas() {
    let job = FramesJob::Image {
        animation: animation(3),
        canvas: (40, 40),
        at: (5, 10),
        size: (10, 10),
        selected: None,
    };
    let frames = build(&job, &AtomicBool::new(false)).expect("built");
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[1].dimensions(), (40, 40));
    assert_eq!(frames[1].get_pixel(6, 11).0, [0, 255, 0, 255]);
    assert_eq!(frames[2].get_pixel(6, 11).0, [0, 0, 255, 255]);
    // The canvas round it is clear.
    assert_eq!(frames[1].get_pixel(0, 0).0[3], 0);
}

#[test]
fn a_selected_picture_has_the_frame_round_its_cells_in_each_frame() {
    let job = FramesJob::Image {
        animation: animation(2),
        canvas: (40, 40),
        at: (0, 0),
        size: (10, 10),
        selected: Some(([255, 255, 0], 0..2, 20)),
    };
    let frames = build(&job, &AtomicBool::new(false)).unwrap();
    for frame in &frames {
        assert_eq!(frame.get_pixel(0, 20).0, [255, 255, 0, 255]);
    }
}

#[test]
fn a_picture_in_a_part_is_laid_on_the_part_without_it_with_a_rectangle_round_it() {
    let base = RgbaImage::from_pixel(60, 60, Rgba([9, 9, 9, 255]));
    let job = FramesJob::Part {
        animation: animation(3),
        base,
        at: (20, 20),
        size: (10, 10),
        outline: Some(([255, 255, 0], 1)),
    };
    let frames = build(&job, &AtomicBool::new(false)).unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[1].get_pixel(25, 25).0, [0, 255, 0, 255]);
    assert_eq!(frames[1].get_pixel(20, 25).0, [255, 255, 0, 255]);
    assert_eq!(frames[1].get_pixel(5, 5).0, [9, 9, 9, 255]);
}

#[test]
fn building_stops_when_asked_and_refuses_what_is_over_the_budget() {
    let job = FramesJob::Image {
        animation: animation(3),
        canvas: (40, 40),
        at: (0, 0),
        size: (10, 10),
        selected: None,
    };
    assert!(build(&job, &AtomicBool::new(true)).is_none());
    // 64 MiB is 16 million pixels: three frames of 3000 × 2000 are over.
    let big = FramesJob::Image {
        animation: animation(3),
        canvas: (3000, 2000),
        at: (0, 0),
        size: (10, 10),
        selected: None,
    };
    assert!(build(&big, &AtomicBool::new(false)).is_none());
}

/// A job of `frames` frames on a canvas of 1000 × 1000 pixels, 4 MB a
/// frame: a piece of it is two frames.
fn large(frames: usize) -> FramesJob {
    FramesJob::Image {
        animation: animation(frames),
        canvas: (1000, 1000),
        at: (5, 10),
        size: (10, 10),
        selected: None,
    }
}

#[test]
fn a_build_goes_a_piece_of_frames_at_a_time_and_gives_the_frames_of_one_go() {
    let job = large(5);
    let stop = AtomicBool::new(false);
    let mut pieces = Build::new(job.clone()).expect("within the budget");
    assert!(matches!(pieces.piece(&stop), Piece::More));
    assert_eq!(pieces.frames.len(), 2, "a piece is two frames of 4 MB");
    assert!(matches!(pieces.piece(&stop), Piece::More));
    let Piece::Done(frames) = pieces.piece(&stop) else {
        panic!("the third piece is the last");
    };
    assert_eq!(frames.len(), 5);
    // The same frames, in the same order, as each drawn by itself.
    for (index, frame_in_pieces) in frames.iter().enumerate() {
        assert_eq!(frame_in_pieces, &frame(&job, index), "frame {index}");
    }
    assert_eq!(Some(frames), build(&job, &stop));
}

#[test]
fn a_frame_larger_than_a_piece_is_a_piece_of_its_own() {
    // 2000 × 2000 pixels is 16 MB a frame, twice a piece.
    let job = FramesJob::Image {
        animation: animation(3),
        canvas: (2000, 2000),
        at: (0, 0),
        size: (10, 10),
        selected: None,
    };
    let mut pieces = Build::new(job).expect("within the budget");
    let stop = AtomicBool::new(false);
    assert!(matches!(pieces.piece(&stop), Piece::More));
    assert_eq!(pieces.frames.len(), 1);
}

#[test]
fn a_build_stopped_between_pieces_makes_no_more_frames() {
    let mut pieces = Build::new(large(5)).expect("within the budget");
    let stop = AtomicBool::new(false);
    assert!(matches!(pieces.piece(&stop), Piece::More));
    stop.store(true, Ordering::SeqCst);
    assert!(matches!(pieces.piece(&stop), Piece::Stopped));
    assert_eq!(pieces.frames.len(), 2);
}
