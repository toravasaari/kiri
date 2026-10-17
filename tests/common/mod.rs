//! Helpers shared by the integration tests.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use base64::Engine;
use image::{Rgba, RgbaImage};
use kiri::images::{DEFAULT_MAX_FILE_SIZE, Fonts, Images, Source};
use kiri::kitty::graphics::{DIACRITICS, PLACEHOLDER};
use kiri::model::Document;
use kiri::render::Inputs;
use kiri::term::Capabilities;
use kiri::theme::Theme;

pub const WIDTH: u16 = 48;

pub fn render(fixture: &str, caps: &Capabilities) -> String {
    render_with(fixture, caps, &Theme::default())
}

pub fn render_with(fixture: &str, caps: &Capabilities, theme: &Theme) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let source = std::fs::read_to_string(&path).expect("fixture is readable");
    let document = kiri::parse::parse(&source);
    let mut out = Vec::new();
    let inputs = Inputs {
        theme: theme.clone(),
        ..Inputs::new(caps.clone())
    };
    kiri::render::render(&document, &inputs, &mut out).expect("rendering to memory succeeds");
    String::from_utf8(out).expect("output is UTF-8")
}

pub fn example_theme(name: &str) -> Theme {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/themes")
        .join(name);
    kiri::theme::load(&path).expect("example theme loads")
}

/// Escape characters and no-break spaces made visible, so snapshots can be
/// read and diffed.
pub fn visible(output: &str) -> String {
    output.replace('\x1b', "␛").replace('\u{a0}', "⍽")
}

pub fn render_source(source: &str, caps: &Capabilities, theme: &Theme) -> String {
    let document = kiri::parse::parse(source);
    let mut out = Vec::new();
    let inputs = Inputs {
        theme: theme.clone(),
        ..Inputs::new(caps.clone())
    };
    kiri::render::render(&document, &inputs, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

/// SGR colour codes in the output: 30-37, 38, 40-47, 48, 58, 90-97 and
/// 100-107. Default-colour codes (39, 49, 59) are not counted.
pub fn colour_codes(output: &str) -> Vec<String> {
    let mut codes = Vec::new();
    for sequence in output.split("\x1b[").skip(1) {
        let Some(end) = sequence.find(|c: char| !c.is_ascii_digit() && c != ';' && c != ':') else {
            continue;
        };
        if !sequence[end..].starts_with('m') {
            continue;
        }
        for code in sequence[..end].split(';') {
            let number: u32 = code.split(':').next().unwrap().parse().unwrap_or(0);
            if matches!(number, 30..=38 | 40..=48 | 58 | 90..=97 | 100..=107) {
                codes.push(sequence[..end].to_owned());
                break;
            }
        }
    }
    codes
}

/// Removes the sequences kiri writes itself: CSI (SGR, erase, cursor
/// forward), OSC 8 hyperlinks and OSC 66 scaled text. Panics on any other
/// escape, and on a control character inside an OSC payload.
pub fn strip_own_sequences(output: &str) -> String {
    let mut out = String::new();
    let mut rest = output;
    while let Some(start) = rest.find('\x1b') {
        out.push_str(&rest[..start]);
        let sequence = &rest[start..];
        let end = if let Some(csi) = sequence.strip_prefix("\x1b[") {
            let params = csi
                .find(|c: char| !c.is_ascii_digit() && c != ';' && c != ':')
                .unwrap_or_else(|| panic!("unterminated CSI: {sequence:?}"));
            assert!(
                matches!(csi.as_bytes()[params], b'm' | b'K' | b'C'),
                "unexpected CSI: {sequence:?}"
            );
            2 + params + 1
        } else if sequence.starts_with("\x1b]8;;") || sequence.starts_with("\x1b]66;") {
            let payload = sequence
                .find("\x1b\\")
                .unwrap_or_else(|| panic!("unterminated OSC: {sequence:?}"));
            assert!(
                !sequence[1..payload].chars().any(char::is_control),
                "control character in an OSC: {sequence:?}"
            );
            payload + 2
        } else {
            panic!("escape from the document: {sequence:?}");
        };
        rest = &sequence[end..];
    }
    out.push_str(rest);
    out
}

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Opaque quadrants: red, green, blue and yellow from the top left.
pub fn quadrants(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba(match (x < width / 2, y < height / 2) {
            (true, true) => [255, 0, 0, 255],
            (false, true) => [0, 255, 0, 255],
            (true, false) => [0, 0, 255, 255],
            (false, false) => [255, 255, 0, 255],
        })
    })
}

pub fn one_image(url: &str, image: RgbaImage) -> Images {
    let mut images = Images::new();
    images.insert(url, Ok(Source::Raster(image)));
    images
}

/// The images of `document` as kiri loads them from the fixtures, with each
/// one that loads replaced by quadrants of the same size. Snapshots then
/// show real sizes and errors, but not the decoders' work, and the image
/// data compresses to a few bytes. Without fonts, as the sizes do not
/// depend on them.
pub fn fixture_images(document: &Document) -> Images {
    let loaded =
        kiri::images::load_with(document, &fixtures(), DEFAULT_MAX_FILE_SIZE, &Fonts::none());
    let mut images = Images::new();
    for (url, image) in loaded.iter() {
        let image = match image {
            Ok(source) => {
                let (width, height) = source.size();
                Ok(Source::Raster(quadrants(width, height)))
            }
            Err(error) => Err(error.clone()),
        };
        images.insert(url, image);
    }
    images
}

pub fn render_document(document: &Document, images: Images, caps: &Capabilities) -> String {
    let inputs = Inputs {
        images,
        ..Inputs::new(caps.clone())
    };
    let mut out = Vec::new();
    kiri::render::render(document, &inputs, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

pub fn render_images(fixture: &str, caps: &Capabilities) -> String {
    let source = std::fs::read_to_string(fixtures().join(fixture)).unwrap();
    let document = kiri::parse::parse(&source);
    render_document(&document, fixture_images(&document), caps)
}

pub fn render_with_images(source: &str, images: Images, caps: &Capabilities) -> String {
    render_document(&kiri::parse::parse(source), images, caps)
}

/// The keys and base64 data of every graphics command in `output`.
pub fn graphics_commands(output: &str) -> Vec<(String, String)> {
    let mut commands = Vec::new();
    let mut rest = output;
    while let Some(start) = rest.find("\x1b_G") {
        let body = &rest[start + 3..];
        let end = body.find("\x1b\\").expect("terminated graphics command");
        let (keys, data) = body[..end].split_once(';').expect("keys and data");
        commands.push((keys.to_owned(), data.to_owned()));
        rest = &body[end + 2..];
    }
    commands
}

/// `visible`, with kitty's graphics made readable: the base64 data of each
/// graphics command becomes its length, and each run of placeholders shows
/// the image row it draws and how many cells it covers. Panics on
/// placeholders that do not count their columns up from the left.
pub fn readable(output: &str) -> String {
    let mut commands = String::new();
    let mut rest = output;
    while let Some(start) = rest.find("\x1b_G") {
        commands.push_str(&rest[..start]);
        let body = &rest[start + 3..];
        let end = body.find("\x1b\\").expect("terminated graphics command");
        let (keys, data) = body[..end].split_once(';').expect("keys and data");
        commands.push_str(&format!("\x1b_G{keys};<{} bytes>\x1b\\", data.len()));
        rest = &body[end + 2..];
    }
    commands.push_str(rest);

    let position = |c: Option<char>| {
        let c = c.expect("placeholder with its diacritics");
        DIACRITICS
            .iter()
            .position(|&d| d == c)
            .expect("a diacritic")
    };
    let mut out = String::new();
    let mut chars = commands.chars().peekable();
    while let Some(c) = chars.next() {
        if c != PLACEHOLDER {
            out.push(c);
            continue;
        }
        let row = position(chars.next());
        assert_eq!(position(chars.next()), 0, "a row starts at column 0");
        let mut cells = 1;
        while chars.next_if_eq(&PLACEHOLDER).is_some() {
            assert_eq!(position(chars.next()), row);
            assert_eq!(position(chars.next()), cells);
            cells += 1;
        }
        out.push_str(&format!("⟦row {row}: {cells} cells⟧"));
    }
    visible(&out)
}

/// The placeholder rows in `output`, as `readable` shows them.
pub fn drawn(output: &str) -> Vec<String> {
    readable(output)
        .split('⟦')
        .skip(1)
        .map(|rest| format!("⟦{}", &rest[..rest.find('⟧').unwrap() + '⟧'.len_utf8()]))
        .collect()
}

/// One image sent to the terminal: the keys of its first command, and its
/// pixels decoded from all of its commands.
pub struct Sent {
    pub keys: HashMap<String, String>,
    pub rgba: Vec<u8>,
}

pub fn sent(output: &str) -> Vec<Sent> {
    let mut images = Vec::new();
    let mut keys = HashMap::new();
    let mut data = String::new();
    for (command_keys, chunk) in graphics_commands(output) {
        let command: HashMap<String, String> = (command_keys.split(','))
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap();
                (key.to_owned(), value.to_owned())
            })
            .collect();
        if command.contains_key("a") {
            keys = command.clone();
        }
        data.push_str(&chunk);
        if command["m"] == "0" {
            let compressed = base64::engine::general_purpose::STANDARD
                .decode(&data)
                .unwrap();
            let mut rgba = Vec::new();
            flate2::read::ZlibDecoder::new(&compressed[..])
                .read_to_end(&mut rgba)
                .unwrap();
            images.push(Sent {
                keys: std::mem::take(&mut keys),
                rgba,
            });
            data.clear();
        }
    }
    images
}

/// `source` with `images`, rendered with `theme`.
pub fn render_images_with_theme(
    source: &str,
    images: Images,
    caps: &Capabilities,
    theme: &Theme,
) -> String {
    let inputs = Inputs {
        images,
        caps: caps.clone(),
        theme: theme.clone(),
    };
    let mut out = Vec::new();
    kiri::render::render(&kiri::parse::parse(source), &inputs, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

// The kiri binary

/// A directory of its own for one test, removed when the test ends.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("kiri-cli-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The kiri binary with nothing from the environment of the tests: no
/// colour settings, and the home and configuration directories in `dir`,
/// so the user's theme stays out.
pub fn command(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kiri"));
    command
        .env_clear()
        .env("HOME", dir.join("home"))
        .env("XDG_CONFIG_HOME", dir.join("config"));
    command
}

/// Runs kiri on a document of `source` in `dir`, with `args` before the file.
pub fn kiri(dir: &Path, source: &str, args: &[&str]) -> Output {
    let document = dir.join("document.md");
    std::fs::write(&document, source).unwrap();
    command(dir).args(args).arg(&document).output().unwrap()
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Writes `text` to `path`, creating its directory.
pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
