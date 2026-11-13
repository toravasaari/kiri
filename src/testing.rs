//! Scratch directories for the tests.

use std::path::{Path, PathBuf};

/// A directory of its own for one test, removed when the test ends.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// A new, empty directory named after `name`. Tests that run at the
    /// same time in one process, as under `cargo test`, may use the same
    /// name: each gets a directory of its own.
    pub(crate) fn new(name: &str) -> Self {
        Self::in_dir(&std::env::temp_dir(), name)
    }

    /// `new`, in the directory `parent` rather than the temporary one.
    pub(crate) fn in_dir(parent: &Path, name: &str) -> Self {
        let base = format!("kiri-{}-{name}", std::process::id());
        for n in 1.. {
            let dir = match n {
                1 => parent.join(&base),
                _ => parent.join(format!("{base}-{n}")),
            };
            // Creating the directory, and not removing what is there,
            // claims a name no other test holds.
            match std::fs::create_dir(&dir) {
                Ok(()) => return Self(dir),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("{}: {e}", dir.display()),
            }
        }
        unreachable!("the names ran out")
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The Markdown documents the tests run on: the examples' (`S*.md`) and
/// the fixtures', by path.
pub(crate) fn documents() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    for (dir, prefix) in [("examples", "S"), ("tests/fixtures", "")] {
        for entry in std::fs::read_dir(root.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name.starts_with(prefix) && name.ends_with(".md") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Whether the process `pid` runs. A zombie does not: it ends once its
/// parent, or init for an orphan, reaps it.
pub(crate) fn runs(pid: &str) -> bool {
    if cfg!(target_os = "linux") {
        // From /proc, since not every container has ps.
        let stat = Path::new("/proc").join(pid).join("stat");
        std::fs::read_to_string(stat).is_ok_and(|stat| {
            stat.rsplit_once(") ")
                .is_some_and(|(_, state)| !state.starts_with('Z'))
        })
    } else {
        // macOS, for one, has no /proc.
        let out = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .expect("ps runs");
        let state = String::from_utf8_lossy(&out.stdout);
        let state = state.trim();
        // ps fails, and prints nothing, when there is no such process.
        assert!(out.status.success() || state.is_empty(), "{out:?}");
        !state.is_empty() && !state.starts_with('Z')
    }
}

#[cfg(test)]
mod tests {
    use super::Scratch;

    #[test]
    fn scratches_of_one_name_get_directories_of_their_own() {
        let first = Scratch::new("scratch-twice");
        std::fs::write(first.join("kept"), "").unwrap();
        let second = Scratch::new("scratch-twice");
        assert_ne!(*first, *second);
        assert!(first.join("kept").exists());
        assert!(second.is_dir());
    }
}
