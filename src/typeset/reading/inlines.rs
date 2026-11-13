//! The inline layout of reading mode: the runs of styled text that a
//! paragraph's inlines make, with the pictures that stand in them.
//!
//! This is reading mode's side of the inline contract; the cell engine's is
//! `layout/inlines.rs`.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use crate::typeset::model::Inline;
use crate::typeset::reading::page::{Look, Picture, Pictures};
use crate::typeset::reading::text::{InlinePicture, Run, Script};
use crate::typeset::style::{Origin, Origins};
use crate::typeset::text::{Cursor, inline_len};

/// The number of a text of a block and a range of bytes in it.
type Doc = Option<(u32, Range<u32>)>;

/// Where the pictures in the text come from, and how wide a line they are
/// in.
#[derive(Clone, Copy)]
pub(super) struct Ink<'a> {
    pub(super) pictures: &'a dyn Pictures,
    pub(super) room: f32,
    /// Where the inlines are in the text of their block.
    pub(super) cursor: &'a Cell<Cursor>,
}

impl Ink<'_> {
    /// Takes the next `len` bytes of the text being laid out: its number and
    /// the bytes, or none when what is laid out has no text.
    fn took(&self, len: usize) -> Doc {
        let mut cursor = self.cursor.get();
        let bytes = cursor.take(len);
        self.cursor.set(cursor);
        cursor.text().map(|text| (text, bytes))
    }
}

/// `content` as runs of styled text, as `look` colours links and code, with
/// the pictures in it as `ink` gives them.
pub(super) fn runs(content: &[Inline], look: &Look, ink: Ink) -> Vec<Run> {
    let mut runs = Vec::new();
    runs_into(&mut runs, content, look, ink);
    runs
}

/// [`runs`], added to `runs`, to its last run when it has the style.
pub(super) fn runs_into(runs: &mut Vec<Run>, content: &[Inline], look: &Look, ink: Ink) {
    collect(content, &Run::default(), (look, ink), runs);
}

#[expect(clippy::too_many_lines)]
fn collect(content: &[Inline], style: &Run, (look, ink): (&Look, Ink), runs: &mut Vec<Run>) {
    for inline in content {
        match inline {
            Inline::Text(text) | Inline::Html(text) => {
                push_from(runs, text, style, ink.took(text.len()));
            }
            Inline::Code(code) => push_from(
                runs,
                code,
                &Run {
                    code: true,
                    color: Some(look.code),
                    ..style.clone()
                },
                ink.took(code.len()),
            ),
            Inline::SoftBreak => push_from(runs, " ", style, ink.took(1)),
            Inline::LineBreak => push_from(runs, "\n", style, ink.took(1)),
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
                        let doc = ink.took(inline_len(inline));
                        runs.push(stands_in(Run::picture(picture, style.link.clone()), doc));
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
                let doc = ink.took(source.len());
                match ink.pictures.formula(&code, (ink.room, f32::MAX)) {
                    Picture::Ready {
                        pixels, id, depth, ..
                    } => {
                        let picture = InlinePicture { pixels, id, depth };
                        runs.push(stands_in(Run::picture(picture, style.link.clone()), doc));
                    }
                    _ => {
                        // As written, it shows all of the source, whatever
                        // it is cut into.
                        let before = ink.cursor.replace(Cursor::off());
                        let mut written_runs = Vec::new();
                        collect(written, style, (look, ink), &mut written_runs);
                        ink.cursor.set(before);
                        for run in written_runs {
                            push_run(runs, whole(run, &doc));
                        }
                    }
                }
            }
        }
    }
}

/// Adds `text` in `style` to `runs`, to the last run when it has that
/// style. It shows nothing of the document.
pub(super) fn push(runs: &mut Vec<Run>, text: &str, style: &Run) {
    push_from(runs, text, style, None);
}

/// Adds `text` in `style`, which shows what `from` says of the document.
pub(super) fn push_with(runs: &mut Vec<Run>, text: &str, style: &Run, from: Origins) {
    if !text.is_empty() {
        let run = Run {
            text: text.to_owned(),
            from,
            ..style.clone()
        };
        push_run(runs, run);
    }
}

/// Adds `text`, which is the bytes `doc` of the document, in `style`.
fn push_from(runs: &mut Vec<Run>, text: &str, style: &Run, doc: Doc) {
    if text.is_empty() {
        return;
    }
    let from = match doc {
        Some((number, bytes)) => {
            let origin = Origin {
                shown: 0..text.len() as u32,
                ..Origin::exact(number, bytes)
            };
            Origins::from_origins([origin])
        }
        None => Origins::default(),
    };
    push_run(
        runs,
        Run {
            text: text.to_owned(),
            from,
            ..style.clone()
        },
    );
}

/// Adds `run` to `runs`, to the last run when it has its style.
fn push_run(runs: &mut Vec<Run>, run: Run) {
    match runs.last_mut() {
        Some(last) if last.picture.is_none() && run.picture.is_none() && same_style(last, &run) => {
            let at = last.text.len();
            last.text.push_str(&run.text);
            last.from.join(run.from, at);
        }
        _ => runs.push(run),
    }
}

/// `run`, a picture, as standing in the place of the bytes `doc` of the
/// document.
fn stands_in(run: Run, doc: Doc) -> Run {
    match doc {
        Some((number, bytes)) => Run {
            from: Origins::from_origins([Origin::picture(number, bytes)]),
            ..run
        },
        None => run,
    }
}

/// `run`, text of a form that is not the document's, as showing all of the
/// bytes `doc`.
fn whole(run: Run, doc: &Doc) -> Run {
    match doc {
        Some((number, bytes)) if run.picture.is_none() && !run.text.is_empty() => Run {
            from: Origins::from_origins([Origin {
                shown: 0..run.text.len() as u32,
                ..Origin::whole(*number, bytes.clone())
            }]),
            ..run
        },
        _ => run,
    }
}

fn same_style(a: &Run, b: &Run) -> bool {
    (a.bold, a.italic, a.code, a.strike, a.color, &a.link)
        == (b.bold, b.italic, b.code, b.strike, b.color, &b.link)
        && (a.background, a.script) == (b.background, b.script)
}
