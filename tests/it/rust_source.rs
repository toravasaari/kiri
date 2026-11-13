//! Reading the Rust source of `src/` as text, for the guards.
//!
//! Both guards read the code of a file without what is not code: comments
//! and the contents of string and char literals are blanked, and so is what
//! `#[cfg(test)]` marks. A file that is test code, by its name or because its
//! module is declared behind `#[cfg(test)]`, is not read. The line breaks are
//! kept, so a line number in the result is the line number in the source.

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Whether `parent` declares the module `name` behind `#[cfg(test)]`, as
/// `#[cfg(test)] mod name;` on two lines: then its whole file is test code.
pub(crate) fn declared_for_tests(parent: &str, name: &str) -> bool {
    let lines: Vec<&str> = parent.lines().map(str::trim).collect();
    lines.windows(2).any(|pair| {
        pair[0] == "#[cfg(test)]"
            && pair[1]
                .strip_suffix(";")
                .and_then(|line| line.rsplit_once("mod "))
                .is_some_and(|(_, module)| module == name)
    })
}

/// The source of the file that declares the module in `relative`: `x.rs` or
/// `x/mod.rs` for `x/name.rs`, and `lib.rs` for a file at the top.
pub(crate) fn parent_source(src: &Path, relative: &Path) -> Option<String> {
    let mut dir = relative.parent()?.to_path_buf();
    if relative.file_name()? == "mod.rs" {
        dir = dir.parent()?.to_path_buf();
    }
    let candidates = if dir.as_os_str().is_empty() {
        vec![src.join("lib.rs"), src.join("main.rs")]
    } else {
        vec![
            src.join(dir.with_extension("rs")),
            src.join(&dir).join("mod.rs"),
        ]
    };
    candidates
        .into_iter()
        .find_map(|path| fs::read_to_string(path).ok())
}

pub(crate) fn is_test_file(relative: &Path) -> bool {
    let in_tests_dir = relative
        .parent()
        .is_some_and(|dir| dir.components().any(|c| c.as_os_str() == "tests"));
    let name = relative.file_name().unwrap().to_string_lossy();
    in_tests_dir || name == "tests.rs" || name == "testing.rs"
}

/// The module a file of `src` defines: `src/a/b.rs` is `a::b`, and a
/// `mod.rs` is its directory. `lib.rs` and `main.rs` are the crate root.
pub(crate) fn module_path(relative: &Path) -> Vec<String> {
    if relative == Path::new("lib.rs") || relative == Path::new("main.rs") {
        return Vec::new();
    }
    let mut module: Vec<String> = relative
        .with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if module.last().is_some_and(|last| last == "mod") {
        module.pop();
    }
    module
}

/// Blanks comments and string literals, and removes what `#[cfg(test)]`
/// marks. The result keeps the line breaks, so line numbers stay.
pub(crate) fn non_test_code(source: &str) -> String {
    let mut code = code_only(source);
    while let Some(at) = code.find("#[cfg(test)]") {
        let end = item_end(&code, at + "#[cfg(test)]".len());
        blank(&mut code, at, end);
    }
    code
}

/// `source` with comments and the contents of string and char literals
/// blanked. Lifetimes are kept.
pub(crate) fn code_only(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                out.push(blanked(chars[i]));
                i += 1;
            }
            // The closing `*/`, if there is one.
            for _ in 0..2 {
                if i < chars.len() {
                    out.push(' ');
                    i += 1;
                }
            }
        } else if c == 'r' && !is_ident(prev(&chars, i)) && raw_string_opens(&chars, i + 1) {
            let hashes = chars[i + 1..].iter().take_while(|&&c| c == '#').count();
            let closing: Vec<char> = std::iter::once('"')
                .chain(std::iter::repeat_n('#', hashes))
                .collect();
            let body = i + 2 + hashes;
            let end = (body..chars.len())
                .find(|&j| chars[j..].starts_with(&closing))
                .map_or(chars.len(), |j| j + closing.len());
            for &c in &chars[i..end] {
                out.push(blanked(c));
            }
            i = end;
        } else if c == '"' {
            out.push('"');
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                let width = if chars[i] == '\\' { 2 } else { 1 };
                for _ in 0..width {
                    if i < chars.len() {
                        out.push(blanked(chars[i]));
                        i += 1;
                    }
                }
            }
            out.push('"');
            i += 1;
        } else if c == '\'' && next == Some('\\') {
            // An escape: `'\n'`, `'\''`, `'\u{1F600}'`.
            let close = (i + 3..chars.len().min(i + 12)).find(|&j| chars[j] == '\'');
            match close {
                Some(j) => {
                    for &c in &chars[i..=j] {
                        out.push(blanked(c));
                    }
                    i = j + 1;
                }
                None => {
                    out.push(c);
                    i += 1;
                }
            }
        } else if c == '\'' && chars.get(i + 2) == Some(&'\'') {
            for &c in &chars[i..i + 3] {
                out.push(blanked(c));
            }
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Whether the `#`s and the quote of a raw string start at `at`.
pub(crate) fn raw_string_opens(chars: &[char], at: usize) -> bool {
    let hashes = chars[at..].iter().take_while(|&&c| c == '#').count();
    chars.get(at + hashes) == Some(&'"')
}

pub(crate) fn prev(chars: &[char], i: usize) -> char {
    if i == 0 { ' ' } else { chars[i - 1] }
}

pub(crate) fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// What a blanked char leaves: a space, or the line break itself.
pub(crate) fn blanked(c: char) -> char {
    if c == '\n' { '\n' } else { ' ' }
}

/// Where the item that a `#[cfg(test)]` attribute ends, searched from
/// `from`: after its `;` at depth 0, or its block's closing brace.
pub(crate) fn item_end(code: &str, from: usize) -> usize {
    let mut depth = 0i32;
    for (i, c) in code[from..].char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' => depth -= 1,
            '}' => {
                depth -= 1;
                if depth <= 0 {
                    return from + i + 1;
                }
            }
            ';' if depth == 0 => return from + i + 1,
            _ => {}
        }
    }
    code.len()
}

/// Replaces the bytes `start..end` of `code`, which are char boundaries,
/// with spaces, keeping the line breaks. Every byte stays where it was, so
/// the offsets of the rest of `code` do not move.
pub(crate) fn blank(code: &mut String, start: usize, end: usize) {
    *code = code
        .char_indices()
        .map(|(i, c)| match (start..end).contains(&i) {
            true if c == '\n' => "\n".to_owned(),
            true => " ".repeat(c.len_utf8()),
            false => c.to_string(),
        })
        .collect();
}

#[test]
fn a_file_declared_behind_cfg_test_is_test_code() {
    let parent = "pub mod layout;\n#[cfg(test)]\npub(crate) mod summary;\nmod quote;\n";
    assert!(declared_for_tests(parent, "summary"));
    assert!(!declared_for_tests(parent, "quote"));
    assert!(!declared_for_tests(parent, "layout"));
}
