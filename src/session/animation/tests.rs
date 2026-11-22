use super::*;
use crate::session::bands::Bands;
use crate::session::edition::Edition;
use crate::session::links::{Links, placed, stops};
use crate::session::reveal::{Seen, Show, reveal};
use crate::session::sources::Session;
use crate::session::view::View;
use crate::typeset::driver::{Content, Stream};
use crate::typeset::images::marks::State;
use crate::typeset::locate::Map;
use crate::typeset::position::Pos;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn key(name: &str) -> ImageKey {
    ImageKey::from(name)
}

/// Three frames of 100 ms, 300 ms a play.
fn clip(name: &str, plays: Plays) -> Clip {
    Clip {
        gaps: vec![ms(100); 3],
        plays,
        name: name.to_owned(),
    }
}

fn times(n: u32) -> Plays {
    Plays::Times(n)
}

/// An animator with the plays setting `limit` that knows `a.gif` and
/// `b.gif`, and the time zero.
fn known(limit: u32, plays: Plays) -> (Animator, Instant) {
    let mut animator = Animator::new(Pacing::new(limit));
    animator.know(vec![
        (key("a.gif"), clip("a.gif", plays)),
        (key("b.gif"), clip("b.gif", plays)),
    ]);
    (animator, Instant::now())
}

#[test]
fn nothing_plays_by_itself() {
    let (mut animator, t0) = known(3, Plays::Forever);
    for at in [0, 1, 1000, 60_000] {
        assert_eq!(animator.tick(t0 + ms(at), 1), []);
    }
    assert_eq!(animator.playing(), None);
    assert_eq!(animator.frames(t0 + ms(5000)), HashMap::new());
    assert_eq!(animator.due(t0, false), None);
}

#[test]
fn a_start_plays_from_the_first_frame() {
    let (mut animator, t0) = known(3, Plays::Forever);
    assert_eq!(
        play(&mut animator, &key("a.gif"), t0),
        [Command::Replay(key("a.gif"), times(3))]
    );
    assert_eq!(animator.playing(), Some(&key("a.gif")));
    assert_eq!(animator.frame(&key("a.gif"), t0 + ms(150)), 1);
}

/// Starts `key` and has its frames come at `now`, after the time the
/// mark is shown at least: the commands of both.
fn play(animator: &mut Animator, key: &ImageKey, now: Instant) -> Vec<Command> {
    let mut commands = animator.start(key, now - WAIT_MIN);
    commands.extend(animator.built(key, true, now));
    commands
}

#[test]
fn a_start_waits_for_the_frames_and_asks_for_them() {
    let (mut animator, t0) = known(3, Plays::Forever);
    assert_eq!(animator.start(&key("a.gif"), t0), []);
    assert_eq!(animator.take_builds(), [key("a.gif")]);
    assert_eq!(animator.take_builds(), [], "told once");
    assert_eq!(animator.playing(), None);
    assert_eq!(animator.marks(t0)[&key("a.gif")], State::Waiting(0));
    // Nothing plays while it waits, however long.
    assert_eq!(animator.frame(&key("a.gif"), t0 + ms(5000)), 0);
}

#[test]
fn the_play_starts_when_the_frames_are_ready() {
    let (mut animator, t0) = known(3, Plays::Forever);
    animator.start(&key("a.gif"), t0);
    let ready = t0 + ms(700);
    assert_eq!(
        animator.built(&key("a.gif"), true, ready),
        [Command::Replay(key("a.gif"), times(3))]
    );
    assert_eq!(animator.playing(), Some(&key("a.gif")));
    // Counted from when it was ready, not from the press.
    assert_eq!(animator.frame(&key("a.gif"), ready + ms(50)), 0);
    assert_eq!(animator.frame(&key("a.gif"), ready + ms(150)), 1);
    assert_eq!(animator.marks(ready)[&key("a.gif")], State::Playing);
}

#[test]
fn frames_that_could_not_be_built_leave_the_first_frame_and_no_button() {
    let (mut animator, t0) = known(3, Plays::Forever);
    animator.start(&key("a.gif"), t0);
    assert_eq!(animator.built(&key("a.gif"), false, t0 + ms(50)), []);
    assert_eq!(animator.playing(), None);
    // No mark of its own any more, and a press does nothing.
    assert!(!animator.marks(t0 + ms(60)).contains_key(&key("a.gif")));
    assert_eq!(
        animator.press(&key("a.gif"), t0 + ms(100)).1,
        Pause::Nothing
    );
    assert_eq!(animator.take_builds(), [key("a.gif")]);
}

#[test]
fn the_waiting_mark_turns_eight_times_a_second_and_the_pill_four() {
    let (mut animator, t0) = known(3, Plays::Forever);
    animator.start(&key("a.gif"), t0);
    let step = |at: u64| animator.marks(t0 + ms(at))[&key("a.gif")];
    assert_eq!(step(0), State::Waiting(0));
    assert_eq!(step(124), State::Waiting(0));
    assert_eq!(step(125), State::Waiting(1));
    assert_eq!(step(375), State::Waiting(3));
    // A round of eight steps, and round again.
    assert_eq!(step(1000), State::Waiting(0));
    assert_eq!(animator.due(t0 + ms(100), false), Some(WAIT_TICK));
    assert_eq!(WAIT_TICK, ms(125));
    assert_eq!(PILL_TICK, ms(250), "the pill stays at four a second");
    assert!(animator.marks_changed(t0));
    assert!(!animator.marks_changed(t0 + ms(100)));
    assert!(animator.marks_changed(t0 + ms(125)));
}

#[test]
fn frames_that_are_ready_at_once_wait_until_the_mark_can_be_seen_turning() {
    let (mut animator, t0) = known(3, Plays::Forever);
    animator.start(&key("a.gif"), t0);
    // Ready after 10 ms: it does not play yet.
    assert_eq!(animator.built(&key("a.gif"), true, t0 + ms(10)), []);
    assert_eq!(animator.playing(), None);
    assert_eq!(
        animator.marks(t0 + ms(300))[&key("a.gif")],
        State::Waiting(2)
    );
    // The rest of the minimum time is due.
    assert_eq!(
        animator.due(t0 + ms(100), false),
        Some(WAIT_TICK.min(WAIT_MIN - ms(100)))
    );
    assert_eq!(animator.tick(t0 + WAIT_MIN - ms(1), 1), []);
    assert_eq!(
        animator.tick(t0 + WAIT_MIN, 1),
        [Command::Replay(key("a.gif"), times(3))]
    );
    // It plays from then, not from the start or from when it was ready.
    assert_eq!(animator.frame(&key("a.gif"), t0 + WAIT_MIN + ms(50)), 0);
    assert_eq!(animator.frame(&key("a.gif"), t0 + WAIT_MIN + ms(150)), 1);
    // Frames that come late play at once.
    let (mut animator, t0) = known(3, Plays::Forever);
    animator.start(&key("a.gif"), t0);
    let late = t0 + WAIT_MIN + ms(300);
    assert_eq!(
        animator.built(&key("a.gif"), true, late),
        [Command::Replay(key("a.gif"), times(3))]
    );
    // Frames that cannot be built give up the wait at once.
    let (mut animator, t0) = known(3, Plays::Forever);
    animator.start(&key("a.gif"), t0);
    animator.built(&key("a.gif"), false, t0 + ms(10));
    assert!(!animator.marks(t0 + ms(11)).contains_key(&key("a.gif")));
}

#[test]
fn another_start_a_move_of_the_view_a_second_press_and_a_reset_cancel_the_wait() {
    let (mut animator, t0) = known(3, Plays::Forever);
    // Another start.
    animator.start(&key("a.gif"), t0);
    animator.take_builds();
    animator.start(&key("b.gif"), t0 + ms(10));
    assert_eq!(animator.take_cancels(), [key("a.gif")]);
    assert_eq!(animator.marks(t0)[&key("a.gif")], State::Stopped);
    // The view.
    animator.tick(t0 + ms(20), 1);
    animator.tick(t0 + ms(30), 2);
    assert_eq!(animator.take_cancels(), [key("b.gif")]);
    assert_eq!(animator.marks(t0)[&key("b.gif")], State::Stopped);
    // A second press.
    animator.start(&key("a.gif"), t0 + ms(40));
    animator.press(&key("a.gif"), t0 + ms(50));
    assert_eq!(animator.take_cancels(), [key("a.gif")]);
    // Reload.
    animator.start(&key("b.gif"), t0 + ms(60));
    animator.reset();
    assert_eq!(animator.take_cancels(), [key("b.gif")]);
    // A build that comes after the cancel does nothing.
    assert_eq!(animator.built(&key("b.gif"), true, t0 + ms(70)), []);
    assert_eq!(animator.playing(), None);
}

#[test]
fn starting_one_stops_the_one_that_plays() {
    let (mut animator, t0) = known(3, Plays::Forever);
    play(&mut animator, &key("a.gif"), t0);
    assert_eq!(
        play(&mut animator, &key("b.gif"), t0 + ms(50)),
        [
            Command::Stop(key("a.gif")),
            Command::Replay(key("b.gif"), times(3))
        ]
    );
    assert_eq!(animator.playing(), Some(&key("b.gif")));
    assert_eq!(animator.frame(&key("a.gif"), t0 + ms(150)), 0);
}

#[test]
fn the_plays_are_the_smaller_of_the_file_and_the_setting() {
    for (file, limit, expected) in [
        (Plays::Forever, 3, times(3)),
        (times(2), 3, times(2)),
        (times(5), 3, times(3)),
        (Plays::Forever, 0, Plays::Forever),
        (times(5), 0, times(5)),
        (times(1), 0, times(1)),
    ] {
        let (mut animator, t0) = known(limit, file);
        assert_eq!(
            play(&mut animator, &key("a.gif"), t0),
            [Command::Replay(key("a.gif"), expected)],
            "file {file:?}, setting {limit}"
        );
    }
}

#[test]
fn a_press_stops_what_plays_and_starts_what_does_not() {
    let (mut animator, t0) = known(3, Plays::Forever);
    let (commands, did) = animator.press(&key("a.gif"), t0);
    assert_eq!(commands, []);
    assert_eq!(did, Pause::Played);
    assert_eq!(animator.take_builds(), [key("a.gif")]);
    let ready = animator.built(&key("a.gif"), true, t0 + WAIT_MIN);
    assert_eq!(ready, [Command::Replay(key("a.gif"), times(3))]);
    let (commands, did) = animator.press(&key("a.gif"), t0 + WAIT_MIN + ms(100));
    assert_eq!(commands, [Command::Stop(key("a.gif"))]);
    assert_eq!(did, Pause::Stopped);
    assert_eq!(animator.frame(&key("a.gif"), t0 + ms(150)), 0);
    let (_, did) = animator.press(&key("nothing.gif"), t0);
    assert_eq!(did, Pause::Nothing);
}

#[test]
fn a_stopped_animation_starts_again_from_the_first_frame() {
    let (mut animator, t0) = known(3, Plays::Forever);
    play(&mut animator, &key("a.gif"), t0);
    animator.stop();
    play(&mut animator, &key("a.gif"), t0 + ms(1000));
    assert_eq!(animator.frame(&key("a.gif"), t0 + ms(1000)), 0);
    assert_eq!(animator.frame(&key("a.gif"), t0 + ms(1150)), 1);
}

#[test]
fn a_change_of_the_view_stops_what_plays() {
    let (mut animator, t0) = known(3, Plays::Forever);
    assert_eq!(animator.tick(t0, 1), []);
    play(&mut animator, &key("a.gif"), t0);
    assert_eq!(animator.tick(t0 + ms(10), 1), []);
    assert_eq!(animator.tick(t0 + ms(20), 2), [Command::Stop(key("a.gif"))]);
    assert_eq!(animator.playing(), None);
    assert_eq!(animator.frame(&key("a.gif"), t0 + ms(150)), 0);
}

#[test]
fn an_animation_ends_after_its_plays_and_rests_on_its_last_frame() {
    let (mut animator, t0) = known(3, times(2));
    play(&mut animator, &key("a.gif"), t0);
    // Two plays of 300 ms; the clock allows the terminal its lag.
    assert_eq!(animator.tick(t0 + ms(600), 1), []);
    assert_eq!(animator.playing(), Some(&key("a.gif")));
    assert_eq!(animator.frame(&key("a.gif"), t0 + ms(600)), 2);
    assert_eq!(animator.tick(t0 + ms(600) + END_GRACE, 1), []);
    assert_eq!(animator.playing(), None);
    assert_eq!(animator.frame(&key("a.gif"), t0 + ms(5000)), 2);
}

#[test]
fn the_end_never_comes_before_the_plays_are_played() {
    let (mut animator, t0) = known(3, times(1));
    play(&mut animator, &key("a.gif"), t0);
    // One play of 300 ms, and the clock allows the terminal its lag.
    animator.tick(t0 + ms(299), 1);
    assert_eq!(animator.playing(), Some(&key("a.gif")));
    animator.tick(t0 + ms(300) + END_GRACE - ms(1), 1);
    assert_eq!(animator.playing(), Some(&key("a.gif")));
    animator.tick(t0 + ms(300) + END_GRACE, 1);
    assert_eq!(animator.playing(), None);
}

#[test]
fn the_pill_does_not_start_a_new_round_before_it_shows_ended() {
    // One play of 300 ms; the end comes a quarter of a second later, by
    // the clock's leave for the terminal's lag.
    let (mut animator, t0) = known(3, times(1));
    play(&mut animator, &key("a.gif"), t0);
    for at in [300, 310, 400, 540] {
        let now = t0 + ms(at);
        animator.tick(now, 1);
        let pill = animator.pill(now).expect("a pill");
        assert_eq!(pill.elapsed, pill.total, "at {at} ms: {pill:?}");
    }
    // A file that plays twice is at the start of its second round at
    // 300 ms, and in the second at 450.
    let (mut animator, t0) = known(3, times(2));
    play(&mut animator, &key("a.gif"), t0);
    assert_eq!(animator.pill(t0 + ms(450)).unwrap().elapsed, ms(150));
    // Past the last play it is full, until it is ended.
    assert_eq!(animator.pill(t0 + ms(610)).unwrap().elapsed, ms(300));
    animator.tick(t0 + ms(600) + END_GRACE, 1);
    assert_eq!(animator.pill(t0 + ms(900)).unwrap().state, State::Ended);
}

#[test]
fn the_pill_is_drawn_again_at_each_tick_for_clips_of_any_length() {
    for seconds in [2, 8, 20, 90] {
        let mut animator = Animator::new(Pacing::new(0));
        let clip = Clip {
            gaps: vec![Duration::from_secs(1); seconds],
            plays: Plays::Forever,
            name: "long.gif".to_owned(),
        };
        animator.know(vec![(key("a.gif"), clip)]);
        let t0 = Instant::now();
        play(&mut animator, &key("a.gif"), t0);
        assert!(animator.pill_changed(t0));
        // Every tick of the pill's, for a good while: another picture
        // each time while it prints tenths of a second; from a minute on
        // it prints whole seconds, and is drawn when they change.
        for tick in 1..=24u32 {
            let now = t0 + PILL_TICK * tick;
            let changed = animator.pill_changed(now);
            let at = PILL_TICK * tick;
            match (seconds < 60, tick % 4 == 0) {
                (true, _) | (false, true) => {
                    assert!(changed, "a clip of {seconds} s at {at:?}")
                }
                // Between whole seconds, only a step of the bar.
                (false, false) => {}
            }
        }
        // And not again for what it has drawn already.
        assert!(!animator.pill_changed(t0 + PILL_TICK * 24 + ms(1)));
    }
}

#[test]
fn the_pill_is_drawn_four_times_a_second_when_polled_ten_times_a_second() {
    // A clip of 59 s, which has no step of the bar in the first 3 s, polled
    // as the loop polls, every 100 ms: one draw for each quarter second
    // since the start, and the first.
    let mut animator = Animator::new(Pacing::new(0));
    let clip = Clip {
        gaps: vec![Duration::from_millis(59_000)],
        plays: Plays::Forever,
        name: "long.gif".to_owned(),
    };
    animator.know(vec![(key("a.gif"), clip)]);
    let t0 = Instant::now();
    play(&mut animator, &key("a.gif"), t0);
    let draws = (0..=30u32)
        .filter(|poll| animator.pill_changed(t0 + ms(100) * *poll))
        .count();
    assert_eq!(draws, 13, "in 3 s");
}

#[test]
fn the_pill_prints_the_round_at_the_last_tick_since_the_start() {
    let (mut animator, t0) = known(3, times(1));
    play(&mut animator, &key("a.gif"), t0);
    // Between ticks it prints the round at the tick before.
    assert_eq!(animator.pill_drawn(t0 + ms(240)).unwrap().elapsed, ms(0));
    assert_eq!(animator.pill_drawn(t0 + ms(260)).unwrap().elapsed, ms(250));
    // The end of the last play is not put off to the next tick.
    assert_eq!(animator.pill_drawn(t0 + ms(300)).unwrap().elapsed, ms(300));
    assert_eq!(animator.pill_drawn(t0 + ms(310)).unwrap().elapsed, ms(300));
}

#[test]
fn what_plays_is_due_to_show_its_progress_four_times_a_second() {
    let (mut animator, t0) = known(3, Plays::Forever);
    assert_eq!(animator.due(t0, false), None);
    play(&mut animator, &key("a.gif"), t0);
    assert_eq!(animator.due(t0, false), Some(ms(250)));
    // And the end is due when it comes sooner.
    let (mut animator, t0) = known(3, times(1));
    play(&mut animator, &key("a.gif"), t0);
    assert_eq!(animator.due(t0 + ms(500), false), Some(ms(50)));
}

#[test]
fn an_ended_animation_is_not_stopped_by_a_change_of_the_view() {
    let (mut animator, t0) = known(3, times(1));
    play(&mut animator, &key("a.gif"), t0);
    animator.tick(t0 + ms(300) + END_GRACE, 1);
    assert_eq!(animator.tick(t0 + ms(2000), 2), []);
    assert_eq!(animator.frame(&key("a.gif"), t0 + ms(2000)), 2);
}

#[test]
fn an_ended_animation_plays_again_on_a_press() {
    let (mut animator, t0) = known(3, times(1));
    play(&mut animator, &key("a.gif"), t0);
    animator.tick(t0 + ms(300) + END_GRACE, 1);
    let at = t0 + ms(1000);
    let (commands, did) = animator.press(&key("a.gif"), at);
    assert_eq!(commands, []);
    assert_eq!(did, Pause::Played);
    assert_eq!(animator.take_builds(), [key("a.gif"), key("a.gif")]);
    let ready = animator.built(&key("a.gif"), true, at + WAIT_MIN);
    assert_eq!(ready, [Command::Replay(key("a.gif"), times(1))]);
    assert_eq!(animator.frame(&key("a.gif"), at), 0);
}

#[test]
fn a_reset_puts_all_back_on_their_first_frame_ended_ones_too() {
    let (mut animator, t0) = known(3, times(1));
    play(&mut animator, &key("a.gif"), t0);
    animator.tick(t0 + ms(300) + END_GRACE, 1);
    play(&mut animator, &key("b.gif"), t0 + ms(1000));
    animator.reset();
    let commands = animator.tick(t0 + ms(1050), 1);
    assert_eq!(
        commands,
        [Command::Stop(key("a.gif")), Command::Stop(key("b.gif"))]
    );
    assert_eq!(animator.playing(), None);
    assert_eq!(animator.frames(t0 + ms(1100)), HashMap::new());
}

#[test]
fn the_focus_coming_back_stops_what_played_on_its_first_frame() {
    let (mut animator, t0) = known(3, times(3));
    play(&mut animator, &key("a.gif"), t0);
    animator.away();
    animator.back();
    // Stopped, with the time at 0 and the plays to start over, and the
    // terminal told with the next tick; nothing plays again.
    assert_eq!(animator.playing(), None);
    let pill = animator.pill(t0 + ms(900)).unwrap();
    assert_eq!((pill.state, pill.elapsed), (State::Stopped, Duration::ZERO));
    assert_eq!(
        animator.tick(t0 + ms(950), 1),
        [Command::Stop(key("a.gif"))]
    );
    // A send of it that the return brings does not play it.
    assert_eq!(animator.sent(&key("a.gif"), t0 + ms(1000)), []);
    // Nor does the focus coming back again.
    animator.back();
    assert_eq!(animator.tick(t0 + ms(1050), 1), []);
}

#[test]
fn the_focus_coming_back_stops_what_kiri_s_clock_ended_while_away() {
    let (mut animator, t0) = known(3, times(1));
    play(&mut animator, &key("a.gif"), t0);
    animator.away();
    animator.tick(t0 + ms(300) + END_GRACE, 1);
    assert_eq!(animator.marks(t0 + ms(2000))[&key("a.gif")], State::Ended);
    animator.back();
    assert_eq!(animator.marks(t0 + ms(2000))[&key("a.gif")], State::Stopped);
    assert_eq!(
        animator.tick(t0 + ms(2000), 1),
        [Command::Stop(key("a.gif"))]
    );
}

#[test]
fn the_focus_coming_back_leaves_alone_what_did_not_play_when_it_went() {
    let (mut animator, t0) = known(3, times(3));
    animator.away();
    animator.back();
    assert_eq!(animator.tick(t0, 1), []);
    play(&mut animator, &key("a.gif"), t0);
    animator.away();
    animator.stop();
    animator.back();
    assert_eq!(animator.tick(t0 + ms(100), 1), []);
}

#[test]
fn an_image_sent_with_its_frames_plays_from_the_first_where_it_is_to_play() {
    let (mut animator, t0) = known(3, times(1));
    play(&mut animator, &key("a.gif"), t0);
    assert_eq!(
        animator.sent(&key("a.gif"), t0 + ms(100)),
        [Command::Replay(key("a.gif"), times(1))]
    );
    animator.tick(t0 + ms(400) + END_GRACE, 1);
    // An ended one is shown as a still image of its last frame, so it
    // is told nothing; nor is one that is stopped.
    assert_eq!(animator.sent(&key("a.gif"), t0 + ms(2000)), []);
    assert_eq!(animator.sent(&key("b.gif"), t0), []);
}

#[test]
fn the_marks_say_where_each_animation_is_in_its_life() {
    let (mut animator, t0) = known(3, times(1));
    let both = |a, b| HashMap::from([(key("a.gif"), a), (key("b.gif"), b)]);
    assert_eq!(animator.marks(t0), both(State::Stopped, State::Stopped));
    play(&mut animator, &key("a.gif"), t0);
    assert_eq!(animator.marks(t0), both(State::Playing, State::Stopped));
    animator.tick(t0 + ms(300) + END_GRACE, 1);
    assert_eq!(animator.marks(t0), both(State::Ended, State::Stopped));
    play(&mut animator, &key("b.gif"), t0 + ms(1000));
    assert_eq!(animator.marks(t0), both(State::Ended, State::Playing));
    animator.reset();
    assert_eq!(animator.marks(t0), both(State::Stopped, State::Stopped));
}

#[test]
fn a_change_of_the_marks_is_told_once() {
    let (mut animator, t0) = known(3, times(1));
    assert!(animator.marks_changed(t0), "the first states are new");
    assert!(!animator.marks_changed(t0));
    play(&mut animator, &key("a.gif"), t0);
    assert!(animator.marks_changed(t0));
    assert!(!animator.marks_changed(t0));
}

#[test]
fn what_kiri_plays_shows_the_frame_of_the_time_played() {
    let (mut animator, t0) = known(3, Plays::Forever);
    animator.redraw(true);
    play(&mut animator, &key("a.gif"), t0);
    assert_eq!(animator.frames(t0 + ms(50)), HashMap::new());
    assert_eq!(
        animator.frames(t0 + ms(150)),
        HashMap::from([(key("a.gif"), 1)])
    );
    assert_eq!(animator.due(t0 + ms(150), true), Some(ms(50)));
}

#[test]
fn new_clips_replace_the_old_and_forgotten_ones_go() {
    let (mut animator, t0) = known(3, Plays::Forever);
    play(&mut animator, &key("a.gif"), t0);
    // Known again as it was: the state stays.
    animator.know(vec![(key("a.gif"), clip("a.gif", Plays::Forever))]);
    assert_eq!(animator.playing(), Some(&key("a.gif")));
    // Changed, or gone: it is stopped on its first frame.
    animator.know(vec![(key("a.gif"), clip("a.gif", times(1)))]);
    assert_eq!(animator.playing(), None);
    animator.know(Vec::new());
    assert!(!animator.knows(&key("a.gif")));
}

fn reading_page(text: &str, pictures: &[(&str, usize)]) -> (Vec<Line>, Images) {
    reading_page_of(text, pictures, 200)
}

/// As `reading_page`, with frames of 20 × `height` pixels: a row of the
/// page is 20 pixels.
fn reading_page_of(text: &str, pictures: &[(&str, usize)], height: u32) -> (Vec<Line>, Images) {
    use crate::typeset::images::animation::Frame;
    let mut images = Images::new();
    for (name, frames) in pictures {
        let frame = Frame {
            image: image::RgbaImage::new(20, height),
            gap: ms(100),
        };
        let animation = Animation {
            frames: vec![frame; *frames],
            plays: Plays::Forever,
            over_budget: false,
        };
        let source = Source::Animated(std::sync::Arc::new(animation));
        images.insert(*name, Ok(source));
    }
    let mut inputs = crate::render::Inputs::new(crate::typeset::caps::Capabilities {
        cell_pixels: Some((10, 20)),
        ..crate::typeset::caps::Capabilities::kitty(80)
    });
    inputs.reading = Some(crate::typeset::reading::engine::Reading {
        pictures: Default::default(),
        glyphs: Default::default(),
        fonts: std::rc::Rc::new(crate::typeset::reading::Fonts::fixture()),
        wide: crate::typeset::reading::page::Wide::Wrap,
        snap: false,
        zoom: 100,
    });
    inputs.images = images;
    let document = crate::typeset::parse::parse(text);
    let lines = crate::typeset::driver::layout(&document, &inputs);
    (lines, inputs.images)
}

/// As `reading_page_of`, the document and the blocks of its lines too.
fn reading_stream_of(
    text: &str,
    pictures: &[(&str, usize)],
    height: u32,
) -> (Stream, Images, crate::typeset::model::Document) {
    use crate::typeset::images::animation::Frame;
    let mut images = Images::new();
    for (name, frames) in pictures {
        let frame = Frame {
            image: image::RgbaImage::new(20, height),
            gap: ms(100),
        };
        let animation = Animation {
            frames: vec![frame; *frames],
            plays: Plays::Forever,
            over_budget: false,
        };
        let source = Source::Animated(std::sync::Arc::new(animation));
        images.insert(*name, Ok(source));
    }
    let mut inputs = inputs_on(images, (10, 20));
    inputs.caps.cell_pixels = Some((10, 20));
    let document = crate::typeset::parse::parse(text);
    let stream = crate::typeset::driver::content(Content::Document(&document), &inputs);
    (stream, inputs.images, document)
}

/// Inputs for reading mode on cells of `cell` pixels, with `images`.
fn inputs_on(images: Images, cell: (u16, u16)) -> crate::render::Inputs {
    let mut inputs = crate::render::Inputs::new(crate::typeset::caps::Capabilities {
        cell_pixels: Some(cell),
        ..crate::typeset::caps::Capabilities::kitty(80)
    });
    inputs.reading = Some(crate::typeset::reading::engine::Reading {
        pictures: Default::default(),
        glyphs: Default::default(),
        fonts: std::rc::Rc::new(crate::typeset::reading::Fonts::fixture()),
        wide: crate::typeset::reading::page::Wide::Wrap,
        snap: false,
        zoom: 100,
    });
    inputs.images = images;
    inputs
}

/// Animations of frames of 300 × 300 pixels, by name and frame count.
/// On cells of 10 × 20 pixels, 60 of them take about 22 MB as the cells
/// the picture lies on; on cells of 32 × 64, which show it 3.2 times as
/// large, about 220 MB, over the budget. The length of the page makes
/// no difference: the cell does.
fn square_clips(pictures: &[(&str, usize)]) -> Images {
    use crate::typeset::images::animation::Frame;
    let mut images = Images::new();
    for (name, frames) in pictures {
        let frame = Frame {
            image: image::RgbaImage::new(300, 300),
            gap: ms(100),
        };
        let animation = Animation {
            frames: vec![frame; *frames],
            plays: Plays::Forever,
            over_budget: false,
        };
        images.insert(*name, Ok(Source::Animated(std::sync::Arc::new(animation))));
    }
    images
}

/// The cells of 10 × 20 pixels, on which the animations of
/// [`square_clips`] fit, and the cells of 32 × 64, on which 60 frames
/// do not.
const SMALL: (u16, u16) = (10, 20);
const LARGE: (u16, u16) = (32, 64);

/// The lines of `text` in reading mode on cells of `cell` pixels, and
/// the images of [`square_clips`] of `pictures`.
fn square_page(text: &str, pictures: &[(&str, usize)], cell: (u16, u16)) -> (Vec<Line>, Images) {
    let inputs = inputs_on(square_clips(pictures), cell);
    let lines = crate::typeset::driver::layout(&crate::typeset::parse::parse(text), &inputs);
    (lines, inputs.images)
}

/// The lines and images of `before` paragraphs and then an animation.
fn reading_lines(before: usize) -> (Vec<Line>, Images) {
    let text: String = (0..before).map(|n| format!("Paragraph {n}\n\n")).collect();
    reading_page(&format!("{text}![a](a.gif)\n"), &[("a.gif", 2)])
}

/// The notes that laying `text` out twice in reading mode on cells of
/// `cell` pixels leaves in the images of [`square_clips`], as text.
fn notes_of(text: &str, pictures: &[(&str, usize)], cell: (u16, u16)) -> Vec<String> {
    let (lines, images) = square_page(text, pictures, cell);
    drop(lines);
    // The same page again: the layout says it again.
    let (_, again) = square_page(text, pictures, cell);
    assert_eq!(images.notes().len(), again.notes().len());
    images
        .notes()
        .into_iter()
        .map(|(_, n)| n.to_string())
        .collect()
}

#[test]
fn a_picture_left_on_its_first_frame_is_warned_of_once_and_one_that_plays_is_not() {
    // One animation plays: nothing to say.
    assert_eq!(
        notes_of("![a](a.gif)\n", &[("a.gif", 2)], SMALL),
        Vec::<String>::new()
    );
    // The page may be long: what counts is the cells the picture is on.
    let long = format!("{}![a](a.gif)\n", paragraphs(40));
    assert_eq!(
        notes_of(&long, &[("a.gif", 60)], SMALL),
        Vec::<String>::new()
    );
    // Over the budget on large cells: 60 frames of the cells it is on.
    let notes = notes_of("![a](a.gif)\n", &[("a.gif", 60)], LARGE);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].starts_with("a.gif: shows only its first frame: its frames would take "),
        "{notes:?}"
    );
    assert!(notes[0].ends_with("over the limit of 64 MiB"), "{notes:?}");
}

#[test]
fn the_notes_are_those_of_the_last_layout() {
    let images = square_clips(&[("a.gif", 60)]);
    let mut inputs = inputs_on(images, LARGE);
    let document = crate::typeset::parse::parse("![a](a.gif)\n");
    let lay = |inputs: &mut crate::render::Inputs, cell| {
        inputs.caps.cell_pixels = Some(cell);
        let _ = crate::typeset::driver::layout(&document, inputs);
    };
    // Large cells: the frames do not fit, and the picture is left still.
    lay(&mut inputs, LARGE);
    assert_eq!(inputs.images.notes().len(), 1);
    // The same images laid out on small cells: no note.
    lay(&mut inputs, SMALL);
    assert_eq!(inputs.images.notes().len(), 0);
    // And back again, once.
    lay(&mut inputs, LARGE);
    assert_eq!(inputs.images.notes().len(), 1);
}

#[test]
fn laying_the_page_out_again_does_not_say_it_again() {
    let text = "![a](a.gif)\n";
    let (_, images) = square_page(text, &[("a.gif", 60)], LARGE);
    assert_eq!(images.notes().len(), 1);
    // The same images laid out again, as the pager does at each layout.
    let inputs = inputs_on(images, LARGE);
    let document = crate::typeset::parse::parse(text);
    for _ in 0..3 {
        let _ = crate::typeset::driver::layout(&document, &inputs);
    }
    assert_eq!(inputs.images.notes().len(), 1);
}

fn paragraphs(count: usize) -> String {
    (0..count).map(|n| format!("Paragraph {n}\n\n")).collect()
}

#[test]
fn a_note_said_again_by_a_later_layout_has_the_text_of_that_layout() {
    let text = "![b](b.gif)\n";
    let (huge, large) = ((40, 80), LARGE);
    let lay = |inputs: &mut crate::render::Inputs, cell| {
        inputs.caps.cell_pixels = Some(cell);
        let _ = crate::typeset::driver::layout(&crate::typeset::parse::parse(text), inputs);
        (inputs.images.notes().into_iter())
            .map(|(_, n)| n.to_string())
            .collect::<Vec<_>>()
    };
    let mut inputs = inputs_on(square_clips(&[("b.gif", 60)]), large);
    let before = lay(&mut inputs, large);
    assert_eq!(before.len(), 1, "{before:?}");
    // The same images laid out again on larger cells: the note says what
    // a fresh layout of it does.
    let again = lay(&mut inputs, huge);
    let mut fresh = inputs_on(square_clips(&[("b.gif", 60)]), huge);
    assert_eq!(again, lay(&mut fresh, huge));
    assert_ne!(again, before, "the sizes are not those of before");
}

/// `clips` as it was before it looked at each part and page once: the
/// reference for the one that does.
fn clips_by_scanning(lines: &[Line], images: &Images) -> Scan {
    let mut scan = Scan::default();
    // The parts of a page of reading mode that hold an animated picture: a
    // picture across the cut of two parts is one animation.
    for line in lines {
        let Some(ImageRow {
            placement,
            page: Some(parts),
            ..
        }) = line.image.as_deref()
        else {
            continue;
        };
        let ImageKey::Page { slice, .. } = placement.key else {
            continue;
        };
        if placement.shows != Shows::Image || (scan.members.iter()).any(|m| m.part == placement.key)
        {
            continue;
        }
        let pictures = parts.animated(images);
        // A picture whose frames would not fit the budget stays still.
        let unfit = parts.unfit(&pictures, FRAME_BUDGET);
        let cell = parts.cell_height();
        for (pic, top) in parts.in_part(slice, &pictures) {
            let still = unfit
                .iter()
                .any(|(other, _)| (other.x, other.y) == (pic.x, pic.y));
            if still || pic.animation.frames.len() < 2 {
                continue;
            }
            let rows = (top / cell).floor().max(0.0) as u16;
            let end = ((top + pic.height as f32) / cell).ceil() as u16;
            if !scan.clips.iter().any(|(key, _)| *key == pic.key) {
                scan.clips.push((
                    pic.key.clone(),
                    Clip::of(&pic.animation, &pic.key, images.alt(&pic.key)),
                ));
            }
            scan.members.push(Member {
                part: placement.key.clone(),
                picture: pic.key,
                rows: rows..end,
            });
        }
    }
    for key in placements(lines)
        .filter(|placement| placement.shows == Shows::Image)
        .map(|placement| &placement.key)
    {
        if scan.clips.iter().any(|(known, _)| known == key) {
            continue;
        }
        if let Some(Ok(Source::Animated(animation))) = images.get(key)
            && animation.frames.len() > 1
        {
            scan.clips
                .push((key.clone(), Clip::of(animation, key, images.alt(key))));
        }
    }
    scan
}

#[test]
fn the_clips_of_lines_are_those_a_scan_of_every_line_finds() {
    let mut with_members = 0;
    for before in [0, 5, 11, 12, 14, 25, 40] {
        let (lines, images) = reading_lines(before);
        let (found, scanned) = (clips(&lines, &images), clips_by_scanning(&lines, &images));
        assert_eq!(
            format!("{found:?}"),
            format!("{scanned:?}"),
            "{before} before"
        );
        with_members += found.members.len();
    }
    // Two pictures, one of them over the budget on large cells, and the
    // same text twice, as parts with equal keys on two pages.
    let text = format!("{}![a](a.gif)\n\n![b](b.gif)\n", paragraphs(30));
    for cell in [SMALL, LARGE] {
        let pictures = [("a.gif", 60), ("b.gif", 2)];
        let (lines, images) = square_page(&text, &pictures, cell);
        let (found, scanned) = (clips(&lines, &images), clips_by_scanning(&lines, &images));
        assert_eq!(
            format!("{found:?}"),
            format!("{scanned:?}"),
            "cells {cell:?}"
        );
        with_members += found.members.len();
    }
    let (mut lines, images) = square_page(&text, &[("a.gif", 2), ("b.gif", 2)], SMALL);
    let (again, _) = square_page(&text, &[("a.gif", 2), ("b.gif", 2)], SMALL);
    lines.extend(again);
    let (found, scanned) = (clips(&lines, &images), clips_by_scanning(&lines, &images));
    assert_eq!(format!("{found:?}"), format!("{scanned:?}"));
    // A page with no pictures.
    let (lines, images) = square_page(&paragraphs(40), &[], SMALL);
    assert_eq!(
        format!("{:?}", clips(&lines, &images)),
        format!("{:?}", clips_by_scanning(&lines, &images))
    );
    assert!(with_members > 5, "{with_members} members");
}

#[test]
fn one_picture_is_one_animation_and_one_stop_wherever_the_parts_of_the_page_cut_it() {
    for before in [0, 5, 11, 12, 14, 25] {
        let (lines, images) = reading_lines(before);
        let mut animator = Animator::new(Pacing::new(0));
        let found = clips(&lines, &images);
        animator.know(found);
        assert_eq!(animator.keys().len(), 1, "after {before} paragraphs");
    }
    // The cut is really there for some of them.
    let parts = |before| {
        let (lines, _) = reading_lines(before);
        let keys: std::collections::HashSet<_> = (lines.iter())
            .filter_map(|line| line.image.as_ref().map(|row| row.placement.key.clone()))
            .collect();
        keys.len()
    };
    assert!((0..30).any(|before| parts(before) > 1));
}

#[test]
fn an_animation_that_does_not_fit_has_no_clip_and_one_that_does_has() {
    // 60 frames do not fit on large cells, and fit on small ones, in a
    // long page or a short one; 2 fit on both.
    let long = format!("{}![a](a.gif)\n", paragraphs(40));
    for text in [&long[..], "![a](a.gif)\n"] {
        let (lines, images) = square_page(text, &[("a.gif", 60)], LARGE);
        assert!(clips(&lines, &images).clips.is_empty());
        let (lines, images) = square_page(text, &[("a.gif", 60)], SMALL);
        assert_eq!(clips(&lines, &images).clips.len(), 1);
        let (lines, images) = square_page(text, &[("a.gif", 2)], LARGE);
        assert_eq!(clips(&lines, &images).clips.len(), 1);
    }
}

#[test]
fn two_pictures_in_one_part_are_two_animations_on_their_own_rows() {
    let (lines, images) = reading_page(
        "![a](a.gif)\n\n![b](b.gif)\n",
        &[("a.gif", 2), ("b.gif", 3)],
    );
    let scan = clips(&lines, &images);
    assert_eq!(scan.clips.len(), 2);
    let part = scan.members[0].part.clone();
    assert!(scan.members.iter().all(|member| member.part == part));
    let mut animator = Animator::new(Pacing::new(3));
    animator.know(scan);
    let keys = animator.keys();
    let rows: Vec<_> = (keys.iter())
        .map(|key| {
            let member = animator
                .members
                .iter()
                .find(|member| member.picture == *key);
            member.unwrap().rows.clone()
        })
        .collect();
    assert_ne!(rows[0], rows[1], "each on its own rows");
    // A row of the part is the picture on it, and a row of neither is none.
    let (first, second) = (rows[0].start, rows[1].start);
    assert_eq!(animator.picture_at(&part, first), Some(keys[0].clone()));
    assert_eq!(animator.picture_at(&part, second), Some(keys[1].clone()));
}

#[test]
fn there_is_no_pill_until_an_animation_starts_and_it_stays_after_it_stops() {
    // Without a limit to the plays, so that the rounds go on.
    let mut animator = Animator::new(Pacing::new(0));
    animator.know(vec![(key("a.gif"), clip("a.gif", Plays::Forever))]);
    let t0 = Instant::now();
    assert_eq!(animator.pill(t0), None);
    play(&mut animator, &key("a.gif"), t0);
    let pill = animator.pill(t0 + ms(1500)).expect("a pill");
    assert_eq!(pill.name, "a.gif");
    assert_eq!(pill.state, State::Playing);
    // A round takes 300 ms: 1500 ms in is the start of the sixth.
    assert_eq!(pill.elapsed, ms(0));
    assert_eq!(pill.total, ms(300));
    let pill = animator.pill(t0 + ms(1350)).unwrap();
    assert_eq!(pill.elapsed, ms(150));
    animator.stop();
    let pill = animator.pill(t0 + ms(2000)).expect("it stays");
    assert_eq!((pill.state, pill.elapsed), (State::Stopped, ms(0)));
    assert_eq!(pill.name, "a.gif");
}

#[test]
fn the_pill_is_of_the_animation_that_played_last_and_an_ended_one_is_all_played() {
    let (mut animator, t0) = known(3, times(1));
    play(&mut animator, &key("a.gif"), t0);
    animator.tick(t0 + ms(300) + END_GRACE, 1);
    let pill = animator.pill(t0 + ms(2000)).unwrap();
    assert_eq!(pill.state, State::Ended);
    assert_eq!((pill.elapsed, pill.total), (ms(300), ms(300)));
    play(&mut animator, &key("b.gif"), t0 + ms(3000));
    assert_eq!(animator.pill(t0 + ms(3100)).unwrap().name, "b.gif");
}

#[test]
fn the_pill_is_drawn_again_when_what_it_shows_changes() {
    let (mut animator, t0) = known(3, Plays::Forever);
    assert!(!animator.pill_changed(t0), "no pill, nothing to draw");
    play(&mut animator, &key("a.gif"), t0);
    assert!(animator.pill_changed(t0));
    assert!(!animator.pill_changed(t0 + ms(10)));
    // What it prints moves a quarter of a second at a time.
    assert!(!animator.pill_changed(t0 + ms(100)));
    assert!(animator.pill_changed(t0 + ms(250)));
}

#[test]
fn a_picture_is_named_by_its_alt_text_or_else_its_file_without_the_extension() {
    let (lines, images) = reading_page(
        "![The race horse](horse.gif)\n\n![](jiggy.gif)\n",
        &[("horse.gif", 2), ("jiggy.gif", 2)],
    );
    let scan = clips(&lines, &images);
    let name = |file: &str| {
        let found = scan.clips.iter().find(|(key, _)| key.to_string() == file);
        found.unwrap().1.name.clone()
    };
    assert_eq!(name("horse.gif"), "The race horse");
    assert_eq!(name("jiggy.gif"), "jiggy");
}

/// A page of reading mode of `before` paragraphs, an animation of 10
/// rows and `after` paragraphs, in a view of 20 rows with its top on
/// line `top`: where Tab (`back` for Shift+Tab) leaves the view, and the
/// lines of the picture.
fn tabbed(
    before: usize,
    after: usize,
    top: impl Fn(&[usize]) -> usize,
    back: bool,
) -> (View, Vec<usize>) {
    tabbed_of(200, before, after, top, back).0
}

/// As `tabbed`, with a picture of `height` pixels, and the number of
/// Tab stops of the page too.
fn tabbed_of(
    height: u32,
    before: usize,
    after: usize,
    top: impl Fn(&[usize]) -> usize,
    back: bool,
) -> ((View, Vec<usize>), usize) {
    let paragraphs =
        |count: usize| -> String { (0..count).map(|n| format!("Paragraph {n}\n\n")).collect() };
    let text = format!("{}![a](a.gif)\n\n{}", paragraphs(before), paragraphs(after));
    let (stream, images, document) = reading_stream_of(&text, &[("a.gif", 2)], height);
    let Stream { lines, blocks } = &stream;
    let mut animator = Animator::new(Pacing::new(0));
    animator.know(clips(lines, &images));
    let map = Map::new(lines, blocks);
    let seen = Seen { map: &map, lines };
    let session = Session::new(["a.md"]);
    let edition = Edition::new(
        Content::Document(&document),
        &session,
        session.base().id(),
        None,
    );
    let candidates = animator.keys();
    let pictures = |range: &std::ops::Range<Pos>| {
        let key = map.picture(lines, range, &candidates)?;
        let name = animator.name_of(&key)?;
        Some((key, name.to_owned()))
    };
    let all = placed(stops(&edition.targets, |_| true, pictures), seen);
    let picture: Vec<usize> = all[0].lines.clone().unwrap().collect();
    let mut view = View::new(vec![1; lines.len()], 20);
    view.go_to(top(&picture));
    // Without a selection, Tab starts from what is on the screen.
    let mut links = Links::default();
    links.select_next(&edition, &all, view.visible(), back);
    let range = links.selected(&edition, &all).unwrap().stop.range.clone();
    let bands = (&mut Bands::default(), &edition);
    reveal(&range, Show::Whole, seen, &mut view, bands).unwrap();
    ((view, picture), all.len())
}

#[test]
fn tab_shows_a_picture_in_a_page_of_reading_mode_whole_or_in_the_middle() {
    // The picture is 10 rows in a view of 20 rows.
    let (view, p) = tabbed(24, 24, |p| p[0] - 15, false);
    assert_eq!(p.len(), 10);
    assert_eq!(view.visible().end, p[9] + 1, "partly below: bottom edge");
    let (view, p) = tabbed(24, 24, |p| p[0] + 4, false);
    assert_eq!(view.top(), p[0], "partly above: top edge");
    let (view, p) = tabbed(24, 24, |p| p[0] - 3, false);
    assert_eq!(view.top(), p[0] - 3, "shown whole: no move");
    // Wholly below and wholly above: five rows on either side.
    let (view, p) = tabbed(24, 24, |_| 0, false);
    assert_eq!(view.top(), p[0] - 5);
    let (view, p) = tabbed(24, 24, |p| p[0] + 30, true);
    assert_eq!(view.top(), p[0] - 5);
    // At the start of the document there is no room above it.
    let (view, _) = tabbed(2, 24, |p| p[0] + 30, false);
    assert_eq!(view.top(), 0);
}

/// The paragraphs before a picture of `height` pixels that put it across
/// the cut of the page's parts, at row 30, and the first row it is in:
/// the picture starts on a row from 22 on, below the 20 rows of a view
/// at the top.
fn across_the_cut(height: u32) -> usize {
    (0..40)
        .find(|&before| {
            let ((_, p), _) = tabbed_of(height, before, 24, |_| 0, false);
            p[0] >= 22 && p[0] < 30 && p[p.len() - 1] >= 30
        })
        .expect("a page with the picture across the cut")
}

#[test]
fn a_picture_across_the_cut_of_a_page_is_one_stop_and_is_placed_whole() {
    // These tests pin what is right: they were written after it.
    // 25 rows: taller than the view of 20, its top goes to the top.
    let before = across_the_cut(500);
    let ((view, p), stops) = tabbed_of(500, before, 24, |_| 0, false);
    assert_eq!((p.len(), stops), (25, 1));
    assert_eq!(view.top(), p[0]);
    // 15 rows: wholly below the view, so in its middle: 2 rows above.
    let before = across_the_cut(300);
    let ((view, p), stops) = tabbed_of(300, before, 24, |_| 0, false);
    assert_eq!((p.len(), stops), (15, 1));
    assert_eq!(view.top(), p[0] - 2);
}
