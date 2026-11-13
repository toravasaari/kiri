//! The session: what the state engine knows of what the reader is looking
//! at. Its sources, where the documents come from (a file, standard input),
//! each with an id, a name and a version that a reload brings forward, and
//! its views, of which one is built: the sources it shows.
//!
//! A source read from a lone regular file also knows that file and how it
//! looked when its version was read ([`SourceFile`]), so the session can say
//! what is to be read again ([`Session::reread`]) and what is watched
//! ([`Session::watches`]). The documents and their layout stay where they
//! are for now, and nothing is read or looked at here: the platform makes
//! the session from the files it was given, and looks at the watches.

use std::path::PathBuf;
use std::time::Duration;

use crate::session::state::{Change, Look, POLL};
use crate::typeset::driver::Content;
use crate::typeset::input::Input;

/// A source in a session. Ids come from the session, in the order the
/// sources were given, and are never used for another source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceId(u32);

/// A state of a source's document. The first is [`Version::FIRST`], and each
/// reload of the source brings the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version(u64);

impl Version {
    /// The version a source has when it is read the first time.
    pub const FIRST: Version = Version(0);

    pub(crate) fn next(self) -> Version {
        Version(self.0 + 1)
    }
}

/// A view in a session. Ids come from the session and are never used for
/// another view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViewId(u32);

/// The file a source is read from: a lone regular file, which can be read
/// again and watched. Its look is that of the file when the source's
/// version was read, taken before the read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub path: PathBuf,
    pub input: Input,
    pub look: Option<Look>,
}

/// What a subscription watches. One kind for now; the configuration file
/// and the theme add theirs when the pager takes them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Watched {
    Source(SourceId),
}

/// A subscription: look at `path` at most every `every`, and tell how it
/// differs from `since`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch {
    pub what: Watched,
    pub path: PathBuf,
    pub every: Duration,
    pub since: Option<Look>,
}

/// What a look at a watch found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Looked {
    pub what: Watched,
    pub change: Change,
}

/// The job of reading a source again: fetch the file, compute the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reread {
    pub source: SourceId,
    pub path: PathBuf,
    pub input: Input,
}

/// Where a document comes from: one file, or standard input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    id: SourceId,
    name: String,
    version: Version,
    file: Option<SourceFile>,
}

impl Source {
    pub fn id(&self) -> SourceId {
        self.id
    }

    /// The name the source is shown by, as the file was given.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The version of the document now.
    pub fn version(&self) -> Version {
        self.version
    }

    /// The file the source is read from, when it is a lone regular file.
    pub fn file(&self) -> Option<&SourceFile> {
        self.file.as_ref()
    }
}

/// What the reader sees: the sources it shows, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    id: ViewId,
    shows: Vec<SourceId>,
}

impl View {
    pub fn id(&self) -> ViewId {
        self.id
    }

    pub fn shows(&self) -> &[SourceId] {
        &self.shows
    }
}

/// The sources and views of one run. A lone file and standard input are a
/// session of one source, like each of several files is one of several.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    sources: Vec<Source>,
    views: Vec<View>,
    next_source: u32,
    next_view: u32,
}

impl Session {
    /// A session with one source for each of `names`, as they are given,
    /// and a view that shows them all.
    pub fn new<I>(names: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        let mut session = Session {
            sources: Vec::new(),
            views: Vec::new(),
            next_source: 0,
            next_view: 0,
        };
        let shows: Vec<SourceId> = names
            .into_iter()
            .map(|name| session.add_source(name.into()))
            .collect();
        session.add_view(shows);
        session
    }

    fn add_source(&mut self, name: String) -> SourceId {
        let id = SourceId(self.next_source);
        self.next_source += 1;
        self.sources.push(Source {
            id,
            name,
            version: Version::FIRST,
            file: None,
        });
        id
    }

    fn add_view(&mut self, shows: Vec<SourceId>) -> ViewId {
        let id = ViewId(self.next_view);
        self.next_view += 1;
        self.views.push(View { id, shows });
        id
    }

    /// The sources, in the order they were given.
    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    /// The source `id`, if the session has it.
    pub fn source(&self, id: SourceId) -> Option<&Source> {
        self.sources.iter().find(|source| source.id == id)
    }

    fn source_mut(&mut self, id: SourceId) -> Option<&mut Source> {
        self.sources.iter_mut().find(|source| source.id == id)
    }

    /// Panics unless the sources are those of `content`: one for each of
    /// its files, and for a lone document at least one. The count and the
    /// order are carried by both, and nothing else ties them.
    pub fn assert_fits(&self, content: Content) {
        let count = self.sources.len();
        match content {
            Content::Document(_) => {
                assert!(count > 0, "a lone document needs a source in the session")
            }
            Content::Files(files) => assert!(
                count == files.len(),
                "the session has {count} sources but the content has {} files",
                files.len()
            ),
        }
    }

    /// The view that is built.
    pub fn view(&self) -> &View {
        self.views.first().expect("a session has a view")
    }

    /// The source `id` is read from `file`. The frontend says so for a lone
    /// regular file only.
    pub fn read_from(&mut self, id: SourceId, file: SourceFile) {
        if let Some(source) = self.source_mut(id) {
            source.file = Some(file);
        }
    }

    /// The job of reading the shown source again: the first source with a
    /// file, which only a lone file has; `None` when nothing can be read
    /// again.
    pub fn reread(&self) -> Option<Reread> {
        self.sources.iter().find_map(|source| {
            let file = source.file.as_ref()?;
            Some(Reread {
                source: source.id,
                path: file.path.clone(),
                input: file.input,
            })
        })
    }

    /// A reread read the document: brings the source `id` to its next
    /// version, which is returned, with `look`, how its file looked when it
    /// was fetched; the other sources keep theirs. `None` when there is no
    /// such source.
    pub fn reload(&mut self, id: SourceId, look: Option<Look>) -> Option<Version> {
        let source = self.source_mut(id)?;
        source.version = source.version.next();
        if let Some(file) = &mut source.file {
            file.look = look;
        }
        Some(source.version)
    }

    /// A reread failed: the source `id` keeps its version and takes `look`,
    /// how its file looked when the reread fetched it, so that the file is
    /// read again only when it changes again.
    pub fn looked(&mut self, id: SourceId, look: Option<Look>) {
        let file = self.source_mut(id).and_then(|source| source.file.as_mut());
        if let Some(file) = file {
            file.look = look;
        }
    }

    /// The subscriptions: each source with a file, while `following`, every
    /// [`POLL`], since the look of its version.
    pub fn watches(&self, following: bool) -> Vec<Watch> {
        let files = self.sources.iter().filter_map(|source| {
            let file = source.file.as_ref()?;
            Some(Watch {
                what: Watched::Source(source.id),
                path: file.path.clone(),
                every: POLL,
                since: file.look,
            })
        });
        files.filter(|_| following).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn ids(session: &Session) -> Vec<SourceId> {
        session.sources().iter().map(Source::id).collect()
    }

    #[test]
    fn a_session_has_a_source_for_each_file_given_in_order() {
        let session = Session::new(["a.md", "b.md", "c.md"]);
        let names: Vec<&str> = session.sources().iter().map(Source::name).collect();
        assert_eq!(names, ["a.md", "b.md", "c.md"]);
        let ids = ids(&session);
        assert_eq!(ids.iter().copied().collect::<HashSet<_>>().len(), 3);
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "{ids:?}");
    }

    #[test]
    fn a_lone_file_is_a_session_of_one_source() {
        let session = Session::new(["notes.md"]);
        assert_eq!(session.sources().len(), 1);
        assert_eq!(session.sources()[0].name(), "notes.md");
        assert_eq!(session.view().shows(), ids(&session));
    }

    #[test]
    fn ids_are_stable_and_found_again() {
        let mut session = Session::new(["a.md", "b.md"]);
        let before = ids(&session);
        session.reload(before[0], None);
        assert_eq!(ids(&session), before);
        assert_eq!(session.source(before[1]).unwrap().name(), "b.md");
        assert_eq!(session.source(SourceId(99)), None);
    }

    #[test]
    fn the_built_view_shows_every_source_in_order() {
        let session = Session::new(["a.md", "b.md"]);
        assert_eq!(session.view().shows(), ids(&session));
        assert_eq!(session.view().id(), ViewId(0));
    }

    #[test]
    fn the_built_view_keeps_its_id_through_a_reload() {
        let mut session = Session::new(["a.md"]);
        let id = ids(&session)[0];
        session.reload(id, None);
        assert_eq!(session.view().id(), ViewId(0));
        assert_eq!(session.view().shows(), [id]);
    }

    #[test]
    fn a_reload_brings_the_version_of_that_source_only() {
        let mut session = Session::new(["a.md", "b.md"]);
        let [a, b] = ids(&session)[..] else {
            unreachable!()
        };
        assert_eq!(session.reload(b, None), Some(Version(1)));
        assert_eq!(session.source(a).unwrap().version(), Version::FIRST);
        assert_eq!(session.source(b).unwrap().version(), Version(1));
        assert_eq!(session.reload(b, None), Some(Version(2)));
        assert_eq!(session.source(a).unwrap().version(), Version::FIRST);
    }

    #[test]
    fn reloading_a_source_the_session_lacks_changes_nothing() {
        let mut session = Session::new(["a.md"]);
        let before = session.clone();
        assert_eq!(session.reload(SourceId(7), None), None);
        assert_eq!(session, before);
    }

    #[test]
    fn a_new_source_or_view_gets_an_id_that_none_of_the_existing_have() {
        let mut session = Session::new(["a.md", "b.md"]);
        let sources = ids(&session);
        let views: Vec<ViewId> = session.views.iter().map(View::id).collect();
        let view = session.add_view(Vec::new());
        assert!(!views.contains(&view), "{view:?} among {views:?}");
        let added = session.add_source("c.md".to_owned());
        assert!(!sources.contains(&added), "{added:?} among {sources:?}");
        assert_eq!(session.sources().len(), 3);
    }

    fn file(path: &str, length: u64) -> SourceFile {
        SourceFile {
            path: PathBuf::from(path),
            input: Input::Markdown,
            look: Some(look(length)),
        }
    }

    fn look(length: u64) -> Look {
        Look {
            modified: None,
            length,
        }
    }

    #[test]
    fn a_source_has_the_file_it_is_read_from_and_the_others_have_none() {
        let mut session = Session::new(["a.md", "b.md"]);
        let [a, b] = ids(&session)[..] else {
            unreachable!()
        };
        assert_eq!(session.source(a).unwrap().file(), None);
        session.read_from(a, file("a.md", 3));
        assert_eq!(session.source(a).unwrap().file(), Some(&file("a.md", 3)));
        assert_eq!(session.source(b).unwrap().file(), None);
    }

    #[test]
    fn a_reread_is_the_job_of_the_first_source_with_a_file() {
        let mut session = Session::new(["a.md", "b.md"]);
        let [a, b] = ids(&session)[..] else {
            unreachable!()
        };
        assert_eq!(session.reread(), None);
        session.read_from(b, file("b.md", 1));
        let job = |source, path: &str| {
            Some(Reread {
                source,
                path: PathBuf::from(path),
                input: Input::Markdown,
            })
        };
        assert_eq!(session.reread(), job(b, "b.md"));
        session.read_from(a, file("a.md", 1));
        assert_eq!(session.reread(), job(a, "a.md"));
    }

    #[test]
    fn a_reload_takes_the_look_the_reread_was_fetched_at_with_the_next_version() {
        let mut session = Session::new(["a.md"]);
        let id = ids(&session)[0];
        session.read_from(id, file("a.md", 1));
        assert_eq!(session.reload(id, Some(look(2))), Some(Version(1)));
        let source = session.source(id).unwrap();
        assert_eq!(source.file().map(|file| file.look), Some(Some(look(2))));
        assert_eq!(source.version(), Version(1));
        // A file that could not be looked at has no look.
        session.reload(id, None);
        let source = session.source(id).unwrap();
        assert_eq!(source.file().map(|file| file.look), Some(None));
    }

    #[test]
    fn a_failed_reread_takes_the_look_and_keeps_the_version() {
        let mut session = Session::new(["a.md", "b.md"]);
        let [a, b] = ids(&session)[..] else {
            unreachable!()
        };
        session.read_from(a, file("a.md", 1));
        session.read_from(b, file("b.md", 1));
        session.looked(a, Some(look(5)));
        let looks = |id| session.source(id).unwrap().file().map(|file| file.look);
        assert_eq!(session.source(a).unwrap().version(), Version::FIRST);
        assert_eq!(looks(a), Some(Some(look(5))));
        assert_eq!(looks(b), Some(Some(look(1))));
    }

    #[test]
    fn a_source_without_a_file_takes_no_look() {
        let mut session = Session::new(["a.md"]);
        let id = ids(&session)[0];
        session.looked(id, Some(look(5)));
        assert_eq!(session.reload(id, Some(look(5))), Some(Version(1)));
        assert_eq!(session.source(id).unwrap().file(), None);
    }

    #[test]
    fn a_followed_source_with_a_file_is_watched_every_poll_since_its_look() {
        let mut session = Session::new(["a.md", "b.md"]);
        let [a, _] = ids(&session)[..] else {
            unreachable!()
        };
        session.read_from(a, file("a.md", 4));
        let watches = session.watches(true);
        let expected = Watch {
            what: Watched::Source(a),
            path: PathBuf::from("a.md"),
            every: POLL,
            since: Some(look(4)),
        };
        assert_eq!(watches, [expected]);
        // The look of the version moves the watch's `since` along.
        session.reload(a, Some(look(9)));
        assert_eq!(session.watches(true)[0].since, Some(look(9)));
    }

    #[test]
    fn nothing_is_watched_unless_followed_or_without_a_file() {
        let mut session = Session::new(["a.md"]);
        assert_eq!(session.watches(true), []);
        let id = ids(&session)[0];
        session.read_from(id, file("a.md", 4));
        assert_eq!(session.watches(false), []);
    }
}
