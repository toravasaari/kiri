//! Text told on standard error.

/// `text` with every control character except newline and tab replaced by
/// U+FFFD. Text told on standard error may quote a document, such as an image
/// path or a formula's source, which must not send escape sequences to the
/// terminal.
pub fn printable(text: &str) -> String {
    (text.chars())
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                char::REPLACEMENT_CHARACTER
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::printable;

    #[test]
    fn control_characters_become_replacement_characters() {
        assert_eq!(
            printable("a\x1bb\x07c\x7fd\u{9b}e"),
            "a\u{fffd}b\u{fffd}c\u{fffd}d\u{fffd}e"
        );
    }

    #[test]
    fn newline_tab_and_letters_stay() {
        assert_eq!(printable("a\n\tä"), "a\n\tä");
    }
}
