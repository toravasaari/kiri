//! What the document cannot do to the terminal.

use kiri::typeset::caps::Capabilities;

use crate::common::{WIDTH, fixed_inputs, render, strip_own_sequences};

#[test]
fn escape_sequences_in_the_document_are_neutralised() {
    let out = render(
        "text \x1b]0;evil\x07 here\n",
        &fixed_inputs(Capabilities::kitty(WIDTH)),
    );
    assert!(!out.contains("\x1b]0;"), "{out:?}");
}

#[test]
fn control_characters_in_the_document_do_not_reach_the_terminal() {
    // ESC and the C1 string terminator U+009C in a heading, a link target,
    // image alt text, a table cell, a code fence's info string and a code
    // block.
    let source = "# Head \x1b]0;evil\x07 \u{9c}\n\n\
        [link](<https://example.com/\x1b]0;evil\x07\u{9c}>) and \
        ![alt \x1bc \u{9c}](image.png)\n\n\
        | Cell |\n|---|\n| \x1b]0;evil\x07 \u{9c} |\n\n\
        ```rust\x1b]0;evil\x07\u{9c}\ncode \x1b[2J \u{9c}\n```\n";
    let document = kiri::typeset::parse::parse(source);
    // The control characters survive parsing, so the renderer sees them.
    assert_eq!(format!("{document:?}").matches("\\u{1b}").count(), 6);
    assert_eq!(format!("{document:?}").matches("\\u{9c}").count(), 6);
    for caps in [
        Capabilities::plain(WIDTH),
        Capabilities::ansi(WIDTH),
        Capabilities::kitty(WIDTH),
    ] {
        let inputs = fixed_inputs(caps.clone());
        let mut out = Vec::new();
        kiri::render::render(&document, &inputs, &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        if caps.hyperlinks {
            // The link target is still written, without the controls.
            assert!(
                out.contains("\x1b]8;;https://example.com/]0;evil\x1b\\"),
                "{out:?}"
            );
        }
        let text = strip_own_sequences(&out);
        assert!(
            !text.chars().any(|c| c.is_control() && c != '\n'),
            "{caps:?}: {out:?}"
        );
    }
}
