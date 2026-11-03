//! Tests of the language table, and the test that generates it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use syntect::parsing::SyntaxReference;
use yaml_rust2::{Yaml, YamlLoader};

use super::super::find_syntax;
use super::{EXTENSIONS, NAMES, extension, name};

/// Linguist languages that have a fitting grammar in two-face's set under
/// another scope or name than Linguist gives them: the language and the
/// grammar. A language here gets its name, aliases and extensions like any
/// linked one.
const LANGUAGE_LINKS: [(&str, &str); 27] = [
    // Linguist's Shell points at a hidden grammar.
    ("Shell", "Bourne Again Shell (bash)"),
    ("Batchfile", "Batch File"),
    ("Fortran", "Fortran (Fixed Form)"),
    ("Fortran Free Form", "Fortran (Modern)"),
    ("Common Lisp", "Lisp"),
    ("Emacs Lisp", "Lisp"),
    ("Scheme", "Lisp"),
    ("Assembly", "x86_64 Assembly"),
    ("CSV", "Comma Separated Values"),
    ("TSV", "Tab Separated Values"),
    ("HCL", "Terraform"),
    ("Jinja", "HTML (Jinja2)"),
    ("JSON with Comments", "JSON"),
    ("Gradle", "Groovy"),
    ("AsciiDoc", "AsciiDoc (Asciidoctor)"),
    ("Ignore List", "Git Ignore"),
    ("HTML+ERB", "HTML (Rails)"),
    ("HTML+PHP", "PHP"),
    ("SVG", "XML"),
    ("XSLT", "XML"),
    ("TSQL", "SQL"),
    ("Cython", "Python"),
    ("RobotFramework", "Robot Framework"),
    ("EditorConfig", "INI"),
    ("desktop", "INI"),
    ("Windows Registry Entries", "INI"),
    ("Org", "orgmode"),
];

/// What kiri adds to the names GitHub knows: the fence name and the name of
/// the grammar it gets. Linguist's own entry wins where it has a grammar.
/// `pwsh` is missing for want of a PowerShell grammar in two-face's set.
const KIRI_ADDITIONS: [(&str, &str); 11] = [
    ("containerfile", "Dockerfile"),
    ("jsonl", "JSON"),
    ("dotenv", "DotENV"),
    ("txt", "Plain Text"),
    ("console", "Bourne Again Shell (bash)"),
    ("shellsession", "Bourne Again Shell (bash)"),
    ("shell-session", "Bourne Again Shell (bash)"),
    ("shellscript", "Bourne Again Shell (bash)"),
    ("proto3", "Protocol Buffer"),
    // syntect alone gives Objective-C.
    ("h", "C"),
    ("fs", "F#"),
];

fn rows() -> impl Iterator<Item = (&'static str, &'static str)> {
    NAMES.iter().chain(EXTENSIONS.iter()).copied()
}

#[test]
fn every_row_points_to_a_visible_grammar_and_both_arrays_are_sorted() {
    let syntaxes = two_face::syntax::extra_newlines();
    for (key, grammar) in rows() {
        let found = syntaxes.find_syntax_by_name(grammar);
        let found = found.unwrap_or_else(|| panic!("{key}: no grammar called {grammar}"));
        assert!(!found.hidden, "{key}: {grammar} is a hidden grammar");
    }
    for array in [&NAMES[..], &EXTENSIONS[..]] {
        for pair in array.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
    }
}

#[test]
fn leaves_out_a_language_that_shares_only_a_file_extension_with_a_grammar() {
    for key in ["prolog", "cobol", "smalltalk", "vba", "mojo"] {
        assert!(name(key).is_none() && extension(key).is_none(), "{key}");
    }
}

#[test]
fn leaves_out_the_extensions_that_two_face_resolves_itself() {
    let syntaxes = two_face::syntax::extra_newlines();
    for (key, _) in EXTENSIONS {
        assert!(syntaxes.find_syntax_by_token(key).is_none(), "{key}");
    }
}

#[test]
fn finds_a_name_in_any_case() {
    for token in ["Shell", "SHELL", "MJS", "Mjs"] {
        assert_eq!(
            find_syntax(Some(token)).name,
            find_syntax(Some(&token.to_lowercase())).name
        );
    }
    assert_ne!(find_syntax(Some("mjs")).name, "Plain Text");
    assert!(name("no-such-language").is_none());
}

fn strings(language: &Yaml, key: &str) -> Vec<String> {
    let value = &language[key];
    value
        .as_vec()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    Yaml::String(text) => Some(text.clone()),
                    Yaml::Integer(number) => Some(number.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The head of `languages.rs`: its documentation and its lookup functions.
fn header(linguist_ref: &str) -> String {
    let head = format!(
        "//! The names a fence may give a language, and the grammar each one gets.\n\
         //!\n\
         //! This file is generated. Do not edit it by hand. It comes from GitHub\n\
         //! Linguist's `languages.yml` (MIT licence, fetched on {linguist_ref}), by\n\
         //! the ignored test `generate_the_language_table` in\n\
         //! `languages/tests.rs`, which also holds kiri's own additions: run it\n\
         //! again to change the table.\n\
         //!\n\
         //! A Linguist language gets a grammar of two-face's syntax set only when\n\
         //! the grammar's scope or name is the language's own, or when kiri says\n\
         //! which grammar fits it; a shared file extension is not enough, and a\n\
         //! hidden grammar is never chosen. There are two arrays, sorted so that\n\
         //! binary search finds a key. `NAMES` has each language's name, its\n\
         //! aliases and kiri's additions, and an earlier one keeps a name that a\n\
         //! later one also wants. `EXTENSIONS` has the file extensions that\n\
         //! two-face's own lookup does not resolve, and is searched after it.\n\n"
    );
    head + "/// The grammar for a fence's language, in the name two-face's set has\n\
         /// for it, or `None` if the table does not know the name. `token` is in\n\
         /// lowercase.\n\
         pub(super) fn name(token: &str) -> Option<&'static str> {\n\
         \x20   find(&NAMES, token)\n\
         }\n\n\
         /// The grammar for a file extension, as `name` gives it. `token` is in\n\
         /// lowercase.\n\
         pub(super) fn extension(token: &str) -> Option<&'static str> {\n\
         \x20   find(&EXTENSIONS, token)\n\
         }\n\n\
         fn find(array: &[(&str, &'static str)], token: &str) -> Option<&'static str> {\n\
         \x20   array\n\
         \x20       .binary_search_by_key(&token, |(key, _)| key)\n\
         \x20       .ok()\n\
         \x20       .map(|index| array[index].1)\n\
         }\n\n"
}

/// The text of the generated file.
fn render(
    linguist_ref: &str,
    names: &BTreeMap<String, String>,
    extensions: &BTreeMap<String, String>,
) -> String {
    let mut out = String::new();
    out.push_str(&header(linguist_ref));
    for (array, table) in [("NAMES", names), ("EXTENSIONS", extensions)] {
        writeln!(out, "static {array}: [(&str, &str); {}] = [", table.len()).unwrap();
        for (key, grammar) in table {
            writeln!(out, "    ({key:?}, {grammar:?}),").unwrap();
        }
        out.push_str("];\n\n");
    }
    out.push_str("#[cfg(test)]\nmod tests;\n");
    out
}

/// Every name, hyphenated name and alias of a Linguist language, with or
/// without a grammar, in lowercase. Such a key is not given to another
/// language's extension.
fn linguist_keys(languages: &Yaml) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for (name, language) in languages.as_hash().unwrap() {
        let name = name.as_str().unwrap().to_lowercase();
        keys.insert(name.replace(char::is_whitespace, "-"));
        keys.insert(name);
        keys.extend(
            strings(language, "aliases")
                .iter()
                .map(|a| a.to_lowercase()),
        );
    }
    keys
}

/// Writes `highlight/languages.rs` from GitHub Linguist's `languages.yml`,
/// whose path is in `KIRI_LINGUIST_YML`, and the date or commit it was
/// fetched at, which goes into the file's header, in `KIRI_LINGUIST_REF`:
///
/// ```sh
/// KIRI_LINGUIST_REF=2026-11-02 KIRI_LINGUIST_YML=languages.yml cargo test --lib generate_the_language_table -- --ignored
/// ```
#[test]
#[ignore = "writes src/typeset/layout/highlight/languages.rs from KIRI_LINGUIST_YML"]
fn generate_the_language_table() {
    let path = std::env::var("KIRI_LINGUIST_YML").expect("set KIRI_LINGUIST_YML");
    let linguist_ref = std::env::var("KIRI_LINGUIST_REF").expect("set KIRI_LINGUIST_REF");
    let yaml = std::fs::read_to_string(path).unwrap();
    let documents = YamlLoader::load_from_str(&yaml).unwrap();
    let syntaxes = two_face::syntax::extra_newlines();
    let visible = |syntax: &&SyntaxReference| !syntax.hidden;
    let by_name = |name: &str| {
        let lower = name.to_lowercase();
        (syntaxes.syntaxes().iter().filter(visible))
            .find(|syntax| syntax.name.to_lowercase() == lower)
    };

    struct Language {
        name: String,
        aliases: Vec<String>,
        extensions: Vec<String>,
        grammar: String,
    }
    let mut languages = Vec::new();
    let mut linked = Vec::new();
    for (name, language) in documents[0].as_hash().unwrap() {
        let name = name.as_str().unwrap().to_string();
        let scope = language["tm_scope"].as_str().unwrap_or("none");
        let found = LANGUAGE_LINKS
            .iter()
            .find(|(linguist, _)| *linguist == name)
            .map(|(_, grammar)| {
                linked.push(name.clone());
                by_name(grammar).unwrap_or_else(|| panic!("{name}: no grammar {grammar}"))
            })
            .or_else(|| {
                (syntaxes.syntaxes().iter().filter(visible))
                    .find(|syntax| syntax.scope.to_string() == scope)
            })
            .or_else(|| by_name(&name));
        let Some(found) = found else { continue };
        languages.push(Language {
            name: name.clone(),
            aliases: strings(language, "aliases"),
            extensions: strings(language, "extensions")
                .iter()
                .map(|extension| extension.trim_start_matches('.').to_string())
                .collect(),
            grammar: found.name.clone(),
        });
    }
    for (name, _) in LANGUAGE_LINKS {
        assert!(linked.iter().any(|l| l == name), "{name}: not in Linguist");
    }
    for (name, grammar) in KIRI_ADDITIONS {
        let found = by_name(grammar);
        assert!(found.is_some(), "{name}: no visible grammar {grammar}");
    }

    // Names, aliases, then kiri's additions: an earlier kind keeps a key
    // that a later one also wants.
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let add = |table: &mut BTreeMap<String, String>, key: String, grammar: &str| {
        if !key.is_empty() && !key.contains(char::is_whitespace) {
            table.entry(key).or_insert_with(|| grammar.to_string());
        }
    };
    for language in &languages {
        add(&mut names, language.name.to_lowercase(), &language.grammar);
    }
    for language in &languages {
        for alias in &language.aliases {
            add(&mut names, alias.to_lowercase(), &language.grammar);
        }
        // Linguist's default alias: the name in lowercase, spaces as hyphens.
        let hyphenated: String = (language.name.to_lowercase().chars())
            .map(|c| if c.is_whitespace() { '-' } else { c })
            .collect();
        add(&mut names, hyphenated, &language.grammar);
    }
    for (key, grammar) in KIRI_ADDITIONS {
        add(&mut names, key.to_string(), grammar);
    }
    // File extensions that neither the names nor two-face's own lookup
    // resolve; the first language to list one keeps it.
    let linguist_keys = linguist_keys(&documents[0]);
    let mut extensions: BTreeMap<String, String> = BTreeMap::new();
    for language in &languages {
        for extension in &language.extensions {
            let key = extension.to_lowercase();
            if !names.contains_key(&key)
                && !linguist_keys.contains(&key)
                && syntaxes.find_syntax_by_token(&key).is_none()
            {
                add(&mut extensions, key, &language.grammar);
            }
        }
    }

    let out = render(&linguist_ref, &names, &extensions);
    std::fs::write(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/typeset/layout/highlight/languages.rs"
        ),
        out,
    )
    .unwrap();
}
