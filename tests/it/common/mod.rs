//! Helpers shared by the integration tests.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;
use image::{Rgba, RgbaImage};
use kiri::diagrams::{DiagramError, Drawn, Renderer, Request};
use kiri::images::{DEFAULT_MAX_FILE_SIZE, Fonts, ImageKey, Images, LoadOptions, Looks, Source};
use kiri::present::kitty::graphics::{DIACRITICS, PLACEHOLDER};
use kiri::render::{Headings, Inputs};
use kiri::typeset::caps::Capabilities;
use kiri::typeset::theme::Theme;

#[path = "../../../src/term/pty.rs"]
mod pty;
#[path = "../../../src/testing.rs"]
mod testing;

#[allow(unused_imports)]
pub(crate) use testing::Scratch;

pub const WIDTH: u16 = 48;

/// The theme the snapshots of features are drawn with, from
/// `tests/fixtures/test-theme.toml`: the defaults as they were when the
/// file was made, so that a change to the built-in theme does not move
/// those snapshots.
pub fn fixed_theme() -> Theme {
    kiri::load::theme::load(&fixtures().join("test-theme.toml")).expect("the test theme loads")
}

/// Inputs with `caps`, the [`fixed_theme`] and no images.
pub fn fixed_inputs(caps: Capabilities) -> Inputs {
    Inputs {
        theme: fixed_theme(),
        ..Inputs::new(caps)
    }
}

/// Inputs with `caps` and `theme`, and no images.
pub fn inputs(caps: &Capabilities, theme: &Theme) -> Inputs {
    Inputs {
        theme: theme.clone(),
        ..Inputs::new(caps.clone())
    }
}

/// A diagram's `key` as the layout for `caps` and `theme` makes it: with
/// the fingerprint of how diagrams are drawn there. Any other key stays.
/// The images of these tests are loaded with the default options,
/// whatever the caps.
pub fn laid_out_key(key: ImageKey, caps: &Capabilities, theme: &Theme) -> ImageKey {
    match key {
        ImageKey::Block { .. } => {
            key.with_look(kiri::typeset::images::diagram_look_for(theme, caps, true))
        }
        other => other,
    }
}

/// The heading `key` as the layout for `caps`, with headings shown as
/// `headings` says, looks it up: with the fingerprint of the look of its
/// level.
pub fn laid_out_heading(key: ImageKey, caps: &Capabilities, headings: Headings) -> ImageKey {
    let ImageKey::Heading { level, .. } = key else {
        panic!("a heading's key: {key:?}");
    };
    let look = kiri::render::heading_look(headings, false, &fixed_theme(), caps);
    key.with_look(
        Looks {
            headings: look,
            ..Looks::default()
        }
        .heading(level),
    )
}

/// `images` keyed as the layout for `caps` and `theme` looks them up.
pub fn laid_out(images: Images, caps: &Capabilities, theme: &Theme) -> Images {
    let mut keyed = Images::new();
    for (key, image) in images.iter() {
        keyed.insert(laid_out_key(key.clone(), caps, theme), image.clone());
    }
    keyed
}

/// Inputs with `caps` and `images`, and the [`fixed_theme`].
pub fn with_images(caps: &Capabilities, images: Images) -> Inputs {
    Inputs {
        images: laid_out(images, caps, &fixed_theme()),
        ..fixed_inputs(caps.clone())
    }
}

/// `source` rendered with `inputs`.
pub fn render(source: &str, inputs: &Inputs) -> String {
    let document = kiri::typeset::parse::parse(source);
    let mut out = Vec::new();
    kiri::render::render(&document, inputs, &mut out).expect("rendering to memory succeeds");
    String::from_utf8(out).expect("output is UTF-8")
}

/// The fixture document `name` rendered with `inputs`.
pub fn render_fixture(name: &str, inputs: &Inputs) -> String {
    render(&fixture(name), inputs)
}

/// The fixture document `name` rendered for `caps`, with the built-in
/// [`fixed_theme`] and no images.
pub fn render_for(name: &str, caps: Capabilities) -> String {
    render_fixture(name, &fixed_inputs(caps))
}

/// The source of the fixture document `name`.
pub fn fixture(name: &str) -> String {
    std::fs::read_to_string(fixtures().join(name)).expect("fixture is readable")
}

pub fn example_theme(name: &str) -> Theme {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/themes")
        .join(name);
    kiri::load::theme::load(&path).expect("example theme loads")
}

/// Escape characters and no-break spaces made visible, so snapshots can be
/// read and diffed.
pub fn visible(output: &str) -> String {
    output.replace('\x1b', "␛").replace('\u{a0}', "⍽")
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

/// The images of the fixture document `name` as kiri loads them, with each
/// one that loads replaced by quadrants of the same size. Snapshots then
/// show real sizes and errors, but not the decoders' work, and the image
/// data compresses to a few bytes. Without fonts, as the sizes do not
/// depend on them.
pub fn fixture_images(name: &str) -> Images {
    fixture_images_with(name, &LoadOptions::default())
}

/// `fixture_images`, loaded with `options`, such as a renderer for the
/// diagrams.
pub fn fixture_images_with(name: &str, options: &LoadOptions<'_>) -> Images {
    let document = kiri::typeset::parse::parse(&fixture(name));
    let loaded = kiri::load::load_with(
        &document,
        &fixtures(),
        DEFAULT_MAX_FILE_SIZE,
        &Fonts::none(),
        options,
    );
    let mut images = Images::new();
    for (key, image) in loaded.iter() {
        let image = match image {
            Ok(source) => {
                let (width, height) = source.size();
                Ok(Source::Raster(quadrants(width, height)))
            }
            Err(error) => Err(error.clone()),
        };
        images.insert(key.clone(), image);
    }
    images
}

/// Draws Mermaid without merman, so tests do not depend on its output:
/// an empty SVG one text row, 20 CSS pixels, high for each line of source,
/// and 10 + n cells of 10 pixels wide, where n is the source's length in
/// characters modulo 30. The width tells diagrams of equal line counts
/// apart, so a snapshot shows which source got which image. A source with
/// a line that opens more brackets than it closes fails, as merman fails
/// on an unclosed label.
pub struct StubDiagrams;

impl Renderer for StubDiagrams {
    fn draws(&self, language: &str) -> bool {
        language.eq_ignore_ascii_case("mermaid")
    }

    fn render(&self, request: &Request) -> Result<Drawn, DiagramError> {
        let &Request { source, theme, .. } = request;
        let unclosed = |line: &str| line.matches('[').count() > line.matches(']').count();
        if source.lines().any(unclosed) {
            return Err("unclosed label".into());
        }
        let width = 10 * (10 + source.chars().count() % 30);
        let height = 20 * source.lines().count();
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"/>"#
        );
        Ok(Drawn::svg(svg, theme, None))
    }
}

/// Diagrams drawn by `StubDiagrams` for a light page, named by line.
pub fn stub_diagrams() -> LoadOptions<'static> {
    LoadOptions {
        renderer: &StubDiagrams,
        ..LoadOptions::default()
    }
}

/// The images of `source`, a Markdown document, with its diagrams drawn
/// by `StubDiagrams`.
pub fn stub_diagram_images(source: &str) -> Images {
    kiri::load::load_with(
        &kiri::typeset::parse::parse(source),
        &fixtures(),
        DEFAULT_MAX_FILE_SIZE,
        &Fonts::none(),
        &stub_diagrams(),
    )
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
/// the image row it draws and how many cells it covers, or which of the
/// image's columns when they do not start at the left. Panics on
/// placeholders that do not count their columns up one by one.
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
        let start = position(chars.next());
        let mut cells = 1;
        while chars.next_if_eq(&PLACEHOLDER).is_some() {
            assert_eq!(position(chars.next()), row);
            assert_eq!(position(chars.next()), start + cells);
            cells += 1;
        }
        out.push_str(&match start {
            0 => format!("⟦row {row}: {cells} cells⟧"),
            start => format!("⟦row {row}: columns {start}..{}⟧", start + cells),
        });
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

// The kiri binary

/// The kiri binary with nothing from the environment of the tests: no
/// colour settings, and the home and configuration directories in `dir`,
/// so the user's theme stays out. Reading mode starts off, as it did before
/// it was the default, so that the tests that look at the text layout keep
/// looking at it; the tests of reading mode take `KIRI_READING` out again.
/// The signals that end kiri do so, also
/// when the tests run in the background or under nohup, which ignore some
/// of them, and kiri leaves an ignored signal ignored.
pub fn command(dir: &Path) -> Command {
    use std::os::unix::process::CommandExt;

    let mut command = Command::new(env!("CARGO_BIN_EXE_kiri"));
    command
        .env_clear()
        .env("HOME", dir.join("home"))
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("KIRI_READING", "0");
    // SAFETY: signal is async-signal-safe and touches no memory.
    unsafe {
        command.pre_exec(|| {
            for signal in [libc::SIGINT, libc::SIGHUP, libc::SIGTERM, libc::SIGQUIT] {
                if libc::signal(signal, libc::SIG_DFL) == libc::SIG_ERR {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    command
}

/// Runs kiri on a document of `source` in `dir`, with `args` before the file.
pub fn kiri(dir: &Path, source: &str, args: &[&str]) -> Output {
    let document = dir.join("document.md");
    std::fs::write(&document, source).unwrap();
    command(dir).args(args).arg(&document).output().unwrap()
}

/// Runs kiri on the file `path` with `args` before it.
pub fn kiri_on(dir: &Path, path: &Path, args: &[&str]) -> Output {
    command(dir).args(args).arg(path).output().unwrap()
}

/// Runs `kiri` with `stdin` written to its standard input.
pub fn piped(mut kiri: Command, stdin: &[u8]) -> Output {
    let mut child = kiri
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let stdin = stdin.to_owned();
    // kiri may stop reading early on an error, so write on the side.
    let writer = std::thread::spawn(move || {
        let _ = input.write_all(&stdin);
    });
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    out
}

/// kiri in `dir` with `args`, reading a pipe of `stdin`.
pub fn kiri_piped(dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut kiri = command(dir);
    kiri.current_dir(dir).args(args);
    piped(kiri, stdin)
}

/// Fails unless `out` succeeded without a word on standard error.
pub fn assert_succeeded(out: &Output) {
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
}

/// The standard output of `out`, which must have succeeded quietly.
pub fn shown(out: &Output) -> String {
    assert_succeeded(out);
    text(&out.stdout)
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `text` with every run of whitespace made one space, so that a phrase
/// is found however the help wraps it.
pub fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Writes `text` to `path`, creating its directory.
pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A program at `path` that runs `script`. Another process writes it, so
/// that no program started by this one, between its fork and its exec,
/// holds the file open for writing when the program is run, which fails
/// with "text file busy".
pub fn executable(path: &Path, script: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut child = Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
}

/// A pseudo-terminal: the terminal side, and the program's side.
pub fn pty() -> (File, File) {
    pty::pty()
}

/// Whether `done` holds within `limit`, asked every 20 ms.
pub fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if done() {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// How `child` exited, or `None` when it did not within `limit`; then it
/// is killed and reaped, so a hanging program fails the test instead of
/// hanging it.
pub fn wait_or_kill(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Everything `child` writes to the terminal side `terminal` until no one
/// holds the program's side any more, with `act` called after each read
/// with everything read so far, the terminal side and `child`. `child`
/// gets `limit` to finish, so a hanging one fails the test instead of
/// hanging it: it is killed and the test panics.
pub fn read_terminal(
    terminal: &mut File,
    child: &mut Child,
    limit: Duration,
    act: impl FnMut(&[u8], &mut File, &Child),
) -> Vec<u8> {
    read_terminal_quiet(terminal, child, limit, None, act)
}

/// [`read_terminal`], where `act` is also called, with the same bytes as
/// the time before, once nothing has been read for `quiet`. A test that
/// waits for nothing to happen waits for that, and not for a time it
/// guesses is long enough.
pub fn read_terminal_quiet(
    terminal: &mut File,
    child: &mut Child,
    limit: Duration,
    quiet: Option<Duration>,
    mut act: impl FnMut(&[u8], &mut File, &Child),
) -> Vec<u8> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};

    let mut seen = Vec::new();
    let mut buffer = [0; 4096];
    let deadline = Instant::now() + limit;
    loop {
        let left = deadline.checked_duration_since(Instant::now());
        let wait = match (left, quiet) {
            (Some(left), Some(quiet)) => Some(left.min(quiet)),
            (left, _) => left,
        };
        let timeout = wait.map(|wait| Timespec::try_from(wait).unwrap());
        let mut fds = [PollFd::new(&*terminal, PollFlags::IN)];
        let ready = left.is_some() && poll(&mut fds, timeout.as_ref()).unwrap() != 0;
        if !ready && left.is_some_and(|left| quiet.is_some_and(|quiet| quiet < left)) {
            act(&seen, terminal, child);
            continue;
        }
        if !ready {
            child.kill().unwrap();
            panic!("kiri did not finish: {:?}", text(&seen));
        }
        // Reading ends with an error once no one holds the program's side.
        let Ok(read @ 1..) = terminal.read(&mut buffer) else {
            break;
        };
        seen.extend_from_slice(&buffer[..read]);
        act(&seen, terminal, child);
    }
    seen
}
