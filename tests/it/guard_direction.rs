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
use std::path::Path;

use crate::rust_source::{
    blank, declared_for_tests, is_ident, is_test_file, module_path, non_test_code, parent_source,
    prev, rust_files,
};

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
    (
        "src/present/encoder.rs",
        "render::loading",
        "the terminal layer split",
    ),
    (
        "src/present/output.rs",
        "render::loading",
        "the terminal layer split",
    ),
    (
        "src/typeset/caps.rs",
        "present::kitty::graphics",
        "giving caps.rs the constant, a small change of its own",
    ),
    (
        "src/typeset/images/decode.rs",
        "images::svg",
        "the font database",
    ),
    (
        "src/typeset/images/drawn.rs",
        "images::svg",
        "the font database",
    ),
    (
        "src/typeset/images/heading.rs",
        "images::svg",
        "the font database",
    ),
    (
        "src/typeset/layout/front_matter.rs",
        "render",
        "the terminal layer split",
    ),
    (
        "src/typeset/layout/mod.rs",
        "render",
        "the terminal layer split",
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
