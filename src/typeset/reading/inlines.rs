//! The inline layout of reading mode: the runs of styled text that a
//! paragraph's inlines make, with the pictures that stand in them.
//!
//! This is reading mode's side of the inline contract; the cell engine's is
//! `layout/inlines.rs`.

use std::rc::Rc;

use crate::typeset::model::Inline;
use crate::typeset::reading::page::{Look, Picture, Pictures};
use crate::typeset::reading::text::{InlinePicture, Run, Script};

/// Where the pictures in the text come from, and how wide a line they are
/// in.
#[derive(Clone, Copy)]
pub(super) struct Ink<'a> {
    pub(super) pictures: &'a dyn Pictures,
    pub(super) room: f32,
}

/// `content` as runs of styled text, as `look` colours links and code, with
/// the pictures in it as `ink` gives them.
pub(super) fn runs(content: &[Inline], look: &Look, ink: Ink) -> Vec<Run> {
    let mut runs = Vec::new();
    collect(content, &Run::default(), (look, ink), &mut runs);
    runs
}

#[expect(clippy::too_many_lines)]
fn collect(content: &[Inline], style: &Run, (look, ink): (&Look, Ink), runs: &mut Vec<Run>) {
    for inline in content {
        match inline {
            Inline::Text(text) | Inline::Html(text) => push(runs, text, style),
            Inline::Code(code) => push(
                runs,
                code,
                &Run {
                    code: true,
                    color: Some(look.code),
                    ..style.clone()
                },
            ),
            Inline::SoftBreak => push(runs, " ", style),
            Inline::LineBreak => push(runs, "\n", style),
            Inline::Emphasis(inner) => {
                collect(
                    inner,
                    &Run {
                        italic: true,
                        ..style.clone()
                    },
                    (look, ink),
                    runs,
                );
            }
            Inline::Strong(inner) => {
                collect(
                    inner,
                    &Run {
                        bold: true,
                        ..style.clone()
                    },
                    (look, ink),
                    runs,
                );
            }
            Inline::Strikethrough(inner) => {
                collect(
                    inner,
                    &Run {
                        strike: true,
                        ..style.clone()
                    },
                    (look, ink),
                    runs,
                );
            }
            Inline::Kbd(inner) => {
                let key = Run {
                    code: true,
                    color: Some(look.code),
                    ..style.clone()
                };
                collect(inner, &key, (look, ink), runs);
            }
            Inline::Link { url, content } => {
                let link = Run {
                    color: Some(look.link),
                    link: Some(Rc::from(url.as_str())),
                    ..style.clone()
                };
                collect(content, &link, (look, ink), runs);
            }
            Inline::Image(image) => {
                // A picture that is there stands in the line; otherwise
                // its alt text does.
                match ink.pictures.image(image, (ink.room, f32::MAX)) {
                    Picture::Ready { pixels, id, .. } => {
                        let picture = InlinePicture {
                            pixels,
                            id,
                            depth: 0.0,
                        };
                        runs.push(Run::picture(picture, style.link.clone()));
                    }
                    _ => collect(&image.alt, style, (look, ink), runs),
                }
            }
            Inline::Mark(inner) => {
                let mark = Run {
                    color: look.marked.foreground.or(style.color),
                    background: look.marked.background.or(style.background),
                    ..style.clone()
                };
                collect(inner, &mark, (look, ink), runs);
            }
            Inline::Subscript(inner) | Inline::Superscript(inner) => {
                let spacing = &look.spacing;
                let drop = match inline {
                    Inline::Subscript(_) => spacing.sub_drop,
                    _ => -spacing.sup_raise,
                };
                let script = Run {
                    script: Some(Script {
                        size: spacing.script_size,
                        drop,
                    }),
                    ..style.clone()
                };
                collect(inner, &script, (look, ink), runs);
            }
            Inline::Formula { source, written } => {
                // The picture of the formula stands in the line, as an
                // image does; without it the source does, as written.
                let code = format!("{source}\n");
                match ink.pictures.formula(&code, (ink.room, f32::MAX)) {
                    Picture::Ready {
                        pixels, id, depth, ..
                    } => {
                        let picture = InlinePicture { pixels, id, depth };
                        runs.push(Run::picture(picture, style.link.clone()));
                    }
                    _ => collect(written, style, (look, ink), runs),
                }
            }
        }
    }
}

/// Adds `text` in `style` to `runs`, to the last run when it has that
/// style.
pub(super) fn push(runs: &mut Vec<Run>, text: &str, style: &Run) {
    if text.is_empty() {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.picture.is_none() && same_style(last, style) => last.text.push_str(text),
        _ => runs.push(Run {
            text: text.to_owned(),
            ..style.clone()
        }),
    }
}

fn same_style(a: &Run, b: &Run) -> bool {
    (a.bold, a.italic, a.code, a.strike, a.color, &a.link)
        == (b.bold, b.italic, b.code, b.strike, b.color, &b.link)
        && (a.background, a.script) == (b.background, b.script)
}
