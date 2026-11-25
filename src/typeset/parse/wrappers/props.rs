//! Property tests of the joiner of wrappers: documents made of the tags it
//! reads, text with characters of more than one byte, comments, blank lines
//! and CRLF, in any order, at the top, in a quote or in a list item, are
//! read without a panic, with places on the source, and with none of their
//! words lost.

use proptest::prelude::*;

use crate::testing::props_config;
use crate::typeset::parse::parse;

/// The pieces a document is made of: tags, text, blank lines, comments and
/// line ends. Text and an image's alt hold a marker word, `MARK`, which a
/// document numbers one by one.
const PIECES: [&str; 26] = [
    "<p align=\"center\">",
    "</p>",
    "<div align=\"center\">",
    "</div>",
    "<center>",
    "</center>",
    "<details>",
    "</details>",
    "<summary>",
    "</summary>",
    "<a name=\"x\">",
    "<a href=\"u\">",
    "</a>",
    "<img src=\"a.png\" alt=\"MARK\">",
    "<h1 align=\"center\">",
    "</h1>",
    "MARK",
    "MARK",
    "\n",
    "\n",
    "\n\n",
    "\n\n",
    "<!-- c -->",
    "\r\n",
    " ",
    "<p>",
];

/// Characters of more than one byte that text and alt text end with.
const WIDE: [&str; 6] = ["é", "€", "日本", "😀", "e\u{301}", ""];

/// A document of `pieces`, each a piece and the characters that end its
/// text, with the marker words numbered, and whether it ends with a line
/// break; and its marker words.
fn document(pieces: &[(usize, usize)], end: bool) -> (String, Vec<String>) {
    let (mut source, mut marks) = (String::new(), Vec::new());
    for &(piece, wide) in pieces {
        let piece = PIECES[piece];
        if piece.contains("MARK") {
            let mark = format!("m{}x", marks.len());
            source.push_str(&piece.replace("MARK", &format!("{mark}{}", WIDE[wide])));
            marks.push(mark);
        } else {
            source.push_str(piece);
        }
    }
    if end {
        source.push('\n');
    }
    (source, marks)
}

/// `source` at the top, in a quote or in a list item, which move its lines
/// to the right.
fn contained(source: &str, container: u8) -> String {
    let prefix = |first: bool| match (container, first) {
        (1, _) => "> ",
        (2, true) => "- ",
        (2, false) => "  ",
        _ => "",
    };
    (source.split_inclusive('\n').enumerate())
        .map(|(at, line)| format!("{}{line}", prefix(at == 0)))
        .collect()
}

fn documents() -> impl Strategy<Value = (String, Vec<String>)> {
    (
        proptest::collection::vec((0..PIECES.len(), 0..WIDE.len()), 0..32),
        any::<bool>(),
        0u8..3,
    )
        .prop_map(|(pieces, end, container)| {
            let (source, marks) = document(&pieces, end);
            (contained(&source, container), marks)
        })
}

proptest! {
    #![proptest_config(props_config())]

    #[test]
    fn any_document_of_wrapper_tags_reads_with_places_on_its_source_and_all_its_words(
        (source, marks) in documents(),
    ) {
        let document = parse(&source);
        for (path, _) in document.blocks() {
            let Some(place) = document.places.get(&path) else {
                continue;
            };
            let bytes = &place.bytes;
            prop_assert!(place.line >= 1, "{path:?}: {place:?} of {source:?}");
            prop_assert!(bytes.start <= bytes.end && bytes.end <= source.len());
            prop_assert!(
                source.is_char_boundary(bytes.start) && source.is_char_boundary(bytes.end),
                "{path:?}: {place:?} of {source:?}"
            );
        }
        // Every word is somewhere in what was read: as text, as source or
        // as an image's alt text.
        let read = format!("{:?}", document.blocks);
        for mark in &marks {
            prop_assert!(read.contains(mark.as_str()), "{mark} of {source:?}: {read}");
        }
    }
}
