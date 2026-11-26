//! A browser's reading of the HTML in a document, as the answer that kiri's reading is compared with.
//!
//! comrak writes the HTML of a document, a browser-grade parser (`scraper`,
//! over html5ever) builds its tree, and that tree is mapped onto the shapes
//! kiri reads: centred blocks, `<details>`, headings and paragraphs. What kiri
//! reads, from `parse`, is mapped onto the same shapes. Only structure and
//! plain text are compared, not inline markup: comrak writes `<strong>` where
//! the outline has `**`, and a Markdown paragraph is a `<p>` of comrak's.
//!
//! The documents are made by a generator with a seed of its own, of the
//! elements of the table, whole, with blank lines anywhere. Where kiri
//! differs from a browser on purpose, they stay clear of it: a tag of a block
//! element is always at the start of a line, so that comrak reads it as an HTML
//! block and not as inline HTML in a paragraph; there is no broken HTML but
//! the `</p>` that may be left out; and the text has no Markdown.

use std::collections::BTreeMap;
use std::fmt::Write;

use scraper::{ElementRef, Html};

use crate::typeset::model::{Block, Inline};
use crate::typeset::parse::parse_all;

/// What a document is, as far as the comparison goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// What a centring element holds.
    Centred(Vec<Shape>),
    /// The text of the summary, and the blocks.
    Details(String, Vec<Shape>),
    Heading(u8, String),
    Paragraph(String),
    /// Kiri only: a block left as source, the name of the tag it starts with
    /// and the text outside its tags.
    Source(String, String),
}

/// `text` with runs of white space as one space and no white space at the
/// ends.
fn plain(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Pushes the paragraph that `run` is, if it holds any text, and empties it.
fn flush(run: &mut String, out: &mut Vec<Shape>) {
    let text = plain(run);
    run.clear();
    if !text.is_empty() {
        out.push(Shape::Paragraph(text));
    }
}

/// `shapes` as the blocks of a centring element: the ones it holds, in one
/// level, and none when it holds none.
fn centred(shapes: Vec<Shape>) -> Option<Shape> {
    let mut inner = Vec::new();
    for shape in shapes {
        match shape {
            Shape::Centred(more) => inner.extend(more),
            shape => inner.push(shape),
        }
    }
    (!inner.is_empty()).then_some(Shape::Centred(inner))
}

// ---------- the browser ----------

pub fn comrak_html(source: &str) -> String {
    let mut options = crate::typeset::parse::options();
    options.render.r#unsafe = true;
    comrak::markdown_to_html(source, &options)
}

fn is_centred(element: &ElementRef<'_>) -> bool {
    (element.value().attr("align")).is_some_and(|align| align.trim().eq_ignore_ascii_case("center"))
}

/// The text of `element`, with a space for each `<br>`.
fn text_of(element: ElementRef<'_>) -> String {
    let mut text = String::new();
    for child in element.children() {
        if let Some(inner) = ElementRef::wrap(child) {
            match inner.value().name() {
                "br" => text.push(' '),
                _ => text.push_str(&text_of(inner)),
            }
        } else if let Some(part) = child.value().as_text() {
            text.push_str(part);
        }
    }
    text
}

/// What a browser reads of the HTML `html`.
pub fn browser(html: &str) -> Vec<Shape> {
    let fragment = Html::parse_fragment(html);
    let mut out = Vec::new();
    let mut run = String::new();
    walk(fragment.root_element(), None, &mut out, &mut run);
    flush(&mut run, &mut out);
    out
}

/// The shapes in `element`, but for the child `skip`.
fn walk(
    element: ElementRef<'_>,
    skip: Option<ElementRef<'_>>,
    out: &mut Vec<Shape>,
    run: &mut String,
) {
    for child in element.children() {
        let Some(inner) = ElementRef::wrap(child) else {
            if let Some(part) = child.value().as_text() {
                run.push_str(part);
            }
            continue;
        };
        if skip.is_some_and(|skip| skip == inner) {
            continue;
        }
        let name = inner.value().name();
        match name {
            "br" => run.push(' '),
            "p" | "summary" => {
                flush(run, out);
                let text = plain(&text_of(inner));
                let shape = (!text.is_empty()).then_some(Shape::Paragraph(text));
                push_aligned(out, &inner, shape);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                flush(run, out);
                let level = name.as_bytes()[1] - b'0';
                let text = plain(&text_of(inner));
                let shape = (!text.is_empty()).then_some(Shape::Heading(level, text));
                push_aligned(out, &inner, shape);
            }
            "div" | "center" => {
                flush(run, out);
                let mut shapes = Vec::new();
                let mut own = String::new();
                walk(inner, None, &mut shapes, &mut own);
                flush(&mut own, &mut shapes);
                if name == "center" || is_centred(&inner) {
                    out.extend(centred(shapes));
                } else {
                    out.extend(shapes);
                }
            }
            "details" => {
                flush(run, out);
                let summary = inner
                    .children()
                    .filter_map(ElementRef::wrap)
                    .find(|child| child.value().name() == "summary");
                let mut shapes = Vec::new();
                let mut own = String::new();
                walk(inner, summary, &mut shapes, &mut own);
                flush(&mut own, &mut shapes);
                let summary = summary.map(|summary| plain(&text_of(summary)));
                let summary = summary.unwrap_or_else(|| "Details".to_owned());
                out.push(Shape::Details(summary, shapes));
            }
            // Other elements, such as `<b>`, are the text in them.
            _ => walk(inner, None, out, run),
        }
    }
}

/// Pushes `shape`, centred when the element has `align="center"`.
fn push_aligned(out: &mut Vec<Shape>, element: &ElementRef<'_>, shape: Option<Shape>) {
    let Some(shape) = shape else { return };
    match is_centred(element) {
        true => out.extend(centred(vec![shape])),
        false => out.push(shape),
    }
}

// ---------- kiri's reading ----------

/// The text of `inlines`: tags are no text, an image is its address.
fn inline_text(inlines: &[Inline]) -> String {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(part) | Inline::Code(part) => text.push_str(part),
            Inline::SoftBreak | Inline::LineBreak => text.push(' '),
            Inline::Image(image) => write!(text, "[img:{}]", image.url).unwrap(),
            Inline::Emphasis(inner)
            | Inline::Strong(inner)
            | Inline::Strikethrough(inner)
            | Inline::Kbd(inner)
            | Inline::Mark(inner)
            | Inline::Subscript(inner)
            | Inline::Superscript(inner)
            | Inline::Underline(inner)
            | Inline::Link { content: inner, .. } => text.push_str(&inline_text(inner)),
            Inline::Html(_) | Inline::Anchor(_) | Inline::Formula { .. } => {}
        }
    }
    text
}

/// `source` without its tags.
fn without_tags(source: &str) -> String {
    let (mut text, mut in_tag) = (String::new(), false);
    for c in source.chars() {
        match (c, in_tag) {
            ('<', _) => in_tag = true,
            ('>', true) => {
                in_tag = false;
                text.push(' ');
            }
            (c, false) => text.push(c),
            _ => {}
        }
    }
    text
}

fn kiri_blocks(blocks: &[Block]) -> Vec<Shape> {
    let mut out = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph(content) => {
                let text = plain(&inline_text(content));
                if !text.is_empty() {
                    out.push(Shape::Paragraph(text));
                }
            }
            Block::Heading { level, content, .. } => {
                out.push(Shape::Heading(*level, plain(&inline_text(content))));
            }
            Block::Centered(inner) => out.extend(centred(kiri_blocks(inner))),
            Block::Details { summary, blocks } => out.push(Shape::Details(
                plain(&inline_text(summary)),
                kiri_blocks(blocks),
            )),
            Block::Html(source) => {
                let text = plain(&without_tags(source));
                if !text.is_empty() {
                    let tag = (source.trim_start().strip_prefix('<'))
                        .map(|rest| rest.trim_start_matches('/'))
                        .and_then(|rest| rest.split(|c: char| !c.is_ascii_alphanumeric()).next())
                        .unwrap_or_default();
                    out.push(Shape::Source(tag.to_ascii_lowercase(), text));
                }
            }
            other => panic!("the comparison has no shape for {other:?}"),
        }
    }
    out
}

/// What kiri reads of `source`.
pub fn kiri(source: &str) -> Vec<Shape> {
    kiri_blocks(&parse_all(source).blocks)
}

// ---------- the documents ----------

/// A generator of numbers, with a seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

struct Generator {
    rng: Rng,
    /// Whether there is no blank line to be, as in a heading or a summary:
    /// one holds text, and what a blank line makes in it, a block of
    /// Markdown, is a fault of its own (P2 of the plan).
    tight: bool,
    /// The words made so far, which are numbered.
    words: usize,
    out: String,
}

impl Generator {
    fn word(&mut self) -> String {
        self.words += 1;
        format!("w{}", self.words)
    }

    /// The text between pieces: nothing, a space, a line break, or a blank
    /// line. A tag of a block element is at the start of a line.
    fn separate(&mut self, block_tag: bool) {
        let between = ["", " ", "\n", "\n", "\n\n", "\n\n"];
        let mut between = between[self.rng.below(6) as usize];
        if self.tight && between == "\n\n" {
            between = "\n";
        }
        if block_tag && !between.contains('\n') {
            between = "\n";
        }
        self.out.push_str(between);
    }

    /// A tag of a block element.
    fn tag(&mut self, tag: &str) {
        self.separate(true);
        self.out.push_str(tag);
    }

    /// Words, with a `<b>` or a `<br>` among them sometimes.
    fn text(&mut self) {
        self.separate(false);
        for at in 0..1 + self.rng.below(3) {
            if at > 0 {
                self.out.push(' ');
            }
            let word = self.word();
            match self.rng.below(6) {
                0 => write!(self.out, "<b>{word}</b>").unwrap(),
                1 => write!(self.out, "{word}<br>").unwrap(),
                _ => self.out.push_str(&word),
            }
        }
    }

    fn align(&mut self) -> &'static str {
        ["", " align=\"center\""][self.rng.below(2) as usize]
    }

    fn block(&mut self, depth: usize) {
        match self.rng.below(if depth < 3 { 11 } else { 6 }) {
            0..=2 => self.text(),
            3 => {
                self.separate(true);
                let word = self.word();
                write!(self.out, "## {word}").unwrap();
            }
            4 => {
                let align = self.align();
                self.tag(&format!("<p{align}>"));
                for _ in 0..1 + self.rng.below(2) {
                    self.text();
                }
                // A `<p>` may leave out its end tag.
                if self.rng.below(4) > 0 {
                    self.tag("</p>");
                }
            }
            5 => {
                let level = 1 + self.rng.below(2);
                let align = self.align();
                self.tag(&format!("<h{level}{align}>"));
                self.tight = true;
                self.text();
                self.tag(&format!("</h{level}>"));
                self.tight = false;
            }
            6..=8 => {
                let (name, align) = match self.rng.below(4) {
                    0 => ("center", ""),
                    _ => ("div", self.align()),
                };
                self.tag(&format!("<{name}{align}>"));
                self.blocks(depth + 1);
                self.tag(&format!("</{name}>"));
            }
            _ => {
                self.tag("<details>");
                if self.rng.below(3) > 0 {
                    self.tag("<summary>");
                    self.tight = true;
                    self.text();
                    self.tag("</summary>");
                    self.tight = false;
                }
                self.blocks(depth + 1);
                self.tag("</details>");
            }
        }
    }

    fn blocks(&mut self, depth: usize) {
        for _ in 0..self.rng.below(4) {
            self.block(depth);
        }
    }
}

/// The document of seed `seed`.
pub fn document(seed: u64) -> String {
    let mut generator = Generator {
        rng: Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1),
        tight: false,
        words: 0,
        out: String::new(),
    };
    for _ in 0..4 {
        generator.rng.next();
    }
    for _ in 0..1 + generator.rng.below(4) {
        generator.block(0);
    }
    generator.out.push('\n');
    generator.out
}

// ---------- the comparison ----------

/// The words that a shape centres, and the places that a shape shows: the
/// facts that tell why two readings of a document differ.
#[derive(Default, PartialEq, Eq)]
struct Facts {
    centred: BTreeMap<String, usize>,
    details: Vec<String>,
    headings: Vec<(u8, String)>,
    /// The tag that the first block left as source starts with.
    source: Option<String>,
}

fn facts(shapes: &[Shape], centred: bool, into: &mut Facts) {
    for shape in shapes {
        match shape {
            Shape::Centred(inner) => facts(inner, true, into),
            Shape::Details(summary, inner) => {
                into.details.push(summary.clone());
                facts(inner, false, into);
            }
            Shape::Heading(level, text) => {
                into.headings.push((*level, text.clone()));
                if centred {
                    for word in text.split(' ') {
                        *into.centred.entry(word.to_owned()).or_default() += 1;
                    }
                }
            }
            Shape::Paragraph(text) | Shape::Source(_, text) => {
                if let Shape::Source(tag, _) = shape {
                    into.source.get_or_insert_with(|| tag.clone());
                }
                if centred {
                    for word in text.split(' ') {
                        *into.centred.entry(word.to_owned()).or_default() += 1;
                    }
                }
            }
        }
    }
}

/// Why `kiri` differs from `browser`, as the first of these that is so.
fn why(browser: &[Shape], kiri: &[Shape]) -> String {
    let (mut b, mut k) = (Facts::default(), Facts::default());
    facts(browser, false, &mut b);
    facts(kiri, false, &mut k);
    if let Some(tag) = k.source {
        format!(
            "a block left as source, which a browser reads: {}",
            match tag.is_empty() {
                true => "starts with text".to_owned(),
                false => format!("starts with <{tag}>"),
            }
        )
    } else if b.centred != k.centred {
        "words centred or not centred".to_owned()
    } else if b.details != k.details {
        "details lost, made or cut".to_owned()
    } else if b.headings != k.headings {
        "headings lost or made".to_owned()
    } else {
        "the same words and places, in other paragraphs or another order".to_owned()
    }
}

/// How many documents of the corpus there are.
pub const DOCUMENTS: u64 = 600;

/// The result of reading the corpus: how many documents differ from a
/// browser's reading under `reading`, and why, with the least seed of each
/// reason.
pub fn compare(reading: fn(&str) -> Vec<Shape>) -> String {
    let mut reasons: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    let mut differ = 0;
    for seed in 0..DOCUMENTS {
        let source = document(seed);
        let (browser, read) = (browser(&comrak_html(&source)), reading(&source));
        if browser != read {
            differ += 1;
            reasons.entry(why(&browser, &read)).or_insert((0, seed)).0 += 1;
        }
    }
    let mut report = format!("{differ} of {DOCUMENTS} documents differ from a browser\n");
    for (reason, (count, first)) in reasons {
        writeln!(report, "  {count:4}  {reason} (first: seed {first})").unwrap();
    }
    report
}

/// Whether `source` reads as a browser reads it.
pub fn agrees(source: &str, reading: fn(&str) -> Vec<Shape>) -> bool {
    browser(&comrak_html(source)) == reading(source)
}
