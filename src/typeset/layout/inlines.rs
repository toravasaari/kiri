//! The layout of inline content: text, emphasis, code, links, scripts, and
//! the images and formulas that are drawn in the text.
//!
//! This is the cell engine's side of the inline contract; reading mode's is
//! `reading/inlines.rs`.
//!
//! It says for each part of the text what of the block's text it shows.

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
            Inline::Link { url, content, .. } => self.link(url, content, style, out),
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
            Inline::Anchor(_) => {}
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
mod tests;
