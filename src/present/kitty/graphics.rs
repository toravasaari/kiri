//! kitty's graphics protocol: sending images and showing them with Unicode
//! placeholders.
//!
//! An image is sent once and then drawn as rows of placeholder characters,
//! which the terminal replaces with the image. The placeholders are ordinary
//! text, so the image scrolls with the document and survives in the
//! scrollback. This module only encodes: it takes pixels already scaled to
//! their final size and the cells they cover, and knows nothing about
//! layout, so the renderer can use another encoder for terminals without
//! this protocol.
//!
//! <https://sw.kovidgoyal.net/kitty/graphics-protocol/>

use std::io::{self, Write};
use std::ops::Range;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use flate2::Compression;
use flate2::write::ZlibEncoder;

/// The start of every sequence of the protocol, an APC with `G`: `ESC _ G`.
/// It ends with ST, `ESC \`.
pub const START: &str = "\x1b_G";

/// How hard the pixels of an image are compressed before they are sent: the
/// time it takes against the bytes that cross the link to the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    /// Level 2, 4 to 6 ms for a part of a page where level 6 takes 10 to
    /// 13, and about 40 % more bytes. For a terminal on the same machine,
    /// where bytes are cheap, but not free: Ghostty reads them slower than
    /// level 2 saves in compression time.
    Fast,
    /// Level 6, for a link where bytes cost more than the time: ssh, or
    /// tmux, which holds every byte it passes in a budget.
    Thorough,
    /// A level that the reader chose with `KIRI_IMAGE_COMPRESSION`.
    Level(u32),
}

impl Effort {
    fn level(self) -> Compression {
        match self {
            Self::Fast => Compression::new(2),
            Self::Thorough => Compression::new(6),
            Self::Level(level) => Compression::new(level),
        }
    }
}

/// The protocol's limit for the base64 data in one escape sequence.
const CHUNK: usize = 4096;

/// The placeholder character the terminal replaces with a piece of an image.
pub const PLACEHOLDER: char = '\u{10EEEE}';

/// Combining characters that number the row and column of each placeholder
/// and carry its image id's most significant byte, in the order of kitty's
/// `rowcolumn-diacritics.txt`.
/// <https://sw.kovidgoyal.net/kitty/graphics-protocol/#unicode-placeholders>
#[rustfmt::skip]
pub const DIACRITICS: [char; 297] = [
    '\u{0305}', '\u{030D}', '\u{030E}', '\u{0310}', '\u{0312}', '\u{033D}', '\u{033E}', '\u{033F}',
    '\u{0346}', '\u{034A}', '\u{034B}', '\u{034C}', '\u{0350}', '\u{0351}', '\u{0352}', '\u{0357}',
    '\u{035B}', '\u{0363}', '\u{0364}', '\u{0365}', '\u{0366}', '\u{0367}', '\u{0368}', '\u{0369}',
    '\u{036A}', '\u{036B}', '\u{036C}', '\u{036D}', '\u{036E}', '\u{036F}', '\u{0483}', '\u{0484}',
    '\u{0485}', '\u{0486}', '\u{0487}', '\u{0592}', '\u{0593}', '\u{0594}', '\u{0595}', '\u{0597}',
    '\u{0598}', '\u{0599}', '\u{059C}', '\u{059D}', '\u{059E}', '\u{059F}', '\u{05A0}', '\u{05A1}',
    '\u{05A8}', '\u{05A9}', '\u{05AB}', '\u{05AC}', '\u{05AF}', '\u{05C4}', '\u{0610}', '\u{0611}',
    '\u{0612}', '\u{0613}', '\u{0614}', '\u{0615}', '\u{0616}', '\u{0617}', '\u{0657}', '\u{0658}',
    '\u{0659}', '\u{065A}', '\u{065B}', '\u{065D}', '\u{065E}', '\u{06D6}', '\u{06D7}', '\u{06D8}',
    '\u{06D9}', '\u{06DA}', '\u{06DB}', '\u{06DC}', '\u{06DF}', '\u{06E0}', '\u{06E1}', '\u{06E2}',
    '\u{06E4}', '\u{06E7}', '\u{06E8}', '\u{06EB}', '\u{06EC}', '\u{0730}', '\u{0732}', '\u{0733}',
    '\u{0735}', '\u{0736}', '\u{073A}', '\u{073D}', '\u{073F}', '\u{0740}', '\u{0741}', '\u{0743}',
    '\u{0745}', '\u{0747}', '\u{0749}', '\u{074A}', '\u{07EB}', '\u{07EC}', '\u{07ED}', '\u{07EE}',
    '\u{07EF}', '\u{07F0}', '\u{07F1}', '\u{07F3}', '\u{0816}', '\u{0817}', '\u{0818}', '\u{0819}',
    '\u{081B}', '\u{081C}', '\u{081D}', '\u{081E}', '\u{081F}', '\u{0820}', '\u{0821}', '\u{0822}',
    '\u{0823}', '\u{0825}', '\u{0826}', '\u{0827}', '\u{0829}', '\u{082A}', '\u{082B}', '\u{082C}',
    '\u{082D}', '\u{0951}', '\u{0953}', '\u{0954}', '\u{0F82}', '\u{0F83}', '\u{0F86}', '\u{0F87}',
    '\u{135D}', '\u{135E}', '\u{135F}', '\u{17DD}', '\u{193A}', '\u{1A17}', '\u{1A75}', '\u{1A76}',
    '\u{1A77}', '\u{1A78}', '\u{1A79}', '\u{1A7A}', '\u{1A7B}', '\u{1A7C}', '\u{1B6B}', '\u{1B6D}',
    '\u{1B6E}', '\u{1B6F}', '\u{1B70}', '\u{1B71}', '\u{1B72}', '\u{1B73}', '\u{1CD0}', '\u{1CD1}',
    '\u{1CD2}', '\u{1CDA}', '\u{1CDB}', '\u{1CE0}', '\u{1DC0}', '\u{1DC1}', '\u{1DC3}', '\u{1DC4}',
    '\u{1DC5}', '\u{1DC6}', '\u{1DC7}', '\u{1DC8}', '\u{1DC9}', '\u{1DCB}', '\u{1DCC}', '\u{1DD1}',
    '\u{1DD2}', '\u{1DD3}', '\u{1DD4}', '\u{1DD5}', '\u{1DD6}', '\u{1DD7}', '\u{1DD8}', '\u{1DD9}',
    '\u{1DDA}', '\u{1DDB}', '\u{1DDC}', '\u{1DDD}', '\u{1DDE}', '\u{1DDF}', '\u{1DE0}', '\u{1DE1}',
    '\u{1DE2}', '\u{1DE3}', '\u{1DE4}', '\u{1DE5}', '\u{1DE6}', '\u{1DFE}', '\u{20D0}', '\u{20D1}',
    '\u{20D4}', '\u{20D5}', '\u{20D6}', '\u{20D7}', '\u{20DB}', '\u{20DC}', '\u{20E1}', '\u{20E7}',
    '\u{20E9}', '\u{20F0}', '\u{2CEF}', '\u{2CF0}', '\u{2CF1}', '\u{2DE0}', '\u{2DE1}', '\u{2DE2}',
    '\u{2DE3}', '\u{2DE4}', '\u{2DE5}', '\u{2DE6}', '\u{2DE7}', '\u{2DE8}', '\u{2DE9}', '\u{2DEA}',
    '\u{2DEB}', '\u{2DEC}', '\u{2DED}', '\u{2DEE}', '\u{2DEF}', '\u{2DF0}', '\u{2DF1}', '\u{2DF2}',
    '\u{2DF3}', '\u{2DF4}', '\u{2DF5}', '\u{2DF6}', '\u{2DF7}', '\u{2DF8}', '\u{2DF9}', '\u{2DFA}',
    '\u{2DFB}', '\u{2DFC}', '\u{2DFD}', '\u{2DFE}', '\u{2DFF}', '\u{A66F}', '\u{A67C}', '\u{A67D}',
    '\u{A6F0}', '\u{A6F1}', '\u{A8E0}', '\u{A8E1}', '\u{A8E2}', '\u{A8E3}', '\u{A8E4}', '\u{A8E5}',
    '\u{A8E6}', '\u{A8E7}', '\u{A8E8}', '\u{A8E9}', '\u{A8EA}', '\u{A8EB}', '\u{A8EC}', '\u{A8ED}',
    '\u{A8EE}', '\u{A8EF}', '\u{A8F0}', '\u{A8F1}', '\u{AAB0}', '\u{AAB2}', '\u{AAB3}', '\u{AAB7}',
    '\u{AAB8}', '\u{AABE}', '\u{AABF}', '\u{AAC1}', '\u{FE20}', '\u{FE21}', '\u{FE22}', '\u{FE23}',
    '\u{FE24}', '\u{FE25}', '\u{FE26}', '\u{10A0F}', '\u{10A38}', '\u{1D185}', '\u{1D186}', '\u{1D187}',
    '\u{1D188}', '\u{1D189}', '\u{1D1AA}', '\u{1D1AB}', '\u{1D1AC}', '\u{1D1AD}', '\u{1D242}', '\u{1D243}',
    '\u{1D244}',
];

/// The most rows or columns one image can cover: one per diacritic.
pub const MAX_CELLS: u16 = DIACRITICS.len() as u16;

/// Pixels to send: 8-bit RGBA, row by row, `width * height * 4` bytes.
pub struct Pixels<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

/// The terminal cells an image covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cells {
    pub columns: u16,
    pub rows: u16,
}

/// An image id of 32 bits. The low 24 bits travel in a placeholder's
/// foreground colour and the high byte, 1 to 255 and so never zero, in its
/// third diacritic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageId(u32);

impl ImageId {
    /// The id as the terminal numbers it.
    pub fn value(self) -> u32 {
        self.0
    }

    /// An id derived from the image's `content` and the size it is shown at,
    /// `width` × `height` pixels in `cells`, and from `run` when given. The
    /// same image at the same size gets the same id on every run without
    /// `run`, so rendering a document again does not replace the images
    /// earlier runs left in the scrollback; a `run` of its own gives each
    /// run ids of its own.
    pub fn new(content: &[u8], width: u32, height: u32, cells: Cells, run: Option<u64>) -> Self {
        Self::of([content], width, height, cells, run)
    }

    /// [`ImageId::new`] for `content` that comes in pieces, which are hashed
    /// one after the other as if they were one.
    pub fn of<'a>(
        content: impl IntoIterator<Item = &'a [u8]>,
        width: u32,
        height: u32,
        cells: Cells,
        run: Option<u64>,
    ) -> Self {
        let [w0, w1, w2, w3] = width.to_le_bytes();
        let [h0, h1, h2, h3] = height.to_le_bytes();
        let [c0, c1] = cells.columns.to_le_bytes();
        let [r0, r1] = cells.rows.to_le_bytes();
        let size = [w0, w1, w2, w3, h0, h1, h2, h3, c0, c1, r0, r1];
        let mut hash = ContentHash::new();
        content.into_iter().for_each(|piece| hash.feed(piece));
        hash.feed(&size);
        if let Some(run) = run {
            hash.feed(&run.to_le_bytes());
        }
        ImageId(fold(hash.finish()))
    }
}

/// A 64-bit hash of a stream of bytes that comes in pieces, reading eight
/// bytes at a time, as an image is megabytes. Unlike std's hasher it gives
/// the same value on every run and in every release of Rust, as ids must.
/// The pieces count as one stream, wherever they are cut, and the length of
/// the stream is part of the value.
struct ContentHash {
    state: u64,
    /// The bytes after the last whole word, `held` of them.
    tail: [u8; 8],
    held: usize,
    len: u64,
}

impl ContentHash {
    const START: u64 = 0xCBF2_9CE4_8422_2325;
    const MULTIPLIER: u64 = 0x9E37_79B9_7F4A_7C15;

    fn new() -> Self {
        Self {
            state: Self::START,
            tail: [0; 8],
            held: 0,
            len: 0,
        }
    }

    /// Mixes in a word: the multiplication carries a change upward only, so
    /// the shift brings it back down for the words after.
    fn word(&mut self, word: u64) {
        self.state = (self.state ^ word).wrapping_mul(Self::MULTIPLIER);
        self.state ^= self.state >> 29;
    }

    fn feed(&mut self, mut bytes: &[u8]) {
        self.len += bytes.len() as u64;
        if self.held > 0 {
            let take = bytes.len().min(8 - self.held);
            self.tail[self.held..self.held + take].copy_from_slice(&bytes[..take]);
            self.held += take;
            bytes = &bytes[take..];
            if self.held < 8 {
                return;
            }
            self.word(u64::from_le_bytes(self.tail));
            self.held = 0;
        }
        let (words, rest) = bytes.as_chunks::<8>();
        for word in words {
            self.word(u64::from_le_bytes(*word));
        }
        self.tail[..rest.len()].copy_from_slice(rest);
        self.held = rest.len();
    }

    fn finish(mut self) -> u64 {
        self.tail[self.held..].fill(0);
        self.word(u64::from_le_bytes(self.tail));
        self.word(self.len);
        self.state
    }
}

/// Folds a 64-bit hash into a 32-bit id: the low 24 bits mix the whole hash,
/// the high byte, 1 to 254, comes from its upper half. The id is never zero,
/// a high byte of zero is left to ids that other programs give, and one of
/// 255 to the shade, which must be above every image ([`SHADE_ID`]).
fn fold(hash: u64) -> u32 {
    let low = (hash ^ (hash >> 24) ^ (hash >> 48)) as u32 & 0xFF_FFFF;
    let high = 1 + ((hash >> 32) % 254) as u32;
    high << 24 | low
}

/// Sends `pixels` as image `id`, compressed, with a virtual placement over
/// `cells`. Nothing appears until [`write_row`] draws the placeholders.
/// An image whose every alpha is 255 goes as RGB, three bytes a pixel,
/// which the terminal then keeps at three; use [`transmit_rgba`] for one
/// that gets frames.
pub fn transmit(
    out: &mut impl Write,
    id: ImageId,
    pixels: &Pixels,
    cells: Cells,
    effort: Effort,
) -> io::Result<()> {
    send_over(out, id, pixels, cells, is_opaque(pixels), effort)
}

/// [`transmit`] that always sends four bytes a pixel: the terminal widens an
/// image to four when it gets frames, so there is nothing to save.
pub fn transmit_rgba(
    out: &mut impl Write,
    id: ImageId,
    pixels: &Pixels,
    cells: Cells,
    effort: Effort,
) -> io::Result<()> {
    send_over(out, id, pixels, cells, false, effort)
}

/// Whether every pixel of `pixels` has an alpha of 255, so that [`transmit`]
/// leaves the alpha out. Stops at the first pixel that does not.
pub fn is_opaque(pixels: &Pixels) -> bool {
    pixels
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[3] == 255)
}

fn send_over(
    out: &mut impl Write,
    id: ImageId,
    pixels: &Pixels,
    cells: Cells,
    rgb: bool,
    effort: Effort,
) -> io::Result<()> {
    if !(1..=MAX_CELLS).contains(&cells.columns) || !(1..=MAX_CELLS).contains(&cells.rows) {
        return Err(invalid(&format!(
            "an image covers 1 to {MAX_CELLS} rows and columns"
        )));
    }
    let payload = compressed(pixels, rgb, effort)?;
    let keys = format!(
        "a=T,U=1,q=2,f={},o=z,i={},s={},v={},c={},r={}",
        if rgb { 24 } else { 32 },
        id.0,
        pixels.width,
        pixels.height,
        cells.columns,
        cells.rows
    );
    write_chunks(out, &keys, &payload)
}

/// Sends `pixels` as image `id`, compressed, without placing it: a
/// [`place`] puts it on the screen.
pub fn transmit_only(
    out: &mut impl Write,
    id: ImageId,
    pixels: &Pixels,
    effort: Effort,
) -> io::Result<()> {
    let keys = format!(
        "a=t,q=2,f=32,o=z,i={},s={},v={}",
        id.0, pixels.width, pixels.height
    );
    write_chunks(out, &keys, &compressed(pixels, false, effort)?)
}

/// Adds `pixels` to image `id` as its next frame, shown for `gap` before
/// the one after it. The frame is as large as the image.
pub fn transmit_frame(
    out: &mut impl Write,
    id: ImageId,
    pixels: &Pixels,
    gap: Duration,
    effort: Effort,
) -> io::Result<()> {
    let keys = format!(
        "a=f,q=2,f=32,o=z,i={},s={},v={},z={}",
        id.0,
        pixels.width,
        pixels.height,
        gap.as_millis()
    );
    // The chunks after the first name the action too: kitty shows a frame
    // that comes in chunks only when they do.
    write_chunks_as(out, &keys, "a=f,", &compressed(pixels, false, effort)?)
}

/// Sets how long the first frame of image `id` stays: the protocol gives
/// the root frame no gap of its own when it is sent.
pub fn first_frame_gap(out: &mut impl Write, id: ImageId, gap: Duration) -> io::Result<()> {
    write!(
        out,
        "{START}a=a,q=2,i={},r=1,z={}\x1b\\",
        id.0,
        gap.as_millis()
    )
}

/// What the terminal is told to do with the animation of an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Stay on the frame it is on.
    Stop,
    /// Play from the first frame, until it has played this many times in
    /// all, or without end for `None`. (The protocol's loop count is one
    /// more than the plays: `v=2` plays once.)
    Replay(Option<u32>),
    /// Show this frame, counting from 1, and stay on it.
    Show(u32),
}

/// Tells the terminal what to do with the animation of image `id`.
pub fn control(out: &mut impl Write, id: ImageId, control: Control) -> io::Result<()> {
    let id = id.0;
    let loops = |plays: Option<u32>| plays.map_or(1, |plays| plays.saturating_add(1));
    match control {
        Control::Stop => write!(out, "{START}a=a,q=2,i={id},s=1\x1b\\"),
        Control::Replay(plays) => write!(
            out,
            "{START}a=a,q=2,i={id},c=1,s=3,v={}\x1b\\",
            loops(plays)
        ),
        Control::Show(frame) => write!(out, "{START}a=a,q=2,i={id},c={frame}\x1b\\"),
    }
}

/// Places image `id` at the cursor, `down` pixels below the top of its
/// cell, over the text there, without moving the cursor. Clearing the
/// screen takes the placement away, and with it the image, when nothing
/// else places it.
pub fn place(out: &mut impl Write, id: ImageId, down: u16) -> io::Result<()> {
    write!(out, "{START}a=p,q=2,i={},C=1,Y={down},z=1\x1b\\", id.0)
}

/// `pixels`, checked against their size, compressed as `effort` says and in
/// base64: as RGB when `rgb`, without their alpha, and otherwise as they are.
fn compressed(pixels: &Pixels, rgb: bool, effort: Effort) -> io::Result<Vec<u8>> {
    let expected = u64::from(pixels.width) * u64::from(pixels.height) * 4;
    if pixels.rgba.len() as u64 != expected {
        return Err(invalid("pixel data does not match the image size"));
    }
    let mut compressor = ZlibEncoder::new(Vec::new(), effort.level());
    if rgb {
        let dropped: Vec<u8> = (pixels.rgba.as_chunks::<4>().0.iter())
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect();
        compressor.write_all(&dropped)?;
    } else {
        compressor.write_all(pixels.rgba)?;
    }
    Ok(STANDARD.encode(compressor.finish()?).into_bytes())
}

/// A cursor move that changes nothing: save the cursor, restore it.
/// kitty shows an image sent under placeholders that are on the screen
/// already only once it gets more output, so it is written in a write of
/// its own after the images a check sent again.
pub const NUDGE: &str = "\x1b7\x1b8";

/// Writes [`NUDGE`].
pub fn nudge(out: &mut impl Write) -> io::Result<()> {
    out.write_all(NUDGE.as_bytes())
}

/// Deletes image `id` from the terminal: its placements, and its data, so
/// that the terminal frees the memory it took.
pub fn delete(out: &mut impl Write, id: ImageId) -> io::Result<()> {
    write!(out, "{START}a=d,d=I,i={},q=2\x1b\\", id.0)
}

/// The image the shade over a panel's surroundings is made of. Ghostty
/// draws every image shown through placeholders at depth -1, as deep as the
/// shade, and of images at one depth those with the larger id over the
/// others: the shade has an id above every image's.
pub const SHADE_ID: u32 = 0xFFFF_FFFF;
const SHADE: ImageId = ImageId(SHADE_ID);

/// The pixels across and down of the shade's image, which is one colour
/// all over. The terminal smooths an image when it stretches it, and fades
/// its outermost half pixel into what is behind: a pixel stretched over an
/// area would fade across all of it, and this many only at its very edge.
const SHADE_SIDE: u32 = 128;

/// How many pixels in from each edge of the shade's image a placement starts.
/// The fade at the edge of the image would otherwise show as a light line
/// wherever the shade meets an area that is not shaded, or another shaded
/// area, at the edges of the panel and the window; from inside the image the
/// colour is the same all the way to the edge of the area.
const SHADE_INSET: u32 = 2;

/// Sends the shade's image: black all over, `alpha` of 255 opaque.
pub fn send_shade(out: &mut impl Write, alpha: u8, effort: Effort) -> io::Result<()> {
    let side = SHADE_SIDE;
    let pixel = [0, 0, 0, alpha];
    let rgba: Vec<u8> = pixel.repeat((side * side) as usize);
    let pixels = Pixels {
        width: side,
        height: side,
        rgba: &rgba,
    };
    write!(
        out,
        "{START}a=t,q=2,f=32,o=z,s={side},v={side},i={SHADE_ID};"
    )?;
    out.write_all(&compressed(&pixels, false, effort)?)?;
    out.write_all(b"\x1b\\")
}

/// Places the shade as placement `number` over the `columns` and `rows`
/// from the cursor, without moving it. The placement shows the image from
/// [`SHADE_INSET`] pixels in, not from its edges.
pub fn place_shade(
    out: &mut impl Write,
    number: u32,
    (columns, rows): (usize, usize),
) -> io::Result<()> {
    let inset = SHADE_INSET;
    let side = SHADE_SIDE - 2 * inset;
    write!(
        out,
        "{START}a=p,q=2,i={SHADE_ID},p={number},c={columns},r={rows},x={inset},y={inset},w={side},h={side},C=1,z={SHADE_Z}\x1b\\"
    )
}

/// Takes the shade away: its placements and its image.
pub fn delete_shade(out: &mut impl Write) -> io::Result<()> {
    delete(out, SHADE)
}

/// Where the shade lies in depth: over the pictures, whether placed as
/// cells of text or directly, and the backgrounds, and under the text,
/// which the pager dims itself where it draws it.
pub const SHADE_Z: i32 = -1;

/// The placement id of the virtual placements [`check`] makes.
const CHECK: u32 = 0x4B49_5249;

/// Asks the terminal whether it holds image `id`, sent for `cells`: a
/// virtual placement over the same cells, which the terminal answers as
/// [`checked`] reads, and which goes again at once. It looks as the
/// image's own placement does, should the terminal draw it meanwhile.
pub fn check(out: &mut impl Write, id: ImageId, cells: Cells) -> io::Result<()> {
    let (columns, rows) = (cells.columns, cells.rows);
    write!(
        out,
        "{START}a=p,U=1,i={},p={CHECK},c={columns},r={rows}\x1b\\",
        id.0
    )?;
    write!(out, "{START}a=d,d=i,i={},p={CHECK},q=2\x1b\\", id.0)
}

/// What `answer`, one APC sequence from the terminal, says about an
/// image [`check`] asked about: its id, and whether the terminal holds
/// it. `None` for any other answer, and for an error other than the
/// image not being there.
pub fn checked(answer: &[u8]) -> Option<(ImageId, bool)> {
    let answer = std::str::from_utf8(answer).ok()?;
    let body = answer.strip_prefix(START)?.strip_suffix("\x1b\\")?;
    let (keys, message) = body.split_once(';')?;
    let (id, placement) = keys.strip_prefix("i=")?.split_once(",p=")?;
    let id: u32 = id.parse().ok()?;
    if placement.parse() != Ok(CHECK) || id == 0 {
        return None;
    }
    let held = match message {
        "OK" => true,
        _ if message.starts_with("ENOENT:") => false,
        _ => return None,
    };
    Some((ImageId(id), held))
}

/// Writes `payload` in chunks of at most [`CHUNK`] bytes. The first sequence
/// carries `keys`; `m=1` marks every chunk but the last.
fn write_chunks(out: &mut impl Write, keys: &str, payload: &[u8]) -> io::Result<()> {
    write_chunks_as(out, keys, "", payload)
}

/// `write_chunks`, with `lead` before the keys of each chunk after the
/// first.
fn write_chunks_as(out: &mut impl Write, keys: &str, lead: &str, payload: &[u8]) -> io::Result<()> {
    let mut chunks = payload.chunks(CHUNK).peekable();
    let mut first = true;
    while let Some(chunk) = chunks.next() {
        let more = u8::from(chunks.peek().is_some());
        if first {
            write!(out, "{START}{keys},m={more};")?;
            first = false;
        } else {
            write!(out, "{START}{lead}q=2,m={more};")?;
        }
        out.write_all(chunk)?;
        out.write_all(b"\x1b\\")?;
    }
    Ok(())
}

/// How the placeholders of a row tell their position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// Each cell names its row, its column and the id's high byte.
    Whole,
    /// The first cell names them, and the first where a link starts or
    /// ends; the terminal takes the others from the cell on their left, as
    /// the protocol allows. A screen of images is then about 4 bytes a cell
    /// instead of 10, under the 8 bytes a cell past which tmux drops
    /// output. Text written later over the middle of the row
    /// leaves the cells after it showing another part of the image, as they
    /// then take their position from the text's cells.
    Short,
}

/// Writes the cells `columns` of row `row` of image `id` as placeholders,
/// one for each column, so that a row can show any part of the image. The
/// foreground colour carries the low 24 bits of the id and a third diacritic
/// carries its high byte, on each cell or as `form` says. The colour is
/// reset at the end.
pub fn write_row(
    out: &mut impl Write,
    id: ImageId,
    row: u16,
    columns: Range<u16>,
    form: Form,
) -> io::Result<()> {
    write_row_linked(out, id, row, columns, &[], form)
}

/// A hyperlink over some of the placeholders of a row: the columns it
/// covers, counted from the first one written, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRun<'a> {
    pub columns: Range<u16>,
    pub url: &'a str,
}

/// `write_row`, with each of `links` as an OSC 8 hyperlink round the
/// placeholders it covers, as for text: kitty keeps a hyperlink on the
/// cells of placeholders and opens it as it does a link in text. The links
/// are in order and do not overlap.
pub fn write_row_linked(
    out: &mut impl Write,
    id: ImageId,
    row: u16,
    columns: Range<u16>,
    links: &[LinkRun<'_>],
    form: Form,
) -> io::Result<()> {
    if row >= MAX_CELLS || columns.end > MAX_CELLS || columns.start > columns.end {
        return Err(invalid("placeholder position has no diacritic"));
    }
    let [high, red, green, blue] = id.0.to_be_bytes();
    write!(out, "\x1b[38;2;{red};{green};{blue}m")?;
    let row = DIACRITICS[usize::from(row)];
    // The id's most significant byte, which the colour has no room for.
    let high = DIACRITICS[usize::from(high)];
    let mut open = false;
    // Whether the next cell names its position: the first does, and in the
    // short form the first where a link starts or ends.
    let mut name = true;
    for (at, column) in DIACRITICS[usize::from(columns.start)..usize::from(columns.end)]
        .iter()
        .enumerate()
    {
        let at = at as u16;
        if let Some(link) = links.iter().find(|link| link.columns.start == at) {
            // Control characters would end the sequence early.
            let url: String = link.url.chars().filter(|c| !c.is_control()).collect();
            write!(out, "\x1b]8;;{url}\x1b\\")?;
            open = true;
            name = true;
        }
        match name || form == Form::Whole {
            true => write!(out, "{PLACEHOLDER}{row}{column}{high}")?,
            false => write!(out, "{PLACEHOLDER}")?,
        }
        name = false;
        if links.iter().any(|link| link.columns.end == at + 1) {
            out.write_all(b"\x1b]8;;\x1b\\")?;
            open = false;
            name = true;
        }
    }
    // A link that reaches past the last cell ends there.
    if open {
        out.write_all(b"\x1b]8;;\x1b\\")?;
    }
    out.write_all(b"\x1b[39m")
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests;
