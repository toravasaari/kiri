//! Front matter: a block of metadata at the very start of a file, between
//! two delimiter lines, as Jekyll, Hugo and GitLab read it.

use crate::model::{FrontMatter, Value};

/// The delimiters and the language each one stands for.
const DELIMITERS: [(&str, &str); 3] = [("---", "yaml"), ("+++", "toml"), (";;;", "json")];

/// The front matter at the start of `source` and the Markdown after it, or
/// `None` when `source` does not start with front matter.
///
/// The first line, after an optional byte order mark, is a delimiter,
/// optionally followed by a language as GitLab writes it (`---php`). The
/// block ends at the first later line that holds the same delimiter alone.
/// Spaces and tabs may end both lines.
pub fn split(source: &str) -> Option<(FrontMatter, &str)> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut lines = Lines(source);
    let first = lines.next()?;
    let (delimiter, language) = DELIMITERS.iter().find_map(|&(delimiter, language)| {
        let specifier = first.strip_prefix(delimiter)?;
        match specifier {
            "" => Some((delimiter, language)),
            _ if is_specifier(specifier) => Some((delimiter, specifier)),
            _ => None,
        }
    })?;
    let mut text = String::new();
    for line in lines.by_ref() {
        if line == delimiter {
            let front_matter = FrontMatter {
                value: value(language, &text),
                language: language.to_owned(),
                source: text,
            };
            return Some((front_matter, lines.0));
        }
        text.push_str(line);
        text.push('\n');
    }
    None
}

/// What `source` in `language` holds: `None` for a language other than
/// YAML, TOML or JSON, an error message for source that does not parse.
/// Blank source, or YAML with only comments, is an empty map.
fn value(language: &str, source: &str) -> Option<Result<Value, String>> {
    let read = match language.to_ascii_lowercase().as_str() {
        "yaml" | "yml" => yaml,
        "toml" => toml,
        "json" => json,
        _ => return None,
    };
    if source.trim().is_empty() {
        return Some(Ok(Value::Map(Vec::new())));
    }
    Some(read(source))
}

/// The first YAML document in `source`, with aliases taking the value of
/// their anchors.
fn yaml(source: &str) -> Result<Value, String> {
    let documents = yaml_rust2::YamlLoader::load_from_str(source).map_err(|e| e.to_string())?;
    Ok(match documents.into_iter().next() {
        Some(document) => from_yaml(document),
        None => Value::Map(Vec::new()),
    })
}

fn from_yaml(yaml: yaml_rust2::Yaml) -> Value {
    use yaml_rust2::Yaml;
    match yaml {
        Yaml::String(text) => Value::String(text),
        Yaml::Real(text) => Value::Number(text),
        Yaml::Integer(number) => Value::Number(number.to_string()),
        Yaml::Boolean(bool) => Value::Bool(bool),
        Yaml::Array(items) => Value::List(items.into_iter().map(from_yaml).collect()),
        Yaml::Hash(entries) => Value::Map(
            (entries.into_iter())
                .map(|(key, value)| (yaml_key(key), from_yaml(value)))
                .collect(),
        ),
        // A value with a tag that does not fit it, such as `!!int a`, is
        // a bad value.
        Yaml::Null | Yaml::Alias(_) | Yaml::BadValue => Value::Null,
    }
}

/// A YAML key as text. A key that is a list or a map, which YAML allows
/// but front matter hardly uses, shows as `[…]` or `{…}`.
fn yaml_key(key: yaml_rust2::Yaml) -> String {
    use yaml_rust2::Yaml;
    match key {
        Yaml::Array(_) => "[…]".to_owned(),
        Yaml::Hash(_) => "{…}".to_owned(),
        Yaml::Null => "null".to_owned(),
        key => match from_yaml(key) {
            Value::String(text) | Value::Number(text) => text,
            Value::Bool(bool) => bool.to_string(),
            _ => String::new(),
        },
    }
}

fn toml(source: &str) -> Result<Value, String> {
    let table: toml::Table = toml::from_str(source).map_err(|e| e.to_string())?;
    Ok(from_toml(toml::Value::Table(table)))
}

fn from_toml(toml: toml::Value) -> Value {
    use toml::Value as Toml;
    match toml {
        Toml::String(text) => Value::String(text),
        Toml::Datetime(datetime) => Value::String(datetime.to_string()),
        // As TOML writes them, so `1.0` keeps its point.
        number @ (Toml::Integer(_) | Toml::Float(_)) => Value::Number(number.to_string()),
        Toml::Boolean(bool) => Value::Bool(bool),
        Toml::Array(items) => Value::List(items.into_iter().map(from_toml).collect()),
        Toml::Table(table) => Value::Map(
            (table.into_iter())
                .map(|(key, value)| (key, from_toml(value)))
                .collect(),
        ),
    }
}

fn json(source: &str) -> Result<Value, String> {
    let json = serde_json::from_str(source).map_err(|e| e.to_string())?;
    Ok(from_json(json))
}

fn from_json(json: serde_json::Value) -> Value {
    use serde_json::Value as Json;
    match json {
        Json::String(text) => Value::String(text),
        Json::Number(number) => Value::Number(number.to_string()),
        Json::Bool(bool) => Value::Bool(bool),
        Json::Null => Value::Null,
        Json::Array(items) => Value::List(items.into_iter().map(from_json).collect()),
        Json::Object(object) => Value::Map(
            (object.into_iter())
                .map(|(key, value)| (key, from_json(value)))
                .collect(),
        ),
    }
}

/// A language after the delimiter: a letter, then letters, digits, `_`,
/// `+` and `-`. The letter first keeps a longer rule, such as `----`, a
/// thematic break.
fn is_specifier(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_alphabetic())
        && (text.chars()).all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
}

/// The lines of the text left, without their line ends and the spaces and
/// tabs before them.
struct Lines<'a>(&'a str);

impl<'a> Iterator for Lines<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.0.is_empty() {
            return None;
        }
        let (line, rest) = self.0.split_once('\n').unwrap_or((self.0, ""));
        self.0 = rest;
        Some(line.trim_end_matches([' ', '\t', '\r']))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The language and the source of the front matter `split` finds,
    /// and the Markdown after it.
    fn split(text: &str) -> Option<((String, String), &str)> {
        let (front_matter, rest) = super::split(text)?;
        Some(((front_matter.language, front_matter.source), rest))
    }

    fn front_matter(language: &str, source: &str) -> (String, String) {
        (language.into(), source.into())
    }

    #[test]
    fn each_delimiter_gives_its_language() {
        for (text, language) in [
            ("---\ntitle: A\n---\n# Body\n", "yaml"),
            ("+++\ntitle: A\n+++\n# Body\n", "toml"),
            (";;;\ntitle: A\n;;;\n# Body\n", "json"),
        ] {
            assert_eq!(
                split(text),
                Some((front_matter(language, "title: A\n"), "# Body\n")),
                "{text}"
            );
        }
    }

    #[test]
    fn a_specifier_after_the_delimiter_names_the_language() {
        assert_eq!(
            split("---php\n$title = 'A';\n---\nBody\n"),
            Some((front_matter("php", "$title = 'A';\n"), "Body\n"))
        );
        assert_eq!(
            split("+++json \t\n{}\n+++\n"),
            Some((front_matter("json", "{}\n"), ""))
        );
        assert_eq!(
            split("---c_99+x-y\nx\n---\n"),
            Some((front_matter("c_99+x-y", "x\n"), ""))
        );
    }

    #[test]
    fn the_block_ends_at_the_same_delimiter_without_a_specifier() {
        // `+++` and `---php` do not end a block opened by `---`.
        assert_eq!(
            split("---\na: 1\n+++\n---php\n--- \t\nBody\n"),
            Some((front_matter("yaml", "a: 1\n+++\n---php\n"), "Body\n"))
        );
        // A delimiter that only starts a line does not end it either.
        assert_eq!(
            split("---\na: 1\n----\n--- x\n---\n"),
            Some((front_matter("yaml", "a: 1\n----\n--- x\n"), ""))
        );
    }

    #[test]
    fn without_a_closing_line_there_is_no_front_matter() {
        assert_eq!(split("---\ntitle: A\n\nBody\n"), None);
        assert_eq!(split("---\n"), None);
        assert_eq!(split("---"), None);
    }

    #[test]
    fn the_delimiter_must_be_the_first_line() {
        assert_eq!(split("\n---\na: 1\n---\n"), None);
        assert_eq!(split("Text\n\n---\na: 1\n---\n"), None);
        assert_eq!(split(" ---\na: 1\n---\n"), None);
    }

    #[test]
    fn other_first_lines_are_not_delimiters() {
        for text in [
            "----\na\n---\n",
            "-- -\na\n---\n",
            "***\na\n***\n",
            "--- yaml\na\n---\n",
            "---{a}\na\n---\n",
            "+++++\na\n+++\n",
        ] {
            assert_eq!(split(text), None, "{text}");
        }
    }

    #[test]
    fn a_byte_order_mark_may_come_first() {
        assert_eq!(
            split("\u{feff}---\na: 1\n---\nBody\n"),
            Some((front_matter("yaml", "a: 1\n"), "Body\n"))
        );
    }

    #[test]
    fn crlf_line_ends_work() {
        assert_eq!(
            split("---\r\na: 1\r\nb: 2\r\n---\r\nBody\r\n"),
            Some((front_matter("yaml", "a: 1\nb: 2\n"), "Body\r\n"))
        );
    }

    #[test]
    fn an_empty_block_is_front_matter() {
        assert_eq!(
            split("---\n---\nBody\n"),
            Some((front_matter("yaml", ""), "Body\n"))
        );
        // The closing line may be the last, without a line end.
        assert_eq!(split("+++\n+++"), Some((front_matter("toml", ""), "")));
    }

    fn string(text: &str) -> Value {
        Value::String(text.into())
    }

    fn number(text: &str) -> Value {
        Value::Number(text.into())
    }

    fn map(entries: &[(&str, Value)]) -> Value {
        Value::Map(
            (entries.iter())
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn reads_yaml_in_source_order() {
        let source = "name: release-notes\n\
                      version: 1.10\n\
                      count: 3\n\
                      draft: false\n\
                      owner: ~\n\
                      tags: [docs, release]\n\
                      author:\n  name: Aino\n  links:\n    - https://example.com\n\
                      an empty list: []\n\
                      1: one\n";
        assert_eq!(
            value("yaml", source),
            Some(Ok(map(&[
                ("name", string("release-notes")),
                ("version", number("1.10")),
                ("count", number("3")),
                ("draft", Value::Bool(false)),
                ("owner", Value::Null),
                ("tags", Value::List(vec![string("docs"), string("release")])),
                (
                    "author",
                    map(&[
                        ("name", string("Aino")),
                        ("links", Value::List(vec![string("https://example.com")])),
                    ])
                ),
                ("an empty list", Value::List(Vec::new())),
                ("1", string("one")),
            ])))
        );
    }

    #[test]
    fn yaml_strings_keep_or_fold_their_lines_as_written() {
        let source = "literal: |\n  one\n  two\nfolded: >\n  one\n  two\nquoted: '1.10'\n";
        assert_eq!(
            value("yaml", source),
            Some(Ok(map(&[
                ("literal", string("one\ntwo\n")),
                ("folded", string("one two\n")),
                ("quoted", string("1.10")),
            ])))
        );
    }

    #[test]
    fn yaml_aliases_take_the_value_of_their_anchor() {
        assert_eq!(
            value("yaml", "a: &x [1]\nb: *x\n"),
            Some(Ok(map(&[
                ("a", Value::List(vec![number("1")])),
                ("b", Value::List(vec![number("1")])),
            ])))
        );
    }

    #[test]
    fn reads_toml_in_source_order() {
        let source = "title = \"Notes\"\nweight = 2\nratio = 1.5\nwhole = 1.0\n\
                      date = 2026-10-20\ndraft = true\n\
                      [params]\nzeta = 1\nalpha = [\"a\", \"b\"]\n";
        assert_eq!(
            value("toml", source),
            Some(Ok(map(&[
                ("title", string("Notes")),
                ("weight", number("2")),
                ("ratio", number("1.5")),
                ("whole", number("1.0")),
                ("date", string("2026-10-20")),
                ("draft", Value::Bool(true)),
                (
                    "params",
                    map(&[
                        ("zeta", number("1")),
                        ("alpha", Value::List(vec![string("a"), string("b")])),
                    ])
                ),
            ])))
        );
    }

    #[test]
    fn reads_json_in_source_order() {
        let source = "{\"title\": \"Notes\", \"weight\": 2, \"ratio\": 1.5, \
                      \"draft\": false, \"owner\": null, \
                      \"params\": {\"zeta\": 1, \"alpha\": [\"a\"]}}\n";
        assert_eq!(
            value("json", source),
            Some(Ok(map(&[
                ("title", string("Notes")),
                ("weight", number("2")),
                ("ratio", number("1.5")),
                ("draft", Value::Bool(false)),
                ("owner", Value::Null),
                (
                    "params",
                    map(&[
                        ("zeta", number("1")),
                        ("alpha", Value::List(vec![string("a")])),
                    ])
                ),
            ])))
        );
    }

    #[test]
    fn content_of_another_shape_is_read_as_it_is() {
        assert_eq!(
            value("yaml", "- a\n- b\n"),
            Some(Ok(Value::List(vec![string("a"), string("b")])))
        );
        assert_eq!(value("json", "\"text\""), Some(Ok(string("text"))));
    }

    #[test]
    fn a_specifier_names_the_format_in_any_case() {
        assert_eq!(
            value("YAML", "a: 1\n"),
            Some(Ok(map(&[("a", number("1"))])))
        );
        assert_eq!(value("yml", "a: 1\n"), Some(Ok(map(&[("a", number("1"))]))));
        assert_eq!(
            value("Json", "{\"a\": 1}"),
            Some(Ok(map(&[("a", number("1"))])))
        );
        assert_eq!(value("php", "$a = 1;\n"), None);
    }

    #[test]
    fn a_blank_block_is_an_empty_map_in_every_format() {
        for language in ["yaml", "toml", "json"] {
            for source in ["", "\n  \n"] {
                assert_eq!(value(language, source), Some(Ok(map(&[]))), "{language}");
            }
        }
        // Only comments are no content either.
        assert_eq!(value("yaml", "# a comment\n"), Some(Ok(map(&[]))));
    }

    #[test]
    fn content_that_does_not_parse_keeps_the_parsers_message() {
        assert_eq!(
            value("yaml", "title: [unclosed\n"),
            Some(Err(
                "while parsing a flow sequence, expected ',' or ']' at byte 17 line 2 column 1"
                    .into()
            ))
        );
        assert_eq!(
            value("json", "{\"a\": }"),
            Some(Err("expected value at line 1 column 7".into()))
        );
        let Some(Err(error)) = value("toml", "a = \n") else {
            panic!("expected an error");
        };
        assert!(error.starts_with("TOML parse error at line 1"), "{error}");
    }
}
