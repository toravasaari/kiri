//! What kiri knows of particular terminals by their name, in one table beside
//! the capabilities.
//!
//! The capabilities (`caps.rs`) are what a terminal answered or what the
//! environment tells. A fact here is something kiri knows about terminals by
//! their name, from a measurement or from their source, that no query tells:
//! each fact has the terminals it holds for and one sentence on why. A
//! terminal the table does not name gets the general path, so a terminal that
//! is not listed behaves as before the fact existed.
//!
//! The name is the terminal's answer to the version query, such as
//! `kitty(0.47.1)`, `ghostty 1.1.3-1.fc43` or `XTerm(407)`.
//!
//! The facts are: Unicode placeholders by version, a placeholder's position
//! taken from its left, an animation shown as its first frame, the screen
//! scrolling right, an image under cells shown late, and tmux answering first.
//! Callers ask `holds(fact, name)` with the name from the version answer.

/// A thing kiri knows about some terminals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fact {
    /// The terminal has the graphics protocol's Unicode placeholders.
    UnicodePlaceholders,
    /// A placeholder's missing row, column and high byte may be left out;
    /// the terminal takes them from the placeholder on its left.
    PositionFromTheLeft,
    /// The terminal shows only the first frame of an animated image, so
    /// kiri sends it the frames one after the other.
    FirstFrameOnly,
    /// The terminal scrolls its screen right for a shifted frame.
    ScrollsRight,
    /// The terminal scrolls its screen right while the graphics are sixel.
    ScrollsRightWithSixel,
    /// An image that comes under cells already on the screen shows only
    /// after further output.
    ShowsLateUnderCells,
    /// The terminal answers the device attributes query itself and passes
    /// it on, so the answer of the terminal outside it comes second.
    AnswersBeforeTheTerminalOutside,
}

/// The terminals a fact holds for, as the start of the name they give.
#[derive(Debug, Clone, Copy)]
struct Named {
    prefix: &'static str,
    /// What the version is followed by in the name: the `)` of kitty's.
    suffix: &'static str,
    /// The first release it holds for, as major and minor version. Without
    /// it, every release with the name does, and the suffix is not read.
    since: Option<(u32, u32)>,
}

impl Named {
    const fn any(prefix: &'static str) -> Self {
        Self {
            prefix,
            suffix: "",
            since: None,
        }
    }

    const fn since(prefix: &'static str, suffix: &'static str, since: (u32, u32)) -> Self {
        Self {
            prefix,
            suffix,
            since: Some(since),
        }
    }

    fn names(&self, name_version: &str) -> bool {
        let Some(rest) = name_version.strip_prefix(self.prefix) else {
            return false;
        };
        let Some(since) = self.since else {
            return true;
        };
        let Some(version) = rest.strip_suffix(self.suffix) else {
            return false;
        };
        let mut parts = version.split('.').map(str::parse::<u32>);
        match (parts.next(), parts.next()) {
            (Some(Ok(major)), Some(Ok(minor))) => (major, minor) >= since,
            _ => false,
        }
    }
}

/// The first kitty release with the graphics protocol's Unicode
/// placeholders.
const KITTY_PLACEHOLDERS: Named = Named::since("kitty(", ")", (0, 28));

/// The first Ghostty release with them: 1.0, its first public one. Tested
/// with 1.1.3.
const GHOSTTY_PLACEHOLDERS: Named = Named::since("ghostty ", "", (1, 0));

/// One fact, the terminals it holds for and why.
struct Entry {
    fact: Fact,
    terminals: &'static [Named],
    why: &'static str,
}

const TABLE: &[Entry] = &[
    Entry {
        fact: Fact::UnicodePlaceholders,
        terminals: &[KITTY_PLACEHOLDERS, GHOSTTY_PLACEHOLDERS],
        why: "No query tells whether a terminal has Unicode placeholders and some that \
              answer the graphics query do not, so kitty from 0.28 and Ghostty from 1.0 \
              are known by name; other terminals that have them are not yet.",
    },
    Entry {
        fact: Fact::PositionFromTheLeft,
        // Named again, so that a terminal that comes to have placeholders
        // is not taken to do this too.
        terminals: &[KITTY_PLACEHOLDERS, GHOSTTY_PLACEHOLDERS],
        why: "kitty and Ghostty have taken a placeholder's position from its left \
              neighbour, as the protocol allows, since their first placeholders.",
    },
    Entry {
        fact: Fact::FirstFrameOnly,
        terminals: &[Named::any("ghostty")],
        why: "Ghostty 1.1.3 shows the first frame of an animated image and says nothing \
              about it, where kitty plays the animation itself.",
    },
    Entry {
        fact: Fact::ScrollsRight,
        terminals: &[
            Named::any("ghostty "),
            Named::any("foot("),
            Named::any("XTerm("),
        ],
        why: "Ghostty, foot (by hand) and xterm were measured to scroll right; kitty is \
              not among them, as after a scroll it can show the next row of a page's \
              picture in the status row, and not every time.",
    },
    Entry {
        fact: Fact::ScrollsRightWithSixel,
        terminals: &[Named::any("foot(")],
        why: "foot keeps every sixel image in its grid with no limit on their number and \
              erases only the ones that scroll off, where xterm keeps only 16 sixel \
              graphics when it draws its window again, which kiri is not told of.",
    },
    Entry {
        fact: Fact::ShowsLateUnderCells,
        terminals: &[Named::any("kitty(")],
        why: "kitty (measured in 0.48.2, taken to hold for the versions before it) shows \
              an image that comes under cells already on the screen only once it gets \
              further output, where Ghostty 1.1.3 shows it at once.",
    },
    Entry {
        fact: Fact::AnswersBeforeTheTerminalOutside,
        terminals: &[Named::any("tmux")],
        why: "tmux answers the device attributes query for itself and passes it on, so \
              the terminal outside it answers second.",
    },
];

fn entry(fact: Fact) -> &'static Entry {
    // Every fact has an entry: a test goes through them.
    TABLE
        .iter()
        .find(|entry| entry.fact == fact)
        .expect("a fact without an entry in the table")
}

/// Whether `fact` holds for the terminal that names itself `terminal` in
/// its answer to the version query. `None`, a terminal that named nothing,
/// is not in the table.
pub fn holds(fact: Fact, terminal: Option<&str>) -> bool {
    terminal.is_some_and(|name| entry(fact).terminals.iter().any(|named| named.names(name)))
}

/// Why `fact` holds for the terminals it does, in a sentence.
pub fn why(fact: Fact) -> &'static str {
    entry(fact).why
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::present::answers::interpret;
    use crate::term::testing::{TMUX_3_8, XTERM_407, ghostty_1_1_3, kitty_0_47_1};

    /// The name a recorded set of answers gives.
    fn named(answers: &str) -> String {
        interpret(answers.as_bytes())
            .terminal
            .expect("the answers name the terminal")
    }

    /// The facts that hold for `terminal`, in the order of `Fact`'s variants
    /// as listed in the table.
    fn facts(terminal: Option<&str>) -> Vec<Fact> {
        TABLE
            .iter()
            .map(|entry| entry.fact)
            .filter(|&fact| holds(fact, terminal))
            .collect()
    }

    #[test]
    fn every_fact_has_one_entry_and_a_reason() {
        use Fact::*;
        for fact in [
            UnicodePlaceholders,
            PositionFromTheLeft,
            FirstFrameOnly,
            ScrollsRight,
            ScrollsRightWithSixel,
            ShowsLateUnderCells,
            AnswersBeforeTheTerminalOutside,
        ] {
            assert_eq!(
                TABLE.iter().filter(|e| e.fact == fact).count(),
                1,
                "{fact:?}"
            );
            assert!(why(fact).ends_with('.'), "{fact:?}");
        }
        assert_eq!(TABLE.len(), 7);
    }

    #[test]
    fn what_recorded_answers_of_kitty_hold() {
        let kitty = named(&kitty_0_47_1(true));
        assert_eq!(kitty, "kitty(0.47.1)");
        assert_eq!(
            facts(Some(&kitty)),
            [
                Fact::UnicodePlaceholders,
                Fact::PositionFromTheLeft,
                Fact::ShowsLateUnderCells
            ]
        );
    }

    #[test]
    fn what_recorded_answers_of_ghostty_hold() {
        let ghostty = named(&ghostty_1_1_3(true));
        assert_eq!(ghostty, "ghostty 1.1.3-1.fc43");
        assert_eq!(
            facts(Some(&ghostty)),
            [
                Fact::UnicodePlaceholders,
                Fact::PositionFromTheLeft,
                Fact::FirstFrameOnly,
                Fact::ScrollsRight
            ]
        );
    }

    #[test]
    fn what_recorded_answers_of_xterm_and_tmux_hold() {
        let xterm = named(XTERM_407);
        assert_eq!(xterm, "XTerm(407)");
        assert_eq!(facts(Some(&xterm)), [Fact::ScrollsRight]);
        let tmux = named(TMUX_3_8);
        assert_eq!(tmux, "tmux 3.8");
        assert_eq!(facts(Some(&tmux)), [Fact::AnswersBeforeTheTerminalOutside]);
    }

    #[test]
    fn foot_scrolls_right_with_sixel_too() {
        let foot = "foot(1.16.2)";
        assert_eq!(
            facts(Some(foot)),
            [Fact::ScrollsRight, Fact::ScrollsRightWithSixel]
        );
    }

    #[test]
    fn terminals_not_named_get_the_general_path() {
        for name in [
            None,
            Some(""),
            Some("WezTerm 20240203-110809-5046fc22"),
            Some("kitty"),
            Some("notkitty(0.30.0)"),
            Some("xterm(407)"),
        ] {
            assert_eq!(facts(name), [], "{name:?}");
        }
    }

    #[test]
    fn a_bare_ghostty_is_read_by_the_animation_fact_only() {
        // As the check was before the table: the name's start, no version.
        assert_eq!(facts(Some("ghostty")), [Fact::FirstFrameOnly]);
    }

    #[test]
    fn a_version_decides_the_placeholders() {
        use Fact::UnicodePlaceholders as P;
        for (name, held) in [
            ("kitty(0.28.0)", true),
            ("kitty(0.47.1)", true),
            ("kitty(0.100.0)", true),
            ("kitty(1.0.0)", true),
            ("kitty(0.27.1)", false),
            ("kitty(0.9.0)", false),
            ("kitty(0.28.0", false),
            ("ghostty 1.1.3-1.fc43", true),
            ("ghostty 1.0.0", true),
            ("ghostty 1.2.3", true),
            ("ghostty 0.9.0", false),
            ("ghostty next", false),
            ("ghostty(1.1.3)", false),
            ("foot(1.16.2)", false),
        ] {
            assert_eq!(holds(P, Some(name)), held, "{name}");
            assert_eq!(holds(Fact::PositionFromTheLeft, Some(name)), held, "{name}");
        }
    }
}
