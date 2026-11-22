use super::*;
use image::RgbaImage;

const LOOK: Look = Look {
    color: [0xdd; 3],
    ..Look::PLAIN
};

fn code(text: &str) -> Run {
    Run {
        code: true,
        ..Run::new(text)
    }
}

fn paragraph(runs: &[Run], width: f32) -> Paragraph {
    lay_out(&Fonts::fixture(), runs, &LOOK, width)
}

#[test]
fn lines_are_one_and_a_half_times_the_font_size_apart() {
    let text = "Reading mode lays text out in a proportional font. ".repeat(4);
    let laid = paragraph(&[Run::new(&text)], 300.0);
    let lines = laid.lines();
    assert!(lines.len() > 2, "{lines:?}");
    for (above, below) in lines.iter().zip(&lines[1..]) {
        assert_eq!(below.top - above.top, 24.0);
        assert_eq!(below.baseline - above.baseline, 24.0);
    }
    assert_eq!(laid.height(), 24.0 * lines.len() as f32);
    assert_eq!(laid.first_baseline(), lines[0].baseline);
}

/// `lines` lines of two coloured words each, one run to a word, with a
/// run of the line break between two lines: as highlighted code is.
fn highlighted(lines: usize) -> Vec<Run> {
    let word = |text: &str, grey: u8| Run {
        color: Some([grey; 3]),
        ..Run::new(text)
    };
    let mut runs = Vec::new();
    for line in 0..lines {
        if line > 0 {
            runs.push(Run::new("\n"));
        }
        runs.push(word("let", 0x10));
        runs.push(word(" x", 0x80));
    }
    runs
}

#[test]
fn the_lines_of_some_bytes_are_those_a_walk_of_every_line_finds() {
    // 40 000 lines are cut in pieces. Each range is answered with the
    // lines it covers and no others, as a walk of all the lines would,
    // at the start, across a cut and at the end.
    let laid = paragraph(&highlighted(40_000), 400.0);
    assert!(laid.pieces.len() > 1);
    let every: Vec<(usize, Range<usize>)> = (laid.pieces.iter())
        .flat_map(|piece| {
            (piece.layout.lines().enumerate()).map(|(line, laid)| {
                let range = laid.text_range();
                (
                    piece.first_line + line,
                    piece.start + range.start..piece.start + range.end,
                )
            })
        })
        .collect();
    let end = laid.text().len();
    let cut = laid.pieces[1].start;
    let ranges = [
        0..0,
        0..1,
        0..end,
        5..30,
        cut - 3..cut + 3,
        cut - 1..cut,
        cut..cut + 1,
        end - 5..end,
        end..end,
        end + 3..end + 9,
    ];
    for bytes in ranges {
        let walked: Vec<_> = (every.iter())
            .filter(|(_, text)| text.end > bytes.start && text.start < bytes.end)
            .cloned()
            .collect();
        let found = laid.lines_in(&bytes);
        assert_eq!(found, walked, "{bytes:?}");
    }
    // A few bytes cover few lines, though the text has 40 000.
    assert!(laid.lines_in(&(cut - 3..cut + 3)).len() <= 2);
}

/// The colours of the glyph runs of line `line`, in order.
fn brushes(laid: &Paragraph, line: usize) -> Vec<Brush> {
    let piece = laid
        .pieces
        .iter()
        .rfind(|piece| piece.first_line <= line)
        .unwrap();
    let line = piece.layout.get(line - piece.first_line).unwrap();
    (line.items())
        .filter_map(|item| match item {
            parley::PositionedLayoutItem::GlyphRun(glyphs) => Some(glyphs.style().brush),
            _ => None,
        })
        .collect()
}

#[test]
fn a_text_of_over_65535_styles_lays_out_in_pieces() {
    // 40 000 lines of two styles each is 80 000 styles in all, more than
    // parley takes in one text.
    let runs = highlighted(40_000);
    let laid = std::panic::catch_unwind(|| paragraph(&runs, 400.0))
        .expect("a text of 80 000 styles lays out");
    assert!(laid.pieces.len() > 1, "{} piece", laid.pieces.len());
    assert_eq!(laid.lines().len(), 40_000);
    assert_eq!(laid.height(), 24.0 * 40_000.0);
    let every = laid.lines();
    for (number, line) in every.iter().enumerate() {
        assert_eq!(laid.line(number), Some(*line), "line {number}");
    }
    assert_eq!(laid.line(40_000), None);
    let last = laid.lines()[39_999];
    assert_eq!(last.top, 24.0 * 39_999.0);
    assert_eq!(laid.text().len(), 40_000 * 5 + 39_999);
    // A byte of the last line is on the last line, past the cut.
    let end = laid.text().len();
    let boxes = laid.boxes(end - 5..end);
    assert_eq!(boxes.iter().map(|b| b.line).collect::<Vec<_>>(), [39_999]);
    let (x, y) = (1.0, last.top + 1.0);
    assert_eq!(laid.index_at(x, y), Some(end - 5));
    // The last line keeps its colours: no styles were left out.
    assert_eq!(brushes(&laid, 39_999), [[0x10; 3], [0x80; 3]]);
}

#[test]
fn every_piece_has_the_direction_of_the_whole_text() {
    // The first line after a cut starts with Hebrew. In one layout the
    // text is set from the left, as its first strong letter is Latin, and
    // so it is in every piece.
    let build = |hebrew: Option<usize>| {
        let mut runs = highlighted(20_000);
        if let Some(line) = hebrew {
            // Runs: two to a line and one for each break before it.
            runs[line * 3 - 1 + 1].text = "שלום".into();
        }
        runs
    };
    let cut = paragraph(&build(None), 400.0).pieces[1].start;
    let line = laid_line(&paragraph(&build(None), 400.0), cut);
    let laid = paragraph(&build(Some(line)), 400.0);
    assert!(laid.pieces.len() > 1);
    let offsets: Vec<f32> = (laid.pieces[1].layout.lines().take(3))
        .map(|line| line.metrics().offset)
        .collect();
    assert_eq!(offsets, [0.0, 0.0, 0.0]);
}

/// The number of the line that the byte `at` of the text is on.
fn laid_line(laid: &Paragraph, at: usize) -> usize {
    laid.text()[..at].matches('\n').count()
}

#[test]
fn emoji_are_counted_when_a_text_is_cut() {
    // No run has a style, but each line has an emoji, which is a style of
    // its own: 40 000 of them are cut in pieces, and none is left out.
    let runs: Vec<Run> = (0..40_000).map(|_| Run::new("©\u{fe0f} x\n")).collect();
    let laid = paragraph(&runs, 400.0);
    assert!(laid.pieces.len() > 1, "{} piece", laid.pieces.len());
    assert_eq!(laid.lines().len(), 40_001);
}

#[test]
fn a_text_that_opens_with_a_mark_has_its_direction_cut_or_not() {
    // A right-to-left mark is the first strong character, before the
    // first letter: the text is set from the right as one piece, and so
    // it is as many.
    let offsets = |lines: usize| -> (usize, Vec<f32>) {
        let mut runs = vec![Run::new("\u{200f}")];
        runs.extend(highlighted(lines));
        let laid = paragraph(&runs, 400.0);
        let first = laid.pieces[0].layout.lines().take(3);
        (
            laid.pieces.len(),
            first.map(|l| l.metrics().offset).collect(),
        )
    };
    let (one, small) = offsets(10);
    let (many, large) = offsets(20_000);
    assert_eq!((one, many > 1), (1, true));
    assert!(small[0] > 0.0, "{small:?}");
    assert_eq!(large, small);
}

#[test]
fn an_emoji_in_two_runs_is_counted_when_a_text_is_cut() {
    // A keycap is a digit and two marks, here in two runs. Nothing has a
    // style but the 40 000 emoji, which are cut in pieces.
    let runs: Vec<Run> = (0..40_000)
        .flat_map(|_| [Run::new("1"), Run::new("\u{fe0f}\u{20e3} x\n")])
        .collect();
    let laid = paragraph(&runs, 400.0);
    assert!(laid.pieces.len() > 1, "{} piece", laid.pieces.len());
}

#[test]
fn an_emoji_is_found_over_the_whole_text_and_belongs_to_the_run_it_starts_in() {
    // The keycap is `1` and two marks: 7 bytes over the two runs, 2..9; the
    // copyright sign with its selector is another, 10..15.
    let runs = [
        Run::new("a 1"),
        Run::new("\u{fe0f}\u{20e3} \u{a9}\u{fe0f} x"),
    ];
    let laid = paragraph(&runs, 400.0);
    let emoji = emoji_of(laid.text());
    assert_eq!(emoji, [2..9, 10..15]);
    assert_eq!(emoji_in(&emoji, &(0..3)), &emoji[..1]);
    assert_eq!(emoji_in(&emoji, &(3..17)), &emoji[1..]);
    assert!(emoji_in(&emoji, &(3..3)).is_empty());
}

#[test]
fn a_line_of_more_emoji_than_a_piece_takes_lays_out() {
    let runs = [Run::new(&"\u{a9}\u{fe0f}".repeat(31_000))];
    let laid = paragraph(&runs, f32::MAX);
    assert_eq!(laid.pieces.len(), 1);
}

#[test]
fn a_text_that_opens_with_a_hebrew_point_has_its_direction_cut_or_not() {
    // A point is a mark with no direction of its own: the first strong
    // letter after it sets the text from the right, cut or not.
    let offsets = |lines: usize| -> (usize, Vec<f32>) {
        let mut runs = vec![Run::new("\u{5b0}\u{5e9}\u{5dc}\u{5d5}\u{5dd}\n")];
        runs.extend(highlighted(lines));
        let laid = paragraph(&runs, 400.0);
        let first = laid.pieces[0].layout.lines().take(3);
        (
            laid.pieces.len(),
            first.map(|l| l.metrics().offset).collect(),
        )
    };
    let (one, small) = offsets(10);
    let (many, large) = offsets(20_000);
    assert_eq!((one, many > 1), (1, true));
    assert!(small[1] > 0.0, "{small:?}");
    assert_eq!(large, small);
}

#[test]
fn a_line_of_over_65535_styles_lays_out_with_fewer_colours() {
    // One line with no break to cut at: the colours past the budget of a
    // piece are left out, and the text is all there.
    let mut runs = highlighted(1);
    runs.extend(highlighted(40_000).into_iter().map(|run| Run {
        text: run.text.replace('\n', " "),
        ..run
    }));
    let laid = std::panic::catch_unwind(|| paragraph(&runs, f32::MAX))
        .expect("a line of 80 000 styles lays out");
    assert_eq!(laid.lines().len(), 1);
    assert_eq!(laid.pieces.len(), 1);
    assert_eq!(laid.text().len(), 5 + 40_000 * 5 + 39_999);
    // The first runs keep their colours and the last ones are in the
    // colour of the text.
    let all = brushes(&laid, 0);
    assert_eq!(all[..2], [[0x10; 3], [0x80; 3]]);
    assert_eq!(all[all.len() - 1], LOOK.color);
}

#[test]
fn inline_code_keeps_lines_their_height() {
    let runs = [
        Run::new("Run "),
        code("make check"),
        Run::new(" before you commit, and "),
        code("make fmt"),
        Run::new(" too."),
    ];
    let laid = paragraph(&runs, 120.0);
    let lines = laid.lines();
    assert!(lines.len() > 2, "{lines:?}");
    assert!(lines.iter().all(|line| line.height == 24.0), "{lines:?}");
}

#[test]
fn inline_code_breaks_only_at_its_spaces() {
    let runs = [
        Run::new("See "),
        code("src/kitty/graphics.rs and --reading-mode"),
        Run::new(" for more."),
    ];
    let laid = paragraph(&runs, 150.0);
    let range = laid.code()[0].clone();
    let starts: Vec<usize> = (laid.pieces[0].layout.lines())
        .map(|line| line.text_range().start)
        .filter(|start| range.contains(start) && *start > range.start)
        .collect();
    assert!(!starts.is_empty(), "the code should wrap at 150 px");
    for start in starts {
        assert_eq!(&laid.text()[start - 1..start], " ", "a break at {start}");
    }
    let (min, _) = laid.content_widths();
    let alone = paragraph(&[code("src/kitty/graphics.rs")], 1000.0);
    assert_eq!(min, alone.width());
}

#[test]
fn a_range_has_a_box_on_each_line_it_covers() {
    let words = "one two three four five six seven eight nine ten";
    let laid = paragraph(&[Run::new(words)], 120.0);
    assert!(laid.lines().len() > 1);
    let boxes = laid.boxes(0..words.len());
    assert_eq!(boxes.len(), laid.lines().len());
    let two = words.find("two").unwrap();
    let [one] = laid.boxes(two..two + 3)[..] else {
        panic!("one box");
    };
    assert_eq!(one.line, 0);
    assert!(one.left > 0.0 && one.right > one.left, "{one:?}");
}

#[test]
fn content_widths_are_the_longest_word_and_the_whole_line() {
    let laid = paragraph(&[Run::new("short and loooooooooooong")], 1000.0);
    let (min, max) = laid.content_widths();
    let long = paragraph(&[Run::new("loooooooooooong")], 1000.0);
    assert_eq!(min, long.width());
    assert_eq!(max, laid.width());
}

#[test]
fn a_newline_breaks_the_line() {
    let laid = paragraph(&[Run::new("one\ntwo")], 1000.0);
    assert_eq!(laid.lines().len(), 2);
}

/// A picture of `width` × `height` pixels, all red.
fn picture(width: u32, height: u32, id: u64) -> Run {
    Run::picture(
        InlinePicture {
            pixels: Rc::new(Pixels::new(RgbaImage::from_pixel(
                width,
                height,
                image::Rgba([255, 0, 0, 255]),
            ))),
            id,
            depth: 0.0,
        },
        None,
    )
}

/// The boxes of `laid`: id, left, top, width and height.
fn boxes(laid: &Paragraph) -> Vec<(u64, f32, f32, f32, f32)> {
    (laid.pieces[0].layout.lines())
        .flat_map(|line| line.items())
        .filter_map(|item| match item {
            parley::PositionedLayoutItem::InlineBox(placed) => {
                Some((placed.id, placed.x, placed.y, placed.width, placed.height))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_picture_in_a_line_sits_on_the_baseline_and_makes_the_line_taller() {
    let runs = [Run::new("Before "), picture(20, 40, 1), Run::new(" after")];
    let laid = paragraph(&runs, 600.0);
    let [(id, left, top, width, height)] = boxes(&laid)[..] else {
        panic!("one box: {:?}", boxes(&laid));
    };
    assert_eq!((id, width, height), (0, 20.0, 40.0));
    assert!(left > 40.0, "after the text before it: {left}");
    let [line] = laid.lines()[..] else {
        panic!("one line");
    };
    // Its bottom edge on the baseline, and the line tall enough.
    assert!(
        (top + height - line.baseline).abs() < 0.01,
        "{top} {line:?}"
    );
    assert!(line.height >= 40.0, "{line:?}");
    assert_eq!(laid.text(), "Before  after");
}

#[test]
fn a_picture_with_a_depth_reaches_below_the_baseline_and_the_line_grows_there() {
    let formula = Run::picture(
        InlinePicture {
            depth: 14.0,
            ..match picture(20, 40, 1).picture {
                Some(picture) => picture,
                None => unreachable!("a picture"),
            }
        },
        None,
    );
    let runs = [Run::new("Before "), formula, Run::new(" after")];
    let laid = paragraph(&runs, 600.0);
    let [(_, _, top, _, height)] = boxes(&laid)[..] else {
        panic!("one box: {:?}", boxes(&laid));
    };
    let [line] = laid.lines()[..] else {
        panic!("one line");
    };
    // Its baseline, 26 pixels from its top, is the line's.
    assert!((top + 26.0 - line.baseline).abs() < 0.01, "{top} {line:?}");
    assert!(
        (top + height - line.baseline - 14.0).abs() < 0.01,
        "{top} {line:?}"
    );
    // The line is as tall as the formula needs on both sides, not
    // taller than the formula and the text's usual room below it.
    assert!(line.height >= 40.0 && line.height < 40.0 + 14.0, "{line:?}");
}

#[test]
fn a_picture_moves_to_the_next_line_whole() {
    let words = "word ".repeat(8);
    let runs = [Run::new(&words), picture(60, 20, 1), Run::new(" end")];
    let laid = paragraph(&runs, 200.0);
    let [(_, left, _, width, _)] = boxes(&laid)[..] else {
        panic!("one box");
    };
    assert!(left + width <= 200.0 + 0.01, "{left} + {width}");
    assert!(laid.lines().len() > 1);
    // Two of them in a row, as badges are, side by side while they fit.
    let badges = [picture(60, 20, 1), Run::new(" "), picture(60, 20, 2)];
    let laid = paragraph(&badges, 400.0);
    let found = boxes(&laid);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].2, found[1].2, "{found:?}");
    assert!(found[1].1 >= found[0].1 + 60.0);
}

#[test]
fn bold_text_is_wider() {
    let regular = paragraph(&[Run::new("Markdown")], 1000.0);
    let bold = Run {
        bold: true,
        ..Run::new("Markdown")
    };
    assert!(paragraph(&[bold], 1000.0).width() > regular.width());
}

#[test]
fn a_point_is_at_a_byte_of_the_text_or_on_a_picture() {
    let picture = InlinePicture {
        pixels: Rc::new(Pixels::new(RgbaImage::new(10, 10))),
        id: 1,
        depth: 0.0,
    };
    let runs = [
        Run::new("one "),
        Run::picture(picture, None),
        Run::new(" two"),
    ];
    let laid = paragraph(&runs, 300.0);
    assert_eq!(laid.index_at(0.5, 5.0), Some(0));
    // Past the end of the line, none.
    assert_eq!(laid.index_at(299.0, 5.0), None);
    let place = laid.picture_box(0).expect("the picture is in a line");
    let middle = (place.left + place.right) / 2.0;
    assert_eq!(laid.picture_at(middle, 5.0), Some(0));
    assert_eq!(laid.picture_at(0.5, 5.0), None);
    assert_eq!(paragraph(&[Run::new("")], 300.0).index_at(1.0, 1.0), None);
}
