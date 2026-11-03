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

/// The file extensions that several Linguist languages list, which would not
/// all get the same grammar, with the language that gets each one: the most
/// used of them. A language here that has no grammar in two-face's set
/// leaves the extension out of the table. The generator fails for an
/// undecided extension, so that a newer `languages.yml` cannot add one
/// unnoticed.
const SHARED_EXTENSIONS: [(&str, &str); 37] = [
    ("asc", "AsciiDoc"),
    ("asy", "Asymptote"),
    ("bb", "Clojure"),
    ("bf", "Brainfuck"),
    ("brd", "Eagle"),
    ("bst", "BibTeX Style"),
    ("csl", "XML"),
    ("es", "JavaScript"),
    ("fr", "Forth"),
    ("frm", "Visual Basic 6.0"),
    ("gml", "Game Maker Language"),
    ("gst", "Gosu"),
    ("i", "SWIG"),
    ("ice", "Slice"),
    ("mod", "Modula-2"),
    ("nas", "Assembly"),
    ("pluginspec", "Ruby"),
    ("prc", "PLSQL"),
    ("pro", "Prolog"),
    ("qs", "Q#"),
    ("res", "ReScript"),
    ("s", "Unix Assembly"),
    ("scd", "SuperCollider"),
    ("sch", "Scheme"),
    ("shader", "ShaderLab"),
    ("sls", "Salt"),
    ("spec", "RPM Spec"),
    ("srt", "SubRip Text"),
    ("sw", "Sway"),
    ("toc", "World of Warcraft Addon Data"),
    ("trigger", "Apex"),
    ("uc", "UnrealScript"),
    ("vcf", "vCard"),
    ("vhost", "ApacheConf"),
    ("workflow", "HCL"),
    ("x", "Linker Script"),
    ("yy", "Yacc"),
];

/// The grammar a file extension gets when several Linguist languages list
/// it. `languages` has each of them with its grammar, if it has one. `None`
/// means the extension stays out of the table.
fn pick_grammar<'a>(
    extension: &str,
    languages: &[(&'a str, Option<&'a str>)],
    choices: &[(&str, &str)],
) -> Result<Option<&'a str>, String> {
    let choice = choices.iter().find(|(key, _)| *key == extension);
    let outcomes: BTreeSet<_> = languages.iter().map(|(_, grammar)| grammar).collect();
    let Some((_, chosen)) = choice else {
        if outcomes.len() <= 1 {
            return Ok(languages.first().and_then(|(_, grammar)| *grammar));
        }
        let names: Vec<_> = languages.iter().map(|(name, _)| *name).collect();
        return Err(format!(
            "{extension}: undecided between {}",
            names.join(", ")
        ));
    };
    match languages.iter().find(|(name, _)| name == chosen) {
        Some((_, grammar)) => Ok(*grammar),
        None => Err(format!("{extension}: the choice {chosen} does not list it")),
    }
}

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

#[test]
fn gives_an_extension_that_several_languages_share_to_the_chosen_one() {
    let languages = [
        ("Erlang", Some("Erlang")),
        ("JavaScript", Some("JavaScript")),
    ];
    let chosen = pick_grammar("es", &languages, &[("es", "JavaScript")]);
    assert_eq!(chosen, Ok(Some("JavaScript")));
    let chosen = pick_grammar("es", &languages, &[("es", "Erlang")]);
    assert_eq!(chosen, Ok(Some("Erlang")));
}

#[test]
fn leaves_the_extension_out_when_the_chosen_language_has_no_grammar() {
    let languages = [("Beef", Some("C#")), ("Brainfuck", None)];
    let chosen = pick_grammar("bf", &languages, &[("bf", "Brainfuck")]);
    assert_eq!(chosen, Ok(None));
}

#[test]
fn fails_for_an_undecided_extension_or_a_choice_that_does_not_list_it() {
    let languages = [("Beef", Some("C#")), ("Brainfuck", None)];
    let undecided = pick_grammar("bf", &languages, &[]).unwrap_err();
    assert!(
        undecided.contains("bf") && undecided.contains("Beef"),
        "{undecided}"
    );
    let wrong = pick_grammar("bf", &languages, &[("bf", "Befunge")]).unwrap_err();
    assert!(wrong.contains("Befunge"), "{wrong}");
}

#[test]
fn needs_no_choice_when_the_languages_get_the_same_grammar() {
    let same = [("SVG", Some("XML")), ("XSLT", Some("XML"))];
    assert_eq!(pick_grammar("x", &same, &[]), Ok(Some("XML")));
    let none = [("A", None), ("B", None)];
    assert_eq!(pick_grammar("x", &none, &[]), Ok(None));
}

#[test]
fn gives_a_shared_extension_the_grammar_of_the_most_used_language() {
    assert_eq!(extension("es"), Some("JavaScript"));
    assert_eq!(extension("vhost"), name("apacheconf"));
    assert!(extension("vhost").is_some());
    assert_eq!(find_syntax(Some("erlang")).name, "Erlang");
    // RPM Spec, Prolog, Brainfuck, SubRip Text and vCard have no grammar.
    for key in ["spec", "pro", "bf", "srt", "vcf"] {
        assert!(extension(key).is_none(), "{key}");
    }
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
         //! which grammar fits it; a shared file extension is not enough, and the\n\
         //! table never points to a hidden grammar. A file extension that several\n\
         //! languages list goes to the most used of them, as the generator's list\n\
         //! decides, or stays out if that language has no grammar. There are two\n\
         //! arrays, sorted so that binary search finds a key. `NAMES` has each\n\
         //! language's name, its aliases and kiri's additions, and an earlier one\n\
         //! keeps a name that a later one also wants. `EXTENSIONS` has the file\n\
         //! extensions that two-face's own lookup does not resolve, and is\n\
         //! searched after it.\n\n"
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

/// A Linguist language and the grammar it gets, if any.
struct Language {
    name: String,
    aliases: Vec<String>,
    extensions: Vec<String>,
    grammar: Option<String>,
}

/// The file extensions of the table with their grammars, from the languages
/// that list each one. Fails for an undecided extension and for a choice
/// that does not list its extension.
fn choose_extensions(
    listed: &BTreeMap<String, Vec<(&str, Option<&str>)>>,
) -> BTreeMap<String, String> {
    let mut extensions = BTreeMap::new();
    let mut problems = Vec::new();
    for (key, choice) in SHARED_EXTENSIONS {
        match listed.get(key) {
            Some(languages) if languages.iter().any(|(name, _)| *name == choice) => {}
            _ => problems.push(format!("{key}: the choice {choice} does not list it")),
        }
    }
    for (key, listing) in listed {
        match pick_grammar(key, listing, &SHARED_EXTENSIONS) {
            Ok(Some(grammar)) if !key.is_empty() && !key.contains(char::is_whitespace) => {
                extensions.insert(key.clone(), grammar.to_string());
            }
            Ok(_) => {}
            Err(problem) => problems.push(problem),
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    extensions
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
        languages.push(Language {
            name: name.clone(),
            aliases: strings(language, "aliases"),
            extensions: strings(language, "extensions")
                .iter()
                .map(|extension| extension.trim_start_matches('.').to_string())
                .collect(),
            grammar: found.map(|found| found.name.clone()),
        });
    }
    for (name, _) in LANGUAGE_LINKS {
        assert!(linked.iter().any(|l| l == name), "{name}: not in Linguist");
    }
    for (name, grammar) in KIRI_ADDITIONS {
        assert!(
            by_name(grammar).is_some(),
            "{name}: no visible grammar {grammar}"
        );
    }

    // Names, aliases, then kiri's additions: an earlier kind keeps a key
    // that a later one also wants.
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let add = |table: &mut BTreeMap<String, String>, key: String, grammar: &str| {
        if !key.is_empty() && !key.contains(char::is_whitespace) {
            table.entry(key).or_insert_with(|| grammar.to_string());
        }
    };
    let languages_with_grammar =
        || (languages.iter()).filter_map(|language| Some((language, language.grammar.as_ref()?)));
    for (language, grammar) in languages_with_grammar() {
        add(&mut names, language.name.to_lowercase(), grammar);
    }
    for (language, grammar) in languages_with_grammar() {
        for alias in &language.aliases {
            add(&mut names, alias.to_lowercase(), grammar);
        }
        // Linguist's default alias: the name in lowercase, spaces as hyphens.
        let hyphenated: String = (language.name.to_lowercase().chars())
            .map(|c| if c.is_whitespace() { '-' } else { c })
            .collect();
        add(&mut names, hyphenated, grammar);
    }
    for (key, grammar) in KIRI_ADDITIONS {
        add(&mut names, key.to_string(), grammar);
    }
    // File extensions that neither the names nor two-face's own lookup
    // resolve. One that several languages list goes to the language of
    // `SHARED_EXTENSIONS`, and the generator fails for an undecided one.
    let linguist_keys = linguist_keys(&documents[0]);
    let mut listed: BTreeMap<String, Vec<(&str, Option<&str>)>> = BTreeMap::new();
    for language in &languages {
        for extension in &language.extensions {
            let key = extension.to_lowercase();
            if !names.contains_key(&key)
                && !linguist_keys.contains(&key)
                && syntaxes.find_syntax_by_token(&key).is_none()
            {
                let entry = (language.name.as_str(), language.grammar.as_deref());
                let listing = listed.entry(key).or_default();
                if !listing.contains(&entry) {
                    listing.push(entry);
                }
            }
        }
    }
    let extensions = choose_extensions(&listed);

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
