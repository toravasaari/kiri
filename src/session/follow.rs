//! What Enter on a link does, worked out from the link and the paths of
//! the files shown: no file system is asked.
//!
//! A link that goes to a place in the file it is in, or to another file of
//! the view, is a jump; any other, an address away or a file that is not
//! shown, is copied, as before. A path is resolved against the directory
//! of the file that has the link, and the files of the view are found by
//! comparing paths as written: `./a.md` and `a.md` are one, but an absolute
//! link to a file given by a relative path is not, nor is a symbolic link.

use std::path::{Component, Path, PathBuf};

use crate::typeset::targets::Goes;

/// What Enter on a link does.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Follow {
    /// To file `file` of the view, at the place named `id`, or at the
    /// start of the file without one.
    Jump { file: usize, id: Option<String> },
    /// Copies the address: an address away, a file that is not shown.
    Copy(String),
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
        Goes::File { path, id, .. } => {
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
        assert_eq!(go("b.md", 0, &shown), Follow::Copy("b.md".into()));
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

    #[test]
    fn a_file_that_is_not_shown_and_an_address_away_are_copied() {
        let shown = paths(&["a.md", "b.md"]);
        for url in [
            "other.md",
            "https://x.example/a.md",
            "mailto:a@b",
            "//host/b.md",
            "image.png",
            "/abs/b.md",
        ] {
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
