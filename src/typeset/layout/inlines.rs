//! The layout of inline content: text, emphasis, code, links, scripts, and
//! the images and formulas that are drawn in the text.
//!
//! This is the cell engine's side of the inline contract; reading mode's is
//! `reading/inlines.rs`.

use std::ops::Range;

use crate::typeset::color::Underline;
use crate::typeset::layout::script::{self, Script};
use crate::typeset::layout::{Layout, embed};
use crate::typeset::model::{Image, Inline};
use crate::typeset::size::VerticalAlign;
use crate::typeset::style::{Origin, Spaces, Span, Style, link_to, push, push_at, push_span};
use crate::typeset::text::{Cursor, inline_len};

/// The number of a text of a block, and a range of bytes in it.
type Doc = Option<(u32, Range<u32>)>;

impl Layout<'_> {
    /// Takes the next `len` bytes of the text being laid out: its number and
    /// the bytes, or none when what is laid out has no text.
    fn took(&self, len: usize) -> Doc {
        let mut cursor = self.cursor.get();
        let bytes = cursor.take(len);
        self.cursor.set(cursor);
        cursor.text().map(|text| (text, bytes))
    }

    /// Starts text number `text` of a block, or none, for what is laid out
    /// without one.
    pub(super) fn text_is(&self, text: Option<usize>) {
        self.cursor.set(text.map_or_else(Cursor::off, Cursor::new));
    }

    /// Appends `text` as it is in the document, where the cursor is.
    fn push_text(&self, out: &mut Vec<Span>, text: &str, style: &Style) {
        push_doc(out, text, style, self.took(text.len()));
    }

    /// What `lay_out` appends to `out`, with its text shown as the whole of
    /// `doc`: a form that is not the document's text, such as a formula as
    /// written or the characters of a superscript.
    fn whole(&self, out: &mut Vec<Span>, doc: &Doc, lay_out: impl FnOnce(&mut Vec<Span>)) {
        let before = self.cursor.replace(Cursor::off());
        let mut spans = Vec::new();
        lay_out(&mut spans);
        self.cursor.set(before);
        for span in spans {
            let shown = match (doc, span.embed.is_none()) {
                (Some((text, bytes)), true) => span.showing(Origin::whole(*text, bytes.clone())),
                _ => span,
            };
            push_span(out, shown);
        }
    }

    pub(super) fn inlines(&self, inlines: &[Inline], style: &Style, out: &mut Vec<Span>) {
        for inline in inlines {
            self.inline(inline, style, out);
        }
    }

    pub(super) fn inline(&self, inline: &Inline, style: &Style, out: &mut Vec<Span>) {
        match inline {
            Inline::Text(text) => self.push_text(out, text, style),
            Inline::Code(code) => {
                let doc = self.took(code.len());
                self.code(style, out, |style, out| push_doc(out, code, style, doc));
            }
            // A key is drawn as inline code is.
            Inline::Kbd(content) => {
                self.code(style, out, |style, out| self.inlines(content, style, out))
            }
            Inline::Emphasis(content) => self.inlines(
                content,
                &self.style(Style {
                    italic: true,
                    ..style.clone()
                }),
                out,
            ),
            Inline::Strong(content) => self.inlines(
                content,
                &self.style(Style {
                    bold: true,
                    ..style.clone()
                }),
                out,
            ),
            Inline::Strikethrough(content) => self.inlines(
                content,
                &self.style(Style {
                    strike: true,
                    ..style.clone()
                }),
                out,
            ),
            Inline::Formula { source, written }
                if let Some(span) = embed::formula(self, source, written, style) =>
            {
                out.push(self.picture(span, self.took(source.len())));
            }
            Inline::Formula { source, written } => {
                let doc = self.took(source.len());
                self.whole(out, &doc, |out| self.inlines(written, style, out));
            }
            Inline::Mark(content) => {
                let mark = self.look.mark;
                let style = if self.caps.color {
                    Style {
                        fg: mark.foreground.or(style.fg),
                        bg: mark.background.or(style.bg),
                        ..style.clone()
                    }
                } else {
                    Style {
                        reverse: true,
                        ..style.clone()
                    }
                };
                self.inlines(content, &self.style(style), out)
            }
            Inline::Subscript(content) => self.script(Script::Sub, content, style, out),
            Inline::Superscript(content) => self.script(Script::Sup, content, style, out),
            Inline::Link { url, content } => self.link(url, content, style, out),
            Inline::Image(image) if let Some(span) = embed::image(self, image, style) => {
                out.push(self.picture(span, self.took(inline_len(inline))));
            }
            Inline::Image(Image { alt, .. }) => {
                let doc = self.took(inline_len(inline));
                let image_style = self.muted(Style {
                    italic: true,
                    ..style.clone()
                });
                // The alt text is the document's; the frame is the layout's.
                push(out, "[image: ", &image_style);
                let alt = Inline::plain_text(alt);
                match doc.filter(|_| !alt.is_empty()) {
                    Some((text, bytes)) => {
                        let span = Span::new(&alt, image_style.clone());
                        push_span(out, span.showing(Origin::whole(text, bytes)));
                    }
                    None => push(out, &alt, &image_style),
                }
                push(out, "]", &image_style);
            }
            Inline::SoftBreak => self.push_text(out, " ", style),
            Inline::LineBreak => self.push_text(out, "\n", style),
            Inline::Html(html) => self.push_text(out, html, &self.muted(style.clone())),
        }
    }

    /// A sub- or superscript: with text sizing, `content` at two thirds of
    /// the size around it, in the same rows, a sup at their top and a sub at
    /// their bottom. Without, in Unicode's `script` characters when it is
    /// plain text and every character has one, or else as it is. A script
    /// inside it is drawn as part of it.
    fn script(&self, script: Script, content: &[Inline], style: &Style, out: &mut Vec<Span>) {
        let content = script::without_scripts(content);
        let length: usize = content.iter().map(inline_len).sum();
        if self.caps.text_sizing {
            let align = match script {
                Script::Sup => VerticalAlign::Top,
                Script::Sub => VerticalAlign::Bottom,
            };
            let style = Style {
                size: style.size.times(2.0 / 3.0, align),
                ..style.clone()
            };
            return self.inlines(&content, &style, out);
        }
        let text: Option<String> = (content.iter())
            .map(|inline| match inline {
                Inline::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        match text.and_then(|text| script::characters(script, &text)) {
            // The characters are not the text: they show all of it.
            Some(characters) => {
                let doc = self.took(length);
                self.whole(out, &doc, |out| push(out, &characters, style));
            }
            None => self.inlines(&content, style, out),
        }
    }

    /// Inline code, or what is drawn as it: `content` in the inline code
    /// colours, padded, or between backticks without colour.
    fn code(
        &self,
        style: &Style,
        out: &mut Vec<Span>,
        content: impl FnOnce(&Style, &mut Vec<Span>),
    ) {
        // Spaces in code are kept as written, so it copies as it is.
        let spaces = if self.look.inline_code.wrap {
            Spaces::Keep
        } else {
            Spaces::KeepTogether
        };
        let style = &Style {
            spaces,
            ..style.clone()
        };
        // Without colour nothing else would mark the code.
        if self.caps.styling && self.caps.color {
            let bg = self.inline_code_background().or(style.bg);
            let code_style = Style {
                fg: self.look.inline_code.color.or(style.fg),
                bg,
                ..style.clone()
            };
            // No-break spaces keep the padding attached to the code.
            let padding = if self.look.inline_code.padding && bg.is_some() {
                "\u{a0}"
            } else {
                ""
            };
            push(out, padding, &code_style);
            content(&code_style, out);
            push(out, padding, &code_style);
        } else {
            push(out, "`", style);
            content(style, out);
            push(out, "`", style);
        }
    }

    /// `span`, which draws the bytes `doc` of the text as a picture, shown
    /// as such.
    fn picture(&self, span: Span, doc: Doc) -> Span {
        match doc {
            Some((text, bytes)) => span.showing(Origin::picture(text, bytes)),
            None => span,
        }
    }

    fn link(&self, url: &str, content: &[Inline], style: &Style, out: &mut Vec<Span>) {
        // The pager joins a link that wraps over lines by its `Rc`, so all
        // the spans of one link have to share this one, also when the link
        // wraps; touching links to one address share one `Rc` (`link_to`).
        let link_style = self.style(Style {
            fg: self.look.link.or(style.fg),
            underline: Underline::Single,
            link: self.caps.hyperlinks.then(|| link_to(out, url)),
            ..style.clone()
        });
        self.inlines(content, &link_style, out);
        // Without clickable links, the target has to be visible.
        if !self.caps.hyperlinks && Inline::plain_text(content) != url {
            let target_style = self.muted(style.clone());
            push(out, &format!(" <{url}>"), &target_style);
        }
    }
}

/// Appends `text` that shows the bytes `doc` of the document as they are.
fn push_doc(out: &mut Vec<Span>, text: &str, style: &Style, doc: Doc) {
    match doc {
        Some((number, bytes)) => push_at(out, text, style, Origin::exact(number, bytes)),
        None => push(out, text, style),
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use crate::render::{FrontMatterStyle, Inputs};
    use crate::typeset::block::Blocks;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::driver::{Content, Stream, content};
    use crate::typeset::model::Document;
    use crate::typeset::parse::parse;
    use crate::typeset::style::Spaces;
    use crate::typeset::style::{Line, Origin};
    use crate::typeset::text::{DocumentText, Gap, Shown, excused_gap};

    fn inputs(width: u16, front_matter: FrontMatterStyle) -> Inputs {
        Inputs {
            front_matter,
            ..Inputs::new(Capabilities::kitty(width))
        }
    }

    /// Every origin on the lines, with the line's innermost block: those
    /// of the spans, and of the rows of images.
    fn origins(line: &Line) -> Vec<Origin> {
        let spans = line
            .spans
            .iter()
            .flat_map(|span| span.origins.iter().cloned());
        let image = line.image.iter().filter_map(|row| row.origin.clone());
        spans.chain(image).collect()
    }

    /// The fixtures and examples, and the widths and front matter styles
    /// they are laid out in.
    fn each_layout(mut check: impl FnMut(&str, &Document, &Stream, FrontMatterStyle)) {
        let styles = [
            FrontMatterStyle::Table,
            FrontMatterStyle::Hide,
            FrontMatterStyle::Code,
        ];
        for path in crate::testing::documents() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let document = parse(&std::fs::read_to_string(&path).unwrap());
            for width in [200, 40] {
                for style in styles {
                    let stream = content(Content::Document(&document), &inputs(width, style));
                    check(&format!("{name} at {width}"), &document, &stream, style);
                }
            }
        }
    }

    #[test]
    fn what_a_span_shows_of_the_document_is_what_the_text_it_names_says() {
        let mut exact = 0;
        let mut others = 0;
        each_layout(|name, document, stream, _| {
            let text = DocumentText::new([document]);
            for (number, line) in stream.lines.iter().enumerate() {
                let found = origins(line);
                if found.is_empty() {
                    continue;
                }
                let inner = line
                    .inner
                    .unwrap_or_else(|| panic!("{name}: line {number} shows text and has no block"));
                let texts = text.texts(0, stream.blocks.path(inner));
                for span in &line.spans {
                    for origin in &span.origins {
                        let at = format!("{name}: line {number}, {:?} {origin:?}", span.text);
                        let Some(piece) = texts.get(origin.text as usize) else {
                            panic!("{at}: the block has no such text");
                        };
                        assert!(origin.doc.end as usize <= piece.text.len(), "{at}");
                        assert!(origin.shown.end as usize <= span.text.len(), "{at}");
                        assert!(origin.shown.start < origin.shown.end, "{at}");
                        let shown =
                            &span.text[origin.shown.start as usize..origin.shown.end as usize];
                        let doc = &piece.text[origin.doc.start as usize..origin.doc.end as usize];
                        if origin.exact {
                            exact += 1;
                            // A tab in the text is shown as a space.
                            assert_eq!(shown, doc.replace('\t', " "), "{at}");
                        } else {
                            others += 1;
                        }
                    }
                }
                if let Some(origin) = line.image.as_ref().and_then(|row| row.origin.as_ref()) {
                    let piece = texts.get(origin.text as usize);
                    assert!(
                        piece.is_some_and(|piece| origin.doc.end as usize <= piece.text.len()),
                        "{name}: line {number}: {origin:?}"
                    );
                }
            }
        });
        assert!(exact > 5000 && others > 20, "{exact} {others}");
    }

    /// What of a text the lines show, by the number of the block and of the
    /// text: for each byte how a line shows it, if one does.
    fn shown_bytes(
        stream: &Stream,
        text: &DocumentText,
    ) -> Vec<(Vec<usize>, usize, Vec<Option<Shown>>)> {
        let mut seen: Vec<(Vec<usize>, usize, Vec<Option<Shown>>)> = Vec::new();
        for id in stream.blocks.ids() {
            let path = stream.blocks.path(id).to_vec();
            for (number, piece) in text.texts(0, &path).iter().enumerate() {
                seen.push((path.clone(), number, vec![None; piece.text.len()]));
            }
        }
        for (number, line) in stream.lines.iter().enumerate() {
            let Some(inner) = line.inner else { continue };
            let path = stream.blocks.path(inner);
            let image = line.image.iter().filter_map(|row| row.origin.clone());
            let in_spans = (line.spans.iter()).flat_map(|span| {
                let collapses = span.style.spaces == Spaces::Collapse;
                span.origins
                    .iter()
                    .cloned()
                    .map(move |origin| (origin, collapses))
            });
            for (origin, collapses) in in_spans.chain(image.map(|origin| (origin, false))) {
                let bytes: Range<usize> = origin.doc.start as usize..origin.doc.end as usize;
                if let Some((_, _, flags)) = (seen.iter_mut())
                    .find(|(at, text, _)| at == path && *text == origin.text as usize)
                {
                    flags[bytes].fill(Some(Shown {
                        line: number,
                        collapses,
                    }));
                }
            }
        }
        seen
    }

    /// What a layout may leave out of the text of the document, and the
    /// number of times it does in the fixtures and examples: an entry has
    /// to match somewhere, or the test fails.
    const LEFT_OUT: &[&str] = &[
        "a space or a line break at a place where a line wraps",
        "the text of hidden front matter",
        "the keys and values of front matter shown as code",
        "a space that comes right after another, which collapses",
        "the carriage return of a line ending",
    ];

    #[test]
    fn every_grapheme_of_the_text_is_shown_but_what_the_layout_names_it_leaves_out() {
        let mut left_out = [0usize; 5];
        let mut shown = 0;
        each_layout(|name, document, stream, style| {
            let text = DocumentText::new([document]);
            let blocks: Vec<_> = document.blocks();
            for (path, number, flags) in shown_bytes(stream, &text) {
                let piece = &text.texts(0, &path)[number];
                let front_matter = blocks.iter().any(|(at, block)| {
                    *at == path && matches!(block, crate::typeset::model::Block::FrontMatter(_))
                });
                for (at, c) in piece.text.char_indices() {
                    if flags[at..at + c.len_utf8()].iter().all(Option::is_some) {
                        shown += 1;
                        continue;
                    }
                    let why = if front_matter && style == FrontMatterStyle::Hide {
                        1
                    } else if front_matter && style == FrontMatterStyle::Code {
                        2
                    } else if let Some(gap) = excused_gap(&piece.text, &flags, at) {
                        match gap {
                            Gap::Wrap => 0,
                            Gap::Collapsed => 3,
                            Gap::CarriageReturn => 4,
                        }
                    } else {
                        panic!(
                            "{name}: {path:?} text {number}: {c:?} at {at} of {:?} is shown by no line",
                            piece.text
                        );
                    };
                    left_out[why] += 1;
                }
            }
        });
        for (count, what) in left_out.iter().zip(LEFT_OUT) {
            assert!(*count > 0, "nothing is left out as {what}");
        }
        assert!(shown > 20_000, "{shown}");
    }

    /// The table and the lines of `source` laid out in a kitty window
    /// `width` columns wide.
    fn lay_out(source: &str, width: u16, front_matter: FrontMatterStyle) -> (Document, Stream) {
        let document = parse(source);
        let stream = content(Content::Document(&document), &inputs(width, front_matter));
        (document, stream)
    }

    /// The path of the innermost block of the first line whose text has
    /// `find`.
    fn inner_of(stream: &Stream, find: &str) -> Vec<usize> {
        let line = (stream.lines.iter())
            .find(|line| crate::typeset::style::text(&line.spans).contains(find))
            .unwrap_or_else(|| panic!("no line has {find:?}"));
        let id = line.inner.expect("a line of a block");
        stream.blocks.path(id).to_vec()
    }

    #[test]
    fn a_line_knows_the_innermost_block_it_shows() {
        let source = "> - a paragraph in a list in a quote\n\
                      > - another item\n\n\
                      > [!NOTE]\n> ## A heading in an alert\n\n\
                      1. an item\n\n   ```\n   code in an item\n   ```\n\n\
                      <details><summary>The summary</summary>\n\nInside details.\n\n</details>\n";
        let (_, stream) = lay_out(source, 80, FrontMatterStyle::Table);
        assert_eq!(inner_of(&stream, "a paragraph in a list"), [0, 0, 0, 0]);
        assert_eq!(inner_of(&stream, "another item"), [0, 0, 1, 0]);
        assert_eq!(inner_of(&stream, "A heading in an alert"), [1, 0]);
        assert_eq!(inner_of(&stream, "code in an item"), [2, 0, 1]);
        assert_eq!(inner_of(&stream, "The summary"), [3]);
        assert_eq!(inner_of(&stream, "Inside details"), [3, 0]);
        // The block of a line stays the top-level block or `<details>`.
        let line = (stream.lines.iter())
            .find(|line| crate::typeset::style::text(&line.spans).contains("a paragraph in a list"))
            .unwrap();
        assert_eq!(stream.blocks.path(line.block.unwrap()), [0]);
    }

    #[test]
    fn the_blank_lines_in_a_container_are_its_own_and_between_blocks_have_none() {
        let (_, stream) = lay_out("> one\n>\n> two\n\nafter\n", 80, FrontMatterStyle::Table);
        let path = |id| stream.blocks.path(id).to_vec();
        let of = |line: usize| {
            let line = &stream.lines[line];
            (line.block.map(path), line.inner.map(path))
        };
        // The line of the quote's bar between its paragraphs belongs to the
        // quote; the blank line after the quote, to no block.
        assert_eq!(of(1), (Some(vec![0]), Some(vec![0])));
        assert_eq!(of(2), (Some(vec![0]), Some(vec![0, 1])));
        assert_eq!(of(3), (None, None));
    }

    #[test]
    fn the_table_has_every_block_in_the_documents_order_with_hidden_front_matter_hidden() {
        let source = "---\ntitle: a\n---\n\n# Head\n\n> quote\n\n- item\n";
        let (document, hidden) = lay_out(source, 80, FrontMatterStyle::Hide);
        let paths: Vec<Vec<usize>> = (hidden.blocks.ids())
            .map(|id| hidden.blocks.path(id).to_vec())
            .collect();
        let known: Vec<Vec<usize>> = document
            .blocks()
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert_eq!(paths, known);
        let front_matter = hidden.blocks.ids().next().unwrap();
        assert!(hidden.blocks.is_hidden(front_matter));
        assert!(
            hidden
                .lines
                .iter()
                .all(|line| line.inner != Some(front_matter))
        );
        let (_, shown) = lay_out(source, 80, FrontMatterStyle::Table);
        let first = shown.blocks.ids().next().unwrap();
        assert!(!shown.blocks.is_hidden(first));
        assert!(shown.lines.iter().any(|line| line.inner == Some(first)));
    }

    #[test]
    fn what_the_layout_makes_up_shows_nothing_of_the_document() {
        let source = "- bullet\n- [x] done\n\n3. three\n\n\
                      > quoted\n\n[!NOTE]\n\n\
                      <details><summary>Sum</summary>\n\nbody\n\n</details>\n\n\
                      ![alt](x.png) and [a link](https://example.com/a)\n\n\
                      ```rust\ncode\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        let caps = Capabilities {
            hyperlinks: false,
            text_sizing: false,
            ..Capabilities::kitty(60)
        };
        let document = parse(source);
        let stream = content(Content::Document(&document), &Inputs::new(caps));
        let made_up = [
            "•",
            "☑",
            "3.",
            "▼",
            "[image: ",
            "] ",
            " <https://example.com/a>",
            "rust",
            "│",
            "┌",
            "─",
        ];
        let mut seen = 0;
        for line in &stream.lines {
            for span in line.lead.iter().chain(&line.spans).chain(&line.trail) {
                for find in made_up {
                    for (at, _) in span.text.match_indices(find) {
                        // The language label is the layout's, not the
                        // document's; the frame of an alt text is too.
                        for byte in at..at + find.len() {
                            assert!(
                                span.origins.of(byte).is_none() || find == "] ",
                                "{find:?} in {:?} shows the document: {:?}",
                                span.text,
                                span.origins
                            );
                        }
                        seen += 1;
                    }
                }
            }
        }
        assert!(seen >= 6, "{seen}");
    }

    #[test]
    fn a_file_header_and_a_notes_text_are_the_layouts() {
        use crate::typeset::model::{Block, File};
        let shown = [File {
            name: "dir/a.md".into(),
            document: parse("Text\n"),
        }];
        let stream = crate::typeset::driver::files(&shown, &Inputs::new(Capabilities::kitty(60)));
        let header = stream
            .lines
            .iter()
            .take_while(|line| !crate::typeset::style::text(&line.spans).contains("Text"))
            .count();
        assert!(header >= 3);
        assert!(
            stream.lines[..header]
                .iter()
                .all(|line| origins(line).is_empty())
        );
        let note = Document::new(vec![
            Block::Note("a note".into()),
            Block::Error("an error".into()),
        ]);
        let stream = content(
            Content::Document(&note),
            &Inputs::new(Capabilities::kitty(60)),
        );
        assert!(stream.lines.iter().all(|line| origins(line).is_empty()));
        let _: &Blocks = &stream.blocks;
    }

    #[test]
    fn the_rows_of_a_picture_stand_in_the_place_of_the_text_they_replace() {
        use crate::typeset::images::{ImageKey, Source};
        let loaded = |key: ImageKey| {
            let mut inputs = Inputs::new(Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(60)
            });
            let key = inputs.drawn(key);
            inputs
                .images
                .insert(key, Ok(Source::Raster(::image::RgbaImage::new(40, 40))));
            inputs
        };
        let origin_of = |source: &str, inputs: &Inputs| {
            let document = parse(source);
            let stream = content(Content::Document(&document), inputs);
            let rows: Vec<_> = stream
                .lines
                .iter()
                .filter_map(|line| line.image.as_ref())
                .collect();
            assert!(!rows.is_empty(), "{source:?} draws a picture");
            let first = rows[0].origin.clone().expect("an origin");
            assert!(rows.iter().all(|row| row.origin.as_ref() == Some(&first)));
            first
        };
        // A lone image stands in the place of its alt text, which is the
        // paragraph's text.
        let image = origin_of("![the alt](p.png)\n", &loaded(ImageKey::from("p.png")));
        assert!(image.picture);
        assert_eq!((image.text, image.doc), (0, 0..7));
        // A drawn diagram, of the code in it.
        let diagram = origin_of(
            "```mermaid\nflowchart LR\n  a --> b\n```\n",
            &loaded(ImageKey::rendered("mermaid", "flowchart LR\n  a --> b\n")),
        );
        assert!(diagram.picture);
        assert_eq!((diagram.text, diagram.doc), (0, 0..22));
    }

    #[test]
    fn an_image_drawn_in_a_line_stands_in_the_place_of_its_alt_text() {
        use crate::typeset::images::{ImageKey, Source};
        let mut inputs = Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(60)
        });
        let key = inputs.drawn(ImageKey::from("p.png"));
        inputs
            .images
            .insert(key, Ok(Source::Raster(::image::RgbaImage::new(20, 20))));
        let document = parse("before ![the alt](p.png) after\n");
        let stream = content(Content::Document(&document), &inputs);
        let embeds: Vec<_> = (stream.lines.iter())
            .flat_map(|line| &line.spans)
            .filter(|span| span.embed.is_some())
            .collect();
        let [embed] = &embeds[..] else {
            panic!("one embed: {embeds:?}")
        };
        let origin = embed.origins.of(0).expect("an origin");
        assert!(origin.picture);
        assert_eq!((origin.text, origin.doc.clone()), (0, 7..14));
    }
}
