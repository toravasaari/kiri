//! Front matter: a block of metadata at the very start of a file, between
//! two delimiter lines, as Jekyll, Hugo and GitLab read it.

use crate::model::FrontMatter;

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

    fn front_matter(language: &str, source: &str) -> FrontMatter {
        FrontMatter {
            language: language.into(),
            source: source.into(),
        }
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
}
