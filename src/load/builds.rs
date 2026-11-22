//! The builds of the frames of animated pictures: a build is a job for each
//! place the picture is shown, and comes as one result once all its jobs are
//! done. The work is done by the runner (`Runner::work`, the CPU lane of
//! `lane.rs`); this keeps what is being built and takes what comes.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use crate::images::ImageKey;
use crate::load::lane::Task;
use crate::load::runner::Runner;
use crate::typeset::images::build::FramesJob;
use crate::typeset::images::size::Placement;

/// The builds under way, by their picture, and what has come of them.
#[derive(Debug)]
pub(super) struct Builds {
    /// The frames being built, by their picture.
    running: HashMap<ImageKey, Building>,
    /// The number of the last build, which tells it from an earlier build
    /// of the same picture.
    next: u64,
    pub(super) sender: Sender<BuildDone>,
    pub(super) receiver: Receiver<BuildDone>,
    /// Builds that came to nothing at once, for [`Builds::poll`].
    nothing: Vec<Finished>,
}

/// The frames of a picture being built: a job for each place it is shown.
#[derive(Debug)]
struct Building {
    id: u64,
    stop: Arc<AtomicBool>,
    /// How many of its jobs have not come, and what the others did.
    waiting: usize,
    frames: Vec<(Placement, Vec<image::RgbaImage>)>,
    failed: bool,
}

/// One job of a build that has come.
#[derive(Debug)]
pub(super) struct BuildDone {
    id: u64,
    picture: ImageKey,
    place: Placement,
    frames: Option<Vec<image::RgbaImage>>,
}

/// The frames of a picture, built: for each place it is shown, the frames
/// as the images the terminal is sent; or `ok` is false when they could not
/// be built, or would take too much.
#[derive(Debug)]
pub struct Finished {
    pub picture: ImageKey,
    pub ok: bool,
    pub frames: Vec<(Placement, Vec<image::RgbaImage>)>,
}

impl Default for Builds {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Builds {
            running: HashMap::new(),
            next: 0,
            sender,
            receiver,
            nothing: Vec::new(),
        }
    }
}

impl Builds {
    /// Starts building the frames of `picture`: `jobs` are each the frames
    /// for a place it is shown at, which `runner` has done. A build of it
    /// that is under way gives way. Without jobs, the build comes to nothing
    /// at once.
    pub(super) fn start(
        &mut self,
        picture: ImageKey,
        jobs: Vec<(Placement, FramesJob)>,
        runner: &mut dyn Runner,
    ) {
        self.cancel(&picture);
        if jobs.is_empty() {
            self.nothing.push(Finished {
                picture,
                ok: false,
                frames: Vec::new(),
            });
            return;
        }
        self.next += 1;
        let (id, stop) = (self.next, Arc::new(AtomicBool::new(false)));
        let building = Building {
            id,
            stop: Arc::clone(&stop),
            waiting: jobs.len(),
            frames: Vec::new(),
            failed: false,
        };
        self.running.insert(picture.clone(), building);
        for (place, job) in jobs {
            let (sender, stop, picture) = (self.sender.clone(), Arc::clone(&stop), picture.clone());
            let done = {
                let picture = picture.clone();
                move |frames| {
                    let _ = sender.send(BuildDone {
                        id,
                        picture,
                        place,
                        frames,
                    });
                }
            };
            runner.work(Task::frames(picture, stop, job, done));
        }
    }

    /// Stops the build of the frames of `picture`, if one is under way.
    pub(super) fn cancel(&mut self, picture: &ImageKey) {
        if let Some(building) = self.running.remove(picture) {
            building.stop.store(true, Ordering::SeqCst);
        }
    }

    /// Whether the frames of `picture` are being built.
    pub(super) fn contains(&self, picture: &ImageKey) -> bool {
        self.running.contains_key(picture)
    }

    /// The builds that are done since the last call, each once; those that
    /// were stopped are left out.
    pub(super) fn poll(&mut self) -> Vec<Finished> {
        let mut finished = std::mem::take(&mut self.nothing);
        for done in self.receiver.try_iter() {
            let Some(building) = self.running.get_mut(&done.picture) else {
                continue;
            };
            if building.id != done.id {
                continue;
            }
            building.waiting -= 1;
            match done.frames {
                Some(frames) => building.frames.push((done.place, frames)),
                None => building.failed = true,
            }
            if building.waiting == 0 {
                let building = self.running.remove(&done.picture).expect("a build");
                finished.push(Finished {
                    picture: done.picture,
                    ok: !building.failed,
                    frames: building.frames,
                });
            }
        }
        finished
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::load::jobs::Gated;
    use crate::load::loads::Loads;
    use crate::typeset::images::animation::{Animation, Frame, Plays};
    use crate::typeset::images::size::{Placement, Shows, Size};

    fn animation() -> Arc<Animation> {
        let frame = Frame {
            image: image::RgbaImage::new(10, 10),
            gap: Duration::from_millis(100),
        };
        Arc::new(Animation {
            frames: vec![frame; 3],
            plays: Plays::Forever,
            over_budget: false,
        })
    }

    fn job(canvas: (u32, u32)) -> FramesJob {
        FramesJob::Image {
            animation: animation(),
            canvas,
            at: (0, 0),
            size: (10, 10),
            selected: None,
        }
    }

    fn place(name: &str) -> Placement {
        let size = Size {
            width: 10,
            height: 10,
            columns: 2,
            rows: 1,
        };
        Placement::block(ImageKey::from(name), size, Shows::Image)
    }

    #[test]
    fn a_build_comes_as_the_frames_for_each_place_once_all_are_built() {
        let (mut loads, held) = Loads::held_builds();
        let picture = ImageKey::from("a.gif");
        let jobs = vec![(place("p1"), job((20, 20))), (place("p2"), job((20, 20)))];
        loads.build(picture.clone(), jobs);
        assert!(loads.building(&picture));
        assert_eq!(held.len(), 2);
        assert!(loads.poll_builds().is_empty(), "nothing is built yet");
        held.release();
        let done = loads.poll_builds();
        assert_eq!(done.len(), 1, "one build, though it had two jobs");
        assert!(done[0].ok && done[0].picture == picture);
        assert_eq!(done[0].frames.len(), 2);
        assert!(done[0].frames.iter().all(|(_, frames)| frames.len() == 3));
        assert!(!loads.building(&picture));
        assert!(loads.poll_builds().is_empty(), "told once");
    }

    #[test]
    fn a_build_that_is_stopped_never_comes() {
        let (mut loads, held) = Loads::held_builds();
        let picture = ImageKey::from("a.gif");
        loads.build(picture.clone(), vec![(place("p"), job((20, 20)))]);
        loads.cancel_build(&picture);
        held.release();
        assert!(loads.poll_builds().is_empty());
    }

    #[test]
    fn a_newer_build_of_a_picture_gives_way_to_the_older_one_and_comes_alone() {
        let (mut loads, held) = Loads::held_builds();
        let picture = ImageKey::from("a.gif");
        loads.build(picture.clone(), vec![(place("p"), job((20, 20)))]);
        loads.build(picture.clone(), vec![(place("p"), job((30, 30)))]);
        held.release();
        let done = loads.poll_builds();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].frames[0].1[0].dimensions(), (30, 30));
    }

    #[test]
    fn frames_that_nobody_waits_for_any_more_are_dropped() {
        let mut loads = Loads::inline_builds();
        let picture = ImageKey::from("a.gif");
        loads.build(picture.clone(), vec![(place("p"), job((20, 20)))]);
        // Built and sent by now; the build is then called off.
        loads.cancel_build(&picture);
        assert!(loads.poll_builds().is_empty());
        assert!(!loads.building(&picture));
    }

    #[test]
    fn a_load_that_holds_its_thread_does_not_delay_a_frame_build() {
        let (mut loads, gated) = (Loads::new(), Gated::default());
        loads.gated(&["slow"], &gated, 0);
        assert_eq!(gated.until(1).len(), 1, "the load holds its thread");
        let picture = ImageKey::from("a.gif");
        loads.build(picture.clone(), vec![(place("p"), job((20, 20)))]);
        // Waits for the frames to come, without a sleep: the lane builds them
        // while the load holds its thread. The result is put back for the poll.
        let came = (loads.builds.receiver)
            .recv_timeout(Duration::from_secs(10))
            .expect("the frames came");
        loads.builds.sender.send(came).unwrap();
        let done = loads.poll_builds();
        let still_loading = !loads.is_empty();
        gated.open.store(true, Ordering::SeqCst);
        assert!(done.len() == 1 && done[0].ok, "{done:?}");
        assert!(still_loading, "the frames came while the load held on");
    }

    #[test]
    fn a_build_without_jobs_or_over_the_budget_comes_as_one_that_failed() {
        let (mut loads, held) = Loads::held_builds();
        loads.build(ImageKey::from("none.gif"), Vec::new());
        let done = loads.poll_builds();
        assert!(done.len() == 1 && !done[0].ok && done[0].frames.is_empty());
        let big = job((3000, 2000));
        loads.build(ImageKey::from("big.gif"), vec![(place("p"), big)]);
        held.release();
        let done = loads.poll_builds();
        assert!(done.len() == 1 && !done[0].ok, "{done:?}");
    }
}
