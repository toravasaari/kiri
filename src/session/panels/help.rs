//! The help as a Markdown document, made from the command table by topic.

use crate::session::commands::{COMMANDS, Topic};
use crate::typeset::model::Document;
use crate::typeset::parse::parse;

/// The topics in the order the help shows them, each with its heading.
pub(crate) const TOPICS: [(Topic, &str); 6] = [
    (Topic::Moving, "Moving"),
    (Topic::Searching, "Searching"),
    (Topic::Links, "Links"),
    (Topic::Files, "Files"),
    (Topic::View, "View"),
    (Topic::Help, "Help"),
];

/// The title of the help's frame.
pub(crate) const TITLE: &str = "Keys";

/// What the bottom edge of the frame says: the keys that close the help.
pub(crate) const FOOTER: &str = "Esc ? q close";

/// The help as Markdown: a heading for each topic, with a list of its
/// commands below, the keys in bold and what they do after them. Every
/// topic is there, also the keys between files with one file.
pub(crate) fn markdown() -> String {
    let mut text = String::new();
    for (topic, heading) in TOPICS {
        text.push_str(&format!("## {heading}\n\n"));
        let listed = COMMANDS.iter().filter(|command| command.topic == topic);
        for command in listed.filter(|command| command.listed()) {
            let (keys, what) = (escape(command.names), escape(command.what));
            text.push_str(&format!("- **{keys}** – {what}\n"));
        }
        text.push('\n');
    }
    text
}

/// `text` with the characters that Markdown reads as syntax kept as they
/// are.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '|' | '*' | '`' | '_' | '[' | ']' | '<' | '>') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// The help as a document, to lay out.
pub(crate) fn document() -> Document {
    parse(&markdown())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The part of the Markdown under `heading`, up to the next heading.
    fn section(heading: &str) -> String {
        let markdown = markdown();
        let start = markdown.find(&format!("## {heading}\n")).unwrap();
        let rest = &markdown[start + 3..];
        rest.split("\n## ").next().unwrap().to_owned()
    }

    #[test]
    fn the_help_has_a_heading_for_every_topic_in_order() {
        let markdown = markdown();
        let headings: Vec<&str> = markdown
            .lines()
            .filter_map(|line| line.strip_prefix("## "))
            .collect();
        assert_eq!(
            headings,
            ["Moving", "Searching", "Links", "Files", "View", "Help"]
        );
    }

    #[test]
    fn every_listed_command_is_under_its_topic_once() {
        let markdown = markdown();
        for (topic, heading) in TOPICS {
            let section = section(heading);
            for command in COMMANDS.iter().filter(|command| command.listed()) {
                let row = format!(
                    "- **{}** – {}\n",
                    escape(command.names),
                    escape(command.what)
                );
                let count = section.matches(&row).count();
                let expected = usize::from(command.topic == topic);
                assert_eq!(count, expected, "{} under {heading}", command.names);
            }
        }
        // A row for each listed command, and none for the other keys of a
        // command that has a row already.
        let listed = COMMANDS.iter().filter(|command| command.listed()).count();
        assert_eq!(markdown.matches("\n- **").count(), listed);
    }

    #[test]
    fn the_keys_between_files_are_always_listed() {
        let files = section("Files");
        for text in [
            ":n Shift+PgDn",
            "next file",
            ":p Shift+PgUp",
            "previous file",
        ] {
            assert!(files.contains(text), "{text}: {files}");
        }
    }

    #[test]
    fn no_key_breaks_the_list() {
        // The names and what they do come out of Markdown as they are: a
        // character that Markdown reads is escaped.
        assert_eq!(escape("a|b*c`d_e[f]<g>"), "a\\|b\\*c\\`d\\_e\\[f\\]\\<g\\>");
        assert_eq!(escape("j ↓ Enter"), "j ↓ Enter");
        let markdown = markdown();
        for row in markdown.lines().filter(|row| row.starts_with("- **")) {
            // One bold run for the keys, and a dash between it and the rest.
            assert_eq!(row.matches("**").count(), 2, "{row}");
            assert_eq!(row.matches(" – ").count(), 1, "{row}");
        }
    }
}
