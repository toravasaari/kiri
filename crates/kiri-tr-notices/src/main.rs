//! Puts together the third-party notices that kiri's release archives carry.
//! See the README of this crate; `scripts/notices.sh` runs it.

mod about;
mod by_hand;
mod metadata;
mod render;
mod syntax;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("kiri-tr-notices: {message}");
            ExitCode::FAILURE
        }
    }
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

/// The files of the disk.
struct Disk;

impl render::Files for Disk {
    fn read(&self, path: &Path) -> Result<String, String> {
        read(path)
    }

    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, String> {
        let mut found = Vec::new();
        let entries =
            fs::read_dir(dir).map_err(|e| format!("cannot list {}: {e}", dir.display()))?;
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                found.extend(self.list(&path)?);
            } else {
                found.push(path);
            }
        }
        found.sort();
        Ok(found)
    }
}

/// `args` are the JSON of `cargo about generate --format json` and of
/// `cargo metadata`; the hand-written parts are read from `notices/` of the
/// current directory, the root of the repository.
fn run(args: &[String]) -> Result<String, String> {
    let [about_path, metadata_path] = args else {
        return Err("usage: kiri-tr-notices ABOUT.json METADATA.json".to_string());
    };
    let notices = Path::new("notices");
    let inputs = render::Inputs {
        about: about::parse(&read(Path::new(about_path))?)?,
        metadata: metadata::parse(&read(Path::new(metadata_path))?)?,
        by_hand: by_hand::parse(&read(&notices.join("by-hand.toml"))?)?,
        header: read(&notices.join("header.md"))?,
        extra: read(&notices.join("extra.md"))?,
        syntax_notices: syntax::notices(two_face::acknowledgement::listing().for_syntaxes()),
    };
    render::notices(&inputs, &Disk, notices)
}
