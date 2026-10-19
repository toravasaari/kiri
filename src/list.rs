//! `kiri --list-emojis`: the emoji shortcodes kiri shows as emoji, from the
//! same data the parser uses, so the list matches what this build renders.

use std::fmt::Write;

use emojis::{Emoji, Group};

/// The emoji a document can name, in Unicode's order: those with at least
/// one shortcode. Skin tones other than the default have none.
fn emojis() -> impl Iterator<Item = &'static Emoji> {
    emojis::iter().filter(|emoji| emoji.shortcode().is_some())
}

/// One line per emoji for a file or a pipe: the emoji, its shortcodes
/// separated by spaces, and its name, separated by tabs.
pub fn emoji_lines() -> String {
    let mut out = String::new();
    for emoji in emojis() {
        let shortcodes: Vec<_> = emoji.shortcodes().collect();
        // Writing to a String does not fail.
        let _ = writeln!(
            out,
            "{}\t{}\t{}",
            emoji.as_str(),
            shortcodes.join(" "),
            emoji.name()
        );
    }
    out
}

/// The list as a Markdown document: a table of emoji under a heading for
/// each Unicode group.
pub fn emoji_document() -> String {
    let mut out = String::new();
    // Writing to a String does not fail.
    let _ = write_document(&mut out);
    out
}

fn write_document(out: &mut String) -> std::fmt::Result {
    writeln!(out, "# Emoji shortcodes\n")?;
    writeln!(
        out,
        "A shortcode such as `:smile:` in a document shows its emoji. \
         These are the {} emoji kiri knows.",
        emojis().count()
    )?;
    for group in Group::iter() {
        let mut rows = emojis().filter(|emoji| emoji.group() == group).peekable();
        if rows.peek().is_none() {
            continue;
        }
        writeln!(out, "\n## {}\n", group_name(group))?;
        writeln!(out, "| Emoji | Shortcodes | Name |")?;
        writeln!(out, "| --- | --- | --- |")?;
        for emoji in rows {
            let shortcodes: Vec<_> = emoji
                .shortcodes()
                .map(|shortcode| format!("`:{shortcode}:`"))
                .collect();
            writeln!(
                out,
                "| {} | {} | {} |",
                escaped(emoji.as_str()),
                shortcodes.join(" "),
                escaped(emoji.name())
            )?;
        }
    }
    Ok(())
}

/// The name Unicode's data gives `group`.
fn group_name(group: Group) -> &'static str {
    match group {
        Group::SmileysAndEmotion => "Smileys & Emotion",
        Group::PeopleAndBody => "People & Body",
        Group::AnimalsAndNature => "Animals & Nature",
        Group::FoodAndDrink => "Food & Drink",
        Group::TravelAndPlaces => "Travel & Places",
        Group::Activities => "Activities",
        Group::Objects => "Objects",
        Group::Symbols => "Symbols",
        Group::Flags => "Flags",
    }
}

/// `text` with its ASCII punctuation escaped, so that keycaps such as
/// `*️⃣` and names such as `keycap: *` stay text in a table cell.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punctuation_is_escaped() {
        assert_eq!(escaped("keycap: *"), "keycap\\: \\*");
        assert_eq!(escaped("😄"), "😄");
    }

    /// Every emoji in the pipe's lines is a row of the document.
    #[test]
    fn the_document_has_a_row_per_line() {
        let rows = emoji_document()
            .lines()
            .filter(|line| {
                line.starts_with("| ") && !line.starts_with("| Emoji") && !line.starts_with("| ---")
            })
            .count();
        assert_eq!(rows, emoji_lines().lines().count());
    }
}
