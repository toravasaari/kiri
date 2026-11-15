//! What Enter on a link does, worked out from the link and the paths of
//! the files shown: no file system is asked.
//!
//! A link that goes to a place in the file it is in, or to another file of
//! the view, is a jump; one to a Markdown file that is not shown is opened
//! as a view of its own; any other, an address away or a file that is not
//! Markdown, is copied. A path is resolved against the directory
//! of the file that has the link, and the files of the view are found by
//! comparing paths as written: `./a.md` and `a.md` are one, but an absolute
//! link to a file given by a relative path is not, nor is a symbolic link.

use std::path::{Component, Path, PathBuf};

use crate::session::edition::Held;
use crate::session::history::Back;
use crate::session::sources::ViewId;
use crate::session::view::Anchor;
use crate::typeset::targets::Goes;

/// What Enter on a link does.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Follow {
    /// To file `file` of the view, at the place named `id`, or at the
    /// start of the file without one.
    Jump { file: usize, id: Option<String> },
    /// A Markdown file that the view does not show, read from `path`, at
    /// the place named `id`, or at its start without one.
    Open { path: PathBuf, id: Option<String> },
    /// Copies the address: an address away, a file that is not shown.
    Copy(String),
}

/// A turn to another view: opening one over the top, or going back to one
/// below. It ends the layout, and the loop carries it out between layouts.
pub(crate) enum Turn {
    Open(Box<Opening>),
    Back(Return),
}

/// A Markdown file to open over the top view, the place to arrive at in
/// it, and the place to come back to. A link names the place by `id`; going
/// forward opens the file again at the `at` it was left at, and keeps what
/// else is left to go forward to, which a link forgets.
pub(crate) struct Opening {
    pub(crate) path: PathBuf,
    pub(crate) id: Option<String>,
    pub(crate) at: Option<Anchor>,
    pub(crate) forward: bool,
    pub(crate) back: Back,
}

/// A view below to go back to, and where in it, when the history has
/// that, and the views the turn closes, for going forward again.
pub(crate) struct Return {
    pub(crate) view: ViewId,
    pub(crate) place: Option<Held<Anchor>>,
    pub(crate) left: Vec<Left>,
}

/// A view that going back closes, and where it was left in it: the place it
/// was at, or the place the link to the next view was followed from, when
/// one is known.
pub(crate) struct Left {
    pub(crate) view: ViewId,
    pub(crate) place: Option<Anchor>,
}

/// Where a link to `url`, which `goes` as its address says, in file `from`
/// of the view, goes among the files `paths`, which the files of the view
/// were read from.
pub(crate) fn follow((url, goes): (&str, &Goes), from: usize, paths: &[Option<PathBuf>]) -> Follow {
    let copy = || Follow::Copy(url.to_owned());
    match goes {
        Goes::Here { id } => Follow::Jump {
            file: from,
            id: Some(id.clone()).filter(|id| !id.is_empty()),
        },
        Goes::File { path, id, markdown } => {
            // A file without a path, standard input, is the working
            // directory's for the links in it.
            let directory = paths
                .get(from)
                .and_then(|path| path.as_deref())
                .and_then(Path::parent)
                .unwrap_or(Path::new(""));
            let wanted = normal(&directory.join(path));
            let found = paths.iter().position(|shown| {
                shown
                    .as_deref()
                    .is_some_and(|shown| normal(shown) == wanted)
            });
            match found {
                Some(file) => Follow::Jump {
                    file,
                    id: id.clone(),
                },
                None if *markdown => Follow::Open {
                    path: wanted,
                    id: id.clone(),
                },
                None => copy(),
            }
        }
        Goes::Nowhere | Goes::Away => copy(),
    }
}

/// `path` with `.` and `..` worked out by its components, as written: no
/// symbolic links are followed and the file system is not asked.
fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                // Above the root is the root; above the start of a
                // relative path stays above it.
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::targets::goes;

    fn go(url: &str, from: usize, paths: &[Option<PathBuf>]) -> Follow {
        follow((url, &goes(url)), from, paths)
    }

    fn paths(names: &[&str]) -> Vec<Option<PathBuf>> {
        names
            .iter()
            .map(|name| (*name != "-").then(|| PathBuf::from(name)))
            .collect()
    }

    fn jump(file: usize, id: Option<&str>) -> Follow {
        Follow::Jump {
            file,
            id: id.map(Into::into),
        }
    }

    #[test]
    fn an_anchor_link_jumps_in_its_own_file() {
        let shown = paths(&["a.md", "b.md"]);
        assert_eq!(go("#x", 1, &shown), jump(1, Some("x")));
        assert_eq!(go("#", 0, &shown), jump(0, None));
        // Also in a file with no path.
        assert_eq!(go("#x", 0, &paths(&["-"])), jump(0, Some("x")));
    }

    #[test]
    fn a_link_to_a_shown_file_jumps_to_it_with_or_without_an_id() {
        let shown = paths(&["a.md", "b.md", "c.md"]);
        assert_eq!(go("c.md", 0, &shown), jump(2, None));
        assert_eq!(go("./b.md#top", 0, &shown), jump(1, Some("top")));
        assert_eq!(go("a.md?x=1#y", 1, &shown), jump(0, Some("y")));
        assert_eq!(
            go("my%20b.md", 0, &paths(&["a.md", "my b.md"])),
            jump(1, None)
        );
    }

    #[test]
    fn a_path_is_resolved_against_the_linking_file_s_directory() {
        let shown = paths(&["docs/a.md", "README.md", "docs/sub/c.md"]);
        assert_eq!(go("../README.md", 0, &shown), jump(1, None));
        assert_eq!(go("sub/c.md", 0, &shown), jump(2, None));
        // From the file in the top directory, `docs/a.md` is the first.
        assert_eq!(go("docs/a.md#x", 1, &shown), jump(0, Some("x")));
        // `b.md` of README's directory is not the one of `docs/`.
        let shown = paths(&["docs/a.md", "b.md"]);
        assert_eq!(go("b.md", 0, &shown), open("docs/b.md", None));
        assert_eq!(go("../b.md", 0, &shown), jump(1, None));
        let absolute = paths(&["/r/docs/a.md", "/r/README.md"]);
        assert_eq!(go("../README.md", 0, &absolute), jump(1, None));
        assert_eq!(go("/r/README.md", 0, &absolute), jump(1, None));
    }

    #[test]
    fn standard_input_resolves_against_the_working_directory() {
        let shown = paths(&["-", "b.md", "docs/c.md"]);
        assert_eq!(go("b.md", 0, &shown), jump(1, None));
        assert_eq!(go("docs/c.md", 0, &shown), jump(2, None));
        // And a file with no path is not one a link finds.
        assert_eq!(go("-", 1, &shown), Follow::Copy("-".into()));
    }

    fn open(path: &str, id: Option<&str>) -> Follow {
        Follow::Open {
            path: PathBuf::from(path),
            id: id.map(Into::into),
        }
    }

    #[test]
    fn a_markdown_file_that_is_not_shown_is_opened() {
        let shown = paths(&["a.md", "b.md"]);
        assert_eq!(go("other.md", 0, &shown), open("other.md", None));
        assert_eq!(
            go("other.md#part", 0, &shown),
            open("other.md", Some("part"))
        );
        assert_eq!(go("x.markdown", 1, &shown), open("x.markdown", None));
    }

    #[test]
    fn an_opened_path_is_resolved_against_the_linking_file_s_directory_and_made_plain() {
        let shown = paths(&["docs/a.md"]);
        assert_eq!(go("../b.md#x", 0, &shown), open("b.md", Some("x")));
        assert_eq!(go("./sub/../c.md", 0, &shown), open("docs/c.md", None));
    }

    #[test]
    fn a_link_from_standard_input_opens_against_the_working_directory() {
        let shown = paths(&["-"]);
        assert_eq!(go("sub/b.md", 0, &shown), open("sub/b.md", None));
        assert_eq!(go("./b.md", 0, &shown), open("b.md", None));
    }

    #[test]
    fn an_absolute_link_to_a_shown_file_is_opened_not_jumped_to() {
        // The shown file was given by a relative path, so the comparison of
        // paths as written does not find it; it opens as a view of its own.
        let shown = paths(&["docs/a.md", "b.md"]);
        assert_eq!(go("/r/b.md", 0, &shown), open("/r/b.md", None));
    }

    #[test]
    fn a_file_that_is_not_markdown_is_still_copied() {
        let shown = paths(&["a.md"]);
        for url in ["image.png", "data.csv", "notes.txt", "/abs/pic.png"] {
            assert_eq!(go(url, 0, &shown), Follow::Copy(url.into()), "{url}");
        }
    }

    #[test]
    fn an_address_away_is_copied() {
        let shown = paths(&["a.md", "b.md"]);
        for url in ["https://x.example/a.md", "mailto:a@b", "//host/b.md"] {
            assert_eq!(go(url, 0, &shown), Follow::Copy(url.into()), "{url}");
        }
    }

    #[test]
    fn normal_works_out_dots_by_the_components_alone() {
        let normal = |path: &str| normal(Path::new(path));
        assert_eq!(normal("./a/../b.md"), Path::new("b.md"));
        assert_eq!(normal("a//b/./c.md"), Path::new("a/b/c.md"));
        assert_eq!(normal("../../a.md"), Path::new("../../a.md"));
        assert_eq!(normal("/../a.md"), Path::new("/a.md"));
        assert_eq!(normal(""), Path::new(""));
    }
}
