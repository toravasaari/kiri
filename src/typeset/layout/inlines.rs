//! The layout of inline content: text, emphasis, code, links, scripts, and
//! the images and formulas that are drawn in the text.
//!
//! This is the cell engine's side of the inline contract; reading mode's is
//! `reading/inlines.rs`.

use crate::typeset::color::Underline;
use crate::typeset::layout::script::{self, Script};
use crate::typeset::layout::{Layout, embed};
use crate::typeset::model::{Image, Inline};
use crate::typeset::size::VerticalAlign;
use crate::typeset::style::{Spaces, Span, Style, link_to, push};

impl Layout<'_> {
    pub(super) fn inlines(&self, inlines: &[Inline], style: &Style, out: &mut Vec<Span>) {
        for inline in inlines {
            self.inline(inline, style, out);
        }
    }

    pub(super) fn inline(&self, inline: &Inline, style: &Style, out: &mut Vec<Span>) {
        match inline {
            Inline::Text(text) => push(out, text, style),
            Inline::Code(code) => self.code(style, out, |style, out| push(out, code, style)),
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
                out.push(span);
            }
            Inline::Formula { written, .. } => self.inlines(written, style, out),
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
                out.push(span);
            }
            Inline::Image(Image { alt, .. }) => {
                let image_style = self.muted(Style {
                    italic: true,
                    ..style.clone()
                });
                push(
                    out,
                    &format!("[image: {}]", Inline::plain_text(alt)),
                    &image_style,
                );
            }
            Inline::SoftBreak => push(out, " ", style),
            Inline::LineBreak => push(out, "\n", style),
            Inline::Html(html) => push(out, html, &self.muted(style.clone())),
        }
    }

    /// A sub- or superscript: with text sizing, `content` at two thirds of
    /// the size around it, in the same rows, a sup at their top and a sub at
    /// their bottom. Without, in Unicode's `script` characters when it is
    /// plain text and every character has one, or else as it is. A script
    /// inside it is drawn as part of it.
    fn script(&self, script: Script, content: &[Inline], style: &Style, out: &mut Vec<Span>) {
        let content = script::without_scripts(content);
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
            Some(characters) => push(out, &characters, style),
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
