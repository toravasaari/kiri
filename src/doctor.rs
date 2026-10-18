//! `kiri --doctor`: a report, in Markdown, of what kiri found out about the
//! terminal and which settings it uses.

use std::ffi::OsStr;
use std::fmt::Write;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::ImageFormat;

use crate::config::{Choices, Environment, Origin, Setting, ThemeSource};
use crate::render::image::DEFAULT_CELL;
use crate::term::{CellSize, Detection, Mode};
use crate::theme::Paper;

/// What the report tells.
#[derive(Debug)]
pub struct Facts<'a> {
    pub env: &'a Environment,
    pub mode: Setting<Mode>,
    /// `--width`, when given.
    pub width: Option<u16>,
    /// The largest image file kiri reads, in megabytes.
    pub max_image_size: Setting<u64>,
    pub detection: &'a Detection,
    pub choices: &'a Choices,
    /// Why the configuration file could not be read.
    pub config_error: Option<String>,
    /// Why the theme could not be loaded.
    pub theme_error: Option<String>,
}

/// The test image in the sample: four coloured squares.
const TEST_IMAGE: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" \
    width=\"64\" height=\"64\">\
    <rect width=\"32\" height=\"32\" fill=\"#d33\"/>\
    <rect x=\"32\" width=\"32\" height=\"32\" fill=\"#3a3\"/>\
    <rect y=\"32\" width=\"32\" height=\"32\" fill=\"#36d\"/>\
    <rect x=\"32\" y=\"32\" width=\"32\" height=\"32\" fill=\"#eb3\"/>\
    </svg>";

/// The report on `facts`, as Markdown. Paths and values under the home
/// directory show it as `~`.
pub fn report(facts: &Facts) -> String {
    let mut out = String::new();
    // Writing to a String does not fail.
    let _ = write_report(facts, &mut out);
    out
}

fn write_report(facts: &Facts, out: &mut String) -> std::fmt::Result {
    let home = Home(facts.env.home.as_deref());
    writeln!(out, "# kiri doctor\n")?;
    writeln!(
        out,
        "kiri {}: what it found out about the terminal, and the settings \
         it uses. Written to a file, this report stays Markdown, ready to \
         paste into a bug report.\n",
        env!("CARGO_PKG_VERSION")
    )?;
    write_problems(facts, &home, out)?;
    write_terminal(facts, out)?;
    write_settings(facts, &home, out)?;
    write_files(facts, &home, out)?;
    writeln!(out, "## Image formats\n")?;
    writeln!(out, "{}\n", image_formats())?;
    write_sample(out)?;
    if let Some(answers) = &facts.detection.answers {
        writeln!(out, "## Probe answers\n")?;
        writeln!(out, "```text\n{}\n```", escaped(answers))?;
    }
    Ok(())
}

fn write_problems(facts: &Facts, home: &Home, out: &mut String) -> std::fmt::Result {
    let problems: Vec<_> = [&facts.config_error, &facts.theme_error]
        .into_iter()
        .flatten()
        .collect();
    if problems.is_empty() {
        return Ok(());
    }
    writeln!(out, "## Problems\n")?;
    for problem in problems {
        let problem = home.hide(problem);
        // The errors start with the path of the file they are about. A
        // longer error, such as one that shows the line in the file, goes
        // in a code block of its own.
        match problem.split_once(": ") {
            Some((path, error)) if error.contains('\n') => {
                writeln!(out, "- `{path}`:\n\n  ```text")?;
                for line in error.trim_end().lines() {
                    writeln!(out, "  {line}")?;
                }
                writeln!(out, "  ```")?;
            }
            Some((path, error)) => writeln!(out, "- `{path}`: {error}")?,
            None => writeln!(out, "- {problem}")?,
        }
    }
    writeln!(out)
}

fn write_terminal(facts: &Facts, out: &mut String) -> std::fmt::Result {
    let detection = facts.detection;
    let caps = &detection.caps;
    let yes_no = |yes| if yes { "yes" } else { "no" };
    writeln!(out, "## Terminal\n")?;
    writeln!(out, "| What | Found |\n|---|---|")?;
    let mut terminal = match (&detection.answers, facts.mode.value) {
        (Some(_), _) => detection.terminal().unwrap_or_else(|| "unknown".into()),
        (None, Mode::Auto) if caps.styling => "did not answer".into(),
        (None, Mode::Auto) => "none".into(),
        (None, mode) => format!("not asked in mode {}", mode_name(mode)),
    };
    if let Some(term) = &facts.env.term {
        write!(terminal, ", `TERM={}`", term.to_string_lossy())?;
    }
    writeln!(out, "| Terminal | {terminal} |")?;
    let window = match detection.window {
        Some(w) => format!(
            "{} × {} cells, {} × {} pixels",
            w.columns, w.rows, w.width, w.height
        ),
        None => "unknown".into(),
    };
    writeln!(out, "| Window | {window} |")?;
    let cell = match detection.cell_size() {
        CellSize::Window(w, h) => format!("{w} × {h} pixels, from the window size"),
        CellSize::Terminal(w, h) => format!("{w} × {h} pixels, from the terminal"),
        CellSize::Unknown => format!(
            "unknown; images take {} × {} pixels",
            DEFAULT_CELL.0, DEFAULT_CELL.1
        ),
    };
    writeln!(out, "| Cell size | {cell} |")?;
    let graphics = match (caps.graphics, caps.draws_images()) {
        (false, _) => "no",
        (true, true) => "yes, with Unicode placeholders",
        (true, false) => "yes, without Unicode placeholders: images show their alt text",
    };
    writeln!(out, "| Graphics | {graphics} |")?;
    writeln!(out, "| Text sizing | {} |", yes_no(caps.text_sizing))?;
    let colours = match (caps.styling, caps.color, caps.truecolor) {
        (false, _, _) => "none",
        (true, false, _) => "off: `NO_COLOR`",
        (true, true, true) => "24-bit",
        (true, true, false) => "without 24-bit colour",
    };
    writeln!(out, "| Colours | {colours} |")?;
    let background = match caps.dark_background {
        Some(true) => "dark",
        Some(false) => "light",
        None => "unknown",
    };
    writeln!(out, "| Background | {background} |")?;
    writeln!(out, "| Hyperlinks | {} |", yes_no(caps.hyperlinks))?;
    writeln!(
        out,
        "| Underline styles | {} |\n",
        yes_no(caps.styled_underlines)
    )
}

fn write_settings(facts: &Facts, home: &Home, out: &mut String) -> std::fmt::Result {
    let choices = facts.choices;
    let on_off = |on| if on { "on" } else { "off" };
    let from = |origin| origin_text(origin, facts.env, home);
    writeln!(out, "## Settings\n")?;
    writeln!(out, "| Setting | Value | From |\n|---|---|---|")?;
    let mode = &facts.mode;
    writeln!(
        out,
        "| Mode | {} | {} |",
        mode_name(mode.value),
        from(mode.origin)
    )?;
    let theme = match &choices.theme.value {
        ThemeSource::File(path) => home.path(path),
        ThemeSource::DefaultFile(path) if path.exists() => home.path(path),
        ThemeSource::DefaultFile(_) | ThemeSource::BuiltIn => "built-in".into(),
        ThemeSource::Paper(Paper::Light) => "light paper".into(),
        ThemeSource::Paper(Paper::Dark) => "dark paper".into(),
    };
    writeln!(out, "| Theme | {theme} | {} |", from(choices.theme.origin))?;
    let margins = &choices.margins;
    writeln!(
        out,
        "| Margins | {} | {} |",
        on_off(margins.value),
        from(margins.origin)
    )?;
    let images = &choices.images;
    writeln!(
        out,
        "| Images | {} | {} |",
        on_off(images.value),
        from(images.origin)
    )?;
    let size = &facts.max_image_size;
    writeln!(
        out,
        "| Largest image | {} MB | {} |",
        size.value,
        from(size.origin)
    )?;
    let width = match (facts.width, facts.detection.window) {
        (Some(_), _) => "`--width`",
        (None, Some(_)) => "terminal",
        (None, None) => "default",
    };
    writeln!(
        out,
        "| Width | {} cells | {width} |\n",
        facts.detection.caps.width
    )
}

fn write_files(facts: &Facts, home: &Home, out: &mut String) -> std::fmt::Result {
    writeln!(out, "## Files\n")?;
    writeln!(out, "| File | Path | Status |\n|---|---|---|")?;
    match crate::config::file(facts.env) {
        Some(file) => {
            let status = match (&facts.config_error, file.path.exists()) {
                (Some(_), _) => "not read",
                (None, true) => "read",
                (None, false) => "not found",
            };
            let path = home.path(&file.path);
            writeln!(out, "| Configuration file | {path} | {status} |")?;
        }
        None => writeln!(out, "| Configuration file | none: `HOME` is not set | |")?,
    }
    if let Some(dir) = crate::config::directory(facts.env) {
        let path = dir.join("theme.toml");
        let status = if path.exists() { "found" } else { "not found" };
        writeln!(out, "| Theme file | {} | {status} |", home.path(&path))?;
    }
    writeln!(out)
}

/// A sample of what kiri renders, with a test image as a `data:` URL so
/// that the report needs no file of its own.
fn write_sample(out: &mut String) -> std::fmt::Result {
    let image = BASE64.encode(TEST_IMAGE);
    writeln!(out, "## Sample\n")?;
    writeln!(out, "### A third-level heading\n")?;
    writeln!(
        out,
        "Text in **bold**, *italics* and `code`, with a \
         [link](https://example.com/).\n"
    )?;
    writeln!(
        out,
        "| Left | Centre | Right |\n|:-----|:------:|------:|\n| one | two | three |\n"
    )?;
    writeln!(
        out,
        "```rust\nfn main() {{\n    println!(\"kiri\");\n}}\n```\n"
    )?;
    writeln!(
        out,
        "![A test image: four coloured squares](data:image/svg+xml;base64,{image})\n"
    )
}

/// The image formats this build decodes, in alphabetical order.
fn image_formats() -> String {
    let mut names: Vec<String> = ImageFormat::all()
        .filter(ImageFormat::reading_enabled)
        .map(|format| match format {
            ImageFormat::WebP => "WebP".into(),
            format => format!("{format:?}").to_uppercase(),
        })
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    let last = names.pop().unwrap_or_default();
    format!("{}, {last} and SVG.", names.join(", "))
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Auto => "auto",
        Mode::Plain => "plain",
        Mode::Ansi => "ansi",
        Mode::Kitty => "kitty",
    }
}

/// Where a setting comes from, in words; an environment variable with its
/// value.
fn origin_text(origin: Origin, env: &Environment, home: &Home) -> String {
    match origin {
        Origin::Default => "default".into(),
        Origin::File => "configuration file".into(),
        Origin::Environment(name) => match variable(env, name) {
            Some(value) => format!("`{name}={}`", home.hide(&value.to_string_lossy())),
            None => format!("`{name}`"),
        },
        Origin::Flag(flag) => format!("`{flag}`"),
    }
}

/// The value of the environment variable `name`, of those kiri reads for
/// its settings.
fn variable<'a>(env: &'a Environment, name: &str) -> Option<&'a OsStr> {
    match name {
        "KIRI_THEME" => env.kiri_theme.as_deref(),
        "KIRI_NO_IMAGES" => env.kiri_no_images.as_deref(),
        _ => None,
    }
}

/// The home directory, which the report shows as `~` so that it does not
/// tell the user's name.
struct Home<'a>(Option<&'a OsStr>);

impl Home<'_> {
    /// `text` with the home directory at the start of a path as `~`.
    fn hide(&self, text: &str) -> String {
        match self.0.map(OsStr::to_string_lossy) {
            Some(home) if !home.is_empty() && home != "/" => {
                let home = home.trim_end_matches('/');
                text.replace(&format!("{home}/"), "~/")
            }
            _ => text.to_owned(),
        }
    }

    /// `path` as code, with the home directory as `~`.
    fn path(&self, path: &Path) -> String {
        format!("`{}`", self.hide(&path.to_string_lossy()))
    }
}

/// The terminal's answers with ESC as `\e`, a backslash as `\\` and other
/// bytes that are not printable ASCII, or a backtick, as `\xNN`.
fn escaped(answers: &[u8]) -> String {
    let mut out = String::new();
    for &byte in answers {
        match byte {
            0x1b => out.push_str("\\e"),
            b'\\' => out.push_str("\\\\"),
            b'`' => out.push_str("\\x60"),
            b' '..=b'~' => out.push(char::from(byte)),
            _ => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    out
}
