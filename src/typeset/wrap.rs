//! Word wrapping for styled text.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::typeset::style::{Spaces, Span, Style, push};

enum Token {
    Word(Vec<Span>),
    /// One space; its style says whether it collapses.
    Space(Style),
    Break,
}

/// Display width in terminal cells, each span measured at its size.
pub fn width(spans: &[Span]) -> usize {
    (spans.iter())
        .map(|span| span.style.size.width(&span.text))
        .sum()
}

/// The cells `line`, which takes `used` cells, takes with `gap` and then
/// `word` appended. Text at a fraction is packed together with the text
/// before it in the same style, so it may add fewer or more cells than it
/// takes on its own, and even take some away.
fn width_after(line: &[Span], used: usize, gap: &[Span], word: &[Span]) -> usize {
    if (gap.iter().chain(word)).all(|span| span.style.size.fraction().is_none()) {
        return used + width(gap) + width(word);
    }
    // Text is only ever merged into the last span.
    let mut tail: Vec<Span> = line.last().cloned().into_iter().collect();
    let before = width(&tail);
    append(&mut tail, gap);
    append(&mut tail, word);
    used + width(&tail) - before
}

/// Appends as many spaces in `style` as fit before `spans` take `max`
/// cells.
pub fn fill(spans: &mut Vec<Span>, max: usize, style: &Style) {
    let used = width(spans);
    let room = max.saturating_sub(used);
    let size = style.size;
    let (numerator, denominator) = (size.fraction()).map_or((1, 1), |fraction| {
        (fraction.numerator(), fraction.denominator())
    });
    // Packed spaces may fill the last group of the text before them, so
    // count from one group more than the room holds and drop the spaces
    // that overflow.
    let most =
        (room / usize::from(size.scale()) + 1) * usize::from(denominator) / usize::from(numerator);
    let spaces = (0..=most)
        .rev()
        .find(|&spaces| {
            let spaces = [Span::new(&" ".repeat(spaces), style.clone())];
            width_after(spans, used, &[], &spaces) <= used + room
        })
        .unwrap_or(0);
    push(spans, &" ".repeat(spaces), style);
}

/// Splits styled code into lines of at most `max` cells, keeping every
/// character, spaces included. Lines break between graphemes, measured as
/// the wrapper measures them, so a flag or a joined emoji stays whole. A
/// grapheme wider than `max` gets a line of its own and overflows it.
pub fn split(spans: &[Span], max: usize) -> Vec<Vec<Span>> {
    let mut lines = Vec::new();
    let mut line = Vec::new();
    split_onto(spans, max, &mut lines, &mut line, 0);
    lines.push(line);
    lines
}

/// Appends `spans` to `line`, which already takes `used` cells, and moves
/// it to `lines` between graphemes wherever it is full, as `split` does.
/// Returns the cells the last line takes.
fn split_onto(
    spans: &[Span],
    max: usize,
    lines: &mut Vec<Vec<Span>>,
    line: &mut Vec<Span>,
    mut used: usize,
) -> usize {
    for span in spans {
        for grapheme in span.text.graphemes(true) {
            let size = span.style.size;
            let mut after = match size.fraction() {
                None => used + size.width(grapheme),
                Some(_) => width_after(line, used, &[], &[Span::new(grapheme, span.style.clone())]),
            };
            if after > max && used > 0 {
                lines.push(std::mem::take(line));
                after = size.width(grapheme);
            }
            push(line, grapheme, &span.style);
            used = after;
        }
    }
    used
}

/// `spans` cut to at most `max` cells, between graphemes, measured as the
/// wrapper measures them. The text kept is measured whole, as text at a
/// fraction is packed and a grapheme of it alone would round up to a cell;
/// packed, a letter more can take a cell less, so the longest beginning
/// that fits is kept. At a whole size it only grows, and the search stops
/// at the first grapheme that does not fit.
pub fn cut(spans: Vec<Span>, max: usize) -> Vec<Span> {
    let mut left = max;
    let mut kept = Vec::new();
    for mut span in spans {
        let size = span.style.size;
        if size.width(&span.text) > left {
            let packed = size.fraction().is_some();
            let mut end = 0;
            for (start, grapheme) in span.text.grapheme_indices(true) {
                let after = start + grapheme.len();
                if size.width(&span.text[..after]) <= left {
                    end = after;
                } else if !packed {
                    break;
                }
            }
            span.text.truncate(end);
        }
        left = left.saturating_sub(size.width(&span.text));
        if !span.text.is_empty() {
            kept.push(span);
        }
    }
    kept
}

/// The end of `text` in at most `max` cells, after `…` when its start is
/// left out; empty when not even one grapheme fits after `…`.
pub fn tail(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_owned();
    }
    let mut start = text.len();
    let mut used = 1;
    for (at, grapheme) in text.grapheme_indices(true).rev() {
        used += grapheme.width();
        if used > max {
            break;
        }
        start = at;
    }
    if start == text.len() {
        return String::new();
    }
    format!("…{}", &text[start..])
}

/// Wraps at spaces to lines of at most `max` cells. `\n` forces a break, and
/// a word longer than a line is split. Runs of spaces collapse to one unless
/// their style keeps them; either way, the spaces where a line breaks are
/// dropped. See `Spaces` for spaces that keep text together.
pub fn wrap(spans: &[Span], max: usize) -> Vec<Vec<Span>> {
    let mut wrapper = Wrapper {
        max: max.max(1),
        lines: Vec::new(),
        line: Vec::new(),
        used: 0,
        gap: Vec::new(),
    };
    for token in tokenize(spans, false) {
        wrapper.token(token);
    }
    let Wrapper {
        mut lines, line, ..
    } = wrapper;
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

struct Wrapper {
    max: usize,
    lines: Vec<Vec<Span>>,
    line: Vec<Span>,
    used: usize,
    /// Spaces waiting for the next word on the line.
    gap: Vec<Span>,
}

impl Wrapper {
    fn token(&mut self, token: Token) {
        match token {
            Token::Break => {
                self.lines.push(std::mem::take(&mut self.line));
                self.used = 0;
                self.gap.clear();
            }
            Token::Space(style) => {
                if !self.collapses(&style) {
                    push(&mut self.gap, " ", &style);
                }
            }
            Token::Word(word) => self.word(&word),
        }
    }

    /// Whether a space in `style` adds nothing to the gap. A kept space
    /// always counts. An ordinary one disappears after another ordinary
    /// space, counts after a kept one, and with nothing in the gap counts
    /// only after text on the line.
    fn collapses(&self, style: &Style) -> bool {
        if style.spaces != Spaces::Collapse {
            return false;
        }
        match self.gap.last() {
            Some(last) => last.style.spaces == Spaces::Collapse,
            None => self.used == 0,
        }
    }

    fn word(&mut self, word: &[Span]) {
        let after = width_after(&self.line, self.used, &self.gap, word);
        let word_width = || width(word);
        if after <= self.max {
            append(&mut self.line, &std::mem::take(&mut self.gap));
            append(&mut self.line, word);
            self.used = after;
        } else if word_width() <= self.max {
            self.break_line();
            append(&mut self.line, word);
            self.used = word_width();
        } else if keeps_together(word) {
            // Too long for one line: it breaks at its spaces after all.
            for token in tokenize(word, true) {
                self.token(token);
            }
        } else {
            // The gap needs room for the word's first character after it,
            // or the word starts on a new line.
            if width_after(&self.line, self.used, &self.gap, &first(word)) <= self.max {
                self.place_gap();
            } else {
                self.break_line();
            }
            self.split_long_word(word);
        }
    }

    fn place_gap(&mut self) {
        let gap = std::mem::take(&mut self.gap);
        self.used = width_after(&self.line, self.used, &gap, &[]);
        append(&mut self.line, &gap);
    }

    /// Starts a new line unless the current one is empty. The gap goes: a
    /// line never starts with the spaces it broke at.
    fn break_line(&mut self) {
        if !self.line.is_empty() {
            self.lines.push(std::mem::take(&mut self.line));
        }
        self.used = 0;
        self.gap.clear();
    }

    /// Breaks a word between graphemes wherever the line is full.
    fn split_long_word(&mut self, word: &[Span]) {
        self.used = split_onto(word, self.max, &mut self.lines, &mut self.line, self.used);
    }
}

fn append(line: &mut Vec<Span>, word: &[Span]) {
    for span in word {
        push(line, &span.text, &span.style);
    }
}

/// The first grapheme of `word`, in its style.
fn first(word: &[Span]) -> Vec<Span> {
    (word.iter())
        .find_map(|span| {
            (span.text.graphemes(true).next())
                .map(|grapheme| Span::new(grapheme, span.style.clone()))
        })
        .into_iter()
        .collect()
}

/// The words the wrapper keeps on one line when they fit: text whose spaces
/// are kept together is one word, with the punctuation attached to it.
pub(super) fn words(spans: &[Span]) -> Vec<Vec<Span>> {
    (tokenize(spans, false).into_iter())
        .filter_map(|token| match token {
            Token::Word(word) => Some(word),
            Token::Space(_) | Token::Break => None,
        })
        .collect()
}

/// Whether the word holds spaces that break only a line too long without
/// them.
pub(super) fn keeps_together(word: &[Span]) -> bool {
    (word.iter()).any(|span| span.style.spaces == Spaces::KeepTogether && span.text.contains(' '))
}

/// With `split_together`, spaces kept together break like any other.
fn tokenize(spans: &[Span], split_together: bool) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word: Vec<Span> = Vec::new();
    for span in spans {
        for (text, style) in pieces(span, split_together) {
            let spaces_break = style.spaces != Spaces::KeepTogether;
            let is_break = |c: char| c == '\n' || (c == ' ' && spaces_break);
            let mut rest = text;
            while let Some(c) = rest.chars().next() {
                if is_break(c) {
                    if !word.is_empty() {
                        tokens.push(Token::Word(std::mem::take(&mut word)));
                    }
                    tokens.push(if c == ' ' {
                        Token::Space(style.clone())
                    } else {
                        Token::Break
                    });
                    rest = &rest[1..];
                } else {
                    let end = rest.find(is_break).unwrap_or(rest.len());
                    push(&mut word, &rest[..end], &style);
                    rest = &rest[end..];
                }
            }
        }
    }
    if !word.is_empty() {
        tokens.push(Token::Word(word));
    }
    tokens
}

/// Splits a span whose spaces are kept together into its leading spaces,
/// the text they join and its trailing spaces. The spaces at the edges join
/// nothing, so they break like kept spaces; with `split_together`, all of
/// them do.
fn pieces(span: &Span, split_together: bool) -> Vec<(&str, Style)> {
    if span.style.spaces != Spaces::KeepTogether {
        return vec![(&span.text, span.style.clone())];
    }
    let kept = Style {
        spaces: Spaces::Keep,
        ..span.style.clone()
    };
    if split_together {
        return vec![(&span.text, kept)];
    }
    let text = span.text.as_str();
    let start = text.len() - text.trim_start_matches(' ').len();
    let end = start.max(text.trim_end_matches(' ').len());
    vec![
        (&text[..start], kept.clone()),
        (&text[start..end], span.style.clone()),
        (&text[end..], kept),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::size::{Fraction, TextSize, VerticalAlign};
    use crate::typeset::style;

    fn plain(text: &str) -> Vec<Span> {
        vec![Span::new(text, Style::default())]
    }

    fn texts(lines: &[Vec<Span>]) -> Vec<String> {
        lines.iter().map(|line| style::text(line)).collect()
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
    fn keeps_the_space_before_a_word_longer_than_the_line() {
        let lines = wrap(&plain("ab verylongword"), 10);
        assert_eq!(texts(&lines), vec!["ab verylon", "gword"]);
        // Exactly one cell left for the first character after the space.
        let lines = wrap(&plain("abcdefgh verylongword"), 10);
        assert_eq!(texts(&lines), vec!["abcdefgh v", "erylongwor", "d"]);
        let lines = wrap(&plain("abcdefg 日本語日本語"), 10);
        assert_eq!(texts(&lines), vec!["abcdefg 日", "本語日本語"]);
        // No room for the space and a character: the word starts a new line.
        let lines = wrap(&plain("abcdefghi verylongwordxx"), 10);
        assert_eq!(texts(&lines), vec!["abcdefghi", "verylongwo", "rdxx"]);
        let lines = wrap(&plain("abcdefgh 日本語日本語"), 10);
        assert_eq!(texts(&lines), vec!["abcdefgh", "日本語日本", "語"]);
    }

    #[test]
    fn counts_wide_characters_as_two_cells() {
        let lines = wrap(&plain("日本 語"), 4);
        assert_eq!(texts(&lines), vec!["日本", "語"]);
    }

    #[test]
    fn keeps_spaces_whose_style_keeps_them() {
        let kept = Style {
            spaces: Spaces::Keep,
            ..Style::default()
        };
        let spans = vec![
            Span::new("run ", Style::default()),
            Span::new("git  commit", kept.clone()),
            Span::new(" now", Style::default()),
        ];
        assert_eq!(texts(&wrap(&spans, 40)), vec!["run git  commit now"]);
        // An ordinary space next to kept ones stays as well.
        let spans = vec![
            Span::new("a ", Style::default()),
            Span::new("  b", kept.clone()),
        ];
        assert_eq!(texts(&wrap(&spans, 40)), vec!["a   b"]);
        // At a break the spaces go, however many there are.
        let spans = vec![Span::new("aaa   bbb  c", kept)];
        assert_eq!(texts(&wrap(&spans, 6)), vec!["aaa", "bbb  c"]);
    }

    #[test]
    fn fills_a_line_to_exactly_the_width() {
        let lines = wrap(&plain("aaa bbb ccc"), 7);
        assert_eq!(texts(&lines), vec!["aaa bbb", "ccc"]);
        // A width of zero counts as one.
        let lines = wrap(&plain("ab"), 0);
        assert_eq!(texts(&lines), vec!["a", "b"]);
    }

    #[test]
    fn empty_text_is_one_empty_line() {
        assert_eq!(texts(&wrap(&plain(""), 10)), vec![""]);
        assert_eq!(texts(&wrap(&plain("   "), 10)), vec![""]);
    }

    #[test]
    fn hard_breaks_keep_empty_lines_and_drop_the_spaces_around_them() {
        let lines = wrap(&plain("one \n\n  two"), 20);
        assert_eq!(texts(&lines), vec!["one", "", "two"]);
        let lines = wrap(&plain("one\n"), 20);
        assert_eq!(texts(&lines), vec!["one"]);
    }

    #[test]
    fn a_run_of_spaces_in_several_styles_is_one_space() {
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let spans = vec![
            Span::new("a ", Style::default()),
            Span::new(" ", bold),
            Span::new(" b", Style::default()),
        ];
        assert_eq!(texts(&wrap(&spans, 20)), vec!["a b"]);
        assert_eq!(texts(&wrap(&spans, 2)), vec!["a", "b"]);
    }

    #[test]
    fn styles_carry_across_a_break() {
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let spans = vec![
            Span::new("aa ", Style::default()),
            Span::new("bold words", bold.clone()),
            Span::new(" cc", Style::default()),
        ];
        let lines = wrap(&spans, 7);
        assert_eq!(texts(&lines), vec!["aa bold", "words", "cc"]);
        assert_eq!(lines[1], vec![Span::new("words", bold)]);
        assert_eq!(lines[2], vec![Span::new("cc", Style::default())]);
    }

    #[test]
    fn a_link_keeps_its_target_on_every_line() {
        let link = Style {
            link: Some("https://example.com".into()),
            ..Style::default()
        };
        let spans = vec![Span::new("click here now", link.clone())];
        let lines = wrap(&spans, 6);
        assert_eq!(texts(&lines), vec!["click", "here", "now"]);
        assert!(lines.iter().flatten().all(|span| span.style == link));
    }

    #[test]
    fn a_long_word_keeps_its_styles_when_split() {
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let spans = vec![
            Span::new("abc", Style::default()),
            Span::new("def", bold.clone()),
        ];
        let lines = wrap(&spans, 4);
        assert_eq!(texts(&lines), vec!["abcd", "ef"]);
        assert_eq!(lines[1], vec![Span::new("ef", bold)]);
        // After a hard break, too.
        let lines = wrap(&plain("x\nabcdefg"), 3);
        assert_eq!(texts(&lines), vec!["x", "abc", "def", "g"]);
    }

    #[test]
    fn wide_characters_break_before_they_would_overflow() {
        let lines = wrap(&plain("日本語"), 5);
        assert_eq!(texts(&lines), vec!["日本", "語"]);
        let lines = wrap(&plain("ab 日本"), 4);
        assert_eq!(texts(&lines), vec!["ab", "日本"]);
        // One cell is too narrow for a wide character: it overflows on a
        // line of its own.
        let lines = wrap(&plain("日本"), 1);
        assert_eq!(texts(&lines), vec!["日", "本"]);
    }

    fn kept(text: &str) -> Span {
        let style = Style {
            spaces: Spaces::Keep,
            ..Style::default()
        };
        Span::new(text, style)
    }

    #[test]
    fn keeps_spaces_at_the_start_of_a_paragraph_and_after_a_hard_break() {
        assert_eq!(texts(&wrap(&[kept("  x")], 10)), vec!["  x"]);
        let spans = vec![Span::new("a\n", Style::default()), kept("  b")];
        assert_eq!(texts(&wrap(&spans, 10)), vec!["a", "  b"]);
    }

    #[test]
    fn a_kept_gap_before_a_word_longer_than_the_line() {
        let long = Span::new("verylongword", Style::default());
        // Room for the gap and the first character: the word starts here.
        let spans = vec![Span::new("ab", Style::default()), kept("  "), long.clone()];
        assert_eq!(texts(&wrap(&spans, 10)), vec!["ab  verylo", "ngword"]);
        // No room: the gap goes and the word starts a new line.
        let spans = vec![Span::new("abcdefgh", Style::default()), kept("  "), long];
        assert_eq!(
            texts(&wrap(&spans, 10)),
            vec!["abcdefgh", "verylongwo", "rd"]
        );
    }

    #[test]
    fn a_kept_run_wider_than_the_line_disappears_at_the_break() {
        let spaces = " ".repeat(12);
        let spans = vec![kept(&format!("x{spaces}y"))];
        assert_eq!(texts(&wrap(&spans, 5)), vec!["x", "y"]);
        // At the start of a paragraph it leaves no empty line behind.
        let spans = vec![kept(&format!("{spaces}y"))];
        assert_eq!(texts(&wrap(&spans, 5)), vec!["y"]);
        let spans = vec![kept(&spaces), Span::new("abcdefgh", Style::default())];
        assert_eq!(texts(&wrap(&spans, 5)), vec!["abcde", "fgh"]);
    }

    fn together(text: &str) -> Span {
        let style = Style {
            spaces: Spaces::KeepTogether,
            ..Style::default()
        };
        Span::new(text, style)
    }

    #[test]
    fn spaces_kept_together_move_the_text_to_a_new_line() {
        // Punctuation attached to the text moves with it.
        let spans = vec![
            Span::new("ab ", Style::default()),
            together("c  d"),
            Span::new(", e", Style::default()),
        ];
        assert_eq!(texts(&wrap(&spans, 6)), vec!["ab", "c  d,", "e"]);
        assert_eq!(texts(&wrap(&spans, 20)), vec!["ab c  d, e"]);
    }

    #[test]
    fn spaces_kept_together_break_a_text_longer_than_the_line() {
        // At its spaces first, then inside a word that is still too long.
        let spans = vec![together("aa bbbbbbb c")];
        assert_eq!(texts(&wrap(&spans, 5)), vec!["aa bb", "bbbbb", "c"]);
    }

    #[test]
    fn spaces_at_the_edges_of_text_kept_together_join_nothing() {
        // They break like kept spaces, so no line starts with one.
        let spans = vec![Span::new("aaaa ", Style::default()), together(" bb")];
        assert_eq!(texts(&wrap(&spans, 5)), vec!["aaaa", "bb"]);
        // A trailing one does not tie the next word to the text.
        let spans = vec![
            Span::new("x ", Style::default()),
            together("aa bb "),
            Span::new("cc", Style::default()),
        ];
        assert_eq!(texts(&wrap(&spans, 8)), vec!["x aa bb", "cc"]);
    }

    #[test]
    fn a_run_of_spaces_takes_the_first_style() {
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let spans = vec![
            Span::new("a ", Style::default()),
            Span::new(" b", bold.clone()),
        ];
        let lines = wrap(&spans, 20);
        assert_eq!(
            lines[0],
            vec![Span::new("a ", Style::default()), Span::new("b", bold)]
        );
    }

    #[test]
    fn breaks_lines_at_newlines() {
        let lines = wrap(&plain("one\ntwo"), 20);
        assert_eq!(texts(&lines), vec!["one", "two"]);
    }

    #[test]
    fn cuts_between_graphemes() {
        let cut_to = |text: &str, columns| -> String { style::text(&cut(plain(text), columns)) };
        // A heart made an emoji by a variation selector, two cells wide.
        assert_eq!(cut_to("a❤️b", 2), "a");
        assert_eq!(cut_to("a❤️b", 3), "a❤️");
        // An e with a combining acute accent, one cell.
        assert_eq!(cut_to("ée", 1), "é");
    }

    #[test]
    fn cuts_text_at_a_fraction_as_it_is_measured_packed() {
        let half = fraction(1, 1, 2);
        // Four letters at half size take two cells, not four.
        let kept = cut(vec![sized("abcdefg", half)], 2);
        assert_eq!(style::text(&kept), "abcd");
        assert_eq!(width(&kept), 2);
        // Where a letter more can take a cell less, the longest beginning
        // that fits is kept, and no longer one fits.
        let size = fraction(1, 5, 7);
        let text = "at educavj日k日日cmbtga日h日f";
        for max in 2..=25 {
            let kept = cut(vec![sized(text, size)], max);
            assert!(width(&kept) <= max, "{max}");
            let end = style::text(&kept).len();
            for (start, grapheme) in text.grapheme_indices(true) {
                let after = start + grapheme.len();
                if after > end {
                    assert!(size.width(&text[..after]) > max, "{max} {after}");
                }
            }
        }
    }

    #[test]
    fn tail_keeps_whole_graphemes() {
        // A heart made an emoji by a variation selector, two cells wide.
        assert_eq!(tail("ab❤️", 3), "…❤️");
        assert_eq!(tail("a❤️b", 3), "…b");
        // An e with a combining acute accent, one cell.
        assert_eq!(tail("abcé", 3), "…cé");
    }

    fn split_texts(text: &str, max: usize) -> Vec<String> {
        texts(&split(&plain(text), max))
    }

    #[test]
    fn splits_code_between_graphemes() {
        // A flag is two regional indicators, two cells together.
        assert_eq!(split_texts("ab🇫🇮", 3), vec!["ab", "🇫🇮"]);
        // An emoji joined from three, two cells wide.
        assert_eq!(split_texts("ab👨‍👩‍👧", 3), vec!["ab", "👨‍👩‍👧"]);
        // A heart made an emoji by a variation selector, two cells wide.
        assert_eq!(split_texts("a❤️", 2), vec!["a", "❤️"]);
        // A letter with a combining accent.
        assert_eq!(split_texts("abe\u{301}", 2), vec!["ab", "e\u{301}"]);
        // Spaces are kept.
        assert_eq!(split_texts("a  b", 2), vec!["a ", " b"]);
    }

    #[test]
    fn a_grapheme_wider_than_the_line_gets_a_line_of_its_own() {
        assert_eq!(split_texts("日本", 1), vec!["日", "本"]);
        assert_eq!(split_texts("a日", 1), vec!["a", "日"]);
    }

    #[test]
    fn a_leading_zero_width_grapheme_stays_with_what_follows() {
        assert_eq!(split_texts("\u{200b}ab", 1), vec!["\u{200b}a", "b"]);
    }

    fn sized(text: &str, size: TextSize) -> Span {
        Span::new(
            text,
            Style {
                size,
                ..Style::default()
            },
        )
    }

    fn fraction(scale: u8, numerator: u8, denominator: u8) -> TextSize {
        let fraction = Fraction::new(numerator, denominator).unwrap();
        TextSize::new(scale)
            .unwrap()
            .with_fraction(fraction, VerticalAlign::Top)
    }

    #[test]
    fn measures_text_at_a_fraction_packed() {
        let half = fraction(1, 1, 2);
        // Seven cells of text at half size take four.
        let lines = wrap(&[sized("aaa bbb", half)], 4);
        assert_eq!(texts(&lines), vec!["aaa bbb"]);
        let lines = wrap(&[sized("aaa bbb c", half)], 4);
        assert_eq!(texts(&lines), vec!["aaa bbb", "c"]);
        assert_eq!(
            texts(&split(&[sized("abcdefg", half)], 2)),
            vec!["abcd", "efg"]
        );
    }

    #[test]
    fn text_at_a_fraction_that_takes_fewer_cells_as_it_grows_still_wraps() {
        // Packing chooses the groups of the whole text together, so a
        // letter more can take a cell less.
        let size = fraction(1, 5, 7);
        for text in [
            "abcdefghijklmnopqrstuvwxyzabcd",
            "at educavj日k日日cmbtga日h日f",
        ] {
            for max in 2..=25 {
                for line in wrap(&[sized(text, size)], max) {
                    assert!(width(&line) <= max, "{text:?} {max}");
                }
                for line in split(&[sized(text, size)], max) {
                    assert!(width(&line) <= max, "{text:?} {max}");
                }
            }
        }
    }

    #[test]
    fn wrapping_text_at_a_fraction_never_overflows() {
        let sizes = [
            fraction(1, 1, 2),
            fraction(1, 2, 3),
            fraction(2, 7, 8),
            fraction(3, 1, 4),
            TextSize::new(2).unwrap(),
        ];
        // Words that pack in fewer cells apart than together, wide
        // characters that end a group early, and a mix of sizes.
        let texts = [
            "a a日 a a日 aa日日 a日a",
            "日本語 の テキスト 日本 語",
            "abc defgh ij klmnopqrstuvwxyz a b c",
        ];
        for size in sizes {
            for text in texts {
                let spans = [
                    sized(text, size),
                    Span::new(" normal ", Style::default()),
                    sized(text, size),
                ];
                for max in 1..=24 {
                    for line in wrap(&spans, max).iter().chain(&split(&spans, max)) {
                        // A grapheme wider than the line gets one of its own.
                        let alone =
                            line.len() == 1 && style::text(line).graphemes(true).count() == 1;
                        assert!(alone || width(line) <= max, "{max}: {line:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn fills_text_at_a_fraction_to_exactly_the_width() {
        let half = fraction(1, 1, 2);
        let mut spans = vec![sized("ab", half)];
        let style = spans[0].style.clone();
        fill(&mut spans, 4, &style);
        assert_eq!(spans, vec![sized("ab      ", half)]);
        assert_eq!(width(&spans), 4);
        // A space in a style of its own starts its own groups.
        let mut spans = vec![sized("abc", half)];
        let other = Style {
            bold: true,
            ..spans[0].style.clone()
        };
        fill(&mut spans, 5, &other);
        assert_eq!(width(&spans), 5);
        assert_eq!(spans[1].text, "      ");
    }

    #[test]
    fn measures_code_like_the_wrapper() {
        for text in ["ab🇫🇮cd", "a❤️b👨‍👩‍👧c", "日本語"] {
            for line in split(&[Span::new(text, Style::default())], 3) {
                assert!(width(&line) <= 3, "{line:?}");
            }
        }
    }
}
