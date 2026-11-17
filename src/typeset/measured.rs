//! Tests of a layout that only measures the stretches of a large code block:
//! it has the lines of the whole layout, lays them out when asked, and its
//! map finds the lines of a stretch that is only measured.

use crate::render::Inputs;
use crate::typeset::caps::Capabilities;
use crate::typeset::driver::{self, Content};
use crate::typeset::laid::Laid;
use crate::typeset::model::{Block, Document};
use crate::typeset::parse::parse;
use crate::typeset::stretch;
use crate::typeset::style::Line;
use crate::typeset::wrap;

fn inputs() -> Inputs {
    Inputs::new(Capabilities::kitty(60))
}

/// `count` lines of Rust: some long enough to wrap, some with tabs, some
/// empty, and comments that go on over the lines where stretches are cut.
fn rust(count: usize) -> String {
    let mut code = String::new();
    for line in 0..count {
        let text = match line % 1024 {
            508 | 1020 => "/* a comment that goes on".to_owned(),
            509..=514 | 1021..=1026 => "   still the comment".to_owned(),
            515 | 1027 => "   the end */ fn after() {}".to_owned(),
            _ if line % 97 == 0 => {
                format!("\tlet tabbed{line} = \"a\\tb\"; // {}", "word ".repeat(20))
            }
            _ if line % 89 == 0 => String::new(),
            _ if line % 53 == 0 => format!("let wide{line} = \"{}\";", "日本語 ".repeat(18)),
            _ => format!("fn f{line}() {{ let x = {line}; }}"),
        };
        code.push_str(&text);
        code.push('\n');
    }
    code
}

fn code_block(language: Option<&str>, code: &str, whole_file: bool) -> Document {
    Document::new(vec![Block::CodeBlock {
        language: language.map(str::to_owned),
        code: code.to_owned(),
        whole_file,
    }])
}

/// A document of a paragraph, the code block and a paragraph.
fn between(block: Document) -> Document {
    let mut blocks = parse("Before.\n").blocks;
    blocks.extend(block.blocks);
    blocks.extend(parse("After.\n").blocks);
    Document::new(blocks)
}

/// The documents the tests lay out: a fenced block between paragraphs, a
/// file shown as code and one shown as text.
fn documents() -> Vec<(&'static str, Document)> {
    let code = rust(2700);
    vec![
        ("fenced", between(code_block(Some("rust"), &code, false))),
        ("a file of code", code_block(Some("rust"), &code, true)),
        ("a file of text", code_block(None, &code, true)),
    ]
}

fn measured(document: &Document) -> Laid {
    driver::laid_measured(Content::Document(document), 0, &inputs())
}

fn whole(document: &Document) -> Laid {
    driver::laid(Content::Document(document), &inputs())
}

/// How many of `lines` are only measured.
fn only_measured(lines: &[Line]) -> usize {
    lines.iter().filter(|line| line.measured.is_some()).count()
}

#[test]
fn a_measured_layout_has_as_many_lines_as_the_whole_one_and_most_hold_nothing() {
    for (what, document) in documents() {
        let (part, all) = (measured(&document), whole(&document));
        assert_eq!(part.len(), all.len(), "{what}");
        assert!(part.is_whole(), "{what}");
        let stretches = part.measured();
        assert!(stretches.len() >= 3, "{what}: {stretches:?}");
        let held: usize = stretches.iter().map(ExactSizeIterator::len).sum();
        assert_eq!(held, only_measured(part.lines()), "{what}");
        assert!(held > all.len() / 2, "{what}: {held} of {}", all.len());
        let caps = &inputs().caps;
        assert_eq!(part.height(caps), all.height(caps), "{what}");
        for (at, (line, eager)) in part.lines().iter().zip(all.lines()).enumerate() {
            assert_eq!(line.block, eager.block, "{what}: block of line {at}");
            assert!(
                line.measured.is_none() || line.spans.is_empty(),
                "{what}: line {at}"
            );
        }
        // The first stretch is laid out, as the whole layout has it.
        let first = stretches[0].start;
        assert!(first >= stretch::LINES, "{what}");
        assert_eq!(part.lines()[..first], all.lines()[..first], "{what}");
    }
}

#[test]
fn the_stretches_near_are_laid_out_as_in_the_whole_layout_and_no_line_moves() {
    for (what, document) in documents() {
        let content = Content::Document(&document);
        let (mut part, all) = (measured(&document), whole(&document));
        let inputs = inputs();
        let lines = all.len();
        // Read on from the top: every stretch is laid out from what is
        // before it, so its colours are those of the whole layout.
        let mut top = 0;
        while top < lines {
            let near = top..top + 40;
            part.show(content, &inputs, near.clone());
            assert_eq!(part.len(), lines, "{what}");
            let near = near.start..near.end.min(lines);
            assert_eq!(
                part.lines()[near.clone()],
                all.lines()[near.clone()],
                "{what}: lines {near:?}"
            );
            top += 300;
        }
        // Asked again for what it has, the layout changes nothing.
        let end = lines - 40..lines;
        part.show(content, &inputs, end.clone());
        assert!(!part.show(content, &inputs, end), "{what}");
    }
}

#[test]
fn the_stretches_far_from_what_is_asked_for_are_only_measured_again() {
    let document = code_block(None, &rust(6000), true);
    let content = Content::Document(&document);
    let (mut part, all) = (measured(&document), whole(&document));
    let (inputs, lines) = (inputs(), all.len());
    let at_first = only_measured(part.lines());
    for top in (0..lines).step_by(300) {
        part.show(content, &inputs, top..top + 40);
    }
    // All but the stretches near the end, and the first, hold nothing.
    let far = part.measured();
    assert!(
        far.iter().all(|stretch| stretch.end < lines - 40),
        "{far:?}"
    );
    let held = only_measured(part.lines());
    assert!(held > lines / 2 && held < at_first, "{held} of {lines}");
    assert_eq!(part.len(), lines);
    // And back at the top they are laid out as they were.
    part.show(content, &inputs, 500..700);
    assert_eq!(part.lines()[500..700], all.lines()[500..700]);
}

#[test]
fn a_stretch_that_is_jumped_to_has_the_text_and_the_lines_of_the_whole_layout() {
    for (what, document) in documents() {
        let content = Content::Document(&document);
        let (mut part, all) = (measured(&document), whole(&document));
        let middle = all.len() / 2;
        assert!(part.lines()[middle].measured.is_some(), "{what}");
        assert!(part.show(content, &inputs(), middle..middle + 40), "{what}");
        let text = |lines: &[Line]| -> Vec<String> {
            (lines.iter())
                .map(|line| crate::typeset::style::text(&line.spans))
                .collect()
        };
        let near = middle..middle + 40;
        assert_eq!(
            text(&part.lines()[near.clone()]),
            text(&all.lines()[near]),
            "{what}"
        );
    }
}

#[test]
fn the_map_finds_the_line_of_a_place_in_a_stretch_that_is_only_measured() {
    for (what, document) in documents() {
        let (part, all) = (measured(&document), whole(&document));
        let mut found = 0;
        for stretch in part.measured() {
            for line in stretch {
                let Some(range) = all.span_of(line..line + 1) else {
                    continue;
                };
                let rows = part.drawn_rows(&range).map(|(_, rows)| rows);
                let eager = all.drawn_rows(&range).map(|(_, rows)| rows);
                assert_eq!(rows, eager, "{what}: line {line}, {range:?}");
                assert_eq!(
                    part.line_of(&range.start),
                    all.line_of(&range.start),
                    "{what}: line {line}"
                );
                found += 1;
            }
        }
        assert!(found > 1000, "{what}: {found}");
        // Every line says which file it is in, and the lines of the layout
        // show what the whole layout shows.
        let last = part.len();
        assert!(part.first_on(last / 2).is_some(), "{what}");
        assert_eq!(part.span_of(0..last), all.span_of(0..last), "{what}");
    }
}

#[test]
fn the_map_goes_on_with_the_lines_laid_out_in_the_place_of_measured_ones() {
    for (what, document) in documents() {
        let content = Content::Document(&document);
        let (mut part, all) = (measured(&document), whole(&document));
        // The map is made of the measured lines, and takes the new ones.
        part.map();
        let middle = all.len() / 2;
        let near = middle..middle + 40;
        assert!(part.show(content, &inputs(), near.clone()), "{what}");
        for line in near {
            assert_eq!(part.first_on(line), all.first_on(line), "{what}: {line}");
            let Some(range) = all.span_of(line..line + 1) else {
                continue;
            };
            assert_eq!(part.locate(&range), all.locate(&range), "{what}: {line}");
            assert_eq!(
                part.line_bytes(&range),
                all.line_bytes(&range),
                "{what}: {line}"
            );
            for column in [0, 3, 20] {
                assert_eq!(part.hit(line, column), all.hit(line, column), "{what}");
            }
        }
    }
}

#[test]
fn the_starts_of_wrapped_lines_are_where_split_cuts_the_text() {
    use crate::typeset::style::{Span, Style};
    let texts = [
        "",
        "short",
        "exactly ten",
        "a line of plain text that is longer than the width it is wrapped at, twice",
        "日本語の文字は幅が二つあるので、折り返しの位置が変わります",
        "mixed ascii and 日本語 and an emoji 👨‍👩‍👧‍👦 with a flag 🇫🇮 in it",
        "e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}",
        "\u{1b}[31mred\u{1b}[0m control characters in a log line",
    ];
    for text in texts {
        for max in [1, 2, 7, 11, 60] {
            // Counted in characters: a span holds a control character as
            // another one, which may take more bytes.
            let mut starts = Vec::new();
            wrap::starts(text, max, |byte| starts.push(text[..byte].chars().count()));
            let lines = wrap::split(&[Span::new(text, Style::default())], max);
            let mut at = 0;
            let want: Vec<usize> = (lines.iter())
                .map(|line| {
                    let start = at;
                    at += (line.iter().map(|span| span.text.chars().count())).sum::<usize>();
                    start
                })
                .collect();
            assert_eq!(starts, want, "{text:?} at {max}");
        }
    }
}
