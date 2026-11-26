//! The rows that the switch to the tree of HTML (roadmap 4.10.2, step 5a)
//! changes by a decision of the plan (P1 to P4), or for a reason that none
//! of them gives: each with its document, its blocks before the switch and
//! its blocks as the tree reads them. A row of the table (`rows.rs`) stays
//! there as it was; its entry here is what it reads as now. Step 5b moves
//! the rows back, one decision at a time, and deletes this table.

/// Why a row reads otherwise after the switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// None of the decisions: a question for the maintainer.
    Unexplained,
}

/// A document whose blocks a decision changes.
pub struct Pending {
    /// The name of its row in the table, or of the test it was pinned by.
    pub name: &'static str,
    pub source: &'static str,
    /// Its blocks before the switch, as `outline` writes them.
    pub old: &'static str,
    /// Its blocks as the tree reads them.
    pub new: &'static str,
    pub decision: Decision,
    /// What changes, in a few words.
    pub why: &'static str,
}

/// The entry for the row named `name`.
pub fn get(name: &str) -> Option<&'static Pending> {
    PENDING_5B.iter().find(|pending| pending.name == name)
}

pub const PENDING_5B: &[Pending] = &[
    Pending {
        name: "a summary with more after it in its block is no summary",
        source: "<details>\n\n<summary>S</summary>more\n\nText.\n\n</details>\n",
        old: r#"
            details "Details" @1=0..54
              html "<summary>S</summary>more\n" @3=11..35
              p "Text." @5=37..42
        "#,
        new: r#"
            details "S" @1=0..54
              p "more" @3~11..35
              p "Text." @5=37..42
        "#,
        decision: Decision::Unexplained,
        why: "K6: a summary counts wherever it is in the details, as in a browser, not only first in its block",
    },
    // Pinned by tests of `parse/tests.rs` before the switch.
];
