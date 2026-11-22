use super::*;
use crate::typeset::images::ImageKey;

const CELL: (u16, u16) = (10, 20);

fn look(rows: u8) -> LevelLook {
    LevelLook {
        rows,
        font_px: 30.0,
        bold: true,
        italic: false,
        color: [0x12, 0x80, 0xf0],
        code: CodeLook {
            color: [0xe0, 0x20, 0xe0],
            background: Some([0x30, 0x30, 0x30]),
            padding: true,
        },
    }
}

fn drawn(text: &str, look: &LevelLook) -> (image::RgbaImage, Vec<u16>) {
    match draw_heading(&plain_words(text), look, CELL, &Fonts::fixture()) {
        Ok(Source::Text { image, breaks, .. }) => (image, breaks),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_heading_image_is_as_tall_as_its_rows() {
    let (image, _) = drawn("Kiri", &look(2));
    assert_eq!(image.height(), 40);
    assert!(image.width() > 0);
}

#[test]
fn a_heading_is_drawn_in_its_colour() {
    let look = look(2);
    let (image, _) = drawn("Kiri", &look);
    let [red, green, blue] = look.color;
    let colour = [red, green, blue, 255];
    assert!(image.pixels().any(|pixel| pixel.0 == colour));
    // The rest is transparent, for the heading's background to show.
    assert_eq!(image.get_pixel(0, 0).0[3], 0);
}

#[test]
fn without_fonts_a_heading_has_no_image() {
    let drawn = draw_heading(&plain_words("Kiri"), &look(2), CELL, &Fonts::none());
    assert_eq!(drawn, Err(no_font()));
}

#[test]
fn the_same_text_at_two_levels_has_two_keys() {
    let (one, two) = (
        ImageKey::heading(1, "Kiri", CELL),
        ImageKey::heading(2, "Kiri", CELL),
    );
    assert_ne!(one, two);
    assert_ne!(one, one.clone().in_document(1));
    assert_eq!(one.clone().in_document(1).document(), 1);
    assert_eq!(one.to_string(), "heading \"Kiri\"");
    // Drawn for other cells, its words start elsewhere.
    assert_ne!(one, ImageKey::heading(1, "Kiri", (8, 16)));
}

#[test]
fn markup_in_heading_text_is_escaped() {
    let (image, breaks) = drawn("a < b & c", &look(1));
    assert!(image.pixels().any(|pixel| pixel.0[3] > 0));
    assert_eq!(breaks.len(), 4);
}

#[test]
fn each_word_starts_on_a_whole_cell() {
    let (image, breaks) = drawn("One two three", &look(2));
    assert_eq!(breaks.len(), 2);
    assert!(breaks[0] > 0 && breaks[0] < breaks[1], "{breaks:?}");
    assert_eq!(image.width() % u32::from(CELL.0), 0);
    let columns = image.width() / u32::from(CELL.0);
    assert!(u32::from(breaks[1]) < columns, "{breaks:?} of {columns}");
}

#[test]
fn each_word_ends_before_the_next_starts() {
    let drawn = draw_heading(
        &plain_words("One two three"),
        &look(2),
        CELL,
        &Fonts::fixture(),
    );
    let Ok(Source::Text {
        image,
        breaks,
        ends,
    }) = drawn
    else {
        panic!("{drawn:?}");
    };
    let columns = (image.width() / u32::from(CELL.0)) as u16;
    assert_eq!(ends.len(), 3, "{ends:?}");
    let starts = [0, breaks[0], breaks[1]];
    for (word, (&start, &end)) in starts.iter().zip(&ends).enumerate() {
        assert!(start < end, "word {word}: {starts:?} {ends:?}");
    }
    assert!(ends[0] <= breaks[0] && ends[1] <= breaks[1], "{ends:?}");
    assert_eq!(ends[2], columns);
}

#[test]
fn a_word_without_ink_still_covers_a_cell() {
    // A zero-width space is a word of its own, and draws nothing.
    let drawn = draw_heading(
        &plain_words("Kiri \u{200B}"),
        &look(2),
        CELL,
        &Fonts::fixture(),
    );
    let Ok(Source::Text {
        image,
        breaks,
        ends,
    }) = drawn
    else {
        panic!("{drawn:?}");
    };
    assert_eq!(ends.len(), 2, "{ends:?}");
    assert!(ends[1] > breaks[0], "{breaks:?} {ends:?}");
    // The image reaches as far as that cell.
    assert_eq!(image.width(), u32::from(ends[1]) * u32::from(CELL.0));
}

/// `words` as heading words, each with the code span it is in.
fn words(words: &[(&str, Option<u16>)]) -> Vec<HeadingWord> {
    (words.iter())
        .map(|&(text, code)| HeadingWord {
            text: text.to_owned(),
            code,
            italic: false,
            glued: false,
        })
        .collect()
}

/// `words` drawn as `look(2)` says, with its starts and ends.
fn drawn_words(words: &[HeadingWord]) -> (image::RgbaImage, Vec<u16>, Vec<u16>) {
    let drawn = draw_heading(words, &look(2), CELL, &Fonts::fixture());
    match drawn {
        Ok(Source::Text {
            image,
            breaks,
            ends,
        }) => (image, std::iter::once(0).chain(breaks).collect(), ends),
        other => panic!("{other:?}"),
    }
}

/// The opaque colour `rgb`.
fn opaque([red, green, blue]: [u8; 3]) -> [u8; 4] {
    [red, green, blue, 255]
}

#[test]
fn a_code_word_is_drawn_on_its_background_in_its_colour() {
    let code = look(2).code;
    let (image, starts, ends) = drawn_words(&words(&[("Kiri", None), ("code", Some(0))]));
    let cells = |word: usize| {
        let x = u32::from(starts[word]) * 10..u32::from(ends[word]) * 10;
        let image = &image;
        x.flat_map(move |x| (0..40).map(move |y| image.get_pixel(x, y).0))
    };
    let background = opaque(code.background.unwrap());
    // On its background all over its cells, padding and all.
    let top = image.get_pixel(u32::from(starts[1]) * 10, 0).0;
    assert_eq!(top, background);
    assert!(cells(1).any(|pixel| pixel == opaque(code.color)));
    // The heading text on none.
    assert!(!cells(0).any(|pixel| pixel == background));
    assert!(!cells(0).any(|pixel| pixel == opaque(code.color)));
}

#[test]
fn code_is_padded_by_a_space_at_the_heading_size() {
    // Narrow cells, so that a space of the 30-pixel font takes more
    // than one.
    let cell = (4, 20);
    let words = words(&[("a", None), ("code", Some(0))]);
    let drawn = draw_heading(&words, &look(2), cell, &Fonts::fixture());
    let Ok(Source::Text { image, breaks, .. }) = drawn else {
        panic!("{drawn:?}");
    };
    let start = u32::from(breaks[0]) * 4;
    let ink = (start..image.width())
        .find(|&x| (0..40).any(|y| image.get_pixel(x, y).0 == opaque(look(2).code.color)));
    // DejaVu Sans, the fixture's only font, has a space of 0.32 em:
    // 9.5 pixels, three cells.
    assert!(ink.unwrap() >= start + 3 * 4, "{ink:?} from {start}");
}

#[test]
fn code_without_padding_has_its_background_from_its_first_cell() {
    let mut look = look(2);
    let words = words(&[("a", None), ("code", Some(0))]);
    let drawn = |look: &LevelLook| match draw_heading(&words, look, CELL, &Fonts::fixture()) {
        Ok(Source::Text { image, breaks, .. }) => (image, breaks),
        other => panic!("{other:?}"),
    };
    let (padded, _) = drawn(&look);
    look.code.padding = false;
    let (image, breaks) = drawn(&look);
    let start = u32::from(breaks[0]) * 10;
    let background = opaque(look.code.background.unwrap());
    assert_eq!(image.get_pixel(start, 0).0, background);
    assert_eq!(image.get_pixel(image.width() - 1, 0).0, background);
    // Narrower by the padding on both sides.
    assert!(
        image.width() < padded.width(),
        "{} {}",
        image.width(),
        padded.width()
    );
}

#[test]
fn a_code_span_over_two_words_has_no_gap_in_its_background() {
    let background = opaque(look(2).code.background.unwrap());
    let row = |image: &image::RgbaImage, x: std::ops::Range<u32>| {
        x.map(|x| image.get_pixel(x, 0).0).collect::<Vec<_>>()
    };
    let (image, starts, ends) = drawn_words(&words(&[("a", Some(0)), ("b", Some(0))]));
    let span = u32::from(starts[0]) * 10..u32::from(ends[1]) * 10;
    assert!(row(&image, span).iter().all(|&pixel| pixel == background));
    // Two spans of code have the page between them.
    let (image, _, ends) = drawn_words(&words(&[("a", Some(0)), ("b", Some(1))]));
    let gap = u32::from(ends[0]) * 10..u32::from(ends[0]) * 10 + 1;
    assert!(
        row(&image, gap).iter().all(|pixel| pixel[3] == 0),
        "{ends:?}"
    );
}

#[test]
fn without_a_code_background_none_is_drawn() {
    let mut look = look(2);
    look.code.background = None;
    let words = words(&[("code", Some(0))]);
    let drawn = draw_heading(&words, &look, CELL, &Fonts::fixture());
    let Ok(Source::Text { image, .. }) = drawn else {
        panic!("{drawn:?}");
    };
    assert_eq!(image.get_pixel(0, 0).0[3], 0);
    assert!(
        image
            .pixels()
            .any(|pixel| pixel.0 == opaque(look.code.color))
    );
}

#[test]
fn the_words_of_inline_code_are_marked_with_their_span() {
    let content = crate::typeset::parse::parse("# a `b c` *d* `e`f, `g`\n");
    let crate::typeset::model::Block::Heading { content, .. } = &content.blocks[0] else {
        panic!("a heading");
    };
    let marked: Vec<(String, Option<u16>, bool)> = (heading_words(content).into_iter())
        .map(|word| (word.text, word.code, word.glued))
        .collect();
    let expected = [
        ("a", None, false),
        ("b", Some(0), false),
        ("c", Some(0), false),
        ("d", None, false),
        // Text against code is a word of its own, glued to it.
        ("e", Some(1), false),
        ("f,", None, true),
        ("g", Some(2), false),
    ];
    let expected: Vec<(String, Option<u16>, bool)> = (expected.iter())
        .map(|&(text, code, glued)| (text.to_owned(), code, glued))
        .collect();
    assert_eq!(marked, expected);
}

#[test]
fn a_glued_word_starts_where_the_one_before_it_ends() {
    let mut words = plain_words("a code ,");
    words[1].code = Some(0);
    words[2].glued = true;
    let (_, starts, ends) = drawn_words(&words);
    // No space between the code's background and the comma.
    assert_eq!(starts[2], ends[1], "{starts:?} {ends:?}");
}

#[test]
fn a_key_is_marked_as_inline_code_is() {
    let content = crate::typeset::parse::parse("# Press <kbd>Ctrl `C`</kbd> `now`\n");
    let crate::typeset::model::Block::Heading { content, .. } = &content.blocks[0] else {
        panic!("a heading");
    };
    let marked: Vec<(String, Option<u16>)> = (heading_words(content).into_iter())
        .map(|word| (word.text, word.code))
        .collect();
    // Code inside the key is part of the key.
    let expected = [
        ("Press", None),
        ("Ctrl", Some(0)),
        ("C", Some(0)),
        ("now", Some(1)),
    ];
    let expected: Vec<(String, Option<u16>)> = (expected.iter())
        .map(|&(text, code)| (text.to_owned(), code))
        .collect();
    assert_eq!(marked, expected);
}

#[test]
fn emphasised_words_are_marked_as_italic() {
    let marked = |source: &str| {
        let document = crate::typeset::parse::parse(source);
        let crate::typeset::model::Block::Heading { content, .. } = &document.blocks[0] else {
            panic!("a heading");
        };
        (heading_words(content).into_iter())
            .map(|word| (word.text, word.italic, word.glued))
            .collect::<Vec<_>>()
    };
    let expected = |words: &[(&str, bool, bool)]| {
        (words.iter())
            .map(|&(text, italic, glued)| (text.to_owned(), italic, glued))
            .collect::<Vec<_>>()
    };
    // Text going into or out of emphasis is a word of its own, glued
    // to the one before it; strong is upright.
    assert_eq!(
        marked("# a *b `c`* d*e* **f**\n"),
        expected(&[
            ("a", false, false),
            ("b", true, false),
            ("c", true, false),
            ("d", false, false),
            ("e", true, true),
            ("f", false, false),
        ])
    );
    assert_eq!(
        marked("# Why *this*?\n"),
        expected(&[
            ("Why", false, false),
            ("this", true, false),
            ("?", false, true),
        ])
    );
}

#[test]
fn an_italic_word_keeps_its_ink_inside_its_cells() {
    // Letters that lean out past their advance on both sides.
    let upright = plain_words("a fjy b");
    let mut words = upright.clone();
    words[1].italic = true;
    let (image, starts, ends) = drawn_words(&words);
    // In italics indeed: the fixture fonts have an oblique face.
    assert_ne!(drawn_words(&upright).0, image);
    let inked = |x: u32| (0..40).any(|y| image.get_pixel(x, y).0[3] > 0);
    // Nothing in the gaps between the words, where a line of the
    // image would cut it off.
    for word in 0..2 {
        let gap = u32::from(ends[word]) * 10..u32::from(starts[word + 1]) * 10;
        assert!(!gap.clone().any(inked), "{gap:?}: {starts:?} {ends:?}");
    }
    // Glued right after an upright word, as in `*this*?` or `d*e*`,
    // with no gap: the italic word leaves the cells before it as the
    // upright word alone draws them, and its ink stays in its own.
    let mut glued = plain_words("ad jfy b");
    glued[1].italic = true;
    glued[1].glued = true;
    let (image, starts, ends) = drawn_words(&glued);
    let mut without = glued.clone();
    without[1].text = "\u{200B}".into();
    let (alone, alone_starts, _) = drawn_words(&without);
    assert_eq!(alone_starts[1], starts[1], "{starts:?} {alone_starts:?}");
    let column = |image: &image::RgbaImage, x: u32| -> Vec<[u8; 4]> {
        (0..40).map(|y| image.get_pixel(x, y).0).collect()
    };
    for x in 0..u32::from(starts[1]) * 10 {
        assert_eq!(
            column(&image, x),
            column(&alone, x),
            "column {x}: {starts:?}"
        );
    }
    let inked = |x: u32| (0..40).any(|y| image.get_pixel(x, y).0[3] > 0);
    let gap = u32::from(ends[1]) * 10..u32::from(starts[2]) * 10;
    assert!(!gap.clone().any(inked), "{gap:?}: {starts:?} {ends:?}");
}

#[test]
fn an_italic_word_is_drawn_in_italics() {
    let mut words = plain_words("Kiri code");
    words[1].italic = true;
    words[1].code = Some(0);
    let texts: Vec<Drawn> = words.iter().map(|word| Drawn::new(0.0, word)).collect();
    let svg = heading_svg(&texts, &[], &look(2), (100, 40), 30.0);
    let styles: Vec<&str> = (svg.split("font-style=\"").skip(1))
        .map(|rest| &rest[..rest.find('"').unwrap()])
        .collect();
    assert_eq!(styles, ["normal", "italic"], "{svg}");
}

#[test]
fn heading_words_split_on_whitespace() {
    let words = plain_words(" One\ttwo  three\n");
    let texts: Vec<&str> = words.iter().map(|word| word.text.as_str()).collect();
    assert_eq!(texts, ["One", "two", "three"]);
}
