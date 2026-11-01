//! What the pager asks of the file behind a document: to read it again, to
//! look at it, and when to look.
//!
//! The pager's loop (`src/pager/mod.rs`) reaches the file only through
//! [`Reload`], which the frontends implement for the platform's files and
//! the simulated frontend for its disk. The session says what is watched
//! (`Session::watches`, in `src/session/sources.rs`) and the [`Watcher`]
//! decides when each watch is looked at, so that the model keeps no clock.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use crate::images::{Images, Loads};
use crate::session::sources::{Looked, Watch, Watched};
use crate::session::state::{Change, Look};
use crate::typeset::caps::Capabilities;
use crate::typeset::model::Document;

/// What the pager asks for the document again, as `r` does.
pub trait Reload {
    /// The document read again, with `images` brought up to date for it
    /// as it is shown in a window with `caps`: it keeps at once what the
    /// document still shows. What `images` held that the document no
    /// longer shows, or that must load anew, is set aside in it, as
    /// [`crate::images::reload_in_background`] does, which also stops in
    /// `loads` the load of an image whose file changed again. It starts no
    /// load: the pager does, for the images its planner wants and `images`
    /// does not hold. `reading` says
    /// whether reading mode is on now, which wants the images in the middle
    /// of text too, whatever the document was opened with.
    /// It also gives how the file looked when it was read, which is before
    /// it was, so that a change made meanwhile counts; `None` when it
    /// could not be looked at.
    fn reload(
        &mut self,
        images: &mut Images,
        loads: &mut Loads,
        caps: &Capabilities,
        reading: bool,
    ) -> (Option<Look>, Reloaded);

    /// How the file at `path` looks now, as the pager asks while it follows
    /// it; why it cannot be looked at otherwise.
    fn look(&mut self, path: &Path) -> Result<Look, String>;
}

/// What reloading the document came to.
pub enum Reloaded {
    Document(Document),
    /// As `Document`, with warnings about it, the first of which the
    /// pager shows.
    Warned(Document, Vec<crate::typeset::diagnostic::Diagnostic>),
    /// The document could not be read again: why, as the status line says
    /// it.
    Failed(String),
}

/// When each watch was last looked at.
#[derive(Debug, Default)]
pub struct Watcher {
    looked: HashMap<Watched, Instant>,
}

impl Watcher {
    /// Looks, with `look`, at each of `watches` that is due at `now`: one
    /// not looked at before, or looked at `every` ago or longer, and gives
    /// what each look found. Forgets the watches that are not in `watches`,
    /// so that one that comes back is looked at at once.
    pub fn due(
        &mut self,
        watches: &[Watch],
        now: Instant,
        mut look: impl FnMut(&Path) -> Result<Look, String>,
    ) -> Vec<Looked> {
        self.looked
            .retain(|what, _| watches.iter().any(|watch| watch.what == *what));
        let mut found = Vec::new();
        for watch in watches {
            let due = (self.looked.get(&watch.what))
                .is_none_or(|at| now.saturating_duration_since(*at) >= watch.every);
            if due {
                self.looked.insert(watch.what, now);
                let change = Change::between(watch.since, look(&watch.path));
                found.push(Looked {
                    what: watch.what,
                    change,
                });
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::session::sources::{Session, SourceFile};
    use crate::typeset::input::Input;

    const EVERY: Duration = Duration::from_millis(100);

    fn look(length: u64) -> Look {
        Look {
            modified: None,
            length,
        }
    }

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    /// Watches of two sources of a session, `a.md` since a look of length
    /// 1 and `b.md` since none.
    fn two() -> (Vec<Watch>, [Watched; 2]) {
        let mut session = Session::new(["a.md", "b.md"]);
        let ids: Vec<_> = session.sources().iter().map(|source| source.id()).collect();
        for (id, name, look) in [(ids[0], "a.md", Some(look(1))), (ids[1], "b.md", None)] {
            let (path, input) = (PathBuf::from(name), Input::Markdown);
            session.read_from(id, SourceFile { path, input, look });
        }
        let watches = session.watches(true);
        let what = [Watched::Source(ids[0]), Watched::Source(ids[1])];
        (watches, what)
    }

    /// A look at any path that finds `look(1)`, which counts its calls.
    fn finds_one(calls: &mut Vec<PathBuf>) -> impl FnMut(&Path) -> Result<Look, String> + '_ {
        |path| {
            calls.push(path.to_owned());
            Ok(look(1))
        }
    }

    #[test]
    fn a_watch_not_looked_at_before_is_due_at_once() {
        let (watches, [a, b]) = two();
        let (mut watcher, mut calls) = (Watcher::default(), Vec::new());
        let looked = watcher.due(&watches, Instant::now(), finds_one(&mut calls));
        // `a.md` looks as it did, and `b.md` had no look to look as.
        let found = [
            Looked {
                what: a,
                change: Change::Unchanged,
            },
            Looked {
                what: b,
                change: Change::Changed(look(1)),
            },
        ];
        assert_eq!(looked, found);
        assert_eq!(calls, [PathBuf::from("a.md"), PathBuf::from("b.md")]);
    }

    #[test]
    fn a_watch_is_not_due_before_its_period_is_over_and_is_at_it() {
        let (watches, _) = two();
        let (mut watcher, mut calls) = (Watcher::default(), Vec::new());
        let start = Instant::now();
        assert_eq!(watcher.due(&watches, start, finds_one(&mut calls)).len(), 2);
        let almost = start + EVERY - Duration::from_nanos(1);
        assert_eq!(watcher.due(&watches, almost, finds_one(&mut calls)), []);
        assert_eq!(calls.len(), 2, "{calls:?}");
        // The period counts from the look, not from the turn that skipped it.
        let at = start + EVERY;
        assert_eq!(watcher.due(&watches, at, finds_one(&mut calls)).len(), 2);
        assert_eq!(
            watcher.due(&watches, at + ms(99), finds_one(&mut calls)),
            []
        );
        assert_eq!(calls.len(), 4, "{calls:?}");
        let later = at + EVERY;
        assert_eq!(watcher.due(&watches, later, finds_one(&mut calls)).len(), 2);
    }

    #[test]
    fn a_watch_that_leaves_the_list_is_looked_at_at_once_when_it_comes_back() {
        let (watches, [a, _]) = two();
        let (mut watcher, mut calls) = (Watcher::default(), Vec::new());
        let start = Instant::now();
        watcher.due(&watches, start, finds_one(&mut calls));
        // Not followed for a while: nothing is watched.
        assert_eq!(watcher.due(&[], start + ms(10), finds_one(&mut calls)), []);
        // Followed again, well before a period is over since the last look.
        let looked = watcher.due(&watches, start + ms(20), finds_one(&mut calls));
        assert_eq!(looked.len(), 2);
        assert_eq!(looked[0].what, a);
        assert_eq!(calls.len(), 4, "{calls:?}");
    }

    #[test]
    fn each_watch_has_its_own_time() {
        let (watches, [a, b]) = two();
        let (mut watcher, mut calls) = (Watcher::default(), Vec::new());
        let start = Instant::now();
        watcher.due(&watches[..1], start, finds_one(&mut calls));
        // `b.md` joins 60 ms later, and is due at once while `a.md` is not.
        let looked = watcher.due(&watches, start + ms(60), finds_one(&mut calls));
        assert_eq!(looked.iter().map(|l| l.what).collect::<Vec<_>>(), [b]);
        let looked = watcher.due(&watches, start + ms(100), finds_one(&mut calls));
        assert_eq!(looked.iter().map(|l| l.what).collect::<Vec<_>>(), [a]);
    }

    #[test]
    fn a_look_that_fails_is_a_file_that_is_gone() {
        let (watches, [a, _]) = two();
        let mut watcher = Watcher::default();
        let gone = |_: &Path| Err("No such file or directory".to_owned());
        let looked = watcher.due(&watches[..1], Instant::now(), gone);
        let change = Change::Gone("No such file or directory".to_owned());
        assert_eq!(looked, [Looked { what: a, change }]);
    }
}
