//! Front matter: a block of metadata at the very start of a file, between
//! two delimiter lines, as Jekyll, Hugo and GitLab read it.

use crate::model::{FrontMatter, FrontMatterError, Value};

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
                // The block starts on the file's second line.
                value: value(language, &text).map(|value| value.map_err(|error| below(error, 1))),
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

/// `error` in a block that starts after `lines` lines of the file.
fn below(error: FrontMatterError, lines: usize) -> FrontMatterError {
    FrontMatterError {
        line: error.line.map(|line| line + lines),
        ..error
    }
}

/// What `source` in `language` holds: `None` for a language other than
/// YAML, TOML or JSON, an error message for source that does not parse.
/// Blank source, or YAML with only comments, is an empty map.
/// The line of an error counts from the first line of `source`.
fn value(language: &str, source: &str) -> Option<Result<Value, FrontMatterError>> {
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
fn yaml(source: &str) -> Result<Value, FrontMatterError> {
    let documents =
        yaml_rust2::YamlLoader::load_from_str(source).map_err(|error| FrontMatterError {
            line: Some(error.marker().line()),
            message: error.info().to_owned(),
        })?;
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

fn toml(source: &str) -> Result<Value, FrontMatterError> {
    let table: toml::Table = toml::from_str(source).map_err(|error| FrontMatterError {
        line: (error.span()).map(|span| source[..span.start].matches('\n').count() + 1),
        message: error.message().trim_end().to_owned(),
    })?;
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

fn json(source: &str) -> Result<Value, FrontMatterError> {
    let json = serde_json::from_str(source).map_err(|error| {
        // The message ends in the position, which the line replaces.
        let message = error.to_string();
        let position = format!(" at line {} column {}", error.line(), error.column());
        FrontMatterError {
            line: (error.line() > 0).then_some(error.line()),
            message: message
                .strip_suffix(&position)
                .unwrap_or(&message)
                .to_owned(),
        }
    })?;
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
mod tests;
