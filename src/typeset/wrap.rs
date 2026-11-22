//! Word wrapping for styled text.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use std::ops::Range;

use crate::typeset::style::{Spaces, Span, Style, push, push_span};

enum Token {
    Word(Vec<Span>),
    /// One space, whose style says whether it collapses, and which shows what
    /// it does of the document.
    Space(Span),
    Break,
}

/// Display width in terminal cells, each span measured at its size.
pub fn width(spans: &[Span]) -> usize {
    (spans.iter()).map(span_width).sum()
}

/// The cells of one span: those of its image, or of its text at its size.
fn span_width(span: &Span) -> usize {
    match &span.embed {
        Some(embed) => usize::from(embed.placement.size.columns),
        None => span.style.size.width(&span.text),
    }
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

/// Where `split` starts each line it makes of `text` at the normal size in
/// one style, as bytes of `text`, without making the lines: `starts` gets
/// one for each line, the first 0.
pub fn starts(text: &str, max: usize, mut start: impl FnMut(usize)) {
    start(0);
    // Printable ASCII is a cell a byte.
    if text.bytes().all(|byte| (0x20..0x7f).contains(&byte)) {
        let max = max.max(1);
        (1..text.len().div_ceil(max)).for_each(|line| start(line * max));
        return;
    }
    // A span shows a control character as a character of its own, a cell
    // wide, which the text is wrapped as.
    let shown: String = (text.chars())
        .map(|c| match c.is_control() {
            true => char::REPLACEMENT_CHARACTER,
            false => c,
        })
        .collect();
    let mut chars = text.chars();
    let (mut used, mut at) = (0, 0);
    for grapheme in shown.graphemes(true) {
        let cells = grapheme.width();
        if used + cells > max && used > 0 {
            start(at);
            used = cells;
        } else {
            used += cells;
        }
        let of_text = chars.by_ref().take(grapheme.chars().count());
        at += of_text.map(char::len_utf8).sum::<usize>();
    }
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
        if span.embed.is_some() {
            // Never split; one wider than the line stands alone.
            let after = used + span_width(span);
            if after > max && used > 0 {
                lines.push(std::mem::take(line));
                used = 0;
            }
            line.push(span.clone());
            used += span_width(span);
            continue;
        }
        for (at, grapheme) in span.text.grapheme_indices(true) {
            let size = span.style.size;
            let mut after = match size.fraction() {
                None => used + size.width(grapheme),
                Some(_) => width_after(line, used, &[], &[Span::new(grapheme, span.style.clone())]),
            };
            if after > max && used > 0 {
                lines.push(std::mem::take(line));
                after = size.width(grapheme);
            }
            push_span(line, span.part(at..at + grapheme.len()));
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
        if span.embed.is_some() {
            if span_width(&span) <= left {
                left -= span_width(&span);
                kept.push(span);
            }
            continue;
        }
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
            span = span.part(0..end);
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

/// How many lines [`wrap`] makes of `text` alone, in a style whose spaces
/// collapse and whose text is at the normal size, without making them:
/// for a text with no character that a span shows otherwise, a control
/// character, a tab or a line break. It follows `wrap` step for step, so that
/// a long table is measured as it is laid out; a property test holds the two
/// together.
pub fn plain_rows(text: &str, max: usize) -> usize {
    debug_assert!(!text.chars().any(char::is_control), "{text:?}");
    let max = max.max(1);
    let (mut lines, mut used, mut gap, mut filled) = (0, 0, 0, false);
    for (at, piece) in text.split(' ').enumerate() {
        // A space before every piece but the first: it counts only after
        // text on the line, and once.
        if at > 0 && gap == 0 && used > 0 {
            gap = 1;
        }
        if piece.is_empty() {
            continue;
        }
        let word = piece.width();
        if used + gap + word <= max {
            used += gap + word;
        } else if word <= max {
            lines += usize::from(filled);
            used = word;
        } else {
            let first = piece
                .graphemes(true)
                .next()
                .map_or(0, UnicodeWidthStr::width);
            if used + gap + first <= max {
                used += gap;
            } else {
                lines += usize::from(filled);
                used = 0;
            }
            let mut place = |cells: usize| {
                let mut after = used + cells;
                if after > max && used > 0 {
                    lines += 1;
                    after = cells;
                }
                used = after;
            };
            // Printable ASCII is a cell a letter, and each letter a grapheme.
            match piece.is_ascii() {
                true => piece.bytes().for_each(|_| place(1)),
                false => piece.graphemes(true).for_each(|g| place(g.width())),
            }
        }
        (gap, filled) = (0, true);
    }
    lines + usize::from(filled || lines == 0)
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
            Token::Space(space) => {
                if !self.collapses(&space.style) {
                    push_span(&mut self.gap, space);
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
        push_span(line, span.clone());
    }
}

/// The first grapheme of `word`, in its style.
fn first(word: &[Span]) -> Vec<Span> {
    (word.iter())
        .find_map(|span| {
            if span.embed.is_some() {
                return Some(span.clone());
            }
            (span.text.graphemes(true).next()).map(|grapheme| span.part(0..grapheme.len()))
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
        if span.embed.is_some() {
            word.push(span.clone());
            continue;
        }
        for (range, style) in pieces(span, split_together) {
            let spaces_break = style.spaces != Spaces::KeepTogether;
            let is_break = |c: char| c == '\n' || (c == ' ' && spaces_break);
            let in_style = |bytes: Range<usize>| Span {
                style: style.clone(),
                ..span.part(bytes)
            };
            let mut at = range.start;
            while let Some(c) = span.text[at..range.end].chars().next() {
                if is_break(c) {
                    if !word.is_empty() {
                        tokens.push(Token::Word(std::mem::take(&mut word)));
                    }
                    tokens.push(if c == ' ' {
                        Token::Space(in_style(at..at + 1))
                    } else {
                        Token::Break
                    });
                    at += 1;
                } else {
                    let end = span.text[at..range.end]
                        .find(is_break)
                        .map_or(range.end, |found| at + found);
                    push_span(&mut word, in_style(at..end));
                    at = end;
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
fn pieces(span: &Span, split_together: bool) -> Vec<(Range<usize>, Style)> {
    if span.style.spaces != Spaces::KeepTogether {
        return vec![(0..span.text.len(), span.style.clone())];
    }
    let kept = Style {
        spaces: Spaces::Keep,
        ..span.style.clone()
    };
    if split_together {
        return vec![(0..span.text.len(), kept)];
    }
    let text = span.text.as_str();
    let start = text.len() - text.trim_start_matches(' ').len();
    let end = start.max(text.trim_end_matches(' ').len());
    vec![
        (0..start, kept.clone()),
        (start..end, span.style.clone()),
        (end..text.len(), kept),
    ]
}

#[cfg(test)]
mod props;

#[cfg(test)]
mod tests;
