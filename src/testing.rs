//! Scratch directories for the tests.

use std::path::{Path, PathBuf};

/// A directory of its own for one test, removed when the test ends.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// An empty directory for the test that calls it `name`, a name no
    /// other test uses.
    pub(crate) fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("kiri-{}-{name}", std::process::id()));
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
