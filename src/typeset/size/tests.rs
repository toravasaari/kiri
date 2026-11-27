use super::*;

#[test]
fn scales_are_one_to_seven() {
    assert_eq!(TextSize::new(0), None);
    assert_eq!(TextSize::new(1), Some(TextSize::NORMAL));
    assert_eq!(TextSize::new(7).map(TextSize::scale), Some(7));
    assert_eq!(TextSize::new(8), None);
}

#[test]
fn fractions_are_below_one_with_a_denominator_up_to_fifteen() {
    assert_eq!(Fraction::new(0, 2), None);
    assert_eq!(Fraction::new(2, 2), None);
    assert_eq!(Fraction::new(3, 2), None);
    assert_eq!(Fraction::new(1, 16), None);
    let fraction = Fraction::new(7, 15).unwrap();
    assert_eq!((fraction.numerator(), fraction.denominator()), (7, 15));
}

#[test]
fn a_numerator_is_at_most_seven() {
    // Packed text takes the numerator's cells, and an escape sequence
    // takes at most seven.
    assert_eq!(Fraction::new(8, 9), None);
    assert_eq!(Fraction::new(14, 15), None);
    assert!(Fraction::new(7, 8).is_some());
}

#[test]
fn a_fraction_keeps_the_scale() {
    let half = Fraction::new(1, 2).unwrap();
    let size = TextSize::new(2)
        .unwrap()
        .with_fraction(half, VerticalAlign::Centre);
    assert_eq!(size.scale(), 2);
    assert_eq!(size.fraction(), Some(half));
    assert_eq!(size.align(), VerticalAlign::Centre);
}

fn half() -> Fraction {
    Fraction::new(1, 2).unwrap()
}

fn at(scale: u8, numerator: u8, denominator: u8) -> TextSize {
    let fraction = Fraction::new(numerator, denominator).unwrap();
    TextSize::new(scale)
        .unwrap()
        .with_fraction(fraction, VerticalAlign::Top)
}

fn groups(fraction: Fraction, text: &str) -> Vec<(&str, u8)> {
    (fraction.groups(text).into_iter())
        .map(|group| (group.text, group.cells))
        .collect()
}

#[test]
fn a_group_takes_the_cells_of_its_text_at_the_fraction_rounded_up() {
    assert_eq!(groups(half(), "abcde"), [("abcde", 3)]);
    let three_quarters = Fraction::new(3, 4).unwrap();
    assert_eq!(groups(three_quarters, "abcdefghi"), [("abcdefghi", 7)]);
    assert_eq!(groups(half(), ""), []);
    // Text of no width is a group of no cells.
    assert_eq!(groups(half(), "\u{200b}"), [("\u{200b}", 0)]);
}

#[test]
fn a_group_never_splits_a_grapheme() {
    // Twenty cells of text at a half take two groups at least.
    let accents = "e\u{301}".repeat(20);
    let groups = half().groups(&accents);
    assert!(groups.len() > 1);
    assert!(groups.iter().all(|group| group.text.starts_with('e')));
    // A flag is two regional indicators, two cells together.
    let flags = "🇫🇮".repeat(10);
    let groups = half().groups(&flags);
    assert!(groups.len() > 1);
    assert!(
        groups
            .iter()
            .all(|group| group.text.len() % "🇫🇮".len() == 0)
    );
}

fn aligned(fraction: Fraction, text: &str) -> Vec<(&str, u8, HorizontalAlign)> {
    (fraction.groups(text).into_iter())
        .map(|group| (group.text, group.cells, group.align))
        .collect()
}

#[test]
fn a_group_takes_up_to_seven_cells() {
    // `a heading` takes 6 3/4 cells at 3/4, so one group of seven, and
    // its gap is at the end of the text.
    let three_quarters = Fraction::new(3, 4).unwrap();
    assert_eq!(
        aligned(three_quarters, "a heading"),
        [("a heading", 7, HorizontalAlign::Left)]
    );
}

#[test]
fn text_starts_at_the_left_of_its_first_group() {
    // A heading starts at the margin, as the text below it does, so the
    // first group ends at a space rather than inside `three`.
    let seven_eighths = Fraction::new(7, 8).unwrap();
    assert_eq!(
        aligned(seven_eighths, "Level three"),
        [
            ("Level", 5, HorizontalAlign::Left),
            (" three", 6, HorizontalAlign::Left)
        ]
    );
}

#[test]
fn text_between_spaces_leaves_even_gaps_on_both_sides() {
    // Inline code in a heading has a no-break space at each end, and its
    // background shows the gaps on both sides.
    let seven_eighths = Fraction::new(7, 8).unwrap();
    assert_eq!(
        aligned(seven_eighths, "\u{a0}code\u{a0}"),
        [("\u{a0}code\u{a0}", 6, HorizontalAlign::Centre)]
    );
}

// kitty draws text at a fraction a little narrower than its cells, and
// the rest of the cells shows as a gap. Gaps go between words.
#[test]
fn two_groups_meet_inside_a_word_without_a_gap() {
    use HorizontalAlign::{Left, Right};
    let three_quarters = Fraction::new(3, 4).unwrap();
    assert_eq!(
        aligned(three_quarters, "abcdefghijkl"),
        [("abcd", 3, Right), ("efghijkl", 6, Left)]
    );
    let seven_eighths = Fraction::new(7, 8).unwrap();
    assert_eq!(
        aligned(seven_eighths, "abcdefghij"),
        [("abcdefgh", 7, Right), ("ij", 2, Left)]
    );
}

#[test]
fn no_gap_shows_inside_a_word_that_two_groups_hold() {
    use HorizontalAlign::{Left, Right};
    for (numerator, denominator) in [(1, 2), (2, 3), (3, 4), (5, 6), (5, 8), (7, 8)] {
        let fraction = Fraction::new(numerator, denominator).unwrap();
        for text in texts(&["a", " "], 10) {
            let groups = fraction.groups(&text);
            let mut at = 0;
            for pair in groups.windows(2) {
                at += pair[0].text.len();
                let inside = !text[..at].ends_with(' ') && !text[at..].starts_with(' ');
                let meet = (pair[0].align, pair[1].align) == (Right, Left);
                assert!(!inside || meet, "{fraction:?} {text:?} {groups:?}");
            }
        }
    }
}

#[test]
fn only_a_word_too_long_for_two_groups_has_a_gap_inside() {
    use HorizontalAlign::{Left, Right};
    // At a half a group holds fourteen letters, and the first starts at
    // the margin.
    assert_eq!(
        aligned(half(), "abcdefghijklmnopqrstuvwxyzabcdefgh"),
        [
            ("abcdefghijklmn", 7, Left),
            ("opqrst", 3, Right),
            ("uvwxyzabcdefgh", 7, Left)
        ]
    );
}

#[test]
fn the_spare_cells_spread_over_the_spaces() {
    use HorizontalAlign::{Centre, Left, Right};
    let three_quarters = Fraction::new(3, 4).unwrap();
    assert_eq!(
        aligned(three_quarters, "Several words in a heading at level four"),
        [
            ("Several ", 6, Left),
            ("word", 3, Right),
            ("s", 1, Left),
            (" in ", 3, Left),
            ("a", 1, Centre),
            (" heading", 6, Centre),
            (" at ", 3, Right),
            ("level", 4, Centre),
            (" four", 4, Left)
        ]
    );
}

/// Every text of up to `length` graphemes from `alphabet`.
fn texts(alphabet: &[&str], length: usize) -> Vec<String> {
    let mut texts = vec![String::new()];
    let mut last = vec![String::new()];
    for _ in 0..length {
        last = (last.iter())
            .flat_map(|text| {
                alphabet
                    .iter()
                    .map(move |grapheme| format!("{text}{grapheme}"))
            })
            .collect();
        texts.extend(last.iter().cloned());
    }
    texts
}

#[test]
fn groups_hold_the_text_in_order_in_at_most_seven_cells() {
    let fractions = [(1, 2), (2, 3), (3, 4), (5, 6), (5, 8), (7, 8), (7, 12)];
    let texts = [
        texts(&["a", " "], 9),
        texts(&["a", " ", "日", "\u{200b}"], 5),
    ]
    .concat();
    for (numerator, denominator) in fractions {
        let fraction = Fraction::new(numerator, denominator).unwrap();
        for text in &texts {
            let groups = fraction.groups(text);
            let joined: String = groups.iter().map(|group| group.text).collect();
            assert_eq!(&joined, text, "{fraction:?}");
            for group in &groups {
                let cells = (group.text.width() * usize::from(numerator))
                    .div_ceil(usize::from(denominator));
                assert!(!group.text.is_empty(), "{fraction:?} {text:?}");
                assert_eq!(usize::from(group.cells), cells, "{fraction:?} {text:?}");
                assert!(group.cells <= 7, "{fraction:?} {text:?}");
            }
        }
    }
}

#[test]
fn text_is_as_wide_as_its_groups_at_its_scale() {
    assert_eq!(TextSize::NORMAL.width("abc"), 3);
    assert_eq!(TextSize::new(3).unwrap().width("日本"), 12);
    assert_eq!(
        TextSize::NORMAL
            .with_fraction(half(), VerticalAlign::Top)
            .width("abc"),
        2
    );
    assert_eq!(at(2, 7, 8).width("abcdefgh"), 14);
    assert_eq!(at(2, 7, 8).width("abcdefghi"), 16);
    assert_eq!(at(1, 1, 2).width("a日b"), 2);
    assert_eq!(at(1, 1, 2).width(""), 0);
}

/// The size nearest to `value` as scale, numerator and denominator.
fn nearest(value: f64) -> Option<(u8, u8, u8)> {
    let size = TextSize::nearest(value, VerticalAlign::Bottom)?;
    Some(match size.fraction() {
        Some(fraction) => {
            assert_eq!(size.align(), VerticalAlign::Bottom);
            (size.scale(), fraction.numerator(), fraction.denominator())
        }
        None => (size.scale(), 1, 1),
    })
}

#[test]
fn a_decimal_size_rounds_to_the_nearest_drawable_fraction() {
    // The scale is the whole number at or above the value, so 1.71
    // takes two rows, never three.
    assert_eq!(nearest(1.71), Some((2, 6, 7)));
    assert_eq!(nearest(2.5), Some((3, 5, 6)));
    assert_eq!(nearest(1.75), Some((2, 7, 8)));
    assert_eq!(nearest(1.25), Some((2, 5, 8)));
    assert_eq!(nearest(0.875), Some((1, 7, 8)));
    assert_eq!(nearest(0.5), Some((1, 1, 2)));
    // 0.65 would be 9/14, but a numerator is at most 7.
    assert_eq!(nearest(1.3), Some((2, 7, 11)));
    // A tie goes to the smaller denominator: 2/4 is 1/2.
    assert_eq!(nearest(1.0 + 1.0 / 3.0), Some((2, 2, 3)));
}

#[test]
fn a_whole_size_has_no_fraction() {
    assert_eq!(nearest(3.0), Some((3, 1, 1)));
    assert_eq!(nearest(7.0), Some((7, 1, 1)));
    assert_eq!(nearest(1.0), Some((1, 1, 1)));
    assert_eq!(
        TextSize::nearest(3.0, VerticalAlign::Bottom),
        TextSize::new(3)
    );
    // Nearer to the whole scale than to any fraction of it.
    assert_eq!(nearest(1.98), Some((2, 1, 1)));
}

#[test]
fn a_decimal_size_is_from_a_half_to_seven() {
    for value in [0.49, 7.01, 0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            TextSize::nearest(value, VerticalAlign::Top),
            None,
            "{value}"
        );
    }
}

/// `size` as scale, numerator and denominator.
fn parts(size: TextSize) -> (u8, u8, u8) {
    size.fraction().map_or((size.scale(), 1, 1), |fraction| {
        (size.scale(), fraction.numerator(), fraction.denominator())
    })
}

#[test]
fn a_size_times_a_factor_keeps_its_scale() {
    let two_thirds = |size: TextSize| parts(size.times(2.0 / 3.0, VerticalAlign::Top));
    assert_eq!(two_thirds(TextSize::NORMAL), (1, 2, 3));
    assert_eq!(two_thirds(TextSize::new(2).unwrap()), (2, 2, 3));
    // 5/6 · 2/3 is 5/9; 7/8 · 2/3 is 7/12; 5/8 · 2/3 is 5/12.
    assert_eq!(two_thirds(at(3, 5, 6)), (3, 5, 9));
    assert_eq!(two_thirds(at(2, 7, 8)), (2, 7, 12));
    assert_eq!(two_thirds(at(2, 3, 4)), (2, 1, 2));
    assert_eq!(two_thirds(at(2, 5, 8)), (2, 5, 12));
    // 7/11 · 2/3 is 14/33, which cannot be drawn: 3/7 is nearest.
    assert_eq!(two_thirds(at(2, 7, 11)), (2, 3, 7));
    let size = TextSize::NORMAL.times(2.0 / 3.0, VerticalAlign::Bottom);
    assert_eq!(size.align(), VerticalAlign::Bottom);
}

#[test]
fn the_normal_size_has_no_fraction() {
    assert_eq!(TextSize::NORMAL.scale(), 1);
    assert_eq!(TextSize::NORMAL.fraction(), None);
    assert_eq!(TextSize::NORMAL.align(), VerticalAlign::Top);
}

#[test]
fn groups_say_where_they_start_and_cover_the_text() {
    let size = at(2, 1, 2);
    for text in [
        "hello, world",
        "a b c d e f g h i j",
        "日本語のテキスト",
        "e\u{301}x",
    ] {
        let groups = size.groups(text);
        let mut next = 0;
        for group in &groups {
            assert_eq!(group.at, next, "{text:?}: {groups:?}");
            assert_eq!(&text[group.at..group.at + group.text.len()], group.text);
            next += group.text.len();
        }
        assert_eq!(next, text.len(), "{text:?}: the groups cover the text");
    }
}

/// Spaces at every size kiri can draw take cells that grow with their
/// number: at least one, and at least the fraction of their number. So the
/// loop that adds spaces while they fit in a room, `Layout::centre_row`,
/// ends. A scale only multiplies the cells, so the fractions are tried at
/// the normal scale.
#[test]
fn spaces_take_more_cells_the_more_there_are_at_every_size() {
    let mut sizes: Vec<TextSize> = (1..=7).filter_map(TextSize::new).collect();
    for denominator in 2..=15 {
        for numerator in 1..=7 {
            if let Some(fraction) = Fraction::new(numerator, denominator) {
                let size = TextSize::NORMAL.with_fraction(fraction, VerticalAlign::Bottom);
                sizes.push(size);
                sizes.push(size.as_one_group(HorizontalAlign::Left));
            }
        }
    }
    for size in sizes {
        for n in 1..=16 {
            let cells = size.width(&" ".repeat(n));
            let least = (size.value() * n as f64).floor() as usize;
            assert!(cells >= least.max(1), "{size:?} {n}: {cells}");
        }
    }
}
