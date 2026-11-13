//! The documents shown, as a version of them: the versions of their files
//! and what is worked out of the documents once for each version.
//!
//! The state engine owns an [`Edition`] (`State::edition`). It is made when
//! a layout starts with versions the edition does not have, so it is made
//! once for the first version and once for each reload, and not again when
//! the window changes size, the mode changes or an image comes. A reload
//! brings the hashes of the new document with it, which the comparison of
//! the two versions made already, so nothing is hashed twice.
//!
//! This is in a file of its own and not in `sources.rs`, which holds what
//! the session knows of the sources and none of their documents.

use std::ops::Range;

use crate::session::sources::{Session, Version};
use crate::typeset::align::Align;
use crate::typeset::compare::Comparison;
use crate::typeset::driver::Content;
use crate::typeset::model::{Document, Hashes};
use crate::typeset::position::Pos;
use crate::typeset::targets::{Targets, targets};
use crate::typeset::text::DocumentText;

/// The versions of the files shown, and what is derived from their
/// documents once for each version.
#[derive(Debug)]
pub(crate) struct Edition {
    /// The version of each file shown, in the order they are shown.
    versions: Vec<Version>,
    /// The text of the documents: where a search looks.
    pub(crate) text: DocumentText,
    /// The links and the embeds of the documents: what Tab stops at.
    pub(crate) targets: Targets,
    /// The hashes of the documents' blocks, one table for each file.
    hashes: Vec<Hashes>,
}

impl Edition {
    /// The edition of `content` at the versions `session` has now, with
    /// `hashes` of its documents when they were made already.
    pub(crate) fn new(content: Content, session: &Session, hashes: Option<Vec<Hashes>>) -> Self {
        let documents = documents(content);
        let hashes = hashes
            .filter(|hashes| hashes.len() == documents.len())
            .unwrap_or_else(|| documents.iter().map(|document| document.hashes()).collect());
        Self {
            versions: versions(content, session),
            targets: targets(documents.iter().copied()),
            text: DocumentText::new(documents),
            hashes,
        }
    }

    /// The version of each file shown.
    pub(crate) fn versions(&self) -> &[Version] {
        &self.versions
    }

    /// The hashes of the blocks of file `file`.
    pub(crate) fn hashes(&self, file: usize) -> Option<&Hashes> {
        self.hashes.get(file)
    }
}

/// A value that is made of places, and the versions of the files whose
/// places they are. Its fields are private, so the only way to have the value
/// in another version is [`Held::align`], and the only way to move a place
/// from one version to another is [`Align`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Held<T> {
    versions: Vec<Version>,
    value: T,
}

impl<T> Held<T> {
    /// `value`, whose places are in the versions of `edition`.
    pub(crate) fn new(edition: &Edition, value: T) -> Self {
        Self {
            versions: edition.versions().to_vec(),
            value,
        }
    }

    /// The value when `edition` is the one it is of, and otherwise `None`.
    pub(crate) fn get(&self, edition: &Edition) -> Option<&T> {
        (self.versions == edition.versions()).then_some(&self.value)
    }
}

impl<T: Aligned> Held<T> {
    /// The value in the new versions `moves` leads to, as `edition` has
    /// them; `None` when it was not of the versions `moves` leads from, or
    /// is no more, such as a place in a block that is gone.
    pub(crate) fn align(&self, moves: &Moves, edition: &Edition) -> Option<Held<T>> {
        if self.versions != moves.from {
            return None;
        }
        Some(Held::new(edition, self.value.aligned(moves)?))
    }
}

/// How the places of one edition move to the next: the alignment of each
/// file whose version changed, and for the others, none, as their places stay.
pub(crate) struct Moves<'a> {
    from: Vec<Version>,
    to: Vec<Version>,
    files: Vec<Option<Align<'a>>>,
}

impl<'a> Moves<'a> {
    /// The moves from `old` to `new`. `changes` is how the document of file
    /// 0, the only one that is read again, differs from the one before; a
    /// file that changed with nothing to align by loses its places.
    pub(crate) fn new(old: &Edition, new: &'a Edition, changes: Option<&Comparison>) -> Self {
        let files = (0..new.versions().len())
            .map(
                |file| match old.versions().get(file) == new.versions().get(file) {
                    true => None,
                    false => changes
                        .filter(|_| file == 0)
                        .map(|changes| Align::new(file, changes, &new.text)),
                },
            )
            .collect();
        Self {
            from: old.versions().to_vec(),
            to: new.versions().to_vec(),
            files,
        }
    }

    /// `pos` in the new versions; `None` for a place in a file that changed
    /// with nothing to align it by.
    pub(crate) fn pos(&self, pos: &Pos) -> Option<Pos> {
        let changed = self.from.get(pos.file) != self.to.get(pos.file);
        match changed {
            false => Some(pos.clone()),
            true => Some(self.files.get(pos.file)?.as_ref()?.pos(pos)),
        }
    }

    /// The path in the new versions of the block at `path` of file `file`;
    /// `None` when the block is gone, or its file changed with nothing to
    /// align by.
    pub(crate) fn block(&self, file: usize, path: &[usize]) -> Option<Vec<usize>> {
        match self.from.get(file) != self.to.get(file) {
            false => Some(path.to_vec()),
            true => self.files.get(file)?.as_ref()?.block(path),
        }
    }

    /// `range` in the new versions.
    pub(crate) fn range(&self, range: &Range<Pos>) -> Option<Range<Pos>> {
        let changed = self.from.get(range.start.file) != self.to.get(range.start.file)
            || self.from.get(range.end.file) != self.to.get(range.end.file);
        match changed {
            false => Some(range.clone()),
            true => Some(self.pos(&range.start)?..self.pos(&range.end)?),
        }
    }
}

/// A value of places that can be moved to the next version.
pub(crate) trait Aligned: Sized {
    /// The value in the versions `moves` leads to, or `None` when it is no
    /// more.
    fn aligned(&self, moves: &Moves) -> Option<Self>;
}

impl Aligned for Pos {
    fn aligned(&self, moves: &Moves) -> Option<Self> {
        moves.pos(self)
    }
}

impl Aligned for Range<Pos> {
    fn aligned(&self, moves: &Moves) -> Option<Self> {
        moves.range(self)
    }
}

/// The documents of `content`, in the order they are shown.
pub(crate) fn documents<'a>(content: Content<'a>) -> Vec<&'a Document> {
    match content {
        Content::Document(document) => vec![document],
        Content::Files(files) => files.iter().map(|file| &file.document).collect(),
    }
}

/// The version of each file of `content` in `session`: a lone document is
/// the first source, and each file of several is the source of its number.
pub(crate) fn versions(content: Content, session: &Session) -> Vec<Version> {
    let sources = session.sources();
    (0..documents(content).len())
        .map(|file| {
            sources
                .get(file)
                .map_or(Version::FIRST, |source| source.version())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::model::File;
    use crate::typeset::parse::parse;

    fn file(name: &str, source: &str) -> File {
        File {
            name: name.to_owned(),
            document: parse(source),
        }
    }

    #[test]
    fn an_edition_has_the_version_of_each_file_and_the_hashes_of_its_documents() {
        let mut session = Session::new(["a.md", "b.md"]);
        let files = [file("a.md", "one\n"), file("b.md", "two\n\nthree\n")];
        let id = session.sources()[1].id();
        session.reload(id, None);
        let edition = Edition::new(Content::Files(&files), &session, None);
        assert_eq!(
            edition.versions(),
            [Version::FIRST, Version::FIRST.next()],
            "{edition:?}"
        );
        assert_eq!(edition.hashes(1), Some(&files[1].document.hashes()));
        assert_eq!(edition.hashes(2), None);
    }

    #[test]
    fn a_lone_document_is_the_first_source_and_a_session_without_one_is_the_first_version() {
        let document = parse("one\n");
        let mut session = Session::new(["a.md"]);
        let id = session.sources()[0].id();
        session.reload(id, None);
        let edition = Edition::new(Content::Document(&document), &session, None);
        assert_eq!(edition.versions(), [Version::FIRST.next()]);
        let bare = Session::new(Vec::<String>::new());
        assert_eq!(
            versions(Content::Document(&document), &bare),
            [Version::FIRST]
        );
    }

    #[test]
    fn hashes_that_came_with_a_document_are_kept_and_not_made_again() {
        let document = parse("one\n");
        let session = Session::new(["a.md"]);
        let made = crate::typeset::model::hashes_made();
        let hashes = vec![document.hashes()];
        let edition = Edition::new(Content::Document(&document), &session, Some(hashes));
        assert_eq!(crate::typeset::model::hashes_made() - made, 1);
        assert_eq!(edition.hashes(0), Some(&document.hashes()));
        // Hashes of another number of documents are not trusted.
        let edition = Edition::new(Content::Document(&document), &session, Some(Vec::new()));
        assert_eq!(edition.hashes(0), Some(&document.hashes()));
    }

    #[test]
    fn the_text_of_an_edition_is_the_text_of_its_documents() {
        let document = parse("one two\n");
        let session = Session::new(["a.md"]);
        let edition = Edition::new(Content::Document(&document), &session, None);
        assert_eq!(edition.text, DocumentText::new([&document]));
    }

    /// The editions of `old` and of `new` read again, the session after.
    fn editions<'a>(old: &'a Document, new: &'a Document) -> (Edition, Edition, Session) {
        let mut session = Session::new(["a.md"]);
        let before = Edition::new(Content::Document(old), &session, None);
        let id = session.sources()[0].id();
        session.reload(id, None);
        let after = Edition::new(Content::Document(new), &session, None);
        (before, after, session)
    }

    #[test]
    fn a_held_value_is_had_only_in_the_edition_it_is_of() {
        let (old, new) = (parse("a\n"), parse("a\n"));
        let (before, after, _) = editions(&old, &new);
        let held = Held::new(&before, Pos::block(0, &[0]));
        assert_eq!(held.get(&before), Some(&Pos::block(0, &[0])));
        assert_eq!(held.get(&after), None);
    }

    #[test]
    fn a_held_place_is_aligned_to_the_new_version_and_a_place_that_is_gone_is_not_held() {
        let (old, new) = (parse("a\n\nb\n\nc\n"), parse("new\n\na\n\nc\n"));
        let (before, after, _) = editions(&old, &new);
        let comparison = crate::typeset::compare::compare(&old, &new);
        let moves = Moves::new(&before, &after, Some(&comparison));
        let b = Held::new(&before, Pos::text(0, &[0], 0, 1));
        let moved = b.align(&moves, &after).expect("aligned");
        assert_eq!(moved.get(&after), Some(&Pos::text(0, &[1], 0, 1)));
        assert_eq!(moved.get(&before), None);
        // A range is aligned at both ends.
        let range = Pos::block(0, &[2])..Pos::end(0, &[2]);
        let moved = Held::new(&before, range).align(&moves, &after).unwrap();
        assert_eq!(
            moved.get(&after),
            Some(&(Pos::block(0, &[2])..Pos::end(0, &[2])))
        );
    }

    #[test]
    fn a_held_value_of_other_versions_than_the_moves_lead_from_is_not_aligned() {
        let (old, new) = (parse("a\n"), parse("a\n\nb\n"));
        let (before, after, _) = editions(&old, &new);
        let comparison = crate::typeset::compare::compare(&old, &new);
        let moves = Moves::new(&before, &after, Some(&comparison));
        let held = Held::new(&after, Pos::block(0, &[0]));
        assert!(held.align(&moves, &after).is_none());
    }

    #[test]
    fn a_place_in_a_file_that_changed_with_nothing_to_align_by_is_lost() {
        let (old, new) = (parse("a\n"), parse("b\n"));
        let (before, after, _) = editions(&old, &new);
        let moves = Moves::new(&before, &after, None);
        let held = Held::new(&before, Pos::block(0, &[0]));
        assert!(held.align(&moves, &after).is_none());
    }
}
