//! The part of `cargo metadata` that the notices use: where each library
//! is, so that its own notice files can be read.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Metadata {
    packages: Vec<Package>,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    version: String,
    manifest_path: PathBuf,
}

/// A library in the dependency graph, as many times as it has versions.
#[derive(Debug, PartialEq)]
pub struct Found {
    pub version: String,
    pub dir: PathBuf,
}

impl Metadata {
    pub fn find(&self, name: &str) -> Vec<Found> {
        let mut found: Vec<Found> = self
            .packages
            .iter()
            .filter(|p| p.name == name)
            .map(|p| Found {
                version: p.version.clone(),
                dir: p
                    .manifest_path
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_default(),
            })
            .collect();
        found.sort_by(|a, b| a.version.cmp(&b.version));
        found
    }

    /// The version of the package `name`, if it is one of the packages.
    pub fn version_of(&self, name: &str) -> Option<&str> {
        self.packages
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.version.as_str())
    }
}

pub fn parse(json: &str) -> Result<Metadata, String> {
    serde_json::from_str(json).map_err(|e| format!("not the JSON of cargo metadata: {e}"))
}
