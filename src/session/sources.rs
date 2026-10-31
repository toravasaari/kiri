//! The session: what the state engine knows of what the reader is looking
//! at. Its sources, where the documents come from (a file, standard input),
//! each with an id, a name and a version that a reload brings forward, and
//! its views, of which one is built: the sources it shows.
//!
//! Only the ids and the names are here. The documents and their layout stay
//! where they are for now, and nothing is read: the platform makes the
//! session from the files it was given.

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

    fn next(self) -> Version {
        Version(self.0 + 1)
    }
}

/// A view in a session. Ids come from the session and are never used for
/// another view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViewId(u32);

/// Where a document comes from: one file, or standard input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    id: SourceId,
    name: String,
    version: Version,
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

    /// The view that is built.
    pub fn view(&self) -> &View {
        self.views.first().expect("a session has a view")
    }

    /// Brings the source `id` to its next version, which is returned; the
    /// other sources keep theirs. `None` when there is no such source.
    pub fn reload(&mut self, id: SourceId) -> Option<Version> {
        let source = self.sources.iter_mut().find(|source| source.id == id)?;
        source.version = source.version.next();
        Some(source.version)
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
        session.reload(before[0]);
        assert_eq!(ids(&session), before);
        assert_eq!(session.source(before[1]).unwrap().name(), "b.md");
        assert_eq!(session.source(SourceId(99)), None);
    }

    #[test]
    fn the_built_view_shows_every_source_in_order() {
        let session = Session::new(["a.md", "b.md"]);
        assert_eq!(session.view().shows(), ids(&session));
        assert_eq!(session.view().id(), session.view().id());
    }

    #[test]
    fn a_reload_brings_the_version_of_that_source_only() {
        let mut session = Session::new(["a.md", "b.md"]);
        let [a, b] = ids(&session)[..] else {
            unreachable!()
        };
        assert_eq!(session.reload(b), Some(Version(1)));
        assert_eq!(session.source(a).unwrap().version(), Version::FIRST);
        assert_eq!(session.source(b).unwrap().version(), Version(1));
        assert_eq!(session.reload(b), Some(Version(2)));
        assert_eq!(session.source(a).unwrap().version(), Version::FIRST);
    }

    #[test]
    fn reloading_a_source_the_session_lacks_changes_nothing() {
        let mut session = Session::new(["a.md"]);
        let before = session.clone();
        assert_eq!(session.reload(SourceId(7)), None);
        assert_eq!(session, before);
    }

    #[test]
    fn ids_of_different_kinds_count_on_their_own_and_are_never_reused() {
        let mut session = Session::new(["a.md"]);
        let first = session.view().id();
        let second = session.add_view(Vec::new());
        assert_ne!(first, second);
        let added = session.add_source("b.md".to_owned());
        assert!(!ids(&session)[..1].contains(&added));
        assert_eq!(session.sources().len(), 2);
    }
}
