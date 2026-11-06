//! The part of `cargo about generate --format json` that the notices use.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct About {
    pub licenses: Vec<License>,
}

/// One licence text, and the libraries it is the licence of.
#[derive(Debug, Deserialize)]
pub struct License {
    /// The SPDX identifier, such as `MIT`.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub text: String,
    pub used_by: Vec<UsedBy>,
}

#[derive(Debug, Deserialize)]
pub struct UsedBy {
    #[serde(rename = "crate")]
    pub library: Library,
}

#[derive(Debug, Deserialize)]
pub struct Library {
    pub name: String,
    pub version: String,
}

pub fn parse(json: &str) -> Result<About, String> {
    serde_json::from_str(json).map_err(|e| format!("not the JSON of cargo about: {e}"))
}
