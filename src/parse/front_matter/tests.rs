
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
fn yaml_after_a_document_end_marker_is_left_out() {
    // `...` ends the YAML document; what follows would be a second one,
    // which front matter does not show.
    assert_eq!(
        value("yaml", "a: 1\n...\nb: 2\n"),
        Some(Ok(map(&[("a", number("1"))])))
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

fn error(line: usize, message: &str) -> Option<Result<Value, FrontMatterError>> {
    Some(Err(FrontMatterError {
        line: Some(line),
        message: message.into(),
    }))
}

#[test]
fn content_that_does_not_parse_keeps_the_parsers_message_and_line() {
    assert_eq!(
        value("yaml", "title: A\ntags: [unclosed\n"),
        error(3, "while parsing a flow sequence, expected ',' or ']'")
    );
    assert_eq!(value("json", "{\n\"a\": }"), error(2, "expected value"));
    assert_eq!(
        value("toml", "a = 1\nb = \n"),
        error(2, "string values must be quoted, expected literal string")
    );
}

#[test]
fn the_line_of_an_error_counts_from_the_start_of_the_file() {
    let (front_matter, _) = super::split("---\ntitle: A\ntags: [unclosed\n---\n").unwrap();
    assert_eq!(
        front_matter.value,
        error(4, "while parsing a flow sequence, expected ',' or ']'")
    );
    // A byte order mark takes no line.
    let (front_matter, _) = super::split("\u{feff};;;\n{\n\"a\": }\n;;;\n").unwrap();
    assert_eq!(front_matter.value, error(3, "expected value"));
    assert_eq!(
        FrontMatterError {
            line: Some(4),
            message: "expected value".into()
        }
        .to_string(),
        "front matter, line 4: expected value"
    );
}
