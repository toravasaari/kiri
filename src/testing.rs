//! Scratch directories for the tests.

use std::path::{Path, PathBuf};

/// A directory of its own for one test, removed when the test ends.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// An empty directory for the test that calls it `name`, a name no
    /// other test uses.
    pub(crate) fn new(name: &str) -> Self {
        Self::in_dir(&std::env::temp_dir(), name)
    }

    /// `new`, in the directory `parent` rather than the temporary one.
    pub(crate) fn in_dir(parent: &Path, name: &str) -> Self {
        let dir = parent.join(format!("kiri-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
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
        std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .is_ok_and(|out| {
                let state = String::from_utf8_lossy(&out.stdout);
                let state = state.trim();
                !state.is_empty() && !state.starts_with('Z')
            })
    }
}
