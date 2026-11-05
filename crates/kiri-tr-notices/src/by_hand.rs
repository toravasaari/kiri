//! `notices/by-hand.toml`: libraries whose notices cargo about cannot make.

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByHand {
    /// Libraries whose licence file cargo about cannot read, or that have
    /// none, so that it puts in a placeholder text. They are left out of its
    /// list and given here, with the files that hold their licences.
    #[serde(default)]
    pub unreadable: Vec<Entry>,
    /// Libraries that carry notices beyond their licence, such as the
    /// licences of code they took from elsewhere. They stay in cargo about's
    /// list as well.
    #[serde(default)]
    pub carried: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub title: String,
    /// What to tell the reader about the entry.
    #[serde(default)]
    pub note: String,
    pub crates: Vec<String>,
    /// Files of this repository, relative to `notices/`.
    #[serde(default)]
    pub files: Vec<String>,
    /// Files in the directory of each of the crates, relative to it.
    #[serde(default)]
    pub crate_files: Vec<String>,
    /// Directories in the directory of each of the crates, whose files, all
    /// of them and those below, are given.
    #[serde(default)]
    pub crate_dirs: Vec<String>,
}

pub fn parse(toml_text: &str) -> Result<ByHand, String> {
    toml::from_str(toml_text).map_err(|e| format!("notices/by-hand.toml: {e}"))
}
