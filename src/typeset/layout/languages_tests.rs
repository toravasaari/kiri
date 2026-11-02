//! Tests of the language table, and the test that generates it.

use std::collections::BTreeMap;
use std::fmt::Write;

use yaml_rust2::{Yaml, YamlLoader};

use super::{TABLE, grammar};

/// What kiri adds to the names GitHub knows: the fence name and the name of
/// the grammar it gets. Linguist's own entry wins where it has a grammar.
/// `pwsh` is missing for want of a PowerShell grammar in two-face's set.
const KIRI_ADDITIONS: [(&str, &str); 7] = [
    ("containerfile", "Dockerfile"),
    ("jsonl", "JSON"),
    ("dotenv", "DotENV"),
    ("txt", "Plain Text"),
    ("console", "Shell-Unix-Generic"),
    ("shellsession", "Shell-Unix-Generic"),
    ("shell-session", "Shell-Unix-Generic"),
];

#[test]
fn every_grammar_in_the_table_exists_and_the_table_is_sorted() {
    let syntaxes = two_face::syntax::extra_newlines();
    for (name, grammar) in TABLE {
        assert!(
            syntaxes.find_syntax_by_name(grammar).is_some(),
            "{name}: no grammar called {grammar}"
        );
    }
    for pair in TABLE.windows(2) {
        assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
    }
}

#[test]
fn finds_a_name_in_any_case() {
    assert_eq!(grammar("Shell"), grammar("shell"));
    assert!(grammar("mjs").is_some());
    assert!(grammar("no-such-language").is_none());
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

/// Writes `languages.rs` from GitHub Linguist's `languages.yml`, whose path
/// is in `KIRI_LINGUIST_YML`:
///
/// ```sh
/// KIRI_LINGUIST_YML=languages.yml cargo test --lib generate_the_language_table -- --ignored
/// ```
#[test]
#[ignore = "writes src/typeset/layout/languages.rs from KIRI_LINGUIST_YML"]
fn generate_the_language_table() {
    let path = std::env::var("KIRI_LINGUIST_YML").expect("set KIRI_LINGUIST_YML");
    let yaml = std::fs::read_to_string(path).unwrap();
    let documents = YamlLoader::load_from_str(&yaml).unwrap();
    let syntaxes = two_face::syntax::extra_newlines();

    struct Language {
        name: String,
        aliases: Vec<String>,
        extensions: Vec<String>,
        grammar: String,
    }
    let mut languages = Vec::new();
    for (name, language) in documents[0].as_hash().unwrap() {
        let name = name.as_str().unwrap().to_string();
        let scope = language["tm_scope"].as_str().unwrap_or("none");
        let extensions = strings(language, "extensions");
        let ours = |extension: &String| extension.trim_start_matches('.').to_string();
        let found = syntaxes
            .syntaxes()
            .iter()
            .find(|syntax| syntax.scope.to_string() == scope)
            .or_else(|| {
                let lower = name.to_lowercase();
                syntaxes
                    .syntaxes()
                    .iter()
                    .find(|syntax| syntax.name.to_lowercase() == lower)
            })
            .or_else(|| {
                let keys: Vec<String> = extensions
                    .iter()
                    .map(ours)
                    .chain(strings(language, "filenames"))
                    .collect();
                syntaxes.syntaxes().iter().find(|syntax| {
                    syntax
                        .file_extensions
                        .iter()
                        .any(|extension| keys.contains(extension))
                })
            });
        let Some(found) = found else { continue };
        languages.push(Language {
            name: name.clone(),
            aliases: strings(language, "aliases"),
            extensions: extensions.iter().map(ours).collect(),
            grammar: found.name.clone(),
        });
    }

    // Names first, then aliases, then extensions: an earlier kind keeps a
    // key that a later one also wants.
    let mut table: BTreeMap<String, String> = BTreeMap::new();
    let mut add = |key: String, grammar: &str| {
        if !key.is_empty() && !key.contains(char::is_whitespace) {
            table.entry(key).or_insert_with(|| grammar.to_string());
        }
    };
    for language in &languages {
        add(language.name.to_lowercase(), &language.grammar);
    }
    for language in &languages {
        for alias in &language.aliases {
            add(alias.to_lowercase(), &language.grammar);
        }
    }
    for (key, grammar) in KIRI_ADDITIONS {
        add(key.to_string(), grammar);
    }
    for language in &languages {
        for extension in &language.extensions {
            add(extension.to_lowercase(), &language.grammar);
        }
    }

    let mut out = String::new();
    out.push_str(
        "//! The names a fence may give a language, and the grammar each one gets.\n\
         //!\n\
         //! This file is generated. Do not edit it by hand. It comes from GitHub\n\
         //! Linguist's `languages.yml` (MIT licence, fetched on 2026-11-02), by\n\
         //! the ignored test `generate_the_language_table` in\n\
         //! `languages_tests.rs`, which also holds kiri's own additions: run it\n\
         //! again to change the table.\n\
         //!\n\
         //! A language's lowercased name and its aliases come first, then the\n\
         //! file extensions of every language; each name goes to a grammar of\n\
         //! two-face's syntax set. The table is sorted, so a name is found by\n\
         //! binary search.\n\n",
    );
    out.push_str(
        "/// The grammar for a fence's language, in the name two-face's set has\n\
         /// for it, or `None` if the table does not know the name.\n\
         pub(super) fn grammar(token: &str) -> Option<&'static str> {\n\
         \x20   let token = token.to_lowercase();\n\
         \x20   TABLE\n\
         \x20       .binary_search_by_key(&token.as_str(), |(name, _)| name)\n\
         \x20       .ok()\n\
         \x20       .map(|index| TABLE[index].1)\n\
         }\n\n",
    );
    writeln!(out, "static TABLE: [(&str, &str); {}] = [", table.len()).unwrap();
    for (key, grammar) in &table {
        writeln!(out, "    ({key:?}, {grammar:?}),").unwrap();
    }
    out.push_str("];\n\n#[cfg(test)]\n#[path = \"languages_tests.rs\"]\nmod tests;\n");
    std::fs::write(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/typeset/layout/languages.rs"
        ),
        out,
    )
    .unwrap();
}
