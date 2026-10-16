//! Word wrapping for styled text.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::style::{Span, Style, push};

enum Token {
    Word(Vec<Span>),
    Space(Style),
    Break,
}

/// Display width in terminal cells.
pub fn width(spans: &[Span]) -> usize {
    spans.iter().map(|span| span.text.width()).sum()
}

/// Wraps at spaces to lines of at most `max` cells. Runs of spaces collapse
/// to one, `\n` forces a break, and a word longer than a line is split.
pub fn wrap(spans: &[Span], max: usize) -> Vec<Vec<Span>> {
    let max = max.max(1);
    let mut lines = Vec::new();
    let mut line = Vec::new();
    let mut used = 0;
    let mut space: Option<Style> = None;

    for token in tokenize(spans) {
        match token {
            Token::Break => {
                lines.push(std::mem::take(&mut line));
                used = 0;
                space = None;
            }
            Token::Space(style) => {
                if used > 0 {
                    space = Some(style);
                }
            }
            Token::Word(word) => {
                let word_width = width(&word);
                let gap = usize::from(space.is_some());
                if used + gap + word_width <= max {
                    if let Some(style) = space.take() {
                        push(&mut line, " ", &style);
                    }
                    append(&mut line, &word);
                    used += gap + word_width;
                } else if word_width <= max {
                    lines.push(std::mem::take(&mut line));
                    append(&mut line, &word);
                    used = word_width;
                    space = None;
                } else {
                    if let Some(style) = space.take()
                        && used + 1 < max
                    {
                        push(&mut line, " ", &style);
                        used += 1;
                    }
                    split_long_word(&word, max, &mut lines, &mut line, &mut used);
                }
            }
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

fn append(line: &mut Vec<Span>, word: &[Span]) {
    for span in word {
        push(line, &span.text, &span.style);
    }
}

/// Breaks a word between graphemes wherever the line is full.
fn split_long_word(
    word: &[Span],
    max: usize,
    lines: &mut Vec<Vec<Span>>,
    line: &mut Vec<Span>,
    used: &mut usize,
) {
    for span in word {
        for grapheme in span.text.graphemes(true) {
            let grapheme_width = grapheme.width();
            if *used + grapheme_width > max && *used > 0 {
                lines.push(std::mem::take(line));
                *used = 0;
            }
            push(line, grapheme, &span.style);
            *used += grapheme_width;
        }
    }
}

fn tokenize(spans: &[Span]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word: Vec<Span> = Vec::new();
    for span in spans {
        let mut rest = span.text.as_str();
        while let Some(c) = rest.chars().next() {
            if c == ' ' || c == '\n' {
                if !word.is_empty() {
                    tokens.push(Token::Word(std::mem::take(&mut word)));
                }
                tokens.push(if c == ' ' {
                    Token::Space(span.style.clone())
                } else {
                    Token::Break
                });
                rest = &rest[1..];
            } else {
                let end = rest.find([' ', '\n']).unwrap_or(rest.len());
                push(&mut word, &rest[..end], &span.style);
                rest = &rest[end..];
            }
        }
    }
    if !word.is_empty() {
        tokens.push(Token::Word(word));
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Vec<Span> {
        vec![Span::new(text, Style::default())]
    }

    fn texts(lines: &[Vec<Span>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.iter().map(|span| span.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn wraps_at_spaces() {
        let lines = wrap(&plain("one two three four"), 9);
        assert_eq!(texts(&lines), vec!["one two", "three", "four"]);
    }

    #[test]
    fn collapses_spaces_and_drops_them_at_line_edges() {
        let lines = wrap(&plain("  one   two  "), 20);
        assert_eq!(texts(&lines), vec!["one two"]);
    }

    #[test]
    fn keeps_styled_runs_without_spaces_together() {
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let spans = vec![
            Span::new("aa ", Style::default()),
            Span::new("bold", bold),
            Span::new(", next", Style::default()),
        ];
        let lines = wrap(&spans, 7);
        assert_eq!(texts(&lines), vec!["aa", "bold,", "next"]);
    }

    #[test]
    fn splits_words_longer_than_the_line() {
        let lines = wrap(&plain("abcdefgh"), 3);
        assert_eq!(texts(&lines), vec!["abc", "def", "gh"]);
    }

    #[test]
    fn counts_wide_characters_as_two_cells() {
        let lines = wrap(&plain("日本 語"), 4);
        assert_eq!(texts(&lines), vec!["日本", "語"]);
    }

    #[test]
    fn breaks_lines_at_newlines() {
        let lines = wrap(&plain("one\ntwo"), 20);
        assert_eq!(texts(&lines), vec!["one", "two"]);
    }
}
