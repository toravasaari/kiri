//! What the screen shows of images, read from the bytes the pager writes
//! and not from how it sends them.
//!
//! A test of a picture that plays should ask what the reader sees, not
//! which images and frames were sent: the same screen can come from the
//! whole part of a page as one animation or from a small image over a
//! still part. [`Shown`] reads the images in the bytes (`a=T` for the
//! first frame, `a=f` for the next ones), reads the placeholder cells of
//! each row (the id in the colour and the first diacritic, the row and
//! the column in the others, and a cell that names nothing from its left
//! neighbour of the same colour, as kitty works it out), and lays the
//! pixels of the images under the cells together as the terminal does.
//!
//! The cells are taken as exactly `cell` pixels and an image as exactly
//! as large as its cells, which the pager's tests arrange with
//! `cell_pixels`; an image kitty would scale to its cells is not
//! reproduced. Deletes are not applied: the last update of a run also
//! holds the quit, which deletes every image, and a test of what is
//! deleted reads the bytes. Output wrapped for tmux is not read.

use std::collections::HashMap;
use std::iter::Peekable;
use std::rc::Rc;
use std::str::Chars;

use base64::Engine;

use super::Fake;
use crate::present::kitty::graphics::{DIACRITICS, PLACEHOLDER};

/// A cell of a row that shows a piece of an image: the image's id, and the
/// row and column of the cell in the image's cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Cell {
    pub id: u32,
    pub row: u16,
    pub column: u16,
}

/// An image the pager sent: its size, and its frames, the first as it was
/// sent and the others as they were added.
#[derive(Debug, Clone)]
pub(super) struct Sent {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Rc<Vec<u8>>>,
}

/// The pixels of the cells that show an image, over the box that holds
/// them: RGBA, row by row, with nothing (alpha 0) where no image is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Picture {
    /// The first `rows` rows' bytes.
    pub fn top(&self, rows: usize) -> &[u8] {
        &self.rgba[..rows * self.width * 4]
    }
}

/// What the screen showed after each update the pager wrote.
pub(super) struct Shown {
    cell: (usize, usize),
    /// For each update, the cells of each row of the screen, by column.
    cells: Vec<Vec<Vec<Option<Cell>>>>,
    /// For each update, the images the terminal held, by id.
    images: Vec<HashMap<u32, Sent>>,
}

impl Shown {
    /// What `terminal` showed, with cells of `cell` pixels.
    pub fn of(terminal: &Fake, cell: (u32, u32)) -> Self {
        let frames = terminal.frames();
        let mut held: HashMap<u32, Sent> = HashMap::new();
        let mut images = Vec::new();
        for frame in &frames {
            read_images(frame, &mut held);
            images.push(held.clone());
        }
        let cells = (terminal.screens().iter())
            .map(|screen| screen.iter().map(|row| read_cells(row)).collect())
            .collect();
        Self {
            cell: (cell.0 as usize, cell.1 as usize),
            cells,
            images,
        }
    }

    /// How many updates there were.
    pub fn updates(&self) -> usize {
        self.cells.len()
    }

    /// The ids of the images the screen after update `update` shows, in
    /// the order of their first cells.
    pub fn ids(&self, update: usize) -> Vec<u32> {
        let mut ids = Vec::new();
        for cell in self.cells[update].iter().flatten().flatten() {
            if !ids.contains(&cell.id) {
                ids.push(cell.id);
            }
        }
        ids
    }

    /// The image `id` as the terminal held it after update `update`.
    pub fn image(&self, update: usize, id: u32) -> Option<&Sent> {
        self.images[update].get(&id)
    }

    /// The updates after which the screen shows an image.
    pub fn with_images(&self) -> Vec<usize> {
        (0..self.updates())
            .filter(|update| !self.ids(*update).is_empty())
            .collect()
    }

    /// The most frames any image shown after update `update` has.
    pub fn frames(&self, update: usize) -> usize {
        (self.ids(update).iter())
            .filter_map(|id| self.image(update, *id))
            .map(|image| image.frames.len())
            .max()
            .unwrap_or(0)
    }

    /// The pixels of the images on the screen after update `update`, with
    /// frame `frame` for each image that has several and its one frame for
    /// the others. `None` when the screen shows no image.
    pub fn picture(&self, update: usize, frame: usize) -> Option<Picture> {
        let rows = &self.cells[update];
        let found = |f: fn(&(usize, usize, Cell)) -> usize, last: bool| {
            let all = rows.iter().enumerate().flat_map(|(y, row)| {
                row.iter()
                    .enumerate()
                    .filter_map(move |(x, cell)| Some((y, x, (*cell)?)))
            });
            match last {
                true => all.map(|c| f(&c)).max(),
                false => all.map(|c| f(&c)).min(),
            }
        };
        let top = found(|c| c.0, false)?;
        let bottom = found(|c| c.0, true)?;
        let left = found(|c| c.1, false)?;
        let right = found(|c| c.1, true)?;
        let (cw, ch) = self.cell;
        let (width, height) = ((right - left + 1) * cw, (bottom - top + 1) * ch);
        let mut rgba = vec![0u8; width * height * 4];
        for (y, row) in rows.iter().enumerate() {
            for (x, cell) in row.iter().enumerate() {
                let Some(cell) = cell else { continue };
                let image = self.image(update, cell.id).unwrap_or_else(|| {
                    panic!(
                        "update {update} shows image {:#x}, which was not sent",
                        cell.id
                    )
                });
                let n = match image.frames.len() {
                    1 => 0,
                    count => {
                        assert!(frame < count, "image {:#x} has {count} frames", cell.id);
                        frame
                    }
                };
                let pixels = &image.frames[n];
                for dy in 0..ch {
                    for dx in 0..cw {
                        let (sx, sy) = (
                            usize::from(cell.column) * cw + dx,
                            usize::from(cell.row) * ch + dy,
                        );
                        if sx >= image.width as usize || sy >= image.height as usize {
                            continue;
                        }
                        let from = (sy * image.width as usize + sx) * 4;
                        let to = (((y - top) * ch + dy) * width + (x - left) * cw + dx) * 4;
                        rgba[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
                    }
                }
            }
        }
        Some(Picture {
            width,
            height,
            rgba,
        })
    }

    /// [`Shown::picture`] of the last update that shows an image.
    pub fn last_picture(&self, frame: usize) -> Picture {
        let last = *self.with_images().last().expect("a screen with an image");
        self.picture(last, frame).unwrap()
    }
}

/// Takes the images that `frame`, the bytes of one update, sends into
/// `held`.
fn read_images(frame: &str, held: &mut HashMap<u32, Sent>) {
    // The command being read: its keys, and the data so far.
    let mut open: Option<(HashMap<&str, &str>, String)> = None;
    for sequence in frame.split("\x1b_G").skip(1) {
        let body = sequence.split("\x1b\\").next().unwrap_or("");
        let (keys, data) = body.split_once(';').unwrap_or((body, ""));
        let keys: HashMap<&str, &str> = keys.split(',').filter_map(|k| k.split_once('=')).collect();
        // A chunk after the first has no id; one that names the action
        // too (`a=f`) is still a chunk.
        let command = match (keys.contains_key("i"), open.take()) {
            (false, Some((first, mut so_far))) => {
                so_far.push_str(data);
                (first, so_far)
            }
            _ => (keys.clone(), data.to_owned()),
        };
        if keys.get("m") == Some(&"1") {
            open = Some(command);
            continue;
        }
        let (keys, data) = command;
        let id: u32 = match keys.get("i").and_then(|id| id.parse().ok()) {
            Some(id) => id,
            None => continue,
        };
        let size = |key: &str| keys.get(key).and_then(|n| n.parse::<u32>().ok());
        match keys.get("a") {
            Some(&"T") => {
                let pixels = Rc::new(unpacked(&data));
                let (width, height) = (size("s").unwrap(), size("v").unwrap());
                let frames = vec![pixels];
                held.insert(
                    id,
                    Sent {
                        width,
                        height,
                        frames,
                    },
                );
            }
            Some(&"f") => {
                let image = held
                    .get_mut(&id)
                    .unwrap_or_else(|| panic!("a frame for image {id:#x}, which was not sent"));
                image.frames.push(Rc::new(unpacked(&data)));
            }
            _ => {}
        }
    }
}

/// The RGBA bytes of `data`, which is zlib-compressed and in base64.
fn unpacked(data: &str) -> Vec<u8> {
    use std::io::Read;
    let packed = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    let mut pixels = Vec::new();
    flate2::read::ZlibDecoder::new(&packed[..])
        .read_to_end(&mut pixels)
        .unwrap();
    pixels
}

/// The parameters of a control sequence whose `[` is read, and its last
/// character.
fn csi(chars: &mut Peekable<Chars<'_>>) -> (String, Option<char>) {
    let mut parameters = String::new();
    loop {
        match chars.next() {
            Some(c) if c.is_ascii_digit() || ";:?".contains(c) => parameters.push(c),
            other => return (parameters, other),
        }
    }
}

/// The numbers of the diacritics that follow a placeholder.
fn diacritics(chars: &mut Peekable<Chars<'_>>) -> Vec<u32> {
    let mut marks = Vec::new();
    while let Some(n) = chars
        .peek()
        .and_then(|next| DIACRITICS.iter().position(|d| d == next))
    {
        marks.push(n as u32);
        chars.next();
    }
    marks
}

/// The cells of `row`, by column, from the bytes written to it: `None`
/// for a cell that is not a placeholder.
fn read_cells(row: &str) -> Vec<Option<Cell>> {
    let mut cells: Vec<Option<Cell>> = Vec::new();
    // The colour the placeholders have now, which is the low 24 bits of
    // their id.
    let mut colour: Option<u32> = None;
    // The last cell written, if it was a placeholder, and its colour.
    let mut left: Option<(Cell, u32)> = None;
    let mut chars = row.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    let (parameters, last) = csi(&mut chars);
                    match last {
                        Some('m') => match parameters.as_str() {
                            "39" | "0" | "" => colour = None,
                            _ => {
                                let numbers: Vec<u32> = (parameters.split(';'))
                                    .filter_map(|n| n.parse().ok())
                                    .collect();
                                if let [38, 2, r, g, b] = numbers[..] {
                                    colour = Some(r << 16 | g << 8 | b);
                                }
                            }
                        },
                        Some('C') => {
                            let n: usize = parameters.parse().unwrap_or(1);
                            cells.resize(cells.len() + n, None);
                            left = None;
                        }
                        Some('K') => {}
                        other => panic!(
                            "a row with a cursor move the screen reader does not know: {other:?} {parameters:?} in {row:?}"
                        ),
                    }
                }
                // An OSC (a link) or an APC (a graphics command) ends at
                // the string terminator.
                Some(']' | '_') => {
                    while let Some(c) = chars.next() {
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            PLACEHOLDER => {
                let marks = diacritics(&mut chars);
                let low = colour.expect("a placeholder with no colour for its id");
                let named = |row: u32, column: u32, high: u32| Cell {
                    id: high << 24 | low,
                    row: row as u16,
                    column: column as u16,
                };
                let next = |left: Option<(Cell, u32)>| match left {
                    Some((cell, same)) if same == low => Some(cell),
                    _ => None,
                };
                let cell = match marks[..] {
                    [row, column, high] => named(row, column, high),
                    [row, column] => named(row, column, 0),
                    [row] => match next(left) {
                        Some(cell) if u32::from(cell.row) == row => Cell {
                            column: cell.column + 1,
                            ..cell
                        },
                        _ => named(row, 0, 0),
                    },
                    [] => match next(left) {
                        Some(cell) => Cell {
                            column: cell.column + 1,
                            ..cell
                        },
                        None => named(0, 0, 0),
                    },
                    _ => panic!("a placeholder with {} marks", marks.len()),
                };
                cells.push(Some(cell));
                left = Some((cell, low));
            }
            _ if c.is_control() => {}
            _ => {
                cells.push(None);
                left = None;
            }
        }
    }
    cells
}

#[test]
fn a_cell_that_names_nothing_is_the_one_after_its_left_neighbour_of_the_same_colour() {
    use crate::present::kitty::graphics::{Cells, Form, ImageId, write_row, write_row_linked};
    let id = ImageId::new(
        &[0; 4],
        1,
        1,
        Cells {
            columns: 1,
            rows: 1,
        },
        None,
    );
    let mut whole = Vec::new();
    write_row(&mut whole, id, 3, 2..6, Form::Whole).unwrap();
    let mut short = Vec::new();
    write_row(&mut short, id, 3, 2..6, Form::Short).unwrap();
    let (whole, short) = (
        String::from_utf8(whole).unwrap(),
        String::from_utf8(short).unwrap(),
    );
    let cells = read_cells(&whole);
    assert_eq!(cells.len(), 4);
    assert_eq!(cells[0].unwrap().row, 3);
    assert_eq!(cells[0].unwrap().column, 2);
    assert_eq!(cells[3].unwrap().column, 5);
    assert_eq!(read_cells(&short), cells);
    // A link in the middle starts a cell that names itself.
    let links = [crate::present::kitty::graphics::LinkRun {
        columns: 1..3,
        url: "https://example.org/",
    }];
    let mut linked = Vec::new();
    write_row_linked(&mut linked, id, 3, 2..6, &links, Form::Short).unwrap();
    assert_eq!(read_cells(&String::from_utf8(linked).unwrap()), cells);
}
