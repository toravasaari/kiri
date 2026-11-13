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

use crate::session::sources::{Session, Version};
use crate::typeset::layout::Content;
use crate::typeset::model::{Document, Hashes};
use crate::typeset::text::DocumentText;

/// The versions of the files shown, and what is derived from their
/// documents once for each version.
#[derive(Debug)]
pub(crate) struct Edition {
    /// The version of each file shown, in the order they are shown.
    versions: Vec<Version>,
    /// The text of the documents: where a search looks.
    pub(crate) text: DocumentText,
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
}
