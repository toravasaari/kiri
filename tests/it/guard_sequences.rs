//! Each terminal protocol's escape sequences only in its own place
//! (AGENTS.md, "Architecture rules"), as a test.
//!
//! The guard reads the non-test code of `src/` with its string and char
//! literals kept, and finds the sequences that only one protocol defines.
//! `PROTOCOLS` says, for each protocol, where its sequences may be written
//! and how they are told; a protocol that gets a place of its own is one
//! row more. General sequences, such as SGR, cursor movement, OSC 8 and OSC
//! 52, belong to no row, and neither do the generic queries and the answers
//! to them. Ghostty has no row: kiri speaks kitty's graphics protocol to it.
//!
//! The text is lowercased first, and ESC is found as `\x1b`, `\u{1b}` with
//! any leading zeros, or the char itself. What it does not see: a sequence
//! put together from parts (`"\x1b" + "_G"`), or written in its C1 form.

use crate::rust_source::{Strings, non_test_sources};

const ESC: char = '\u{1b}';

/// A protocol, the places its sequences may be written in (a file, or a
/// directory ending in `/`), and whether a line of lowercased code with ESC
/// as a char holds one of its sequences.
type Protocol = (&'static str, &'static [&'static str], fn(&str) -> bool);

const PROTOCOLS: &[Protocol] = &[
    ("kitty", &["src/present/kitty/"], kitty),
    ("sixel", &["src/present/sixel/"], sixel),
    (
        "tmux passthrough",
        &["src/present/tmux.rs", "src/present/tmux/"],
        tmux,
    ),
];

/// Graphics (APC `_G`), text sizing (OSC 66) and the Unicode placeholder.
fn kitty(line: &str) -> bool {
    line.contains(&format!("{ESC}_g"))
        || line.contains(&format!("{ESC}]66;"))
        || line.contains("\\u{10eeee}")
        || line.contains("0x10eeee")
        || line.contains('\u{10EEEE}')
}

/// A sixel image: DCS, its numeric parameters, then `q`. A parameter may
/// be a `format!` argument, `{aspect}`.
fn sixel(line: &str) -> bool {
    line.match_indices(&format!("{ESC}p")).any(|(at, found)| {
        let mut rest = &line[at + found.len()..];
        loop {
            rest = rest.trim_start_matches(|c: char| c.is_ascii_digit() || c == ';');
            match rest
                .strip_prefix('{')
                .and_then(|inside| inside.split_once('}'))
            {
                Some((_, after)) => rest = after,
                None => return rest.starts_with('q'),
            }
        }
    })
}

/// tmux's passthrough of a sequence to the terminal outside.
fn tmux(line: &str) -> bool {
    line.contains(&format!("{ESC}ptmux;"))
}

#[test]
fn each_protocols_sequences_are_only_in_its_own_place() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    for source in non_test_sources(&src, Strings::Keep) {
        let code = with_esc(&source.code.to_lowercase());
        for (number, line) in code.lines().enumerate() {
            for &(protocol, places, holds) in PROTOCOLS {
                if holds(line) && !in_place(&source.shown, places) {
                    found.push(format!(
                        "  {}:{}: {protocol} (belongs in {})",
                        source.shown,
                        number + 1,
                        places.join(", ")
                    ));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "a protocol's escape sequence is written outside its place \
         (AGENTS.md, \"Architecture rules\"):\n{}\n\
         Write it in the protocol's own module and call that from here.\n",
        found.join("\n")
    );
}

/// `code` with each way Rust writes ESC in a literal made the char itself.
/// It is already lowercased, so `\x1B` is `\x1b`.
fn with_esc(code: &str) -> String {
    let mut code = code.replace("\\x1b", &ESC.to_string());
    for zeros in (0..=4).rev() {
        code = code.replace(&format!("\\u{{{}1b}}", "0".repeat(zeros)), &ESC.to_string());
    }
    code
}

/// Whether the file `shown` is one of `places`.
fn in_place(shown: &str, places: &[&str]) -> bool {
    places
        .iter()
        .any(|place| shown == *place || (place.ends_with('/') && shown.starts_with(place)))
}

#[test]
fn the_guard_tells_each_protocols_sequences() {
    let holds = |text: &str| -> Vec<&str> {
        let line = with_esc(&text.to_lowercase());
        (PROTOCOLS.iter())
            .filter(|(_, _, holds)| holds(&line))
            .map(|&(protocol, _, _)| protocol)
            .collect()
    };
    assert_eq!(holds(r#""\x1B_Ga=T,f=100""#), ["kitty"]);
    assert_eq!(holds(r#"format!("\u{001b}]66;s=2;{text}")"#), ["kitty"]);
    assert_eq!(holds(r"const P: char = '\u{10EEEE}';"), ["kitty"]);
    assert_eq!(holds("let p = 0x10EEEE;"), ["kitty"]);
    assert_eq!(holds("let p = '\u{10EEEE}';"), ["kitty"]);
    assert_eq!(holds(r#"b"\x1bP0;1;0q""#), ["sixel"]);
    assert_eq!(holds(r#"format!("\x1bP0;{aspect};0q")"#), ["sixel"]);
    assert_eq!(holds(r#""\u{1b}Pq""#), ["sixel"]);
    assert_eq!(
        holds(r#"const OPEN: &str = "\x1bPtmux;";"#),
        ["tmux passthrough"]
    );
    // General sequences and the answers to generic queries.
    assert!(holds(r#""\x1bP>|kitty(0.47.1)\x1b\\""#).is_empty());
    assert!(holds(r#"text.find("\x1bP1$r")"#).is_empty());
    assert!(holds(r#""\x1b]52;c;{}\x1b\\""#).is_empty());
    assert!(holds(r#""\x1b[?2026h""#).is_empty());
    assert!(holds(r##"const C: &str = "#10EEEE";"##).is_empty());
}

#[test]
fn a_place_is_a_file_or_a_directory() {
    let places = ["src/present/tmux.rs", "src/present/tmux/"];
    assert!(in_place("src/present/tmux.rs", &places));
    assert!(in_place("src/present/tmux/tests.rs", &places));
    assert!(!in_place("src/present/tmuxfoo.rs", &places));
}
