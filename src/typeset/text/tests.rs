use super::*;
use crate::typeset::parse::{code, parse};

/// The texts of the first block of `source`, as `before text` strings,
/// with `\t` and `\n` for the two kinds of before.
fn texts_of(source: &str) -> Vec<String> {
    let document = parse(source);
    pieces(&document.blocks[0])
        .into_iter()
        .map(|piece| {
            let before = match piece.before {
                Before::Line => "\n",
                Before::Cell => "\t",
            };
            format!("{before}{}", piece.text)
        })
        .collect()
}

#[test]
fn a_paragraph_and_a_heading_have_one_text_with_the_inlines_flattened() {
    assert_eq!(
        texts_of("one *two* **three** [four](u)\n"),
        ["\none two three four"]
    );
    assert_eq!(texts_of("## A `b` c\n"), ["\nA b c"]);
    assert_eq!(texts_of("![alt *text*](a.png) end\n"), ["\nalt text end"]);
    assert_eq!(texts_of("the $x^2$ and $`y`$ z\n"), ["\nthe x^2 and y z"]);
    assert_eq!(texts_of("a\nb  \nc\n"), ["\na b\nc"]);
    assert_eq!(
        texts_of("a <kbd>b</kbd> <i>c</i> <abbr>d</abbr>\n"),
        ["\na b c <abbr>d</abbr>"]
    );
}

#[test]
fn a_code_block_has_its_code_without_the_last_line_break_and_not_its_language() {
    assert_eq!(
        texts_of("```rust\nfn a() {}\n\nb\n```\n"),
        ["\nfn a() {}\n\nb"]
    );
    assert_eq!(texts_of("```\n\n```\n"), ["\n"]);
    let file = code("one\ntwo\n", Some("txt"));
    assert_eq!(pieces(&file.blocks[0]), [Piece::line("one\ntwo")]);
}

#[test]
fn a_table_has_a_text_for_each_cell_and_a_tab_between_those_of_a_row() {
    let source = "| a | *b* |\n|---|---|\n| 1 | 2 |\n| 3 |\n";
    assert_eq!(texts_of(source), ["\na", "\tb", "\n1", "\t2", "\n3", "\t"]);
}

#[test]
fn details_have_the_summary_and_an_alert_its_title_and_html_its_source() {
    let source = "<details>\n<summary>The <b>sum</b></summary>\n\nBody\n\n</details>\n";
    assert_eq!(texts_of(source), ["\nThe sum"]);
    assert_eq!(texts_of("> [!NOTE] A title\n> text\n"), ["\nA title"]);
    assert!(texts_of("> [!NOTE]\n> text\n").is_empty());
    assert_eq!(
        texts_of("<table>\nraw\n</table>\n"),
        ["\n<table>\nraw\n</table>"]
    );
}

#[test]
fn blocks_that_hold_blocks_and_blocks_without_text_have_none() {
    for source in ["> quote\n", "- a\n- b\n", "---\n"] {
        assert!(texts_of(source).is_empty(), "{source:?}");
    }
    for block in [
        Block::Note("N rows are left out".into()),
        Block::Error("cannot be shown".into()),
        Block::ImageFile("a.png".into()),
    ] {
        assert!(pieces(&block).is_empty());
    }
}

#[test]
fn a_centred_wrapper_has_no_text_and_the_blocks_inside_it_have_theirs() {
    let wrapper = Block::Centered(vec![
        Block::Heading {
            level: 1,
            content: vec![Inline::Text("Name".into())],
            id: None,
        },
        Block::Paragraph(vec![Inline::Text("words".into())]),
    ]);
    assert!(pieces(&wrapper).is_empty());
    for part in [Part::Content, Part::Summary, Part::Source, Part::Title] {
        assert_eq!(number(&wrapper, part), None, "{part:?}");
    }
    let document = Document::new(vec![wrapper]);
    let texts: Vec<String> = (document.blocks().into_iter())
        .flat_map(|(_, block)| pieces(block))
        .map(|piece| piece.text.to_string())
        .collect();
    assert_eq!(texts, ["Name", "words"]);
}

#[test]
fn front_matter_has_its_keys_and_values_as_the_table_shows_them() {
    let source = "---\ntitle: One\ntags: [a, b]\nowner:\nnone: {}\nmeta:\n  year: 2026\n  deep:\n    more: 1\nstops:\n  - name: A\n    km: 0\n  - plain\n---\n";
    assert_eq!(
        texts_of(source),
        [
            "\ntitle", "\tOne", "\ntags", "\ta, b", "\nowner", "\t", "\nnone", "\t", "\nmeta",
            "\nyear", "\t2026", "\ndeep", "\nmore", "\t1", "\nstops", "\nname", "\tA", "\nkm",
            "\t0", "\nplain",
        ]
    );
}

#[test]
fn front_matter_that_is_not_a_map_is_its_source_and_an_empty_one_has_no_text() {
    assert_eq!(texts_of("---\n- a\n- b\n---\n"), ["\n- a\n- b"]);
    assert_eq!(texts_of("---php\n$a = 1;\n---\n"), ["\n$a = 1;"]);
    assert_eq!(texts_of("---\n: [\n---\n"), ["\n: ["]);
    assert!(texts_of("---\n---\n").is_empty());
    assert!(texts_of("---\n\n---\n").is_empty());
}

#[test]
fn what_number_says_of_a_block_is_what_pieces_has_of_it() {
    let parts = [Part::Content, Part::Summary, Part::Source, Part::Title];
    let mut seen = [0usize; 4];
    for path in crate::testing::documents() {
        let document = parse(&std::fs::read_to_string(&path).unwrap());
        for (at, block) in document.blocks() {
            let texts = pieces(block);
            let numbered: Vec<(Part, usize)> = (parts.iter())
                .filter_map(|&part| Some((part, number(block, part)?)))
                .collect();
            let name = format!("{} {at:?}", path.display());
            match block {
                // Their texts are numbered by their grids.
                Block::Table(_) => assert!(numbered.is_empty(), "{name}"),
                Block::FrontMatter(front_matter) => {
                    let source = front_matter_texts(front_matter) == FrontMatterTexts::Source;
                    assert_eq!(numbered.len(), usize::from(source), "{name}");
                }
                // One text, and the part that says it.
                _ => assert_eq!(numbered.len(), texts.len(), "{name}"),
            }
            for (part, at_number) in numbered {
                assert!(at_number < texts.len(), "{name}: {part:?}");
                seen[parts.iter().position(|&p| p == part).unwrap()] += 1;
            }
        }
    }
    assert!(seen.iter().all(|&count| count > 0), "{seen:?}");
}

#[test]
fn a_block_has_a_text_of_a_part_only_where_it_has_that_part() {
    let details = "<details><summary>s</summary>\n\nb\n\n</details>\n";
    let wrong = [
        ("# h\n", Part::Source),
        ("p\n", Part::Title),
        ("```\nc\n```\n", Part::Content),
        ("<table>\n</table>\n", Part::Summary),
        ("> [!NOTE]\n> a\n", Part::Title),
        ("---\ntitle: A\n---\n", Part::Source),
        ("---\n---\n", Part::Source),
    ];
    let right = [
        ("# h\n", Part::Content),
        ("p\n", Part::Content),
        ("```\nc\n```\n", Part::Source),
        ("<table>\n</table>\n", Part::Source),
        (details, Part::Summary),
        ("> [!NOTE] t\n> a\n", Part::Title),
        ("---\n- a\n---\n", Part::Source),
    ];
    for (source, part) in wrong {
        let block = parse(source).blocks.remove(0);
        assert_eq!(number(&block, part), None, "{source:?} {part:?}");
    }
    for (source, part) in right {
        let block = parse(source).blocks.remove(0);
        assert_eq!(number(&block, part), Some(0), "{source:?} {part:?}");
    }
}

#[test]
fn front_matter_has_no_texts_leaves_or_its_source() {
    let kind = |source: &str| {
        let document = parse(source);
        let Block::FrontMatter(front_matter) = &document.blocks[0] else {
            panic!("front matter")
        };
        match front_matter_texts(front_matter) {
            FrontMatterTexts::None => "none",
            FrontMatterTexts::Leaves(_) => "leaves",
            FrontMatterTexts::Source => "source",
        }
    };
    assert_eq!(kind("---\n---\n"), "none");
    assert_eq!(kind("---\n\u{a0}\n---\n"), "none");
    assert_eq!(kind("---\n- a\n---\n"), "source");
    assert_eq!(kind("---php\n$a\n---\n"), "source");
    // A comment is not a blank source: it has no keys, but it is there.
    assert_eq!(kind("---\n# c\n---\n"), "leaves");
    assert_eq!(kind("---\na: 1\n---\n"), "leaves");
}

#[test]
fn the_texts_of_a_block_come_by_file_and_path_and_a_missing_one_has_none() {
    let first = parse("# A\n\n> b\n>\n> - c\n");
    let second = parse("d\n");
    let text = DocumentText::new([&first, &second]);
    assert_eq!(text.texts(0, &[0]).to_vec(), [Piece::line("A")]);
    assert_eq!(text.texts(0, &[1, 1, 0, 0]).to_vec(), [Piece::line("c")]);
    assert_eq!(text.texts(1, &[0]).to_vec(), [Piece::line("d")]);
    assert!(text.texts(0, &[1]).is_empty());
    assert!(text.texts(0, &[7]).is_empty());
    assert!(text.texts(2, &[0]).is_empty());
}

#[test]
fn the_places_of_the_fixtures_and_examples_only_grow() {
    let documents: Vec<Document> = crate::testing::documents()
        .iter()
        .map(|path| parse(&std::fs::read_to_string(path).unwrap()))
        .collect();
    let grows = |text: &DocumentText, what: &str| {
        let all: Vec<Pos> = text.positions().collect();
        assert!(all.len() > 3, "{what}");
        for pair in all.windows(2) {
            assert!(pair[0] < pair[1], "{what}: {:?} {:?}", pair[0], pair[1]);
        }
        all.len()
    };
    let mut together = 0;
    for (index, document) in documents.iter().enumerate() {
        together += grows(&DocumentText::new([document]), &format!("document {index}"));
    }
    assert_eq!(
        grows(&DocumentText::new(&documents), "all documents"),
        together
    );
}

#[test]
fn the_places_of_a_block_are_itself_its_texts_and_its_end_after_the_blocks_inside() {
    let document = parse("> a\n>\n> b\n\nc\n");
    let text = DocumentText::new([&document]);
    let places: Vec<(Vec<usize>, Within)> = text.positions().map(|p| (p.path, p.within)).collect();
    let text_start = |text| Within::Text { text, offset: 0 };
    assert_eq!(
        places,
        [
            (vec![0], Within::Block),
            (vec![0, 0], Within::Block),
            (vec![0, 0], text_start(0)),
            (vec![0, 0], Within::End),
            (vec![0, 1], Within::Block),
            (vec![0, 1], text_start(0)),
            (vec![0, 1], Within::End),
            (vec![0], Within::End),
            (vec![1], Within::Block),
            (vec![1], text_start(0)),
            (vec![1], Within::End),
        ]
    );
}

fn text_of_files(sources: &[&str]) -> (DocumentText, Vec<Document>) {
    let documents: Vec<Document> = sources.iter().map(|source| parse(source)).collect();
    (DocumentText::new(&documents), documents)
}

/// The matches of `term` in `sources`, as `file/path:text@bytes`.
fn found(sources: &[&str], term: &str, ignore_case: bool) -> Vec<String> {
    let (text, _) = text_of_files(sources);
    (text.find(term, ignore_case).into_iter())
        .map(|range| {
            let (Within::Text { text: t, offset: a }, Within::Text { offset: b, .. }) =
                (&range.start.within, &range.end.within)
            else {
                panic!("{range:?}");
            };
            assert_eq!(
                (range.start.file, &range.start.path),
                (range.end.file, &range.end.path)
            );
            format!("{}/{:?}:{t}@{a}..{b}", range.start.file, range.start.path)
        })
        .collect()
}

#[test]
fn a_term_is_found_in_every_kind_of_text_and_not_in_what_is_not_text() {
    let source = [
        "---\nkey: needle value\nneedle: x\n---\n",
        "# needle heading\n",
        "paragraph needle\n",
        "| h |  |\n|---|--|\n| needle | 2 |\n",
        "<details>\n<summary>needle sum</summary>\n\nBody\n\n</details>\n",
        "> [!NOTE] needle title\n> text\n",
        "```needle\ncode needle\n```\n",
        "<table>needle</table>\n",
    ]
    .join("\n");
    assert_eq!(
        found(&[&source], "needle", false),
        [
            "0/[0]:1@0..6",
            "0/[0]:2@0..6",
            "0/[1]:0@0..6",
            "0/[2]:0@10..16",
            "0/[3]:2@0..6",
            "0/[4]:0@0..6",
            "0/[5]:0@0..6",
            "0/[6]:0@5..11",
            "0/[7]:0@7..13",
        ]
    );
}

#[test]
fn kiris_own_notes_and_errors_are_not_found() {
    let document = Document::new(vec![
        Block::Note("needle".into()),
        Block::Error("needle".into()),
    ]);
    assert!(
        DocumentText::new([&document])
            .find("needle", false)
            .is_empty()
    );
}

#[test]
fn a_term_is_found_across_a_soft_break_and_not_across_a_cell_or_a_block() {
    assert_eq!(found(&["one\ntwo\n"], "one two", false), ["0/[0]:0@0..7"]);
    assert!(found(&["| a | b |\n|---|---|\n"], "a b", false).is_empty());
    assert!(found(&["a\n\nb\n"], "a b", false).is_empty());
    assert!(found(&["a\n\nb\n"], "a\nb", false).is_empty());
}

#[test]
fn the_case_and_the_no_break_space_follow_the_search() {
    // `İ` is two bytes and lower-cases to two characters.
    assert_eq!(
        found(&["İ Äiti äiti\n"], "äiti", true),
        ["0/[0]:0@3..8", "0/[0]:0@9..14"]
    );
    assert_eq!(found(&["İ Äiti äiti\n"], "äiti", false), ["0/[0]:0@9..14"]);
    assert_eq!(
        found(&["run\u{a0}make\u{a0}check\n"], "make check", false),
        ["0/[0]:0@5..16"]
    );
    assert_eq!(
        found(&["aaaa\n"], "aa", false),
        ["0/[0]:0@0..2", "0/[0]:0@2..4"]
    );
    assert!(found(&["a\n"], "", false).is_empty());
}

#[test]
fn matches_come_in_the_order_of_the_documents() {
    let found = found(&["x\n\n> x\n", "x\n"], "x", false);
    assert_eq!(found, ["0/[0]:0@0..1", "0/[1, 0]:0@0..1", "1/[0]:0@0..1"]);
}

/// The text between the places `a` and `b`.
fn between(sources: &[&str], a: Pos, b: Pos) -> String {
    text_of_files(sources).0.text(&(a..b))
}

#[test]
fn the_text_of_a_range_inside_one_text_is_a_part_of_it() {
    let at = |offset| Pos::text(0, &[0], 0, offset);
    assert_eq!(between(&["hello world\n"], at(2), at(7)), "llo w");
    assert_eq!(between(&["hello world\n"], at(0), at(11)), "hello world");
}

#[test]
fn the_text_of_a_range_across_blocks_has_a_line_break_between_them() {
    let sources = ["first\n\nsecond\n\nthird\n"];
    let a = Pos::text(0, &[0], 0, 2);
    assert_eq!(
        between(&sources, a.clone(), Pos::text(0, &[2], 0, 3)),
        "rst\nsecond\nthi"
    );
    assert_eq!(between(&sources, a, Pos::block(0, &[2])), "rst\nsecond");
}

#[test]
fn the_text_of_a_range_across_a_table_has_tabs_between_cells_and_lines_between_rows() {
    let sources = ["| a | b |\n|---|---|\n| c | d |\n"];
    assert_eq!(
        between(&sources, Pos::block(0, &[0]), Pos::end(0, &[0])),
        "a\tb\nc\td"
    );
    assert_eq!(
        between(&sources, Pos::text(0, &[0], 1, 0), Pos::text(0, &[0], 3, 1)),
        "b\nc\td"
    );
}

#[test]
fn the_text_of_a_range_across_the_border_of_files_and_the_ends_of_blocks() {
    let sources = ["a\n\n> b\n", "c\n"];
    let all = between(&sources, Pos::block(0, &[0]), Pos::end(1, &[0]));
    assert_eq!(all, "a\nb\nc");
    // The end of a quote is after what is in it, and its start before.
    assert_eq!(
        between(&sources, Pos::block(0, &[1]), Pos::end(0, &[1])),
        "b"
    );
    assert_eq!(
        between(&sources, Pos::end(0, &[0]), Pos::block(1, &[0])),
        "b"
    );
}

#[test]
fn the_text_of_a_range_over_a_block_without_text_is_empty_and_so_is_a_range_with_no_extent() {
    let sources = ["a\n\n---\n\nb\n"];
    assert_eq!(
        between(&sources, Pos::block(0, &[1]), Pos::end(0, &[1])),
        ""
    );
    assert_eq!(
        between(&sources, Pos::block(0, &[0]), Pos::end(0, &[2])),
        "a\nb"
    );
    let here = Pos::text(0, &[0], 0, 0);
    assert_eq!(between(&sources, here.clone(), here), "");
    assert_eq!(
        between(&sources, Pos::end(0, &[2]), Pos::block(0, &[0])),
        "",
        "backwards"
    );
}

#[test]
fn a_place_in_the_middle_of_a_character_or_past_the_end_is_moved_to_one_there_is() {
    let at = |offset| Pos::text(0, &[0], 0, offset);
    // `ä` and `好` take two and three bytes.
    assert_eq!(between(&["ä好b\n"], at(1), at(5)), "ä好");
    assert_eq!(between(&["ä好b\n"], at(2), at(100)), "好b");
    assert_eq!(
        between(&["ä好b\n"], at(0), Pos::text(0, &[0], 7, 0)),
        "ä好b"
    );
}

#[test]
fn an_empty_cell_in_the_middle_of_a_row_keeps_its_tabs() {
    let sources = ["| a | | c |\n|---|---|---|\n"];
    assert_eq!(
        between(&sources, Pos::block(0, &[0]), Pos::end(0, &[0])),
        "a\t\tc"
    );
}

/// Words that a layout adds to a document, which are not its text:
/// what comes with a list item (a bullet, a number such as `3.` or `[3]`), a
/// task's box, the frame of a table or a code block and a heading's rule
/// (the marks that layouts draw them with, and nothing else), the name
/// of an alert's kind, and the label of a code block's language
/// (`languages` of the document). Three more are told apart by their
/// shape and need the text of the block (`block`, the text of the block
/// the line is of and of the blocks inside it): the header of a column
/// and a colon before a value when a table is shown as a card for each
/// row, the brackets and the word `image` around the alt text of an
/// image that is not drawn, and a formula that is not drawn, which shows
/// as it is written between dollars. Each only when the document does
/// not have the word as text.
fn is_added_by_the_layout(block: &str, word: &str, languages: &[&str]) -> bool {
    let marker = (word.strip_suffix('.'))
        .or_else(|| word.strip_prefix('[')?.strip_suffix(']'))
        .unwrap_or(word);
    let number = !marker.is_empty() && marker.chars().all(|c| c.is_ascii_digit());
    let kind = ["Note", "Tip", "Important", "Warning", "Caution"].contains(&word);
    // Box drawing, block elements and geometric shapes, such as the
    // arrow of a folded block, the icon of an alert (a private use
    // character), the bullets of lists and the boxes of tasks.
    let mark = |c: char| {
        ('\u{2500}'..='\u{25ff}').contains(&c)
            || ('\u{e000}'..='\u{f8ff}').contains(&c)
            || "•◦▪‣⁃☐☑☒".contains(c)
    };
    let drawn = word.chars().all(mark);
    let label = (word.strip_suffix(':')).is_some_and(|name| block.contains(name));
    // An image that is not drawn shows as `[image: alt text]`.
    // Two images side by side join their labels, as `One][image:`.
    let image = word == "[image:"
        || (word.contains(']')
            && (word.split("][image:")).all(|part| block.contains(&part.replace(['[', ']'], ""))));
    // A formula that is not drawn shows as it is written between
    // dollars, as Markdown reads it, where `\\` is `\`: each part
    // between backslashes has to be in the text of the block.
    let written =
        word.contains('$') && (word.replace('$', "").split('\\')).all(|part| block.contains(part));
    number || kind || drawn || label || image || written || languages.contains(&word)
}

#[test]
fn every_word_a_layout_shows_is_in_the_text_of_the_document() {
    use crate::typeset::caps::Capabilities;
    use crate::typeset::driver::{Content, Stream, content};
    use crate::typeset::inputs::Inputs;
    use crate::typeset::style;
    let inputs = Inputs::new(Capabilities::kitty(200));
    let mut checked = 0;
    for path in crate::testing::documents() {
        let document = parse(&std::fs::read_to_string(&path).unwrap());
        let text = DocumentText::new([&document]);
        let languages: Vec<&str> = (document.blocks().into_iter())
            .filter_map(|(_, block)| match block {
                Block::CodeBlock { language, .. } => language.as_deref(),
                Block::FrontMatter(front_matter) => Some(front_matter.language.as_str()),
                _ => None,
            })
            .collect();
        let mut missing = Vec::new();
        let Stream { lines, blocks } = content(Content::Document(&document), &inputs);
        // The text of a block and of the blocks inside it.
        let block_text = |path: &[usize]| -> String {
            (document.blocks().into_iter())
                .filter(|(at, _)| at.starts_with(path))
                .flat_map(|(at, _)| {
                    (text.texts(0, &at).to_vec().into_iter()).map(|piece| piece.text.to_string())
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        for line in &lines {
            let block = line
                .block
                .map(|id| block_text(blocks.get(id).path()))
                .unwrap_or_default();
            for word in style::text(&line.spans).split_whitespace() {
                checked += 1;
                if text.find(word, false).is_empty()
                    && !is_added_by_the_layout(&block, word, &languages)
                {
                    missing.push(word.to_owned());
                }
            }
        }
        assert!(missing.is_empty(), "{}: {missing:?}", path.display());
    }
    assert!(checked > 5000, "{checked}");
}

#[test]
fn an_inline_gives_as_many_bytes_as_flatten_writes_of_it() {
    let document = parse(
        "a *b* `c` [d **e**](u) ![alt *f*](i.png) $x^2$ <kbd>k</kbd> <mark>m</mark> \\\nh\ni\n",
    );
    let Block::Paragraph(content) = &document.blocks[0] else {
        panic!("a paragraph");
    };
    let mut text = String::new();
    flatten(content, &mut text, &mut |inline, bytes| {
        assert_eq!(inline_len(inline), bytes.len(), "{inline:?}");
    });
    assert_eq!(content.iter().map(inline_len).sum::<usize>(), text.len());
    assert!(text.contains('\n') && text.contains("alt f"), "{text:?}");
}

#[test]
fn a_cursor_takes_and_skips_in_order_and_may_be_off() {
    let mut cursor = Cursor::new(3);
    assert_eq!(cursor.text(), Some(3));
    assert_eq!(cursor.take(4), 0..4);
    assert_eq!(cursor.skip(&Inline::Text("abc".into())), 4..7);
    assert_eq!(cursor.take(0), 7..7);
    assert_eq!(cursor.take(2), 7..9);
    assert_eq!(Cursor::off().text(), None);
}
