//! Printing the files, for output that is not a terminal's pager: each file
//! under its header as it comes, an image file read only then, and the
//! check that kitty holds the images that were sent.
//!
//! What does not belong here is the choice to print instead of paging
//! (`src/session/plan.rs`) nor the rendering itself (`src/render/`).

use std::cell::Cell;
use std::io::{self, Write};

use crate::images::{ImageKey, Images, Source};
use crate::present::encoder::Encoder;
use crate::present::output::Page;
use crate::render::Inputs;
use crate::session::plan;
use crate::typeset::caps::Capabilities;
use crate::typeset::model::Document;

use super::given::{Drawn, Given, Shown, image_file};
use super::loads::read_image_file;
use super::open::image_reader;
use super::route::{Show, tell_drawn, told, told_line};
use super::{Platform, tell};

/// Prints `given`, each file under its header, as it comes: an image file
/// is read only then, so that no more than one is held at once. A file
/// alone has no header. A warning about a file is told before it, and why
/// the diagram of a diagram file was not drawn after it; a file that cannot
/// be shown is told about at its place and noted in `failed`.
pub(super) fn print_files(show: &Show, given: Vec<Given>, inputs: &mut Inputs) -> io::Result<()> {
    let Show {
        context,
        settings,
        lone,
        failed,
    } = *show;
    let platform = context.platform;
    plan::printing(inputs.reading.as_mut().map(|reading| &mut reading.wide));
    let (animations, named) = (context.animations, context.animations_named);
    inputs.caps = plan::printing_animations(inputs.caps.clone(), animations, named);
    let mut out = crate::term::wrapped(platform.stdout(), &inputs.caps);
    let mut apart = false;
    let held = Held::new(platform);
    for (number, given) in given.into_iter().enumerate() {
        let name = given.name;
        let drawn = given.drawn;
        for warning in given.warnings.iter().filter(|_| !lone) {
            tell(platform, warning.clone().in_file(&name));
        }
        let image = |image: Source, name: String, inputs: &mut Inputs, out: &mut _| match lone {
            true => print_alone(image, Page::default(), inputs, &held, out),
            false => print_image(image, name, number, apart, inputs, &held, out),
        };
        let shown = match given.shown {
            Shown::Failed(error) => {
                told(platform, failed, error);
                continue;
            }
            Shown::Image { format, head, rest } => {
                let reader = image_reader(platform, given.file.as_deref(), head, rest)
                    .map_err(|error| format!("{name}: {error}"));
                match reader.and_then(|reader| {
                    read_image_file(context, &inputs.caps, &given.path, format, reader)
                }) {
                    Ok(read) => image(read, name, inputs, &mut out),
                    Err(error) => {
                        told(platform, failed, error);
                        continue;
                    }
                }
            }
            Shown::Read(read) => image(read, name, inputs, &mut out),
            // The diagram that was drawn, on the page of the theme.
            Shown::Document { .. } if lone && matches!(drawn, Some(Drawn::Done)) => {
                let key = ImageKey::from(name.as_str()).in_document(number);
                let drawn = inputs.images.remove(&key);
                let (background, foreground) = (inputs.theme.background, inputs.theme.foreground);
                let page = Page {
                    background,
                    foreground,
                };
                match drawn {
                    Some(Ok(image)) => print_alone(image, page, inputs, &held, &mut out),
                    _ => Ok(()),
                }
            }
            Shown::Document { document, .. } => {
                let stderr = settings.render_errors.stderr();
                for warning in inputs.images.warnings_in(number, stderr) {
                    tell(platform, told_line(lone, &name, warning));
                }
                let file = crate::typeset::model::File { name, document };
                print_file(&file, number, (apart, lone), inputs, &held, &mut out)
            }
            // What could not be drawn is told after it.
            Shown::Diagram { document, .. } => {
                let file = crate::typeset::model::File { name, document };
                print_file(&file, number, (apart, lone), inputs, &held, &mut out)
            }
            Shown::AsIs(text) => {
                let file = crate::typeset::model::File {
                    name,
                    document: Document::default(),
                };
                match lone {
                    true => platform.stdout().write_all(text.as_bytes()),
                    false => crate::render::render_file(&file, number, apart, inputs, &mut out)
                        .and_then(|_| out.write_all(text.as_bytes())),
                }
            }
        };
        apart = true;
        shown.and_then(|()| out.flush())?;
        if let Some(drawn) = &drawn {
            tell_drawn(platform, drawn, failed);
        }
    }
    Ok(())
}

/// Prints `file`, numbered `number` among the files shown, under its
/// header, or without one when it is `lone`, after the blank line that
/// comes between two files when `apart`, and checks its images as
/// [`Held::check`] does.
fn print_file(
    file: &crate::typeset::model::File,
    number: usize,
    (apart, lone): (bool, bool),
    inputs: &Inputs,
    held: &Held,
    out: &mut impl Write,
) -> io::Result<()> {
    let sent = match lone {
        true => crate::render::render(&file.document, inputs, out),
        false => crate::render::render_file(file, number, apart, inputs, out),
    }?;
    held.check(sent, &inputs.images, &inputs.caps, out)
}

/// Prints `image`, a file alone, as large as fits the window and without a
/// header, on `page`, and checks it as [`Held::check`] does.
fn print_alone(
    image: Source,
    page: Page,
    inputs: &Inputs,
    held: &Held,
    out: &mut impl Write,
) -> io::Result<()> {
    crate::render::render_image_on(image, &inputs.caps, page, out)
        .and_then(|(sent, images)| held.check(sent, &images, &inputs.caps, out))
}

/// Prints `image`, the image file `name` numbered `number` among the files
/// shown, under its header, as `render_file` prints a file, with the
/// images `inputs` holds, and checks it as [`Held::check`] does.
fn print_image(
    image: Source,
    name: String,
    number: usize,
    apart: bool,
    inputs: &mut Inputs,
    held: &Held,
    out: &mut impl Write,
) -> io::Result<()> {
    let key = ImageKey::from(name.as_str()).in_document(number);
    inputs.images.insert(key.clone(), Ok(image));
    let shown = crate::render::render_file(&image_file(name), number, apart, inputs, out)
        .and_then(|sent| held.check(sent, &inputs.images, &inputs.caps, out));
    inputs.images.remove(&key);
    shown
}

/// The check that kitty holds the images that were sent, for the files of
/// one run.
pub(super) struct Held<'a> {
    platform: &'a dyn Platform,
    /// Whether kitty did not answer the check of a file's images.
    silent: Cell<bool>,
}

impl<'a> Held<'a> {
    pub(super) fn new(platform: &'a dyn Platform) -> Self {
        Held {
            platform,
            silent: Cell::new(false),
        }
    }

    /// Flushes `out` and makes sure kitty holds the images `encoder` sent
    /// there, from `images`, to a terminal with `caps`. tmux drops output
    /// for a client that cannot keep up, and the images in it with it, so
    /// through tmux kiri asks kitty for them and sends again what it lost,
    /// a few times at most. Without tmux, nothing is asked. Once kitty did
    /// not answer at all, as from a pane tmux does not show, nothing more
    /// is asked, so that each file does not wait for it.
    pub(super) fn check(
        &self,
        mut encoder: Encoder,
        images: &Images,
        caps: &Capabilities,
        out: &mut impl Write,
    ) -> io::Result<()> {
        out.flush()?;
        for _ in 0..crate::present::encoder::AGAIN {
            let sent = encoder.take_sent();
            if sent.is_empty() || self.silent.get() {
                break;
            }
            let Some(lost) = self.platform.lost_images(&sent) else {
                self.silent.set(true);
                break;
            };
            for id in lost {
                encoder.send_again(id, images, caps, out)?;
            }
            out.flush()?;
        }
        Ok(())
    }
}
