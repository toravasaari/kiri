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

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use flate2::Compression;
use flate2::write::ZlibEncoder;

/// The start of every sequence of the protocol, an APC with `G`: `ESC _ G`.
/// It ends with ST, `ESC \`.
pub const START: &str = "\x1b_G";

/// The protocol's limit for the base64 data in one escape sequence.
const CHUNK: usize = 4096;

/// The placeholder character the terminal replaces with a piece of an image.
pub const PLACEHOLDER: char = '\u{10EEEE}';

/// Combining characters that number the row and column of each placeholder,
/// in the order of kitty's `rowcolumn-diacritics.txt`.
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

/// An image id: 24 bits and never zero, so it fits in a foreground colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageId(u32);

impl ImageId {
    /// An id derived from the image's `content` and the size it is shown at,
    /// `width` × `height` pixels in `cells`, and from `run` when given. The
    /// same image at the same size gets the same id on every run without
    /// `run`, so rendering a document again does not replace the images
    /// earlier runs left in the scrollback; a `run` of its own gives each
    /// run ids of its own.
    pub fn new(content: &[u8], width: u32, height: u32, cells: Cells, run: Option<u64>) -> Self {
        // FNV-1a: simple, and unlike std's hasher stable across releases.
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        let [w0, w1, w2, w3] = width.to_le_bytes();
        let [h0, h1, h2, h3] = height.to_le_bytes();
        let [c0, c1] = cells.columns.to_le_bytes();
        let [r0, r1] = cells.rows.to_le_bytes();
        let size = [w0, w1, w2, w3, h0, h1, h2, h3, c0, c1, r0, r1];
        let run = run.map(u64::to_le_bytes);
        for &byte in content.iter().chain(&size).chain(run.iter().flatten()) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01B3);
        }
        ImageId(fold(hash))
    }
}

/// Folds a 64-bit hash into a 24-bit id that is not zero.
fn fold(hash: u64) -> u32 {
    let id = (hash ^ (hash >> 24) ^ (hash >> 48)) as u32 & 0xFF_FFFF;
    id.max(1)
}

/// Sends `pixels` as image `id`, compressed, with a virtual placement over
/// `cells`. Nothing appears until [`write_row`] draws the placeholders.
pub fn transmit(
    out: &mut impl Write,
    id: ImageId,
    pixels: &Pixels,
    cells: Cells,
) -> io::Result<()> {
    if !(1..=MAX_CELLS).contains(&cells.columns) || !(1..=MAX_CELLS).contains(&cells.rows) {
        return Err(invalid(&format!(
            "an image covers 1 to {MAX_CELLS} rows and columns"
        )));
    }
    let payload = compressed(pixels)?;
    let keys = format!(
        "a=T,U=1,q=2,f=32,o=z,i={},s={},v={},c={},r={}",
        id.0, pixels.width, pixels.height, cells.columns, cells.rows
    );
    write_chunks(out, &keys, &payload)
}

/// Sends `pixels` as image `id`, compressed, without placing it: a
/// [`place`] puts it on the screen.
pub fn transmit_only(out: &mut impl Write, id: ImageId, pixels: &Pixels) -> io::Result<()> {
    let keys = format!(
        "a=t,q=2,f=32,o=z,i={},s={},v={}",
        id.0, pixels.width, pixels.height
    );
    write_chunks(out, &keys, &compressed(pixels)?)
}

/// Places image `id` at the cursor, `down` pixels below the top of its
/// cell, over the text there, without moving the cursor. Clearing the
/// screen takes the placement away, and with it the image, when nothing
/// else places it.
pub fn place(out: &mut impl Write, id: ImageId, down: u16) -> io::Result<()> {
    write!(out, "{START}a=p,q=2,i={},C=1,Y={down},z=1\x1b\\", id.0)
}

/// `pixels`, checked against their size, compressed and in base64.
fn compressed(pixels: &Pixels) -> io::Result<Vec<u8>> {
    let expected = u64::from(pixels.width) * u64::from(pixels.height) * 4;
    if pixels.rgba.len() as u64 != expected {
        return Err(invalid("pixel data does not match the image size"));
    }
    let mut compressor = ZlibEncoder::new(Vec::new(), Compression::default());
    compressor.write_all(pixels.rgba)?;
    Ok(STANDARD.encode(compressor.finish()?).into_bytes())
}

/// Deletes image `id` from the terminal: its placements, and its data, so
/// that the terminal frees the memory it took.
pub fn delete(out: &mut impl Write, id: ImageId) -> io::Result<()> {
    write!(out, "{START}a=d,d=I,i={},q=2\x1b\\", id.0)
}

/// Writes `payload` in chunks of at most [`CHUNK`] bytes. The first sequence
/// carries `keys`; `m=1` marks every chunk but the last.
fn write_chunks(out: &mut impl Write, keys: &str, payload: &[u8]) -> io::Result<()> {
    let mut chunks = payload.chunks(CHUNK).peekable();
    let mut first = true;
    while let Some(chunk) = chunks.next() {
        let more = u8::from(chunks.peek().is_some());
        if first {
            write!(out, "{START}{keys},m={more};")?;
            first = false;
        } else {
            write!(out, "{START}q=2,m={more};")?;
        }
        out.write_all(chunk)?;
        out.write_all(b"\x1b\\")?;
    }
    Ok(())
}

/// Writes the cells `columns` of row `row` of image `id` as placeholders,
/// one for each column, so that a row can show any part of the image. The
/// foreground colour carries the id and is reset at the end.
pub fn write_row(
    out: &mut impl Write,
    id: ImageId,
    row: u16,
    columns: Range<u16>,
) -> io::Result<()> {
    if row >= MAX_CELLS || columns.end > MAX_CELLS || columns.start > columns.end {
        return Err(invalid("placeholder position has no diacritic"));
    }
    let [_, red, green, blue] = id.0.to_be_bytes();
    write!(out, "\x1b[38;2;{red};{green};{blue}m")?;
    let row = DIACRITICS[usize::from(row)];
    for column in &DIACRITICS[usize::from(columns.start)..usize::from(columns.end)] {
        write!(out, "{PLACEHOLDER}{row}{column}")?;
    }
    out.write_all(b"\x1b[39m")
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    const ID: ImageId = ImageId(0x123456);

    fn cells(columns: u16, rows: u16) -> Cells {
        Cells { columns, rows }
    }

    /// Pixels that do not compress well, to get more than one chunk.
    fn noise(len: usize) -> Vec<u8> {
        let mut state: u32 = 1;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (state >> 16) as u8
            })
            .collect()
    }

    /// The keys and base64 data of each graphics sequence in `out`.
    fn sequences(out: &[u8]) -> Vec<(String, String)> {
        let out = std::str::from_utf8(out).unwrap();
        let rest = out.strip_prefix("\x1b_G").expect("starts with a sequence");
        rest.split("\x1b_G")
            .map(|sequence| {
                let body = sequence.strip_suffix("\x1b\\").expect("terminated");
                let (keys, data) = body.split_once(';').expect("keys and data");
                (keys.to_string(), data.to_string())
            })
            .collect()
    }

    #[test]
    fn sends_a_small_image_in_one_sequence() {
        let rgba = [255, 0, 0, 255, 0, 0, 255, 128];
        let pixels = Pixels {
            width: 2,
            height: 1,
            rgba: &rgba,
        };
        let mut out = Vec::new();
        transmit(&mut out, ID, &pixels, cells(1, 1)).unwrap();
        let sequences = sequences(&out);
        assert_eq!(sequences.len(), 1);
        assert_eq!(
            sequences[0].0,
            "a=T,U=1,q=2,f=32,o=z,i=1193046,s=2,v=1,c=1,r=1,m=0"
        );
    }

    #[test]
    fn sends_compressed_rgba_in_chunks() {
        let rgba = noise(64 * 64 * 4);
        let pixels = Pixels {
            width: 64,
            height: 64,
            rgba: &rgba,
        };
        let mut out = Vec::new();
        transmit(&mut out, ID, &pixels, cells(8, 4)).unwrap();
        let sequences = sequences(&out);
        assert!(sequences.len() > 1);
        let (last, rest) = sequences.split_last().unwrap();
        assert!(rest[0].0.starts_with("a=T,"));
        assert!(rest[0].0.ends_with(",m=1"));
        for (keys, data) in &rest[1..] {
            assert_eq!(keys, "q=2,m=1");
            assert_eq!(data.len(), CHUNK);
        }
        assert_eq!(rest[0].1.len(), CHUNK);
        assert_eq!(last.0, "q=2,m=0");

        let data: String = sequences.iter().map(|(_, data)| data.as_str()).collect();
        let compressed = base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap();
        let mut decoded = Vec::new();
        flate2::read::ZlibDecoder::new(&compressed[..])
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, rgba);
    }

    #[test]
    fn exactly_one_chunk_of_data_needs_one_sequence() {
        let payload = vec![b'A'; CHUNK];
        let mut out = Vec::new();
        write_chunks(&mut out, "a=T", &payload).unwrap();
        let expected = format!("\x1b_Ga=T,m=0;{}\x1b\\", "A".repeat(CHUNK));
        assert_eq!(String::from_utf8(out).unwrap(), expected);
    }

    #[test]
    fn data_past_one_chunk_continues_in_the_next() {
        let payload = vec![b'A'; CHUNK + 4];
        let mut out = Vec::new();
        write_chunks(&mut out, "a=T", &payload).unwrap();
        let expected = format!(
            "\x1b_Ga=T,m=1;{}\x1b\\\x1b_Gq=2,m=0;AAAA\x1b\\",
            "A".repeat(CHUNK)
        );
        assert_eq!(String::from_utf8(out).unwrap(), expected);
    }

    #[test]
    fn deletes_an_image_with_its_data() {
        let mut out = Vec::new();
        delete(&mut out, ID).unwrap();
        // An upper-case `I` frees the data too, not only the placements.
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\x1b_Ga=d,d=I,i=1193046,q=2\x1b\\"
        );
    }

    #[test]
    fn rejects_pixels_that_do_not_match_the_size() {
        let pixels = Pixels {
            width: 2,
            height: 2,
            rgba: &[0; 12],
        };
        let error = transmit(&mut Vec::new(), ID, &pixels, cells(1, 1)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_cells_without_diacritics() {
        let pixels = Pixels {
            width: 1,
            height: 1,
            rgba: &[0; 4],
        };
        for cells in [
            cells(MAX_CELLS + 1, 1),
            cells(1, MAX_CELLS + 1),
            cells(0, 1),
        ] {
            let error = transmit(&mut Vec::new(), ID, &pixels, cells).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{cells:?}");
            let message = "an image covers 1 to 297 rows and columns";
            assert_eq!(error.to_string(), message, "{cells:?}");
        }
        let error = write_row(&mut Vec::new(), ID, MAX_CELLS, 0..1).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        let error = write_row(&mut Vec::new(), ID, 0, 0..MAX_CELLS + 1).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        #[allow(clippy::reversed_empty_ranges)]
        let error = write_row(&mut Vec::new(), ID, 0, 3..2).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn row_of_placeholders_carries_the_id_and_positions() {
        let mut out = Vec::new();
        write_row(&mut out, ID, 2, 0..3).unwrap();
        let expected = "\x1b[38;2;18;52;86m\
            \u{10EEEE}\u{030E}\u{0305}\
            \u{10EEEE}\u{030E}\u{030D}\
            \u{10EEEE}\u{030E}\u{030E}\
            \x1b[39m";
        assert_eq!(String::from_utf8(out).unwrap(), expected);
    }

    #[test]
    fn a_row_can_start_in_the_middle_of_the_image() {
        let mut out = Vec::new();
        write_row(&mut out, ID, 2, 1..3).unwrap();
        // The first cell names its column, the second after it.
        let expected = "\x1b[38;2;18;52;86m\
            \u{10EEEE}\u{030E}\u{030D}\
            \u{10EEEE}\u{030E}\u{030E}\
            \x1b[39m";
        assert_eq!(String::from_utf8(out).unwrap(), expected);
    }

    #[test]
    fn last_row_and_column_use_the_last_diacritic() {
        let mut out = Vec::new();
        write_row(&mut out, ID, MAX_CELLS - 1, 0..MAX_CELLS).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.ends_with("\u{10EEEE}\u{1D244}\u{1D244}\x1b[39m"));
        assert_eq!(out.matches(PLACEHOLDER).count(), usize::from(MAX_CELLS));
    }

    #[test]
    fn diacritics_follow_kittys_table() {
        assert_eq!(DIACRITICS[0], '\u{0305}');
        assert_eq!(DIACRITICS[296], '\u{1D244}');
        assert!(DIACRITICS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn id_is_stable_across_runs() {
        assert_eq!(ImageId::new(b"kiri", 40, 20, cells(4, 1), None).0, 0x4351F5);
    }

    #[test]
    fn id_depends_on_content_and_size() {
        let id = ImageId::new(b"kiri", 40, 20, cells(4, 1), None);
        assert_ne!(id, ImageId::new(b"kirI", 40, 20, cells(4, 1), None));
        assert_ne!(id, ImageId::new(b"kiri", 41, 20, cells(4, 1), None));
        assert_ne!(id, ImageId::new(b"kiri", 40, 21, cells(4, 1), None));
        assert_ne!(id, ImageId::new(b"kiri", 40, 20, cells(5, 1), None));
        assert_ne!(id, ImageId::new(b"kiri", 40, 20, cells(4, 2), None));
    }

    #[test]
    fn a_run_of_its_own_gives_ids_of_its_own() {
        let id = |run| ImageId::new(b"kiri", 40, 20, cells(4, 1), run);
        assert_ne!(id(Some(1)), id(None));
        assert_ne!(id(Some(1)), id(Some(2)));
        assert_eq!(id(Some(1)), id(Some(1)));
    }

    #[test]
    fn id_fits_in_24_bits_and_is_never_zero() {
        assert_eq!(fold(0), 1);
        for hash in [1, 0xFF_FFFF, 0x1_0000_0001, u64::MAX, 0xF09A_0A2E_B649_8FD9] {
            let id = fold(hash);
            assert!(id != 0 && id <= 0xFF_FFFF, "{hash:#x} -> {id:#x}");
        }
    }

    #[test]
    fn an_image_sent_apart_is_placed_at_the_cursor() {
        let rgba = [1, 2, 3, 4];
        let pixels = Pixels {
            width: 1,
            height: 1,
            rgba: &rgba,
        };
        let mut out = Vec::new();
        transmit_only(&mut out, ID, &pixels).unwrap();
        let sent = String::from_utf8(std::mem::take(&mut out)).unwrap();
        assert!(
            sent.starts_with("\x1b_Ga=t,q=2,f=32,o=z,i=1193046,s=1,v=1,m=0;"),
            "{sent:?}"
        );
        place(&mut out, ID, 4).unwrap();
        assert_eq!(out, b"\x1b_Ga=p,q=2,i=1193046,C=1,Y=4,z=1\x1b\\");
        let short = Pixels {
            width: 2,
            height: 1,
            rgba: &rgba,
        };
        assert!(transmit_only(&mut out, ID, &short).is_err());
    }
}
