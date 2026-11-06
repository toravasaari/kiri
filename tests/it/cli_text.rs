//! The kiri binary end to end with `--format text`: any file shown as its
//! source, without rendering it.

use crate::common::{
    Scratch, assert_succeeded, colour_codes, kiri_on, kiri_piped, sent, strip_own_sequences, text,
    words, write,
};

const MARKDOWN: &str = "---\ntitle: Notes\n---\n# Title\n\n*Text* and ![Icon](icon.png)\n";

#[test]
fn format_text_shows_a_markdown_file_as_its_source() {
    let dir = Scratch::new("text-markdown");
    let notes = dir.join("notes.md");
    write(&notes, MARKDOWN);
    std::fs::copy(
        crate::common::fixtures().join("images/icon.png"),
        dir.join("icon.png"),
    )
    .unwrap();
    for format in ["text", "txt", "TEXT"] {
        for mode in ["plain", "kitty"] {
            let out = kiri_on(&dir, &notes, &["--mode", mode, "--format", format]);
            assert_succeeded(&out);
            let stdout = text(&out.stdout);
            assert!(sent(&stdout).is_empty(), "{format} {mode}: {stdout:?}");
            // Front matter, heading, emphasis and image as written.
            let shown = strip_own_sequences(&stdout);
            for line in MARKDOWN.lines() {
                assert!(
                    shown.contains(line),
                    "{format} {mode}: {line:?} in {shown:?}"
                );
            }
        }
    }
}

#[test]
fn format_text_highlights_a_language_its_extension_names() {
    let dir = Scratch::new("text-language");
    let source = "fn main() {\n    let answer = 42;\n}\n";
    let colours = |name: &str| {
        let path = dir.join(name);
        write(&path, source);
        let out = kiri_on(&dir, &path, &["--mode", "ansi", "--format", "text"]);
        assert_succeeded(&out);
        let stdout = text(&out.stdout);
        let shown = strip_own_sequences(&stdout);
        assert!(shown.contains("let answer = 42;"), "{name}: {shown:?}");
        let mut codes = colour_codes(&stdout);
        codes.sort();
        codes.dedup();
        codes.len()
    };
    // Rust's tokens in colours of their own, more than plain text has.
    assert!(colours("main.rs") > colours("main.unknown"));
}

#[test]
fn format_text_reads_standard_input() {
    let dir = Scratch::new("text-stdin");
    let out = kiri_piped(
        &dir,
        &["--mode", "plain", "--format", "text"],
        b"# Title\n*text*\n",
    );
    assert_succeeded(&out);
    let stdout = text(&out.stdout);
    assert!(stdout.contains("# Title\n"), "{stdout:?}");
    assert!(stdout.contains("*text*\n"), "{stdout:?}");
}

#[test]
fn format_text_writes_plain_output_as_the_file_is() {
    // As cat does: no indent, no wrapping, no added lines, so that the
    // output can be copied or piped on.
    let dir = Scratch::new("text-bare");
    let source = format!("line one\n    indented\n{}\nlast", "word ".repeat(30));
    let path = dir.join("notes.txt");
    write(&path, &source);
    for args in [
        &["--format", "text"][..],
        &["--mode", "plain", "--format", "text"],
    ] {
        let out = kiri_on(&dir, &path, args);
        assert_succeeded(&out);
        assert_eq!(text(&out.stdout), source, "{args:?}");
        let out = kiri_piped(&dir, args, source.as_bytes());
        assert_succeeded(&out);
        assert_eq!(text(&out.stdout), source, "{args:?} from standard input");
    }
}

#[test]
fn format_text_keeps_the_file_s_escapes_from_the_terminal() {
    let dir = Scratch::new("text-escapes");
    let path = dir.join("log.txt");
    write(&path, "red \x1b[31mtext\x1b[0m\n");
    let out = kiri_on(&dir, &path, &["--mode", "kitty", "--format", "text"]);
    assert_succeeded(&out);
    // Panics on any escape that is not kiri's own.
    strip_own_sequences(&text(&out.stdout));
}

#[test]
fn format_text_needs_utf_8() {
    let dir = Scratch::new("text-not-utf-8");
    let path = dir.join("data.txt");
    std::fs::write(&path, b"\xff\xfe text").unwrap();
    let out = kiri_on(&dir, &path, &["--mode", "plain", "--format", "text"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!("kiri: {}: not UTF-8 text\n", path.display())
    );
}

#[test]
fn a_text_file_that_turns_non_utf_8_late_is_not_utf_8_text() {
    // The first bytes are plain ASCII, so the file looks like text; a
    // Latin-1 byte after them is what fails, and the name says it was text.
    let dir = Scratch::new("text-late-not-utf-8");
    let path = dir.join("a.txt");
    let mut bytes = vec![b'a'; 100];
    bytes.extend_from_slice(b" caf\xe9\n");
    std::fs::write(&path, bytes).unwrap();
    let out = kiri_on(&dir, &path, &["--mode", "plain"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        text(&out.stderr),
        format!("kiri: {}: not UTF-8 text\n", path.display())
    );
}

#[test]
fn help_names_the_text_format() {
    let dir = Scratch::new("text-help");
    let out = crate::common::command(&dir).arg("--help").output().unwrap();
    let help = words(&text(&out.stdout));
    assert!(help.contains("text (the file as it is"), "{help}");
}

#[test]
fn a_file_that_is_not_markdown_is_shown_as_it_is_without_asking() {
    // No `--format`: a YAML file that starts like Markdown, and a license
    // with no extension, come out as they are in plain output.
    let dir = Scratch::new("text-by-name");
    let yaml = "# Settings\nname: *kiri*\nitems:\n  - one\n";
    let license = "# Not a heading\n\nSome *words*.\n";
    for (name, source) in [("config.yaml", yaml), ("LICENSE", license)] {
        let path = dir.join(name);
        write(&path, source);
        let out = kiri_on(&dir, &path, &[]);
        assert_succeeded(&out);
        assert_eq!(text(&out.stdout), source, "{name}");
    }
    // A Markdown extension is still Markdown.
    let path = dir.join("notes.md");
    write(&path, license);
    let out = kiri_on(&dir, &path, &[]);
    assert_succeeded(&out);
    assert!(
        text(&out.stdout).contains("Some words."),
        "{:?}",
        text(&out.stdout)
    );
}

#[test]
fn a_pipe_given_by_its_path_is_read_as_standard_input() {
    // As `kiri <(cat notes.md)` gives it: a path that is not a regular
    // file, so Markdown unless its content says otherwise.
    let dir = Scratch::new("text-pipe");
    let out = kiri_piped(
        &dir,
        &["--mode", "plain", "/dev/stdin"],
        b"# Title\n\n*b*\n",
    );
    assert_succeeded(&out);
    let shown = text(&out.stdout);
    assert!(shown.contains('b') && !shown.contains("*b*"), "{shown:?}");
    let out = kiri_piped(
        &dir,
        &["--mode", "plain", "/dev/stdin"],
        b"{\"a\": \"*b*\"}\n",
    );
    assert_succeeded(&out);
    assert!(text(&out.stdout).contains("{\"a\": \"*b*\"}"));
}
