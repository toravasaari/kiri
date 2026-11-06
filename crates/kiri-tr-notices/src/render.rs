//! Puts the parts of the notices into one Markdown text.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::{Path, PathBuf};

use crate::about::{About, Library};
use crate::by_hand::{ByHand, Entry};
use crate::metadata::Metadata;

/// What `cargo about` puts in the text of a licence of which it found no
/// file, in the place of a copyright line.
const PLACEHOLDER: &str = "<copyright holders>";

const RUST_LIBRARIES_INTRO: &str = "\
kiri is built from the Rust libraries below, each under the licence named
with it. Where a library offers a choice of licences, the one named is the
one taken. The source of a library is at the address given with it.
";

const BY_HAND_INTRO: &str = "\
These libraries come with notices that the list above does not hold: either
their licence cannot be read from their packages, or they carry notices of
code they took from elsewhere.
";

const SYNTAX_INTRO: &str = "\
kiri highlights code with the syntax definitions of
[two-face](https://crates.io/crates/two-face), which it collects from
[bat](https://github.com/sharkdp/bat). The licence texts below are those of
the definitions that ask for one.
";

/// Reads files, so that the notices can be made from a fake set of them.
pub trait Files {
    fn read(&self, path: &Path) -> Result<String, String>;
    /// The files in `dir` and below it, in the order of their paths.
    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, String>;
}

pub struct Inputs {
    pub about: About,
    pub metadata: Metadata,
    pub by_hand: ByHand,
    pub header: String,
    pub extra: String,
    pub syntax_notices: String,
}

/// The notices. `notices_dir` is where `by-hand.toml`'s `files` and the
/// `@include` lines of `extra` are relative to.
pub fn notices(inputs: &Inputs, files: &dyn Files, notices_dir: &Path) -> Result<String, String> {
    let version = inputs
        .metadata
        .version_of("kiri-the-reader")
        .ok_or("cargo metadata does not have the package kiri-the-reader")?;
    let mut out = inputs.header.replace("{version}", version);
    if !out.ends_with("\n\n") {
        out.push_str(if out.ends_with('\n') { "\n" } else { "\n\n" });
    }
    out.push_str(&rust_libraries(inputs)?);
    out.push_str(&libraries_by_hand(inputs, files, notices_dir)?);
    out.push_str("## Syntax definitions\n\n");
    out.push_str(SYNTAX_INTRO);
    out.push('\n');
    out.push_str(&inputs.syntax_notices);
    out.push_str("## Other material\n\n");
    out.push_str(&expand_includes(&inputs.extra, files, notices_dir)?);
    Ok(out)
}

/// Text in a fence of four backticks, so that a fence inside it stays whole.
fn fenced(text: &str) -> Result<String, String> {
    if text.contains("````") {
        return Err("a licence text holds a fence of four backticks".to_string());
    }
    Ok(format!("````text\n{}\n````\n\n", text.trim_end()))
}

fn source_line(name: &str, version: &str) -> String {
    format!("- {name} {version}, source at <https://crates.io/crates/{name}/{version}>\n")
}

/// The licences of all the libraries that are not given by hand.
fn rust_libraries(inputs: &Inputs) -> Result<String, String> {
    let apart: BTreeSet<&str> = inputs
        .by_hand
        .unreadable
        .iter()
        .flat_map(|entry| entry.crates.iter().map(String::as_str))
        .collect();
    let listed: BTreeSet<&str> = inputs
        .about
        .licenses
        .iter()
        .flat_map(|license| license.used_by.iter().map(|u| u.library.name.as_str()))
        .collect();
    if let Some(name) = apart.iter().find(|name| !listed.contains(**name)) {
        return Err(format!(
            "notices/by-hand.toml has {name} as unreadable, but cargo about does not list it: \
             remove it from the file"
        ));
    }

    for license in &inputs.about.licenses {
        if license.text.contains(PLACEHOLDER) {
            let mut names: Vec<&str> = license
                .used_by
                .iter()
                .map(|u| u.library.name.as_str())
                .filter(|name| !apart.contains(name))
                .collect();
            if names.is_empty() {
                continue;
            }
            names.sort_unstable();
            names.dedup();
            return Err(format!(
                "cargo about found no readable licence file for {}: see how each is licensed, \
                 and add it to `unreadable` in notices/by-hand.toml",
                names.join(", ")
            ));
        }
    }

    let groups = groups(&inputs.about, &apart);
    let mut out = format!("## Rust libraries\n\n{RUST_LIBRARIES_INTRO}\n");
    for group in &groups {
        writeln!(out, "### {}\n\nUsed by:\n", group.name).expect("writing to a String");
        let mut split_out = false;
        for (library, copyrights) in &group.used {
            out.push_str(&source_line(&library.name, &library.version));
            for line in copyrights {
                writeln!(out, "  - {line}").expect("writing to a String");
                split_out = true;
            }
        }
        out.push('\n');
        if split_out {
            out.push_str("The copyright notices above go with this licence text:\n\n");
        }
        out.push_str(&fenced(&group.text)?);
    }
    Ok(out)
}

/// Libraries that have the same licence text, with the copyright lines that
/// were taken out of it, which are each library's own.
struct Group<'a> {
    name: &'a str,
    text: String,
    used: Vec<(&'a Library, Vec<String>)>,
}

/// The licences of the libraries that are not given by hand, each text once.
/// Texts that differ only in white space are one; the MIT texts, which mostly
/// differ only in their copyright lines, are one as well, the lines listed
/// with the libraries they belong to.
fn groups<'a>(about: &'a About, apart: &BTreeSet<&str>) -> Vec<Group<'a>> {
    let mut groups: Vec<(String, Group<'a>)> = Vec::new();
    for license in &about.licenses {
        let (text, copyrights) = if license.id == "MIT" {
            split_copyright_lines(&license.text)
        } else {
            (license.text.trim_end().to_string(), Vec::new())
        };
        let key = format!(
            "{}\n{}",
            license.name,
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        );
        let at = match groups.iter().position(|(k, _)| *k == key) {
            Some(at) => at,
            None => {
                groups.push((
                    key,
                    Group {
                        name: &license.name,
                        text,
                        used: Vec::new(),
                    },
                ));
                groups.len() - 1
            }
        };
        for used in &license.used_by {
            if !apart.contains(used.library.name.as_str()) {
                groups[at].1.used.push((&used.library, copyrights.clone()));
            }
        }
    }
    let mut groups: Vec<Group<'a>> = groups
        .into_iter()
        .map(|(_, group)| group)
        .filter(|group| !group.used.is_empty())
        .collect();
    for group in &mut groups {
        group
            .used
            .sort_by(|a, b| (&a.0.name, &a.0.version).cmp(&(&b.0.name, &b.0.version)));
    }
    groups.sort_by(|a, b| (a.name, &a.used[0].0.name).cmp(&(b.name, &b.used[0].0.name)));
    groups
}

/// A text without its lines of copyright, and those lines.
fn split_copyright_lines(text: &str) -> (String, Vec<String>) {
    let mut body = Vec::new();
    let mut copyrights = Vec::new();
    for line in text.lines() {
        if is_copyright_line(line) {
            copyrights.push(line.trim().to_string());
        } else {
            body.push(line);
        }
    }
    let body = body.join("\n");
    (body.trim().to_string(), copyrights)
}

/// "Copyright (c) 2014 X", "Copyright 2014 X" or "(c) X" at the start of a
/// line, but not a line of the licence that happens to start with the word.
fn is_copyright_line(line: &str) -> bool {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix("Copyright") {
        return rest.starts_with(' ') && !rest.trim_start().starts_with("notice");
    }
    line.starts_with('\u{a9}') || line.starts_with("(c) ") || line.starts_with("(C) ")
}

/// The libraries of `by-hand.toml`, each with its own files.
fn libraries_by_hand(
    inputs: &Inputs,
    files: &dyn Files,
    notices_dir: &Path,
) -> Result<String, String> {
    let entries: Vec<&Entry> = inputs
        .by_hand
        .unreadable
        .iter()
        .chain(&inputs.by_hand.carried)
        .collect();
    if entries.is_empty() {
        return Ok(String::new());
    }
    let mut out = format!("## Libraries with notices of their own\n\n{BY_HAND_INTRO}\n");
    for entry in entries {
        out.push_str(&entry_section(entry, &inputs.metadata, files, notices_dir)?);
    }
    Ok(out)
}

fn entry_section(
    entry: &Entry,
    metadata: &Metadata,
    files: &dyn Files,
    notices_dir: &Path,
) -> Result<String, String> {
    if entry.files.is_empty() && entry.crate_files.is_empty() && entry.crate_dirs.is_empty() {
        return Err(format!(
            "notices/by-hand.toml: {} has no files, crate_files or crate_dirs",
            entry.title
        ));
    }
    let mut out = format!("### {}\n\n", entry.title);
    if !entry.note.is_empty() {
        writeln!(out, "{}\n", entry.note.trim()).expect("writing to a String");
    }
    out.push_str("Used by:\n\n");
    let mut found = Vec::new();
    for name in &entry.crates {
        let versions = metadata.find(name);
        if versions.is_empty() {
            return Err(format!(
                "notices/by-hand.toml has {name} under {}, but kiri does not use it: \
                 remove it from the file",
                entry.title
            ));
        }
        for version in versions {
            out.push_str(&source_line(name, &version.version));
            found.push((name, version));
        }
    }
    out.push('\n');

    let mut items: Vec<(String, String)> = Vec::new();
    for file in &entry.files {
        items.push((file.clone(), files.read(&notices_dir.join(file))?));
    }
    for (name, library) in &found {
        for file in &entry.crate_files {
            let text = files.read(&library.dir.join(file))?;
            items.push((format!("{name} {}: {file}", library.version), text));
        }
        for dir in &entry.crate_dirs {
            for path in files.list(&library.dir.join(dir))? {
                let relative = path.strip_prefix(&library.dir).unwrap_or(&path);
                let label = format!("{name} {}: {}", library.version, relative.display());
                items.push((label, files.read(&path)?));
            }
        }
    }
    out.push_str(&texts_once(&items)?);
    Ok(out)
}

/// The texts, each once, under the labels of all the files that hold it.
/// Files whose texts differ only in white space hold the same text.
fn texts_once(items: &[(String, String)]) -> Result<String, String> {
    let mut once: Vec<(String, Vec<&str>, &str)> = Vec::new();
    for (label, text) in items {
        let key = text.split_whitespace().collect::<Vec<_>>().join(" ");
        match once.iter_mut().find(|(k, _, _)| *k == key) {
            Some((_, labels, _)) => labels.push(label),
            None => once.push((key, vec![label.as_str()], text.as_str())),
        }
    }
    let mut out = String::new();
    for (_, labels, text) in once {
        let labels: Vec<String> = labels.iter().map(|label| format!("`{label}`")).collect();
        writeln!(out, "{}\n", labels.join(", ")).expect("writing to a String");
        out.push_str(&fenced(text)?);
    }
    Ok(out)
}

/// `extra` with each line `@include FILE` replaced by the text of the file,
/// in a fence.
fn expand_includes(extra: &str, files: &dyn Files, notices_dir: &Path) -> Result<String, String> {
    let mut out = String::new();
    for line in extra.lines() {
        if let Some(file) = line.strip_prefix("@include ") {
            let text = files.read(&notices_dir.join(file.trim()))?;
            out.push_str(&fenced(&text)?);
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
