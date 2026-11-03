//! `kiri --doctor`: a report, in Markdown, of what kiri found out about the
//! terminal and which settings it uses.

use std::ffi::OsStr;
use std::fmt::Write;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::ImageFormat;

use crate::config::{AUTO_DETECT, Choices, Environment, Named, Origin, Passthrough, ThemeSource};
use crate::diagrams::find::{self, Choice, Search};
use crate::diagrams::{BuiltIn, Command, LANGUAGES};
use crate::term::tmux::Pane;
use crate::term::{CellSize, Detection, Mode};
use crate::typeset::caps::Graphics;
use crate::typeset::images::size::DEFAULT_CELL;
use crate::typeset::size::TextSize;
use crate::typeset::theme::{Paper, Theme};

/// What the report tells.
#[derive(Debug)]
pub struct Facts<'a> {
    pub env: &'a Environment,
    pub detection: &'a Detection,
    pub choices: &'a Choices,
    /// The theme kiri uses: the default one when it could not be loaded.
    pub theme: &'a Theme,
    /// Why the configuration file could not be read.
    pub config_error: Option<String>,
    /// Why the theme could not be loaded.
    pub theme_error: Option<String>,
    pub output: Output,
}

/// Where the report goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// Rendered in a terminal.
    Rendered,
    /// As Markdown into a file or a pipe.
    Markdown,
    /// As Markdown without the sample, with `--raw`.
    Raw,
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
    let home_dir = crate::config::home_directory(facts.env);
    let home = Home(home_dir.as_deref().map(Path::as_os_str));
    writeln!(out, "# kiri doctor\n")?;
    writeln!(
        out,
        "kiri {}: terminal features and settings.\n",
        env!("CARGO_PKG_VERSION")
    )?;
    write_problems(facts, &home, out)?;
    write_terminal(facts, out)?;
    write_settings(facts, &home, out)?;
    write_files(facts, &home, out)?;
    writeln!(out, "## Image formats\n")?;
    writeln!(out, "{}\n", image_formats())?;
    write_diagrams(facts, &home, out)?;
    if facts.output != Output::Raw {
        write_sample(out)?;
    }
    if let Some(answers) = &facts.detection.answers {
        writeln!(out, "## Probe answers\n")?;
        writeln!(out, "```text\n{}\n```\n", escaped(answers))?;
    }
    if facts.output == Output::Rendered {
        writeln!(
            out,
            "Run `kiri --doctor --raw` to get this report as text for a bug \
             report."
        )?;
    }
    Ok(())
}

fn write_problems(facts: &Facts, home: &Home, out: &mut String) -> std::fmt::Result {
    let problems: Vec<String> = [&facts.config_error, &facts.theme_error]
        .into_iter()
        .flatten()
        .cloned()
        .chain(facts.choices.warnings.iter().map(ToString::to_string))
        .collect();
    if problems.is_empty() {
        return Ok(());
    }
    writeln!(out, "## Problems\n")?;
    for problem in &problems {
        // A problem starts with what it is about: the path of a file, or
        // the name of an environment variable. A longer error, such as one
        // that shows the line in the file, goes in a code block of its own.
        match problem.split_once(": ") {
            Some((path, error)) if error.contains('\n') => {
                writeln!(out, "- {}:\n\n  ```text", code(&home.hide(path)))?;
                for line in home.hide_in(error).trim_end().lines() {
                    writeln!(out, "  {line}")?;
                }
                writeln!(out, "  ```")?;
            }
            Some((path, error)) => {
                writeln!(out, "- {}: {}", code(&home.hide(path)), home.hide_in(error))?
            }
            None => writeln!(out, "- {}", home.hide_in(problem))?,
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
    let mut terminal = match (&detection.answers, facts.choices.mode.value) {
        (Some(_), _) => detection
            .terminal()
            .map_or_else(|| "unknown".into(), |name| code(&name)),
        (None, Mode::Auto) if detection.background => {
            "not asked: kiri runs in the background".into()
        }
        (None, Mode::Auto) if caps.styling => "did not answer".into(),
        (None, Mode::Auto) => "none".into(),
        (None, mode) => format!("not asked in mode {}", mode.name()),
    };
    if let Some(term) = &facts.env.term {
        let term = format!("TERM={}", term.to_string_lossy());
        write!(terminal, ", {}", code(&term))?;
    }
    row(out, &["Terminal", &terminal])?;
    if let Some(pane) = &detection.tmux {
        let how = facts.choices.passthrough.value;
        row(out, &["tmux", &tmux(pane, how)])?;
    }
    let window = match detection.window {
        Some(w) => format!(
            "{} × {} cells, {} × {} pixels",
            w.columns, w.rows, w.width, w.height
        ),
        None => "unknown".into(),
    };
    row(out, &["Window", &window])?;
    let cell = match detection.cell_size() {
        CellSize::Window(w, h) => format!("{w} × {h} pixels, from the window size"),
        CellSize::Terminal(w, h) => format!("{w} × {h} pixels, from the terminal"),
        CellSize::Unknown => format!(
            "unknown; images take {} × {} pixels",
            DEFAULT_CELL.0, DEFAULT_CELL.1
        ),
    };
    row(out, &["Cell size", &cell])?;
    let graphics = match (&caps.graphics, caps.draws_images()) {
        (Graphics::None, _) => "no",
        (_, true) => "yes, with Unicode placeholders",
        (_, false) => "yes, without Unicode placeholders: images show their alt text",
    };
    row(out, &["Graphics", graphics])?;
    if let Graphics::Kitty(kitty) = &caps.graphics {
        let protocol = match &kitty.terminal {
            Some(terminal) => format!("{}, from {}", caps.graphics.name(), code(terminal)),
            None => format!("{}, the terminal tells no version", caps.graphics.name()),
        };
        row(out, &["Graphics protocol", &protocol])?;
        let animations = match (kitty.animates, caps.tmux_passthrough) {
            (true, _) => "played by the terminal itself",
            (false, true) => "the first frame only, behind tmux",
            (false, false) => "not played by the terminal: kiri redraws the frames in the pager",
        };
        row(out, &["Animations", animations])?;
    }
    row(out, &["Text sizing", yes_no(caps.text_sizing)])?;
    let colours = match (caps.styling, caps.color, caps.truecolor) {
        (false, _, _) => "none",
        (true, false, _) => "off: `NO_COLOR`",
        (true, true, true) => "24-bit",
        (true, true, false) => "without 24-bit colour",
    };
    row(out, &["Colours", colours])?;
    let background = match caps.dark_background {
        Some(true) => "dark",
        Some(false) => "light",
        None => "unknown",
    };
    row(out, &["Background", background])?;
    let hyperlinks = match &detection.tmux {
        Some(pane) if !caps.hyperlinks && pane.hyperlinks == Some(false) => tmux_links(pane),
        _ => yes_no(caps.hyperlinks).into(),
    };
    row(out, &["Hyperlinks", &hyperlinks])?;
    row(out, &["Underline styles", yes_no(caps.styled_underlines)])?;
    writeln!(out)
}

fn write_settings(facts: &Facts, home: &Home, out: &mut String) -> std::fmt::Result {
    let choices = facts.choices;
    let from = |origin| origin_text(origin, facts.env, home);
    writeln!(out, "## Settings\n")?;
    writeln!(out, "| Setting | Value | From |\n|---|---|---|")?;
    let mode = &choices.mode;
    row(out, &["Mode", mode.value.name(), &from(mode.origin)])?;
    let theme = match &choices.theme.value {
        ThemeSource::File(path) | ThemeSource::DefaultFile(path) => home.path(path),
        ThemeSource::Named { name, path } => format!("{} ({})", code(name), home.path(path)),
        ThemeSource::Unknown { name, .. } => format!("{} (unknown)", code(name)),
        ThemeSource::BuiltIn => "built-in".into(),
        ThemeSource::Paper(Paper::Light) => "light paper".into(),
        ThemeSource::Paper(Paper::Dark) => "dark paper".into(),
    };
    row(out, &["Theme", &theme, &from(choices.theme.origin)])?;
    let sizes = heading_sizes(facts.theme);
    // A theme that does not load leaves the default sizes.
    let origin = match facts.theme_error {
        Some(_) => Origin::Default,
        None => choices.theme.origin,
    };
    row(out, &["Heading sizes", &sizes, &from(origin)])?;
    let margins = &choices.margins;
    row(
        out,
        &["Margins", margins.value.name(), &from(margins.origin)],
    )?;
    let images = &choices.images;
    row(out, &["Images", images.value.name(), &from(images.origin)])?;
    let paging = &choices.paging;
    row(out, &["Paging", paging.value.name(), &from(paging.origin)])?;
    let follow = &choices.follow;
    row(out, &["Follow", follow.value.name(), &from(follow.origin)])?;
    let jump = &choices.follow_jump;
    row(out, &["Follow jump", jump.value.name(), &from(jump.origin)])?;
    let first = &choices.initial_load;
    row(
        out,
        &["Initial load", first.value.name(), &from(first.origin)],
    )?;
    let passthrough = &choices.passthrough;
    let how = passthrough.value.name();
    row(out, &["Passthrough", how, &from(passthrough.origin)])?;
    let search_case = &choices.search_case;
    let case = search_case.value.name();
    row(out, &["Search case", case, &from(search_case.origin)])?;
    let front_matter = &choices.front_matter;
    let style = front_matter.value.name();
    row(out, &["Front matter", style, &from(front_matter.origin)])?;
    let headings = &choices.headings;
    let how = headings.value.name();
    row(out, &["Headings", how, &from(headings.origin)])?;
    let rows = &choices.table_rows;
    let name = rows.value.name();
    row(out, &["Table rows", name, &from(rows.origin)])?;
    let diagrams = &choices.diagrams;
    let on = diagrams.value.name();
    row(out, &["Diagrams", on, &from(diagrams.origin)])?;
    let limit = &choices.diagram_timeout;
    let seconds = format!("{} s", limit.value.as_secs_f64());
    row(out, &["Diagram time limit", &seconds, &from(limit.origin)])?;
    let panel = &choices.diagram_background_panel;
    let on = panel.value.name();
    row(out, &["Diagram panels", on, &from(panel.origin)])?;
    let errors = &choices.render_errors;
    let name = errors.value.name();
    row(out, &["Render errors", name, &from(errors.origin)])?;
    let mouse = &choices.mouse;
    row(out, &["Mouse", mouse.value.name(), &from(mouse.origin)])?;
    let reading = &choices.reading;
    row(
        out,
        &["Reading", reading.value.name(), &from(reading.origin)],
    )?;
    let wide = &choices.wide;
    row(out, &["Wide", wide.value.name(), &from(wide.origin)])?;
    let size = &choices.max_image_size;
    let mb = format!("{} MB", size.value.0);
    row(out, &["Largest image", &mb, &from(size.origin)])?;
    let given = choices.width.value.is_some() || choices.width.origin != Origin::Default;
    let width = match (given, facts.detection.window) {
        (true, _) => from(choices.width.origin),
        (false, Some(_)) => "terminal".to_owned(),
        (false, None) => from(Origin::Default),
    };
    let cells = format!("{} cells", facts.detection.caps.width);
    row(out, &["Width", &cells, &width])?;
    writeln!(out)
}

/// The sizes of h1 to h6 as times the normal size. A size the theme gave
/// that kiri rounded to draw it shows what was written and what is drawn,
/// such as `1.3 (drawn 1.27 = 2 · 7/11)`.
fn heading_sizes(theme: &Theme) -> String {
    let sizes: Vec<String> = (1..=6)
        .map(|level| {
            let heading = theme.heading(level);
            let drawn = decimal(heading.size);
            match (&heading.written_scale, heading.size.fraction()) {
                (Some(written), Some(fraction)) => format!(
                    "{written} (drawn {drawn} = {} · {}/{})",
                    heading.size.scale(),
                    fraction.numerator(),
                    fraction.denominator()
                ),
                (Some(written), None) => format!("{written} (drawn {drawn})"),
                (None, _) => drawn,
            }
        })
        .collect();
    sizes.join(", ")
}

/// `size` as a decimal with at most two places, such as `1.75` or `2`.
fn decimal(size: TextSize) -> String {
    let text = format!("{:.2}", size.value());
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn write_files(facts: &Facts, home: &Home, out: &mut String) -> std::fmt::Result {
    writeln!(out, "## Files\n")?;
    writeln!(out, "| File | Path | From | Status |\n|---|---|---|---|")?;
    match crate::config::file(facts.env) {
        Some(file) => {
            let status = match (&facts.config_error, file.path.exists()) {
                (Some(_), _) => "not read",
                (None, true) => "read",
                (None, false) => "not found",
            };
            let path = home.path(&file.path);
            let from = origin_text(file.origin, facts.env, home);
            row(out, &["Configuration file", &path, &from, status])?;
        }
        None => row(
            out,
            &["Configuration file", "none: `HOME` is not set", "", ""],
        )?,
    }
    if let Some(dir) = crate::config::directory(facts.env) {
        let path = dir.value.join("theme.toml");
        let status = if path.exists() { "found" } else { "not found" };
        let from = origin_text(dir.origin, facts.env, home);
        row(out, &["Theme file", &home.path(&path), &from, status])?;
    }
    write_work_files(facts, home, out)?;
    writeln!(out)
}

/// Rows for where programs that draw diagrams keep their files: the
/// cache directory, and the temporary directory when that cannot be used.
fn write_work_files(facts: &Facts, home: &Home, out: &mut String) -> std::fmt::Result {
    use crate::diagrams::external::{LONGEST_WORK_DIR, Unusable, not_writable, unusable};

    const WORK_FILES: &str = "Diagram work files";
    let env = facts.env;
    let why = |problem: Unusable| match problem {
        Unusable::TooLong => format!("its path is longer than {LONGEST_WORK_DIR} bytes"),
        Unusable::NotADirectory(path) => format!("{} is not a directory", home.path(&path)),
        Unusable::NotWritable(path) => format!("{} cannot be written to", home.path(&path)),
    };
    match crate::config::cache_directory_setting(env) {
        Some(cache) => {
            let work = cache.value.join("diagrams");
            let from = origin_text(cache.origin, env, home);
            let Some(problem) = unusable(&work) else {
                return row(out, &[WORK_FILES, &home.path(&work), &from, "usable"]);
            };
            let status = format!("not usable: {}", why(problem));
            row(out, &[WORK_FILES, &home.path(&work), &from, &status])?;
        }
        None => row(out, &[WORK_FILES, "none: `HOME` is not set", "", ""])?,
    }
    let temp = crate::config::temp_directory_setting(env);
    let from = origin_text(temp.origin, env, home);
    let temp = temp.value;
    let status = match not_writable(&temp) {
        None => "usable".to_owned(),
        Some(problem) => format!("not usable: {}", why(problem)),
    };
    row(out, &[WORK_FILES, &home.path(&temp), &from, &status])
}

/// The renderers for each diagram language: the built-in one and the
/// external command, a row for each.
fn write_diagrams(facts: &Facts, home: &Home, out: &mut String) -> std::fmt::Result {
    let on = facts.choices.diagrams.value;
    let search = Search::new(facts.env);
    writeln!(out, "## Diagrams\n")?;
    writeln!(out, "| Language | Renderer | Status |\n|---|---|---|")?;
    for language in LANGUAGES.iter().map(|language| language.name) {
        let choices = facts.choices;
        let configured = &choices.diagram_commands;
        let choice = find::choose(language, configured, &choices.diagram_no_search, &search);
        let command = |found: &str, command: &Command| {
            format!(
                "command {found}: {}",
                code(&home.hide_in(&command.argv.join(" ")))
            )
        };
        let (external, used) = match &choice {
            Choice::Configured(argv) => (command("configured", argv), true),
            Choice::Found(argv) => (command("found", argv), true),
            Choice::NotFound(program) => (format!("command: {} not found", code(program)), false),
            Choice::NotLookedFor(program) => {
                let off = code(&format!("{AUTO_DETECT} = false"));
                (
                    format!("command: {} not looked for: {off}", code(program)),
                    false,
                )
            }
            Choice::None => ("command: none configured".to_owned(), false),
        };
        let (built_in, status) = match BuiltIn::version(language) {
            Some(version) if used => (
                format!("built-in: {version}"),
                "not used: a command is configured",
            ),
            Some(version) if on => (format!("built-in: {version}"), "used"),
            Some(version) => (format!("built-in: {version}"), "not used: diagrams are off"),
            None => ("built-in".to_owned(), "not in this build"),
        };
        row(out, &[&code(language), &built_in, status])?;
        let status = match (used, on) {
            (true, true) => "used",
            (true, false) => "not used: diagrams are off",
            (false, _) => "not used",
        };
        row(out, &[&code(language), &external, status])?;
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

/// Where a setting comes from, in words; an environment variable with its
/// value.
fn origin_text(origin: Origin, env: &Environment, home: &Home) -> String {
    match origin {
        Origin::Default => "default".into(),
        Origin::File => "configuration file".into(),
        Origin::Environment(name) => match env.get(name) {
            Some(value) => code(&format!("{name}={}", home.hide(&value.to_string_lossy()))),
            None => code(name),
        },
        Origin::Flag(flag) => format!("`{flag}`"),
    }
}

/// The home directory, which the report shows as `~` so that it does not
/// tell the user's name.
struct Home<'a>(Option<&'a OsStr>);

impl Home<'_> {
    /// The path `text` with the home directory at its start as `~`.
    fn hide(&self, text: &str) -> String {
        let home = self.0.map(Path::new).filter(|home| home.parent().is_some());
        match home.and_then(|home| Path::new(text).strip_prefix(home).ok()) {
            Some(rest) if rest.as_os_str().is_empty() => "~".into(),
            Some(rest) => format!("~/{}", rest.to_string_lossy()),
            None => text.to_owned(),
        }
    }

    /// `text` with the home directory as `~` wherever a path starts with
    /// it: after the start, a space, a quote or a bracket, and before a
    /// `/` or the end of the path.
    fn hide_in(&self, text: &str) -> String {
        let Some(home) = self
            .0
            .map(Path::new)
            .filter(|home| home.parent().is_some())
            .and_then(|home| home.to_str())
        else {
            return text.to_owned();
        };
        let home = home.trim_end_matches('/');
        let ends_path = |c: char| !(c.is_alphanumeric() || matches!(c, '.' | '_' | '-'));
        let mut out = String::new();
        let mut rest = text;
        // The character of `text` before `rest`, which a match at the start
        // of `rest` follows.
        let mut previous = None;
        while let Some(at) = rest.find(home) {
            let (before, after) = (&rest[..at], &rest[at + home.len()..]);
            let starts = before
                .chars()
                .next_back()
                .or(previous)
                .is_none_or(|c| c.is_whitespace() || "\"'`([{=:".contains(c));
            let ends = after.chars().next().is_none_or(ends_path);
            out.push_str(before);
            out.push_str(if starts && ends { "~" } else { home });
            previous = home.chars().next_back();
            rest = after;
        }
        out.push_str(rest);
        out
    }

    /// `path` as code, with the home directory as `~`.
    fn path(&self, path: &Path) -> String {
        code(&self.hide(&path.to_string_lossy()))
    }
}

/// A table row of `cells`. A `|` in a cell, also in code, is escaped so
/// that it does not end the cell.
/// What kiri learnt about the tmux pane it wraps graphics for, with the
/// passthrough setting `how`.
fn tmux(pane: &Pane, how: Passthrough) -> String {
    let mut found = match &pane.client {
        Some(client) => format!("passthrough for {}", code(client)),
        None => "passthrough".into(),
    };
    match (&pane.passthrough, how) {
        (None, Passthrough::Tmux) => found += "; tmux not asked, as passthrough is `tmux`",
        (None, _) if pane.client.is_none() => found += "; tmux did not answer",
        (None, _) => found += ", allow-passthrough unknown",
        _ if pane.turned_on => found += ", allow-passthrough `on`, turned on by kiri",
        (Some(value), _) if value == "off" => {
            found += ", allow-passthrough `off`: images cannot pass";
        }
        (Some(value), _) => found += &format!(", allow-passthrough {}", code(value)),
    }
    // Whether images come back matters only where they can pass.
    let passes = pane.turned_on
        || pane
            .passthrough
            .as_ref()
            .is_some_and(|value| value != "off");
    if passes {
        found += match pane.focus_events {
            Some(true) => ", focus-events `on`",
            Some(false) => {
                ", focus-events `off`: images come back after a window switch only \
                 with `set -g focus-events on` in tmux.conf, or with Ctrl+L"
            }
            None => ", focus-events unknown",
        };
    }
    found
}

/// The Hyperlinks row when tmux does not pass links on: the line for
/// tmux.conf that lets it, with the terminal's name when tmux told it.
/// tmux gives a client its terminal features when it attaches, so the
/// line counts only for a client that attaches after tmux has read it.
fn tmux_links(pane: &Pane) -> String {
    let line = |name: &str| code(&format!("set -as terminal-features ',{name}:hyperlinks'"));
    let then = "then reload tmux.conf, detach and attach again";
    match &pane.term_name {
        Some(name) => format!(
            "no: tmux does not pass them on; {} in tmux.conf lets it; {then}",
            line(name)
        ),
        None => format!(
            "no: tmux does not pass them on; {} in tmux.conf, with the `TERM` of \
             the terminal outside tmux, lets it; {then}",
            line("TERM")
        ),
    }
}

fn row(out: &mut String, cells: &[&str]) -> std::fmt::Result {
    for cell in cells {
        write!(out, "| {} ", cell.replace('|', "\\|"))?;
    }
    writeln!(out, "|")
}

/// `text` as a code span: between more backticks than it has in a row, and
/// with spaces inside them when it starts or ends with one. Control
/// characters show as `\xNN`.
fn code(text: &str) -> String {
    let text = printable(text);
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{text}{pad}{fence}")
}

/// `text` with control characters, such as a tab or a newline, as `\xNN`,
/// so that they cannot break the Markdown they are in.
fn printable(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    fn hide(home: &str, text: &str) -> String {
        Home(Some(OsStr::new(home))).hide(text)
    }

    #[test]
    fn hides_the_home_directory_at_the_start_of_a_path() {
        assert_eq!(hide("/home/u", "/home/u/.config/kiri"), "~/.config/kiri");
        assert_eq!(hide("/home/u/", "/home/u/.config/kiri"), "~/.config/kiri");
        assert_eq!(hide("/home/u", "/home/u"), "~");
    }

    #[test]
    fn leaves_the_home_directory_elsewhere_alone() {
        assert_eq!(hide("/home/u", "/x/home/u/t.toml"), "/x/home/u/t.toml");
        assert_eq!(hide("/home/u", "/home/u2/t.toml"), "/home/u2/t.toml");
        assert_eq!(hide("/home/u", "relative/t.toml"), "relative/t.toml");
    }

    fn hide_in(home: &str, text: &str) -> String {
        Home(Some(OsStr::new(home))).hide_in(text)
    }

    #[test]
    fn hides_the_home_directory_where_paths_start_inside_a_text() {
        assert_eq!(
            hide_in("/home/u", "theme = \"/home/u/x.toml\" (/home/u)"),
            "theme = \"~/x.toml\" (~)"
        );
        assert_eq!(hide_in("/home/u/", "at /home/u/x: no"), "at ~/x: no");
        assert_eq!(hide_in("/home/u", "/home/u/home/u"), "~/home/u");
        assert_eq!(hide_in("/home/u", "x/home/u/home/u"), "x/home/u/home/u");
    }

    #[test]
    fn leaves_the_home_directory_alone_inside_other_paths_in_a_text() {
        let text = "/x/home/u/t.toml and /home/u2/t.toml and a/home/u";
        assert_eq!(hide_in("/home/u", text), text);
        assert_eq!(hide_in("/", text), text);
        assert_eq!(Home(None).hide_in("/home/u/t"), "/home/u/t");
    }

    #[test]
    fn hides_nothing_without_a_home_directory() {
        assert_eq!(hide("", "/home/u/t.toml"), "/home/u/t.toml");
        assert_eq!(hide("/", "/t.toml"), "/t.toml");
        assert_eq!(Home(None).hide("/home/u/t.toml"), "/home/u/t.toml");
    }
}
