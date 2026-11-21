use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use super::*;
use crate::typeset::images::animation::{Animation, Frame, Plays};

/// The names of the pictures that pieces were done for, in order.
type Log = Arc<Mutex<Vec<&'static str>>>;

fn log(log: &Log) -> Vec<&'static str> {
    log.lock().unwrap().clone()
}

fn first(order: &[&str]) -> HashMap<ImageKey, usize> {
    (order.iter().enumerate())
        .map(|(at, name)| (ImageKey::from(*name), at))
        .collect()
}

/// A task for the picture `name` of `pieces` pieces, each noted in `log`.
fn noting(name: &'static str, pieces: usize, log: &Log) -> (Task, Arc<AtomicBool>) {
    let (log, stop, mut left) = (Arc::clone(log), Arc::new(AtomicBool::new(false)), pieces);
    let task = Task::new(ImageKey::from(name), Arc::clone(&stop), move || {
        log.lock().unwrap().push(name);
        left -= 1;
        if left == 0 { Step::Done } else { Step::More }
    });
    (task, stop)
}

#[test]
fn work_that_waits_goes_in_the_order_the_loads_prefer_and_then_as_it_came() {
    let (lane, noted) = (Lane::new(0, Order::default()), Log::default());
    lane.order.set(first(&["near", "next"]));
    for name in ["far", "next", "other", "near", "next"] {
        lane.add(noting(name, 1, &noted).0);
    }
    lane.work_here();
    // `far` and `other` have no place in the order: last, as they came.
    assert_eq!(
        log(&noted),
        ["near", "next", "next", "far", "other"],
        "a picture's place in the order, and then the order they came in"
    );
}

#[test]
fn a_long_build_does_not_hold_a_nearer_one_for_more_than_a_piece() {
    let (lane, noted) = (Arc::new(Lane::new(0, Order::default())), Log::default());
    lane.order.set(first(&["near", "long"]));
    let (near, _) = noting("near", 1, &noted);
    let (again, mut near) = (Arc::clone(&lane), Some(near));
    let (long_log, mut pieces) = (Arc::clone(&noted), 0);
    // The nearer build comes while the long one is at its first piece.
    let long = Task::new(
        ImageKey::from("long"),
        Arc::new(AtomicBool::new(false)),
        move || {
            long_log.lock().unwrap().push("long");
            pieces += 1;
            if let Some(near) = near.take() {
                again.add(near);
            }
            if pieces == 4 { Step::Done } else { Step::More }
        },
    );
    lane.add(long);
    lane.work_here();
    assert_eq!(log(&noted), ["long", "near", "long", "long", "long"]);
}

#[test]
fn a_build_far_from_the_view_runs_when_the_nearer_ones_are_done() {
    let (lane, noted) = (Lane::new(0, Order::default()), Log::default());
    lane.order.set(first(&["near"]));
    lane.add(noting("far", 2, &noted).0);
    lane.add(noting("near", 2, &noted).0);
    lane.work_here();
    assert_eq!(log(&noted), ["near", "near", "far", "far"]);
}

#[test]
fn work_that_was_stopped_is_not_run_nor_carried_on() {
    let (lane, noted) = (Lane::new(0, Order::default()), Log::default());
    let (kept, _) = noting("kept", 1, &noted);
    let (stopped, stop) = noting("stopped", 1, &noted);
    stop.store(true, Ordering::SeqCst);
    // Stopped by its own first piece: it does not go on to a second.
    let half_stop = Arc::new(AtomicBool::new(false));
    let (flag, half_log) = (Arc::clone(&half_stop), Arc::clone(&noted));
    let half = Task::new(ImageKey::from("half"), half_stop, move || {
        half_log.lock().unwrap().push("half");
        flag.store(true, Ordering::SeqCst);
        Step::More
    });
    for task in [kept, stopped, half] {
        lane.add(task);
    }
    lane.work_here();
    assert_eq!(log(&noted), ["kept", "half"]);
    assert!(lock(&lane.shared).tasks.is_empty());
}

#[test]
fn finishing_does_every_piece_unless_the_work_was_stopped() {
    let noted = Log::default();
    let (task, _) = noting("a", 3, &noted);
    task.finish();
    assert_eq!(log(&noted), ["a", "a", "a"]);
    let (task, stop) = noting("b", 3, &noted);
    stop.store(true, Ordering::SeqCst);
    task.finish();
    assert_eq!(log(&noted).len(), 3, "a stopped task does no piece");
}

/// A gate that holds the pieces that run until a test opens it.
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    fn wait(&self) {
        let (open, changed) = &*self.0;
        let guard = open.lock().unwrap();
        let _ = changed
            .wait_timeout_while(guard, Duration::from_secs(10), |open| !*open)
            .unwrap();
    }

    fn open(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

#[test]
fn no_more_than_two_threads_work_at_once() {
    let lane = Lane::new(THREADS, Order::default());
    let (gate, running, most): (_, Arc<AtomicUsize>, Arc<AtomicUsize>) = (
        Gate::default(),
        Arc::new(AtomicUsize::new(0)),
        Arc::default(),
    );
    let (sender, done) = mpsc::channel();
    let started = Arc::new((Mutex::new(0), Condvar::new()));
    for number in 0..6 {
        let (gate, running, most, sender) = (
            gate.clone(),
            Arc::clone(&running),
            Arc::clone(&most),
            sender.clone(),
        );
        let started = Arc::clone(&started);
        let task = Task::new(
            ImageKey::from(format!("{number}.gif")),
            Arc::new(AtomicBool::new(false)),
            move || {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                *started.0.lock().unwrap() += 1;
                started.1.notify_all();
                gate.wait();
                running.fetch_sub(1, Ordering::SeqCst);
                let _ = sender.send(());
                Step::Done
            },
        );
        lane.add(task);
    }
    // Two threads are at work, each held at the gate; four tasks wait.
    {
        let (count, changed) = &*started;
        let guard = count.lock().unwrap();
        let (guard, _) = changed
            .wait_timeout_while(guard, Duration::from_secs(10), |count| *count < 2)
            .unwrap();
        assert_eq!(*guard, 2);
    }
    {
        let waiting = lock(&lane.shared);
        assert_eq!((waiting.workers, waiting.tasks.len()), (2, 4));
    }
    gate.open();
    for _ in 0..6 {
        done.recv_timeout(Duration::from_secs(10))
            .expect("a task done");
    }
    assert_eq!(most.load(Ordering::SeqCst), 2);
}

#[test]
fn work_is_done_here_where_no_thread_can_be_had() {
    let (lane, noted) = (Lane::new(2, Order::default()), Log::default());
    lane.add_with(noting("a", 2, &noted).0, |work| {
        drop(work);
        Err(io::Error::other("no thread"))
    });
    assert_eq!(log(&noted), ["a", "a"]);
    // The thread that failed to start is not counted among those at work.
    assert_eq!(lock(&lane.shared).workers, 0);
}

fn animation(frames: usize) -> Arc<Animation> {
    let frame = Frame {
        image: image::RgbaImage::new(10, 10),
        gap: Duration::from_millis(100),
    };
    Arc::new(Animation {
        frames: vec![frame; frames],
        plays: Plays::Forever,
        over_budget: false,
    })
}

/// A job of `frames` frames on a canvas of 1000 × 1000 pixels: two frames
/// to a piece.
fn job(frames: usize) -> FramesJob {
    FramesJob::Image {
        animation: animation(frames),
        canvas: (1000, 1000),
        at: (0, 0),
        size: (10, 10),
        selected: None,
    }
}

#[test]
fn a_frame_build_gives_its_frames_at_once_after_its_last_piece() {
    let (sender, came) = mpsc::channel();
    let mut task = Task::frames(
        "a.gif".into(),
        Arc::new(AtomicBool::new(false)),
        job(5),
        move |frames| sender.send(frames).unwrap(),
    );
    assert_eq!((task.piece)(), Step::More);
    assert_eq!((task.piece)(), Step::More);
    assert!(came.try_recv().is_err(), "nothing comes before the last");
    assert_eq!((task.piece)(), Step::Done);
    let frames = came.try_recv().unwrap().expect("built");
    let old = crate::typeset::images::build::build(&job(5), &AtomicBool::new(false));
    assert_eq!(Some(frames), old, "the frames the old closure built");
    assert!(came.try_recv().is_err(), "told once");
}

#[test]
fn a_frame_build_that_is_stopped_or_too_large_comes_as_none() {
    let (sender, came) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let mut task = Task::frames("a.gif".into(), Arc::clone(&stop), job(5), {
        let sender = sender.clone();
        move |frames| sender.send(frames).unwrap()
    });
    assert_eq!((task.piece)(), Step::More);
    stop.store(true, Ordering::SeqCst);
    assert_eq!((task.piece)(), Step::Done);
    assert_eq!(came.try_recv().unwrap(), None);
    // 5 frames of 4000 × 4000 pixels are over the budget.
    let big = FramesJob::Image {
        animation: animation(5),
        canvas: (4000, 4000),
        at: (0, 0),
        size: (10, 10),
        selected: None,
    };
    let mut task = Task::frames("big.gif".into(), Arc::default(), big, move |frames| {
        sender.send(frames).unwrap()
    });
    assert_eq!((task.piece)(), Step::Done);
    assert_eq!(came.try_recv().unwrap(), None);
}

/// A job whose first piece panics: a picture of no size cannot be scaled.
fn panicking_job() -> FramesJob {
    FramesJob::Image {
        animation: animation(2),
        canvas: (10, 10),
        at: (0, 0),
        size: (0, 0),
        selected: None,
    }
}

/// Runs the thread work that `Lane::add_with` was given, on a thread of
/// its own, and waits for it to end: whether it ended without a panic.
fn run_on_a_thread(work: Box<dyn FnOnce() + Send>) -> bool {
    std::thread::spawn(work).join().is_ok()
}

#[test]
fn a_frame_build_whose_piece_panics_comes_as_none_and_the_lane_goes_on() {
    let lane = Lane::new(1, Order::default());
    let (sender, came) = mpsc::channel();
    let tell = |name: &'static str| {
        let sender = sender.clone();
        move |frames: Option<Vec<RgbaImage>>| sender.send((name, frames.is_some())).unwrap()
    };
    let (kept, flag) = (Arc::new(Mutex::new(None)), Arc::new(AtomicBool::new(false)));
    let (to_keep, noted) = (Arc::clone(&kept), Log::default());
    // The thread is made but not started, so that both tasks wait for it.
    lane.add_with(
        Task::frames(
            "bad.gif".into(),
            Arc::clone(&flag),
            panicking_job(),
            tell("bad"),
        ),
        move |work| {
            *to_keep.lock().unwrap() = Some(work);
            Ok(())
        },
    );
    lane.add_with(
        Task::frames("good.gif".into(), flag, job(3), tell("good")),
        |_| unreachable!("the thread is there"),
    );
    lane.add_with(noting("after", 1, &noted).0, |_| {
        unreachable!("the thread is there")
    });
    let work = kept.lock().unwrap().take().expect("a thread made");
    assert!(run_on_a_thread(work), "the panic was caught in the piece");
    let told: Vec<_> = came.try_iter().collect();
    assert_eq!(told, [("bad", false), ("good", true)]);
    assert_eq!(log(&noted), ["after"], "the thread went on to the rest");
    let waiting = lock(&lane.shared);
    assert_eq!((waiting.workers, waiting.tasks.len()), (0, 0));
}

#[test]
fn a_thread_that_ends_by_a_panic_is_not_counted_and_the_lane_starts_another() {
    let lane = Lane::new(1, Order::default());
    let kept = Arc::new(Mutex::new(None));
    let to_keep = Arc::clone(&kept);
    let bad = Task::new(
        ImageKey::from("bad"),
        Arc::new(AtomicBool::new(false)),
        || panic!("a piece nobody caught"),
    );
    lane.add_with(bad, move |work| {
        *to_keep.lock().unwrap() = Some(work);
        Ok(())
    });
    assert_eq!(lock(&lane.shared).workers, 1);
    let work = kept.lock().unwrap().take().expect("a thread made");
    assert!(!run_on_a_thread(work), "the thread ended by the panic");
    assert_eq!(lock(&lane.shared).workers, 0, "no longer counted");
    // Work that comes now gets a thread again.
    let (made, noted) = (Arc::new(AtomicUsize::new(0)), Log::default());
    let counter = Arc::clone(&made);
    lane.add_with(noting("next", 1, &noted).0, move |work| {
        counter.fetch_add(1, Ordering::SeqCst);
        drop(work);
        Ok(())
    });
    assert_eq!(made.load(Ordering::SeqCst), 1);
}
