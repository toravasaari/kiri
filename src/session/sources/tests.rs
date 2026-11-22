use super::*;
use std::collections::HashSet;

fn base(session: &Session) -> ViewId {
    session.base().id()
}

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
    assert_eq!(session.base().shows(), ids(&session));
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
    assert_eq!(session.base().shows(), ids(&session));
    assert_eq!(session.base().id(), ViewId(0));
}

#[test]
fn the_built_view_keeps_its_id_through_a_reload() {
    let mut session = Session::new(["a.md"]);
    let id = ids(&session)[0];
    session.reload(id, None);
    assert_eq!(session.base().id(), ViewId(0));
    assert_eq!(session.base().shows(), [id]);
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
    assert_eq!(session.reread(base(&session)), None);
    session.read_from(b, file("b.md", 1));
    let job = |source, path: &str, at: usize| {
        Some(Reread {
            source,
            path: PathBuf::from(path),
            input: Input::Markdown,
            document: at,
        })
    };
    assert_eq!(session.reread(base(&session)), job(b, "b.md", 1));
    session.read_from(a, file("a.md", 1));
    assert_eq!(session.reread(base(&session)), job(a, "a.md", 0));
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
    let watches = session.watches(base(&session), true);
    let expected = Watch {
        what: Watched::Source(a),
        path: PathBuf::from("a.md"),
        every: POLL,
        since: Some(look(4)),
    };
    assert_eq!(watches, [expected]);
    // The look of the version moves the watch's `since` along.
    session.reload(a, Some(look(9)));
    assert_eq!(
        session.watches(base(&session), true)[0].since,
        Some(look(9))
    );
}

#[test]
fn nothing_is_watched_unless_followed_or_without_a_file() {
    let mut session = Session::new(["a.md"]);
    assert_eq!(session.watches(base(&session), true), []);
    let id = ids(&session)[0];
    session.read_from(id, file("a.md", 4));
    assert_eq!(session.watches(base(&session), false), []);
}

fn opened(session: &mut Session, name: &str) -> ViewId {
    session.open(name.to_owned(), file(name, 1))
}

#[test]
fn opening_adds_a_view_of_one_new_source_numbered_after_the_others() {
    let mut session = Session::new(["a.md", "b.md", "c.md"]);
    let before = ids(&session);
    let view = opened(&mut session, "other/d.md");
    let shown = session.view(view).unwrap();
    assert_eq!(shown.document(0), 3);
    assert_eq!(session.base().document(2), 2);
    let sources = session.sources_of(view);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].name(), "other/d.md");
    assert!(!before.contains(&sources[0].id()));
    assert_eq!(sources[0].file(), Some(&file("other/d.md", 1)));
    // The next one is numbered after it.
    let next = opened(&mut session, "e.md");
    assert_eq!(session.view(next).unwrap().document(0), 4);
    // The base still shows its own sources.
    assert_eq!(session.base().shows(), before);
}

#[test]
fn a_view_rereads_and_watches_only_its_own_sources() {
    let mut session = Session::new(["a.md"]);
    let a = ids(&session)[0];
    session.read_from(a, file("a.md", 1));
    let view = opened(&mut session, "d.md");
    let opened_source = session.sources_of(view)[0].id();
    let job = session.reread(view).unwrap();
    assert_eq!(job.source, opened_source);
    assert_eq!(job.path, PathBuf::from("d.md"));
    assert_eq!(job.document, 1);
    let job = session.reread(base(&session)).unwrap();
    assert_eq!((job.source, job.document), (a, 0));
    let watched = |view| -> Vec<Watched> {
        let watches = session.watches(view, true);
        watches.iter().map(|watch| watch.what).collect()
    };
    assert_eq!(watched(view), [Watched::Source(opened_source)]);
    assert_eq!(watched(base(&session)), [Watched::Source(a)]);
    assert_eq!(session.reread(ViewId(99)), None);
}

#[test]
fn closing_a_view_drops_the_sources_only_it_shows_and_numbers_are_not_reused() {
    let mut session = Session::new(["a.md"]);
    let view = opened(&mut session, "d.md");
    let dropped = session.sources_of(view)[0].id();
    session.close(view);
    assert!(session.view(view).is_none());
    assert!(session.source(dropped).is_none());
    assert_eq!(session.sources().len(), 1);
    let again = opened(&mut session, "d.md");
    assert_ne!(again, view);
    assert_ne!(session.sources_of(again)[0].id(), dropped);
    assert_eq!(session.view(again).unwrap().document(0), 2);
}

#[test]
fn closing_a_view_keeps_a_source_another_view_shows() {
    let mut session = Session::new(["a.md"]);
    let other = session.another_view();
    session.close(other);
    assert_eq!(session.sources().len(), 1);
    assert_eq!(session.sources_of(base(&session)).len(), 1);
}

#[test]
fn assert_fits_counts_the_sources_of_the_view() {
    use crate::typeset::model::File;
    let mut session = Session::new(["a.md", "b.md"]);
    let view = opened(&mut session, "d.md");
    let file = |name: &str| File {
        name: name.to_owned(),
        document: crate::typeset::parse::parse("x"),
    };
    // Two files fit the base, though the session has three sources.
    session.assert_fits(base(&session), Content::Files(&[file("a"), file("b")]));
    session.assert_fits(view, Content::Files(&[file("d")]));
    let document = crate::typeset::parse::parse("x");
    session.assert_fits(view, Content::Document(&document));
}

#[test]
#[should_panic(expected = "the session has 1 sources but the content has 2 files")]
fn assert_fits_panics_when_the_view_has_another_count() {
    use crate::typeset::model::File;
    let mut session = Session::new(["a.md", "b.md"]);
    let view = opened(&mut session, "d.md");
    let files: Vec<File> = ["a", "b"]
        .map(|name| File {
            name: name.to_owned(),
            document: crate::typeset::parse::parse("x"),
        })
        .into();
    session.assert_fits(view, Content::Files(&files));
}

#[test]
fn the_sources_of_a_view_are_in_its_order() {
    let session = Session::new(["c.md", "a.md", "b.md"]);
    let names: Vec<&str> = (session.sources_of(base(&session)).into_iter())
        .map(Source::name)
        .collect();
    assert_eq!(names, ["c.md", "a.md", "b.md"]);
    assert!(session.sources_of(ViewId(7)).is_empty());
}
