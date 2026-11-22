use super::*;
use crate::typeset::size::{Fraction, TextSize, VerticalAlign};
use crate::typeset::style;

fn plain(text: &str) -> Vec<Span> {
    vec![Span::new(text, Style::default())]
}

fn texts(lines: &[Vec<Span>]) -> Vec<String> {
    lines.iter().map(|line| style::text(line)).collect()
}

/// A span that is the cells of an image `columns` wide, with `text`
/// for a search to find.
fn embed(text: &str, columns: u16) -> Span {
    use crate::typeset::images::ImageKey;
    use crate::typeset::images::size::{Placement, Shows, Size};
    let size = Size {
        width: u32::from(columns) * 10,
        height: 20,
        columns,
        rows: 1,
    };
    let placement = Placement::block(ImageKey::from("e.png"), size, Shows::Image);
    let embed = style::Embed::new(std::rc::Rc::new(placement), 0, 1);
    Span::embed(text, Style::default(), embed)
}

fn embeds(lines: &[Vec<Span>]) -> Vec<usize> {
    let count = |line: &Vec<Span>| line.iter().filter(|span| span.embed.is_some()).count();
    lines.iter().map(count).collect()
}

#[test]
fn an_embed_counts_the_columns_of_its_image_whatever_its_text() {
    assert_eq!(width(&[embed("an alt text", 3)]), 3);
    assert_eq!(width(&[embed("", 3), Span::new("ab", Style::default())]), 5);
}

#[test]
fn an_embed_is_kept_and_never_merged_with_the_text_around_it() {
    let spans = vec![
        Span::new("a", Style::default()),
        embed("x", 2),
        embed("y", 2),
    ];
    let lines = wrap(&spans, 20);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].len(), 3);
    assert_eq!(embeds(&lines), [2]);
    let mut merged = Vec::new();
    push(&mut merged, "a", &Style::default());
    push(&mut merged, "b", &Style::default());
    assert_eq!(merged.len(), 1);
}

#[test]
fn an_embed_is_part_of_the_word_around_it_and_its_text_is_not_scanned_for_spaces() {
    // "a", the embed, then ".": one word, so the line breaks before "a".
    let spans = vec![
        Span::new("xx a", Style::default()),
        embed("two words", 2),
        Span::new(".", Style::default()),
    ];
    let lines = wrap(&spans, 6);
    assert_eq!(lines.len(), 2);
    assert_eq!(style::text(&lines[0]), "xx");
    assert_eq!(width(&lines[1]), 4);
    assert_eq!(embeds(&lines), [0, 1]);
    // Spaces around it still break.
    let spans = vec![
        Span::new("a ", Style::default()),
        embed("e", 2),
        Span::new(" b", Style::default()),
    ];
    assert_eq!(wrap(&spans, 20).len(), 1);
    assert_eq!(wrap(&spans, 3).len(), 3);
}

#[test]
fn an_embed_wider_than_the_line_stands_alone_and_is_not_split() {
    let spans = vec![
        Span::new("ab ", Style::default()),
        embed("wide", 8),
        Span::new(" cd", Style::default()),
    ];
    let lines = wrap(&spans, 5);
    assert_eq!(lines.len(), 3);
    assert_eq!(embeds(&lines), [0, 1, 0]);
    assert_eq!(width(&lines[1]), 8);
    let lines = split(&[embed("wide", 8), Span::new("ab", Style::default())], 5);
    assert_eq!(embeds(&lines), [1, 0]);
    let lines = split(&[Span::new("abcd", Style::default()), embed("e", 3)], 5);
    assert_eq!(embeds(&lines), [0, 1]);
}

#[test]
fn cut_keeps_an_embed_that_fits_and_drops_one_that_does_not() {
    let spans = vec![Span::new("ab", Style::default()), embed("", 3)];
    assert_eq!(embeds(&[cut(spans.clone(), 5)]), [1]);
    assert_eq!(embeds(&[cut(spans, 4)]), [0]);
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
                    let alone = line.len() == 1 && style::text(line).graphemes(true).count() == 1;
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

/// Spans of `parts` that show the text they make up, `parts` in order:
/// text number 0, each byte of the shown text the same byte of the
/// document. `doc` is that text.
fn shown_as_written(parts: &[(&str, Style)]) -> (Vec<Span>, String) {
    let mut spans = Vec::new();
    let mut doc = String::new();
    for (text, style) in parts {
        let start = doc.len() as u32;
        doc.push_str(text);
        style::push_at(
            &mut spans,
            text,
            style,
            style::Origin::exact(0, start..doc.len() as u32),
        );
    }
    (spans, doc)
}

/// Asserts that each byte of `lines` shows the byte of `doc` that it is.
fn assert_each_byte_shows_itself(lines: &[Vec<Span>], doc: &str, what: &str) {
    for (number, line) in lines.iter().enumerate() {
        for span in line.iter().filter(|span| span.embed.is_none()) {
            for origin in &span.origins {
                assert!(
                    origin.shown.end as usize <= span.text.len(),
                    "{what}: line {number}: {origin:?} reaches past {:?}",
                    span.text
                );
            }
            for (byte, shown) in span.text.bytes().enumerate() {
                let at = span
                    .origins
                    .of(byte)
                    .and_then(|origin| origin.at(byte as u32));
                let Some(at) = at else {
                    panic!(
                        "{what}: line {number}: byte {byte} of {:?} shows nothing",
                        span.text
                    );
                };
                assert_eq!(
                    doc.as_bytes()[at as usize],
                    shown,
                    "{what}: line {number}: {:?}",
                    span.text
                );
            }
        }
    }
}

fn written() -> Vec<(&'static str, Style)> {
    let code = Style {
        spaces: Spaces::KeepTogether,
        bold: true,
        ..Style::default()
    };
    let plain = Style::default();
    vec![
        ("Hello wide world, a few ", plain.clone()),
        ("code with spaces", code),
        (" and 日本語のテキスト and 🇫🇮 flags ", plain.clone()),
        ("end", plain),
    ]
}

#[test]
fn wrap_keeps_what_every_byte_shows_of_the_document() {
    let (spans, doc) = shown_as_written(&written());
    for max in 1..=30 {
        assert_each_byte_shows_itself(&wrap(&spans, max), &doc, &format!("wrap {max}"));
    }
}

#[test]
fn split_keeps_what_every_byte_shows_of_the_document() {
    let (spans, doc) = shown_as_written(&written());
    for max in 1..=30 {
        assert_each_byte_shows_itself(&split(&spans, max), &doc, &format!("split {max}"));
    }
}

#[test]
fn cut_keeps_what_every_byte_shows_of_the_document() {
    let (spans, doc) = shown_as_written(&written());
    for max in 0..=40 {
        let kept = cut(spans.clone(), max);
        assert_each_byte_shows_itself(&[kept], &doc, &format!("cut {max}"));
    }
}

#[test]
fn text_at_a_fraction_and_with_embeds_keeps_what_it_shows() {
    let half = fraction(1, 1, 2);
    let small = Style {
        size: half,
        ..Style::default()
    };
    let parts = [("one two three ", small.clone()), ("four five six", small)];
    let (spans, doc) = shown_as_written(&parts);
    for max in 1..=12 {
        assert_each_byte_shows_itself(&wrap(&spans, max), &doc, &format!("wrap {max}"));
        assert_each_byte_shows_itself(&split(&spans, max), &doc, &format!("split {max}"));
    }
    // An embed goes through whole, with what it shows.
    let mut with_embed = Vec::new();
    style::push_at(
        &mut with_embed,
        "ab ",
        &Style::default(),
        style::Origin::exact(0, 0..3),
    );
    let mut picture = embed("alt", 2);
    picture.origins = {
        let mut one = Vec::new();
        style::push_at(
            &mut one,
            "alt",
            &Style::default(),
            style::Origin::picture(0, 3..6),
        );
        one.remove(0).origins
    };
    with_embed.push(picture);
    let lines = wrap(&with_embed, 3);
    let kept = lines
        .iter()
        .flatten()
        .find(|span| span.embed.is_some())
        .expect("the embed");
    assert!(kept.origins.of(0).is_some_and(|origin| origin.picture));
}

#[test]
fn the_spaces_a_line_leaves_out_leave_no_origin() {
    let (spans, doc) = shown_as_written(&[("aaa   bbb", Style::default())]);
    let lines = wrap(&spans, 3);
    assert_eq!(texts(&lines), ["aaa", "bbb"]);
    // The second word is the bytes 6 to 8, and nothing shows the spaces
    // that were left out.
    let shown_at: Vec<u32> = lines[1]
        .iter()
        .flat_map(|span| {
            (0..span.text.len()).filter_map(|b| span.origins.of(b).and_then(|o| o.at(b as u32)))
        })
        .collect();
    assert_eq!(shown_at, [6, 7, 8]);
    assert_each_byte_shows_itself(&lines, &doc, "wrap");
}

#[test]
fn a_whole_is_whole_on_both_lines_when_a_word_of_it_is_split() {
    let mut spans = Vec::new();
    style::push_at(
        &mut spans,
        "abcdef",
        &Style::default(),
        style::Origin::whole(0, 10..13),
    );
    for lines in [wrap(&spans, 3), split(&spans, 3)] {
        assert_eq!(texts(&lines), ["abc", "def"]);
        for line in &lines {
            let origin = line[0].origins.iter().next().expect("an origin");
            assert_eq!(
                (origin.doc.clone(), origin.exact),
                (10..13, false),
                "{line:?}"
            );
        }
    }
}
