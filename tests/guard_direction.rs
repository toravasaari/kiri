//! The direction guard: the non-test code of each part uses only the parts
//! above it in the order of docs/architecture.md, "Modules": typesetting,
//! the terminal layer, the state engine and the platform.
//!
//! A file's part is its directory's: `src/typeset/`, `src/present/` and
//! `src/session/`, and the platform for the rest of `src/`. The test reads
//! the source as text, so it has limits. It sees `use` trees and paths that
//! start with `crate::`, `super::`, `self::` or `kiri::`. It does not see
//! names that reach a module by another route, such as a macro that expands
//! to a path, or a name a module re-exports under another path. Comments and
//! string literals are blanked first, and the item that a `#[cfg(test)]`
//! attribute marks is removed. The item ends at its first `;` or at the
//! `}` that closes its block, so a field or a statement with an `else`
//! branch is cut short; no such item is marked in `src/` today. Only the
//! exact attribute `#[cfg(test)]` is recognised: `cfg(all(test, ..))` and
//! `cfg(not(test))` are not, and none is used in `src/`. Files named
//! `tests.rs` or `testing.rs`, files under a `tests/` directory, and files
//! whose module is declared behind `#[cfg(test)]` are test code and are
//! not read.
//!
//! The crossings that exist are listed in `EXCEPTIONS`, with the work that
//! removes each. The list is complete: a crossing that is not listed fails
//! the test, and so does an entry whose crossing is gone, so the list can
//! only shrink. Adding an entry is the maintainer's decision.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The parts, lowest first: a part may use its own and the ones before it.
/// The platform is last and is not a directory of its own.
const PARTS: [&str; 4] = ["typeset", "present", "session", "platform"];

/// Every crossing that exists today, as (file, module it reaches, and the
/// work that removes it in the words of docs/architecture.md).
const EXCEPTIONS: &[(&str, &str, &str)] = &[
    (
        "src/present/cell_query.rs",
        "term::tmux",
        "the terminal layer split",
    ),
    (
        "src/present/clipboard.rs",
        "term::tmux",
        "the terminal layer split",
    ),
    ("src/present/encoder.rs", "images", "loading and sessions"),
    (
        "src/present/encoder.rs",
        "render::loading",
        "the terminal layer split",
    ),
    ("src/present/output.rs", "images", "loading and sessions"),
    (
        "src/present/output.rs",
        "render::loading",
        "the terminal layer split",
    ),
    ("src/session/loading.rs", "images", "loading and sessions"),
    (
        "src/typeset/caps.rs",
        "present::kitty::graphics",
        "giving caps.rs the constant, a small change of its own",
    ),
    (
        "src/typeset/diagrams/language.rs",
        "diagrams",
        "loading and sessions",
    ),
    (
        "src/typeset/diagrams/language.rs",
        "diagrams::external",
        "loading and sessions",
    ),
    (
        "src/typeset/diagrams/math.rs",
        "diagrams",
        "loading and sessions",
    ),
    (
        "src/typeset/diagrams/mermaid/merman.rs",
        "diagrams",
        "loading and sessions",
    ),
    (
        "src/typeset/diagrams/mermaid/mod.rs",
        "diagrams",
        "loading and sessions",
    ),
    (
        "src/typeset/diagrams/mermaid/mod.rs",
        "diagrams::external",
        "loading and sessions",
    ),
    (
        "src/typeset/images/decode.rs",
        "images",
        "loading and sessions",
    ),
    (
        "src/typeset/images/decode.rs",
        "images::svg",
        "the font database",
    ),
    (
        "src/typeset/images/decode.rs",
        "images::url",
        "loading and sessions",
    ),
    (
        "src/typeset/images/drawn.rs",
        "diagrams",
        "loading and sessions",
    ),
    (
        "src/typeset/images/drawn.rs",
        "images",
        "loading and sessions",
    ),
    (
        "src/typeset/images/drawn.rs",
        "images::svg",
        "the font database",
    ),
    (
        "src/typeset/images/heading.rs",
        "images",
        "loading and sessions",
    ),
    (
        "src/typeset/images/heading.rs",
        "images::svg",
        "the font database",
    ),
    (
        "src/typeset/images/size.rs",
        "images",
        "loading and sessions",
    ),
    ("src/typeset/input.rs", "diagrams", "loading and sessions"),
    ("src/typeset/input.rs", "images", "loading and sessions"),
    (
        "src/typeset/layout/front_matter.rs",
        "render",
        "the terminal layer split",
    ),
    (
        "src/typeset/layout/mod.rs",
        "diagrams",
        "loading and sessions",
    ),
    (
        "src/typeset/layout/mod.rs",
        "images",
        "loading and sessions",
    ),
    (
        "src/typeset/layout/mod.rs",
        "render",
        "the terminal layer split",
    ),
    (
        "src/typeset/reading/engine.rs",
        "diagrams",
        "loading and sessions",
    ),
    (
        "src/typeset/reading/engine.rs",
        "images",
        "loading and sessions",
    ),
    (
        "src/typeset/reading/engine.rs",
        "render",
        "the terminal layer split",
    ),
    (
        "src/typeset/reading/engine.rs",
        "render::loading",
        "the terminal layer split",
    ),
    (
        "src/typeset/reading/fonts.rs",
        "images",
        "the font database",
    ),
    (
        "src/typeset/reading/page/code.rs",
        "diagrams",
        "loading and sessions",
    ),
    ("src/typeset/style.rs", "images", "loading and sessions"),
];

#[test]
fn non_test_code_uses_only_the_parts_above_it() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let found = crossings(&src);
    let listed: BTreeSet<(String, String)> = EXCEPTIONS
        .iter()
        .map(|&(file, to, _)| (file.to_owned(), to.to_owned()))
        .collect();
    let unlisted: Vec<String> = found
        .difference(&listed)
        .map(|(file, to)| format!("  {file} uses {to}"))
        .collect();
    let gone: Vec<String> = listed
        .difference(&found)
        .map(|(file, to)| format!("  {file} no longer uses {to}"))
        .collect();
    let mut message = String::new();
    if !unlisted.is_empty() {
        message += &format!(
            "code uses a part below its own (docs/architecture.md, \"Modules\"):\n{}\n\
             Move the code, or, if the crossing has to stay, add it to EXCEPTIONS with \
             the work that removes it. That is the maintainer's decision.\n",
            unlisted.join("\n")
        );
    }
    if !gone.is_empty() {
        message += &format!(
            "listed in EXCEPTIONS but no longer in the code:\n{}\n\
             Remove those entries: the list can only shrink.\n",
            gone.join("\n")
        );
    }
    assert!(message.is_empty(), "{message}");
}

/// The crossings in the non-test code under `src`, as (file, module path).
fn crossings(src: &Path) -> BTreeSet<(String, String)> {
    let mut files = Vec::new();
    rust_files(src, &mut files);
    assert!(!files.is_empty(), "no source files under {}", src.display());
    let mut found = BTreeSet::new();
    for file in files {
        let relative = file.strip_prefix(src).unwrap();
        let name = module_path(relative).last().cloned().unwrap_or_default();
        let for_tests =
            parent_source(src, relative).is_some_and(|parent| declared_for_tests(&parent, &name));
        if is_test_file(relative) || for_tests {
            continue;
        }
        let name = format!("src/{}", relative.display());
        let code = non_test_code(&fs::read_to_string(&file).unwrap());
        let module = module_path(relative);
        let part = part_of(relative);
        for target in reaches(&code, &module, src) {
            if rank(&target) > part {
                found.insert((name.clone(), target.join("::")));
            }
        }
    }
    found
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
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
fn declared_for_tests(parent: &str, name: &str) -> bool {
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
fn parent_source(src: &Path, relative: &Path) -> Option<String> {
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

fn is_test_file(relative: &Path) -> bool {
    let in_tests_dir = relative
        .parent()
        .is_some_and(|dir| dir.components().any(|c| c.as_os_str() == "tests"));
    let name = relative.file_name().unwrap().to_string_lossy();
    in_tests_dir || name == "tests.rs" || name == "testing.rs"
}

/// The part a file of `src` belongs to, as its index in `PARTS`.
fn part_of(relative: &Path) -> usize {
    let mut parts = relative.components();
    match (parts.next(), parts.next()) {
        (Some(dir), Some(_)) => PARTS[..3]
            .iter()
            .position(|part| dir.as_os_str() == *part)
            .unwrap_or(3),
        _ => 3,
    }
}

/// The part a module path belongs to, as its index in `PARTS`.
fn rank(module: &[String]) -> usize {
    module
        .first()
        .and_then(|first| PARTS[..3].iter().position(|part| *part == first.as_str()))
        .unwrap_or(3)
}

/// The module a file of `src` defines: `src/a/b.rs` is `a::b`, and a
/// `mod.rs` is its directory. `lib.rs` and `main.rs` are the crate root.
fn module_path(relative: &Path) -> Vec<String> {
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
fn non_test_code(source: &str) -> String {
    let mut code = code_only(source);
    while let Some(at) = code.find("#[cfg(test)]") {
        let end = item_end(&code, at + "#[cfg(test)]".len());
        blank(&mut code, at, end);
    }
    code
}

/// `source` with comments and the contents of string and char literals
/// blanked. Lifetimes are kept.
fn code_only(source: &str) -> String {
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
fn raw_string_opens(chars: &[char], at: usize) -> bool {
    let hashes = chars[at..].iter().take_while(|&&c| c == '#').count();
    chars.get(at + hashes) == Some(&'"')
}

fn prev(chars: &[char], i: usize) -> char {
    if i == 0 { ' ' } else { chars[i - 1] }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// What a blanked char leaves: a space, or the line break itself.
fn blanked(c: char) -> char {
    if c == '\n' { '\n' } else { ' ' }
}

/// Where the item that a `#[cfg(test)]` attribute ends, searched from
/// `from`: after its `;` at depth 0, or its block's closing brace.
fn item_end(code: &str, from: usize) -> usize {
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
fn blank(code: &mut String, start: usize, end: usize) {
    *code = code
        .char_indices()
        .map(|(i, c)| match (start..end).contains(&i) {
            true if c == '\n' => "\n".to_owned(),
            true => " ".repeat(c.len_utf8()),
            false => c.to_string(),
        })
        .collect();
}

/// The bodies of the `use` items in `code`, and `code` without them.
fn use_items(code: &str) -> (Vec<String>, String) {
    let mut bodies = Vec::new();
    let mut rest = code.to_owned();
    let mut from = 0;
    while let Some(found) = code[from..].find("use") {
        let at = from + found;
        from = at + 3;
        let word_before = code[..at].chars().next_back().is_none_or(|c| !is_ident(c));
        let word_after = code[at + 3..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace);
        if !word_before || !word_after {
            continue;
        }
        let Some(semi) = code[at..].find(';') else {
            break;
        };
        let end = at + semi + 1;
        bodies.push(code[at + 3..end - 1].to_owned());
        blank(&mut rest, at, end);
    }
    (bodies, rest)
}

/// The paths of a `use` tree, one per leaf: `crate::{a::{b, c}, d as e}`
/// is `crate::a::b`, `crate::a::c` and `crate::d`.
fn expand(tree: &str) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    for item in split_top(tree) {
        let item = item.trim();
        match item.find('{') {
            Some(open) => {
                let close = item.rfind('}').unwrap_or(item.len());
                let prefix = segments(item[..open].trim().trim_end_matches("::"));
                for inner in expand(&item[open + 1..close]) {
                    let mut path = prefix.clone();
                    path.extend(inner);
                    paths.push(path);
                }
            }
            None if !item.is_empty() => {
                let path = item.split(" as ").next().unwrap_or(item);
                paths.push(segments(path.trim()));
            }
            None => {}
        }
    }
    paths
}

/// `text` split at the commas that are not inside braces.
fn split_top(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

fn segments(path: &str) -> Vec<String> {
    path.split("::")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The paths in `code` that start at a crate, `super`, `self` or `kiri`
/// and have a segment after it, in the order they are written.
fn chains(code: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = code.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_ident(chars[i]) || is_ident(prev(&chars, i)) {
            i += 1;
            continue;
        }
        let mut path = Vec::new();
        loop {
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            path.push(chars[start..i].iter().collect::<String>());
            let continues = i + 2 < chars.len()
                && chars[i] == ':'
                && chars[i + 1] == ':'
                && is_ident(chars[i + 2]);
            if !continues {
                break;
            }
            i += 2;
        }
        if path.len() > 1 && matches!(path[0].as_str(), "crate" | "super" | "self" | "kiri") {
            out.push(path);
        }
    }
    out
}

/// The modules that the non-test code `code` of the module `module`
/// refers to. Paths that leave the crate, such as `std::`, are not listed.
fn reaches(code: &str, module: &[String], src: &Path) -> Vec<Vec<String>> {
    let (bodies, rest) = use_items(code);
    let mut paths: Vec<Vec<String>> = bodies.iter().flat_map(|body| expand(body)).collect();
    paths.extend(chains(&rest));
    paths
        .iter()
        .filter_map(|path| absolute(path, module, src))
        .collect()
}

/// The module that `path`, as written in a module `module`, names. `None`
/// when the path does not start at this crate.
fn absolute(path: &[String], module: &[String], src: &Path) -> Option<Vec<String>> {
    let (first, rest) = path.split_first()?;
    let mut out = match first.as_str() {
        "crate" | "kiri" => Vec::new(),
        "self" => module.to_vec(),
        "super" => {
            let mut parent = module.to_vec();
            parent.pop()?;
            parent
        }
        _ => return None,
    };
    for segment in rest {
        match segment.as_str() {
            "self" | "*" => {}
            "super" => {
                out.pop()?;
            }
            name => out.push(name.to_owned()),
        }
    }
    Some(longest_module(&out, src))
}

/// The longest prefix of `path` that is a module of `src`: `typeset::Foo`
/// is `typeset`. The crate root when no prefix is one.
fn longest_module(path: &[String], src: &Path) -> Vec<String> {
    for len in (1..=path.len()).rev() {
        let base = src.join(path[..len].join("/"));
        if base.with_extension("rs").is_file() || base.join("mod.rs").is_file() {
            return path[..len].to_vec();
        }
    }
    Vec::new()
}

#[test]
fn the_reader_sees_use_trees_and_paths_and_skips_comments_strings_and_tests() {
    let source = "// use crate::pager::X;\n\
                  use crate::{\n    render::{self, Inputs},\n    present as p,\n};\n\
                  fn f() -> crate::typeset::Foo { let s = \"crate::pager\"; }\n\
                  #[cfg(test)]\nuse crate::session::Y;\n\
                  let c = '\"'; let r = r#\"crate::config\"#;\n";
    let code = non_test_code(source);
    assert!(
        !code.contains("pager") && !code.contains("session"),
        "{code}"
    );
    assert!(!code.contains("config") && !code.contains("X;"), "{code}");
    let (bodies, rest) = use_items(&code);
    let paths: Vec<Vec<String>> = bodies.iter().flat_map(|b| expand(b)).collect();
    let shown: Vec<String> = paths.iter().map(|p| p.join("::")).collect();
    assert_eq!(
        shown,
        [
            "crate::render::self",
            "crate::render::Inputs",
            "crate::present"
        ]
    );
    let inline: Vec<String> = chains(&rest).iter().map(|p| p.join("::")).collect();
    assert_eq!(inline, ["crate::typeset::Foo"]);
    // The line breaks are kept, so a line number in the code is the line
    // number in the source.
    assert_eq!(code.matches('\n').count(), source.matches('\n').count());
}

#[test]
fn a_part_is_the_directory_of_a_file_and_a_path_is_its_module() {
    assert_eq!(part_of(Path::new("typeset/layout/table.rs")), 0);
    assert_eq!(part_of(Path::new("present/kitty/mod.rs")), 1);
    assert_eq!(part_of(Path::new("session/bands.rs")), 2);
    assert_eq!(part_of(Path::new("render/mod.rs")), 3);
    assert_eq!(part_of(Path::new("main.rs")), 3);
    assert_eq!(
        module_path(Path::new("typeset/layout/mod.rs")),
        ["typeset", "layout"]
    );
    assert_eq!(module_path(Path::new("pager/keys.rs")), ["pager", "keys"]);
    assert!(module_path(Path::new("lib.rs")).is_empty());
}

#[test]
fn a_file_declared_behind_cfg_test_is_test_code() {
    let parent = "pub mod layout;\n#[cfg(test)]\npub(crate) mod summary;\nmod quote;\n";
    assert!(declared_for_tests(parent, "summary"));
    assert!(!declared_for_tests(parent, "quote"));
    assert!(!declared_for_tests(parent, "layout"));
}
