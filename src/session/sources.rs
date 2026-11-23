//! The session: what the state engine knows of what the reader is looking
//! at. Its sources, where the documents come from (a file, standard input),
//! each with an id, a name and a version that a reload brings forward, and
//! its views: the sources each shows.
//!
//! A [`Session`] holds a `Source` for each file given (a lone file and standard
//! input are a session of one), with its `SourceId`, its name and its
//! `Version`, which a reload brings forward. A source read from a lone regular
//! file also knows that file and how it looked when that version was read
//! ([`SourceFile`]), so the session can say what a reload reads again
//! ([`Session::reread`], a `Reread`) and what is watched ([`Session::watches`],
//! a `Watch` for each source with a file while it is followed, every `POLL`,
//! since that look). Nothing is read or looked at here: the platform makes the
//! session from the files it was given, and looks at the watches.
//!
//! The session also holds its `View`s, each with its `ViewId`, the `SourceId`s
//! it shows and the number the image keys of its first document carry
//! (`View::document`, unique in the session, 0 for the base). `Session::open`
//! makes a view of a file opened, and `close` takes it and the sources only it
//! shows away. What is reread, watched, named and checked (`reread`,
//! `watches`, `sources_of`, `assert_fits`) is asked by view. Ids and numbers
//! come from the session and are not used twice.
//!
//! The session does not hold the documents or their layout: the pager reads
//! the names and the count of its files from it, and whether there is a file
//! to reload or follow.
//!
//! The file is not `session.rs` because clippy's `module_inception` forbids a
//! module named like its parent.

use std::path::{Path, PathBuf};
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

impl ViewId {
    /// The base view, which is the first view a session has: [`Session::new`]
    /// makes it first.
    pub(crate) const BASE: ViewId = ViewId(0);
}

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
    /// The number the image keys of the document it brings carry.
    pub document: usize,
}

/// Where a document comes from: one file, or standard input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    id: SourceId,
    name: String,
    version: Version,
    file: Option<SourceFile>,
    /// The path the source was read from, when it was a file, also among
    /// several, which are not read again.
    origin: Option<PathBuf>,
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

    /// The path the source was read from, as it was given: of a lone
    /// file, and of a file among several. Standard input has none.
    pub fn path(&self) -> Option<&Path> {
        (self.file.as_ref().map(|file| file.path.as_path())).or(self.origin.as_deref())
    }
}

/// What the reader sees: the sources it shows, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    id: ViewId,
    shows: Vec<SourceId>,
    /// The number the image keys of its first document carry; the others
    /// follow it in order. Unique in the session.
    first: usize,
}

impl View {
    pub fn id(&self) -> ViewId {
        self.id
    }

    /// The number the image keys of the document of file `file` of the
    /// view carry: unique in the session.
    pub fn document(&self, file: usize) -> usize {
        self.first + file
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
    next_document: usize,
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
            next_document: 0,
        };
        let shows: Vec<SourceId> = names
            .into_iter()
            .map(|name| session.add_source(name.into()))
            .collect();
        let base = session.add_view(shows);
        debug_assert_eq!(base, ViewId::BASE);
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
            origin: None,
        });
        id
    }

    fn add_view(&mut self, shows: Vec<SourceId>) -> ViewId {
        let id = ViewId(self.next_view);
        self.next_view += 1;
        let first = self.next_document;
        self.next_document += shows.len();
        self.views.push(View { id, shows, first });
        id
    }

    /// A second view of the same sources, for a test of what keeps the
    /// places of views apart.
    #[cfg(test)]
    pub(crate) fn another_view(&mut self) -> ViewId {
        let shows = self.base().shows().to_vec();
        self.add_view(shows)
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

    /// Panics unless the sources of `view` are those of `content`: one for
    /// each of its files, and for a lone document at least one. The count
    /// and the order are carried by both, and nothing else ties them.
    pub fn assert_fits(&self, view: ViewId, content: Content) {
        let count = self.sources_of(view).len();
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

    /// The view the session starts with, which shows the files given.
    pub fn base(&self) -> &View {
        self.views.first().expect("a session has a base view")
    }

    /// The view `id`, if the session has it.
    pub fn view(&self, id: ViewId) -> Option<&View> {
        self.views.iter().find(|view| view.id == id)
    }

    /// The sources `view` shows, in its order; none for a view the
    /// session lacks.
    pub fn sources_of(&self, view: ViewId) -> Vec<&Source> {
        let shows = self.view(view).map_or(&[][..], View::shows);
        shows.iter().filter_map(|id| self.source(*id)).collect()
    }

    /// A file was opened: a new source named `name`, read from `file`, and
    /// a new view that shows it. Its image keys carry the number after
    /// those of every view so far.
    pub fn open(&mut self, name: String, file: SourceFile) -> ViewId {
        let id = self.add_source(name);
        self.read_from(id, file);
        self.add_view(vec![id])
    }

    /// The view `view` is closed, and the sources no other view shows
    /// with it. Ids and numbers are not used again. The base view is not
    /// closed.
    pub fn close(&mut self, view: ViewId) {
        debug_assert!(view != self.base().id, "the base view is not closed");
        if view == self.base().id {
            return;
        }
        self.views.retain(|other| other.id != view);
        let shown: Vec<SourceId> = (self.views.iter())
            .flat_map(|view| view.shows.iter().copied())
            .collect();
        self.sources.retain(|source| shown.contains(&source.id));
    }

    /// The source `id` is read from `file`. The frontend says so for a lone
    /// regular file only.
    pub fn read_from(&mut self, id: SourceId, file: SourceFile) {
        if let Some(source) = self.source_mut(id) {
            source.file = Some(file);
        }
    }

    /// The source `id` came from the file at `path`, as it was given. This
    /// does not make it one to read again: [`Session::read_from`] does.
    pub fn came_from(&mut self, id: SourceId, path: PathBuf) {
        if let Some(source) = self.source_mut(id) {
            source.origin = Some(path);
        }
    }

    /// The job of reading a source of `view` again: its first source with
    /// a file, which only a lone file has; `None` when nothing can be read
    /// again.
    pub fn reread(&self, view: ViewId) -> Option<Reread> {
        let shown = self.view(view)?;
        (self.sources_of(view).into_iter().enumerate()).find_map(|(at, source)| {
            let file = source.file.as_ref()?;
            Some(Reread {
                source: source.id,
                path: file.path.clone(),
                input: file.input,
                document: shown.document(at),
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

    /// The subscriptions: each source of `view` with a file, while
    /// `following`, every [`POLL`], since the look of its version.
    pub fn watches(&self, view: ViewId, following: bool) -> Vec<Watch> {
        let files = self.sources_of(view).into_iter().filter_map(|source| {
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
mod tests;
