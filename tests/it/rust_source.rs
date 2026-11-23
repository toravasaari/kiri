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

/// Whether `code_only` keeps the contents of string and char literals.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Strings {
    /// Blanked, so that a name in a string is not taken for code.
    Blank,
    /// Kept, for a guard that looks for what strings hold, such as escape
    /// sequences.
    Keep,
}

/// Blanks comments and removes what `#[cfg(test)]` marks; `strings` tells
/// whether string and char literals are blanked too. The result has the
/// source's byte offsets and line breaks, so line numbers stay.
pub(crate) fn non_test_code(source: &str, strings: Strings) -> String {
    // The items are found in the text with blanked strings: with strings
    // kept, a `[` in one would leave `item_end` a bracket short.
    let mut code = code_only(source, Strings::Blank);
    let mut items = Vec::new();
    while let Some(at) = code.find("#[cfg(test)]") {
        let end = item_end(&code, at + "#[cfg(test)]".len());
        blank(&mut code, at, end);
        items.push((at, end));
    }
    if strings == Strings::Blank {
        return code;
    }
    let mut kept = code_only(source, Strings::Keep);
    for (start, end) in items {
        blank(&mut kept, start, end);
    }
    kept
}

/// `source` with comments blanked, and the contents of string and char
/// literals too when `strings` is `Blank`. Lifetimes are kept. A blanked
/// char leaves as many spaces as it has bytes, so the result has the
/// source's byte offsets.
pub(crate) fn code_only(source: &str, strings: Strings) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let literal = |out: &mut String, c: char| match strings {
        Strings::Keep => out.push(c),
        Strings::Blank => push_blanked(out, c),
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                push_blanked(&mut out, chars[i]);
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                push_blanked(&mut out, chars[i]);
                i += 1;
            }
            // The closing `*/`, if there is one.
            for _ in 0..2 {
                if i < chars.len() {
                    out.push(' ');
                    i += 1;
                }
            }
        } else if c == 'r' && raw_prefix(&chars, i) && raw_string_opens(&chars, i + 1) {
            let hashes = chars[i + 1..].iter().take_while(|&&c| c == '#').count();
            let closing: Vec<char> = std::iter::once('"')
                .chain(std::iter::repeat_n('#', hashes))
                .collect();
            let body = i + 2 + hashes;
            let end = (body..chars.len())
                .find(|&j| chars[j..].starts_with(&closing))
                .map_or(chars.len(), |j| j + closing.len());
            for &c in &chars[i..end] {
                literal(&mut out, c);
            }
            i = end;
        } else if c == '"' {
            out.push('"');
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                let width = if chars[i] == '\\' { 2 } else { 1 };
                for _ in 0..width {
                    if i < chars.len() {
                        literal(&mut out, chars[i]);
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
                        literal(&mut out, c);
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
                literal(&mut out, c);
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

/// Whether the `r` at `i` starts a raw string: it does not end a name, or
/// it follows the `b` or `c` of a raw byte or C string (`br"…"`).
fn raw_prefix(chars: &[char], i: usize) -> bool {
    let before = prev(chars, i);
    !is_ident(before) || (matches!(before, 'b' | 'c') && !is_ident(prev(chars, i - 1)))
}

/// What a blanked char leaves: the line break itself, or as many spaces as
/// the char has bytes.
fn push_blanked(out: &mut String, c: char) {
    if c == '\n' {
        out.push('\n');
    } else {
        out.extend(std::iter::repeat_n(' ', c.len_utf8()));
    }
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

#[test]
fn comments_are_blanked_and_literals_kept_only_when_asked() {
    let source = "// \x1b_G\nlet s = \"\\x1b_G\"; let c = '\\u{10EEEE}';\n";
    let kept = non_test_code(source, Strings::Keep);
    assert!(!kept.contains("// "), "{kept}");
    assert!(
        kept.contains("\"\\x1b_G\"") && kept.contains("'\\u{10EEEE}'"),
        "{kept}"
    );
    let blanked = non_test_code(source, Strings::Blank);
    assert!(
        !blanked.contains("x1b") && !blanked.contains("10EEEE"),
        "{blanked}"
    );
}

#[test]
fn a_bracket_in_a_kept_string_does_not_stretch_a_test_item() {
    let source =
        "#[cfg(test)]\nmod tests {\n    const A: &str = \"\\x1b[1;1H\";\n}\nfn real() {}\n";
    let kept = non_test_code(source, Strings::Keep);
    assert!(kept.contains("fn real() {}"), "{kept}");
    assert!(!kept.contains("x1b"), "{kept}");
}

#[test]
fn the_result_has_the_sources_bytes_and_lines() {
    let source = "let s = \"ä€\"; // é\n#[cfg(test)]\nmod t { fn x() {} }\nfn real() { \"ü\" }\n";
    for strings in [Strings::Blank, Strings::Keep] {
        let code = non_test_code(source, strings);
        assert_eq!(code.len(), source.len(), "{strings:?}");
        assert_eq!(code.lines().count(), source.lines().count(), "{strings:?}");
        assert!(
            code.contains("fn real()") && !code.contains("fn x"),
            "{code}"
        );
    }
    assert!(non_test_code(source, Strings::Keep).contains("\"ä€\""));
}

#[test]
fn a_raw_byte_string_is_one_literal() {
    let source = "let s = br#\"a=\"http://x\"\"#;\nlet t = cr\"c\";\nfn real() {}\n";
    let code = non_test_code(source, Strings::Blank);
    assert!(!code.contains("http") && !code.contains("r\""), "{code}");
    assert!(code.contains("fn real() {}"), "{code}");
}
