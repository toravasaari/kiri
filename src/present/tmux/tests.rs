use super::*;

/// What `parts`, written one after another, come out as.
fn wrapped(parts: &[&[u8]], wrap: bool) -> String {
    let mut out = Wrapping::new(Vec::new(), wrap);
    for part in parts {
        out.write_all(part).unwrap();
    }
    out.flush().unwrap();
    String::from_utf8(out.into_inner()).unwrap()
}

const GRAPHICS: &str = "a\x1b_Gi=1;AAAA\x1b\\b";
const WRAPPED: &str = "a\x1bPtmux;\x1b\x1b_Gi=1;AAAA\x1b\x1b\\\x1b\\b";

#[test]
fn wraps_a_graphics_sequence_for_tmux() {
    assert_eq!(wrapped(&[GRAPHICS.as_bytes()], true), WRAPPED);
}

#[test]
fn a_sequence_split_over_writes_is_wrapped_whole() {
    let bytes: Vec<&[u8]> = GRAPHICS.as_bytes().chunks(1).collect();
    assert_eq!(wrapped(&bytes, true), WRAPPED);
}

#[test]
fn two_sequences_in_a_row_share_a_passthrough() {
    let two = "\x1b_Ga=d\x1b\\\x1b_Gi=2;B\x1b\\";
    let expected = "\x1bPtmux;\x1b\x1b_Ga=d\x1b\x1b\\\
                        \x1b\x1b_Gi=2;B\x1b\x1b\\\x1b\\";
    assert_eq!(wrapped(&[two.as_bytes()], true), expected);
    // In separate writes each sequence gets its own passthrough: the end
    // of a write closes it.
    let parts = [
        b"\x1b_Ga=d\x1b\\".as_slice(),
        b"\x1b_Gi=2;B\x1b\\".as_slice(),
    ];
    assert_eq!(
        wrapped(&parts, true),
        "\x1bPtmux;\x1b\x1b_Ga=d\x1b\x1b\\\x1b\\\
            \x1bPtmux;\x1b\x1b_Gi=2;B\x1b\x1b\\\x1b\\"
    );
}

#[test]
fn another_byte_between_sequences_closes_the_passthrough() {
    let text = "\x1b_Ga=d\x1b\\x\x1b_Gi=2;B\x1b\\";
    let expected = "\x1bPtmux;\x1b\x1b_Ga=d\x1b\x1b\\\x1b\\x\
                        \x1bPtmux;\x1b\x1b_Gi=2;B\x1b\x1b\\\x1b\\";
    assert_eq!(wrapped(&[text.as_bytes()], true), expected);
    // An escape sequence of another kind closes it too, and the ESC
    // that might have started a graphics sequence is not lost.
    let other = "\x1b_Ga=d\x1b\\\x1b[1m\x1b_Gi=2;B\x1b\\";
    let expected = "\x1bPtmux;\x1b\x1b_Ga=d\x1b\x1b\\\x1b\\\x1b[1m\
                        \x1bPtmux;\x1b\x1b_Gi=2;B\x1b\x1b\\\x1b\\";
    assert_eq!(wrapped(&[other.as_bytes()], true), expected);
}

#[test]
fn the_end_of_a_write_closes_the_passthrough() {
    let mut inner = Vec::new();
    let mut out = Wrapping::new(&mut inner, true);
    out.write_all(b"\x1b_Ga=d\x1b\\\x1b_Gi=2;B\x1b\\").unwrap();
    // Nothing is open now: a write straight to the tty beside this
    // one would not end up inside the passthrough.
    assert!(out.inner.ends_with(b"\x1b\x1b\\\x1b\\"));
    // A held ESC does not keep it open either.
    out.write_all(b"\x1b_Gi=3;C\x1b\\\x1b").unwrap();
    assert!(out.inner.ends_with(b"\x1b\x1b\\\x1b\\"));
    out.write_all(b"[1m").unwrap();
    let text = String::from_utf8(inner).unwrap();
    assert_eq!(text.matches("\x1bPtmux;").count(), 2);
    assert!(text.ends_with("\x1b\\\x1b[1m"));
}

/// A graphics sequence with a payload of `n` bytes.
fn sequence(n: usize) -> String {
    format!("\x1b_Gi=1;{}\x1b\\", "A".repeat(n))
}

#[test]
fn the_budget_closes_the_passthrough_and_a_new_one_opens() {
    const CHUNK: usize = 4096;
    let count = BUDGET / CHUNK + 3;
    let all = sequence(CHUNK).repeat(count);
    let text = wrapped(&[all.as_bytes()], true);
    let passthroughs: Vec<&str> = text.split("\x1bPtmux;").skip(1).collect();
    assert_eq!(passthroughs.len(), 2, "one budget's worth, then the rest");
    let first = passthroughs[0].len();
    assert!(first >= BUDGET, "{first}");
    assert!(first < BUDGET + CHUNK + 16, "{first}");
    assert!(passthroughs.iter().all(|one| one.ends_with("\x1b\\")));
    // Nothing but the sequences is lost, none cut by the close.
    assert_eq!(text.matches("\x1b\x1b_G").count(), count);
    assert_eq!(text.matches("\x1b\x1b\\").count(), count);
}

#[test]
fn a_sequence_split_over_writes_keeps_the_passthrough_open() {
    let two = "\x1b_Ga=d\x1b\\\x1b_Gi=2;B\x1b\\";
    // Cut inside the second sequence: nothing closes the passthrough
    // at the end of the first write, as a sequence is open there, and
    // the second sequence is wrapped whole, in the same passthrough.
    let (head, tail) = two.as_bytes().split_at(two.len() - 4);
    let text = wrapped(&[head, tail], true);
    let expected = "\x1bPtmux;\x1b\x1b_Ga=d\x1b\x1b\\\
                        \x1b\x1b_Gi=2;B\x1b\x1b\\\x1b\\";
    assert_eq!(text, expected);
    // A sequence open at the end of a write stays open: nothing
    // closes the passthrough until the sequence ends.
    let mut inner = Vec::new();
    let mut out = Wrapping::new(&mut inner, true);
    out.write_all(head).unwrap();
    assert!(!out.inner.ends_with(b"\x1b\\"));
    out.write_all(tail).unwrap();
    assert!(inner.ends_with(b"\x1b\x1b\\\x1b\\"));
}

#[test]
fn other_escape_sequences_pass_untouched() {
    let other = "\x1b[1m\x1b]8;;u\x1b\\x\x1b\x1b_X\x1b\\";
    assert_eq!(wrapped(&[other.as_bytes()], true), other);
    let bytes: Vec<&[u8]> = other.as_bytes().chunks(1).collect();
    assert_eq!(wrapped(&bytes, true), other);
}

#[test]
fn a_flush_does_not_release_a_held_esc() {
    let mut inner = Vec::new();
    let mut out = Wrapping::new(&mut inner, true);
    out.write_all(b"x\x1b").unwrap();
    out.flush().unwrap();
    assert_eq!(out.inner.as_slice(), b"x");
    out.write_all(b"[1m").unwrap();
    out.flush().unwrap();
    assert_eq!(inner, b"x\x1b[1m");
}

#[test]
fn without_wrapping_nothing_changes() {
    assert_eq!(wrapped(&[GRAPHICS.as_bytes()], false), GRAPHICS);
}
