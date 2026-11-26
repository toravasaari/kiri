//! Property tests of the reading of the HTML in a document: documents made
//! of the tags it reads, text with characters of more than one byte,
//! comments, blank lines and CRLF, in any order, at the top, in a quote or in
//! a list item, are read without a panic, with places on the source, and with
//! none of their words lost; every tag of them read or shown; and whole
//! documents of the elements of the table read as a browser reads them.

use proptest::prelude::*;

use super::browser;
use crate::testing::props_config;
use crate::typeset::parse::parse_all;

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
        let document = parse_all(&source);
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

/// Counts of the tags `name` in `text`: starts (`<name`) or ends
/// (`</name`), not `<namex`.
fn count_tags(text: &str, name: &str, end: bool) -> usize {
    let open = format!("<{}{name}", if end { "/" } else { "" });
    text.match_indices(&open)
        .filter(|(at, _)| {
            let after = text[at + open.len()..].chars().next();
            !after.is_some_and(|c| c.is_ascii_alphanumeric())
        })
        .count()
}

proptest! {
    #![proptest_config(props_config())]

    /// Every tag of a document, broken or not, is read or shown dim, and none
    /// is shown twice or made up.
    ///
    /// What can be said without reading the whole tree, from the debug text
    /// of what was read:
    ///
    /// - for eight tags, the copies shown dim are no more than the ones in
    ///   the source (none made up or shown twice);
    /// - a `<details>` of the source is a `Block::Details` or shown dim;
    ///   the same for `<summary>`, whose read form is a `Details` with a
    ///   summary of its own, so none is lost;
    /// - the end tags that were read are no more than the start tags that
    ///   were read, so a stray end tag that is lost is found;
    /// - the centring elements read are no more than the source's.
    ///
    /// What it leaves out: that every other tag is kept (a start tag of a
    /// `<p>`, `<div>`, `<center>`, `<a>` or heading that is lost, and an end
    /// tag that is lost while another is made up) and that a tag not shown is
    /// part of the element read: that needs a dump of the tree. The rows of
    /// the table test pin those.
    #[test]
    fn every_tag_of_any_document_is_read_or_shown_and_none_is_made_up(
        (source, _marks) in documents(),
    ) {
        let document = parse_all(&source);
        let shown = format!("{:?}", document.blocks);
        for name in ["p", "div", "center", "details", "summary", "a", "h1", "img"] {
            for end in [false, true] {
                let (in_source, in_reading) =
                    (count_tags(&source, name, end), count_tags(&shown, name, end));
                prop_assert!(
                    in_reading <= in_source,
                    "{} of {name}: {in_reading} shown, {in_source} in {source:?}: {shown}",
                    if end { "end tags" } else { "start tags" },
                );
            }
        }
        // An end tag is shown dim, or it ends an element whose start tag was
        // read: the end tags that were read are no more than the start tags
        // that were read. A stray end tag that is lost breaks this.
        for name in ["p", "div", "center", "details", "summary", "a", "h1"] {
            let read_starts = count_tags(&source, name, false) - count_tags(&shown, name, false);
            let read_ends = count_tags(&source, name, true) - count_tags(&shown, name, true);
            prop_assert!(
                read_ends <= read_starts,
                "{read_ends} end tags of {name} read, {read_starts} start tags: {source:?}: {shown}"
            );
        }
        // A `<summary>` that was read is a `Details` with a summary of its
        // own, so read and shown add up to the source.
        let summaries = shown.matches("Details {").count()
            - shown.matches("Details { summary: [Text(\"Details\")]").count();
        prop_assert_eq!(
            summaries + count_tags(&shown, "summary", false),
            count_tags(&source, "summary", false),
            "{:?}: {}",
            source,
            shown
        );
        // Centring elements that were read, `Centered(`, are no more than
        // the centring start tags of the source (`<center>` and `align=`).
        // No more is said: an empty one is read and leaves no block.
        let centring = |text: &str| count_tags(text, "center", false) + text.matches("align=").count();
        prop_assert!(
            shown.matches("Centered(").count() + centring(&shown) <= centring(&source),
            "{:?}: {}",
            source,
            shown
        );
        let read = shown.matches("Details {").count();
        let dim = count_tags(&shown, "details", false);
        prop_assert_eq!(
            read + dim,
            count_tags(&source, "details", false),
            "{:?}: {}",
            source,
            shown
        );
    }
}

/// One text: words, with a `<b>` or a `<br>` among them sometimes.
#[derive(Debug, Clone)]
struct Run(Vec<u8>);

#[derive(Debug, Clone)]
enum Node {
    /// A Markdown paragraph.
    Text(Run),
    /// `## word`.
    Atx,
    /// A `<p>` that may leave out its end tag.
    P {
        centred: bool,
        texts: Vec<Run>,
        closed: bool,
    },
    /// `<h1>` to `<h6>`, closed by the end tag of its own level.
    H { level: u8, centred: bool, text: Run },
    /// `<center>` or `<div>`.
    Wrap {
        center: bool,
        centred: bool,
        inner: Vec<Node>,
    },
    Details {
        summary: Option<Run>,
        inner: Vec<Node>,
    },
}

fn run() -> impl Strategy<Value = Run> {
    proptest::collection::vec(0u8..6, 1..4).prop_map(Run)
}

fn nodes() -> impl Strategy<Value = Vec<Node>> {
    let leaf = prop_oneof![
        3 => run().prop_map(Node::Text),
        1 => Just(Node::Atx),
        1 => (any::<bool>(), proptest::collection::vec(run(), 1..3), 0u8..4).prop_map(
            |(centred, texts, closed)| Node::P { centred, texts, closed: closed > 0 }
        ),
        1 => (1u8..7, any::<bool>(), run())
            .prop_map(|(level, centred, text)| Node::H { level, centred, text }),
    ];
    let node = leaf.prop_recursive(3, 24, 4, |node| {
        let inner = proptest::collection::vec(node, 0..4);
        prop_oneof![
            3 => (0u8..4, any::<bool>(), inner.clone()).prop_map(
                |(kind, centred, inner)| Node::Wrap { center: kind == 0, centred, inner }
            ),
            1 => (proptest::option::weighted(0.66, run()), inner)
                .prop_map(|(summary, inner)| Node::Details { summary, inner }),
        ]
    });
    proptest::collection::vec(node, 1..5)
}

/// Writes nodes as Markdown with HTML in it. `seps` are the texts between
/// pieces, used in turn. A tag of a block element is at the start of a line;
/// a heading or a summary holds no blank line, which would make a block of
/// Markdown in it (P2 of the plan).
struct Writer<'a> {
    seps: &'a [u8],
    at: usize,
    tight: bool,
    words: usize,
    out: String,
}

impl Writer<'_> {
    fn separate(&mut self, block_tag: bool) {
        let choice = self.seps.get(self.at).copied().unwrap_or(2);
        self.at += 1;
        let mut between = ["", " ", "\n", "\n", "\n\n", "\n\n"][usize::from(choice % 6)];
        if self.tight && between == "\n\n" {
            between = "\n";
        }
        if block_tag && !between.contains('\n') {
            between = "\n";
        }
        self.out.push_str(between);
    }

    fn tag(&mut self, tag: &str) {
        self.separate(true);
        self.out.push_str(tag);
    }

    fn text(&mut self, run: &Run) {
        self.separate(false);
        for (at, decoration) in run.0.iter().enumerate() {
            if at > 0 {
                self.out.push(' ');
            }
            self.words += 1;
            let word = format!("w{}", self.words);
            match decoration {
                0 => self.out.push_str(&format!("<b>{word}</b>")),
                1 => self.out.push_str(&format!("{word}<br>")),
                _ => self.out.push_str(&word),
            }
        }
    }

    fn tight_text(&mut self, run: &Run) {
        self.tight = true;
        self.text(run);
    }

    fn nodes(&mut self, nodes: &[Node]) {
        for node in nodes {
            self.node(node);
        }
    }

    fn node(&mut self, node: &Node) {
        let align = |centred: bool| if centred { " align=\"center\"" } else { "" };
        match node {
            Node::Text(run) => self.text(run),
            Node::Atx => {
                self.separate(true);
                self.words += 1;
                self.out.push_str(&format!("## w{}", self.words));
            }
            Node::P {
                centred,
                texts,
                closed,
            } => {
                self.tag(&format!("<p{}>", align(*centred)));
                for run in texts {
                    self.text(run);
                }
                if *closed {
                    self.tag("</p>");
                }
            }
            Node::H {
                level,
                centred,
                text,
            } => {
                self.tag(&format!("<h{level}{}>", align(*centred)));
                self.tight_text(text);
                self.tag(&format!("</h{level}>"));
                self.tight = false;
            }
            Node::Wrap {
                center,
                centred,
                inner,
            } => {
                let (name, align) = match center {
                    true => ("center", ""),
                    false => ("div", align(*centred)),
                };
                self.tag(&format!("<{name}{align}>"));
                self.nodes(inner);
                self.tag(&format!("</{name}>"));
            }
            Node::Details { summary, inner } => {
                self.tag("<details>");
                if let Some(summary) = summary {
                    self.tag("<summary>");
                    self.tight_text(summary);
                    self.tag("</summary>");
                    self.tight = false;
                }
                self.nodes(inner);
                self.tag("</details>");
            }
        }
    }
}

fn whole_documents() -> impl Strategy<Value = String> {
    (nodes(), proptest::collection::vec(0u8..6, 0..64)).prop_map(|(nodes, seps)| {
        let mut writer = Writer {
            seps: &seps,
            at: 0,
            tight: false,
            words: 0,
            out: String::new(),
        };
        writer.nodes(&nodes);
        writer.out.push('\n');
        writer.out
    })
}

proptest! {
    #![proptest_config(props_config())]

    /// A whole document of the elements of the table, with blank lines
    /// anywhere, is read as a browser reads it: what comrak writes for it,
    /// read by a browser-grade parser (`scraper`) and mapped onto centred
    /// blocks, details, headings and paragraphs (`browser.rs`), equals what
    /// `parse` reads, mapped onto the same shapes. Only structure and plain
    /// text are compared, not inline markup, and not images (the documents
    /// have none).
    ///
    /// The documents have headings of levels 1 to 6, `<p>`, `<div>`,
    /// `<center>`, `<details>` and `<summary>`, and Markdown paragraphs and
    /// `##` headings. They have no `<a>`, no images and no inline element
    /// but `<b>` and `<br>`; the rows of `rows.rs` cover those.
    ///
    /// The documents stay clear of where kiri differs from a browser on
    /// purpose (the plan, "Where kiri differs from a browser on purpose"):
    ///
    /// - Broken HTML is not repaired by kiri, so the documents are whole,
    ///   but for the `</p>` that may be left out, which a browser also
    ///   supplies at the next block.
    /// - A block tag inside a Markdown paragraph is inline HTML for comrak,
    ///   so it is dim in kiri and the paragraph is not cut, as a browser cuts
    ///   it. The documents put every block tag at the start of a line.
    /// - A list, a table or a quote in a centring element is drawn where it
    ///   is, a browser centres its text. The documents have none of the
    ///   three.
    /// - An unknown element is its dim tags and its content as text, where
    ///   GitHub hides it. The documents have only the elements of the table
    ///   (`<b>` and `<br>` apart, which only mark text).
    /// - Elements that are source for now (`<table>`, `<pre>` and others)
    ///   are not in the documents.
    /// - A heading closed by the end tag of another level (`<h2>T</h3>`) is
    ///   a heading for a browser, and an error with a dim end tag for kiri
    ///   (the maintainer, 2026-11-26; the row of `rows.rs` pins it). The
    ///   documents close every heading with its own level.
    /// - A centred `<p>` split by blank lines is stacked by kiri as
    ///   paragraphs, one under the other, where a browser draws its images
    ///   side by side on one line. The comparison ignores images, so this
    ///   property cannot judge that difference; the row "an image line in a
    ///   centred p that blank lines split is centred" pins kiri's side.
    #[test]
    fn a_whole_document_of_the_elements_reads_as_a_browser_reads_it(source in whole_documents()) {
        let browser = browser::browser(&browser::comrak_html(&source));
        let kiri = browser::kiri(&source);
        prop_assert!(browser == kiri, "{source:?}\nbrowser: {browser:#?}\nkiri: {kiri:#?}");
    }
}
