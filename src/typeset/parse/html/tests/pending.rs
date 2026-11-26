//! The rows that the switch to the tree of HTML (roadmap 4.10.2, step 5a)
//! changes by a decision of the plan (P1 to P4), or for a reason that none
//! of them gives: each with its document, its blocks before the switch and
//! its blocks as the tree reads them. A row of the table (`rows.rs`) stays
//! there as it was; its entry here is what it reads as now. Step 5b moves
//! the rows back, one decision at a time, and deletes this table.

/// Why a row reads otherwise after the switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// A Markdown block ends an open `<p>`.
    P4,
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
        name: "a centred p that blank lines split is one centred block",
        source: "<p align=\"center\">\n\nSome *text*.\n\n- an item\n\n</p>\n\nAfter.\n",
        old: r#"
            centred @1=0..49
              p "Some *text*." @3=20..32
              list @5=34..43
                item
                  p "an item" @5=36..43
            p "After." @9=51..57
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "Some *text*." @3=20..32
            list @5=34..43
              item
                p "an item" @5=36..43
            html "</p>\n" @7=45..49
            p "After." @9=51..57
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tags are dim and nothing is centred",
    },
    Pending {
        name: "a split wrapper holds an image and headings of html and markdown",
        source: "<p align=\"center\">\n\n# Title\n\n<img src=\"a.png\" alt=\"A\">\n\n<h2>Sub</h2>\n\n</p>\n",
        old: r#"
            centred @1=0..74
              h1 "Title" @3=20..27
              p "![A](a.png)" @5=29..54
              h2 "Sub" @7=56..68
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            h1 "Title" @3=20..27
            p "![A](a.png)" @5=29..54
            h2 "Sub" @7=56..68
            html "</p>\n" @9=70..74
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tags are dim and nothing is centred",
    },
    Pending {
        name: "a split wrapper in a quote",
        source: "> <p align=\"center\">\n>\n> x\n>\n> </p>\n",
        old: r#"
            quote @1=0..35
              centred @1=2..35
                p "x" @3=25..26
        "#,
        new: r#"
            quote @1=0..35
              html "<p align=\"center\">\n" @1=2..20
              p "x" @3=25..26
              html "</p>\n" @5=31..35
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tags are dim and nothing is centred",
    },
    Pending {
        name: "a wrapper nests in a details",
        source: "<details>\n<summary>S</summary>\n\n<p align=\"center\">\n\nIn.\n\n</p>\n\n</details>\n",
        old: r#"
            details "S" @1=0..73
              centred @4=32..61
                p "In." @6=52..55
        "#,
        new: r#"
            details "S" @1=0..73
              html "<p align=\"center\">\n" @4=32..50
              p "In." @6=52..55
              html "</p>\n" @8=57..61
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tags are dim and nothing is centred",
    },
    Pending {
        name: "a div end tag while a centred p is open stays source",
        source: "<p align=\"center\">\n\nA\n\n</div>\n\nB\n\n</p>\n",
        old: r#"
            centred @1=0..38
              p "A" @3=20..21
              html "</div>\n" @5=23..29
              p "B" @7=31..32
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            html "</div>\n" @5=23..29
            p "B" @7=31..32
            html "</p>\n" @9=34..38
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its end tag is a stray, dim",
    },
    Pending {
        name: "a stray end tag that starts a block is cut off the text after it",
        source: "<p align=\"center\">\n\nA\n\n</div>\nAfter.\n\n</p>\n",
        old: r#"
            centred @1=0..42
              p "A" @3=20..21
              html "</div>" @5~23..36
              p "After." @6~23..36
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            html "</div>" @5~23..36
            p "After." @6~23..36
            html "</p>\n" @8=38..42
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its end tag is a stray, dim",
    },
    Pending {
        name: "a details end tag ends a centred p in the details",
        source: "<details><summary>S</summary>\n\n<p align=\"center\">\n\ncentred text\n\n</details>\n\nafter\n",
        old: r#"
            details "S" @1=0..75
              centred @3=31..63
                p "centred text" @5=51..63
            p "after" @9=77..82
        "#,
        new: r#"
            details "S" @1=0..75
              html "<p align=\"center\">\n" @3=31..49
              p "centred text" @5=51..63
            p "after" @9=77..82
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p in the details: its tag is dim",
    },
    Pending {
        name: "the start of a centred p ends a plain p",
        source: "<p>\n\nplain text\n\n<p align=\"center\">\n\ncentred text\n\n</p>\n\nafter\n",
        old: r#"
            html "<p>\n" @1=0..3
            p "plain text" @3=5..15
            centred @5=17..55
              p "centred text" @7=37..49
            p "after" @11=57..62
        "#,
        new: r#"
            html "<p>\n" @1=0..3
            p "plain text" @3=5..15
            html "<p align=\"center\">\n" @5=17..35
            p "centred text" @7=37..49
            html "</p>\n" @9=51..55
            p "after" @11=57..62
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tags are dim and nothing is centred",
    },
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
    Pending {
        name: "a leading end tag in upper case with a blank, text after it",
        source: "<p align=\"center\">\n\nA\n\n</P >\ntext\n",
        old: r#"
            centred @1=0..28
              p "A" @3=20..21
            p "text" @6~23..33
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            html "</P >" @5~23..33
            p "text" @6~23..33
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its end tag is a stray, dim",
    },
    Pending {
        name: "a centred div start while a centred p is open",
        source: "<p align=\"center\">\n\nA\n\n<div align=\"center\">\n\nB\n\n</div>\n",
        old: r#"
            centred @1=0..21
              p "A" @3=20..21
            centred @5=23..54
              p "B" @7=45..46
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            centred @5=23..54
              p "B" @7=45..46
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tag is dim and nothing is centred",
    },
    Pending {
        name: "a center start while a centred p is open",
        source: "<p align=\"center\">\n\nA\n\n<center>\n\nB\n\n</center>\n",
        old: r#"
            centred @1=0..21
              p "A" @3=20..21
            centred @5=23..45
              p "B" @7=33..34
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            centred @5=23..45
              p "B" @7=33..34
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tag is dim and nothing is centred",
    },
    Pending {
        name: "a centred p start while a centred p is open",
        source: "<p align=\"center\">\n\nA\n\n<p align=\"center\">\n\nB\n\n</p>\n",
        old: r#"
            centred @1=0..21
              p "A" @3=20..21
            centred @5=23..50
              p "B" @7=43..44
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            html "<p align=\"center\">\n" @5=23..41
            p "B" @7=43..44
            html "</p>\n" @9=46..50
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tag is dim and nothing is centred",
    },
    Pending {
        name: "a plain p start while a centred p is open",
        source: "<p align=\"center\">\n\nA\n\n<p>\n\nB\n\n</p>\n",
        old: r#"
            centred @1=0..21
              p "A" @3=20..21
            html "<p>\n" @5=23..26
            p "B" @7=28..29
            html "</p>\n" @9=31..35
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            html "<p>\n" @5=23..26
            p "B" @7=28..29
            html "</p>\n" @9=31..35
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tag is dim and nothing is centred",
    },
    Pending {
        name: "a one-block centred p ends an open centred p",
        source: "<p align=\"center\">\n\nA\n\n<p align=\"center\">B</p>\n\nC\n\n</p>\n\nD\n",
        old: r#"
            centred @1=0..21
              p "A" @3=20..21
            centred @5=23..46
              p "B" @5~23..46
            p "C" @7=48..49
            html "</p>\n" @9=51..55
            p "D" @11=57..58
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            centred @5=23..46
              p "B" @5~23..46
            p "C" @7=48..49
            html "</p>\n" @9=51..55
            p "D" @11=57..58
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tag is dim and nothing is centred",
    },
    Pending {
        name: "a one-block centred div ends an open centred p",
        source: "<p align=\"center\">\n\nA\n\n<div align=\"center\">B</div>\n\nC\n\n</p>\n",
        old: r#"
            centred @1=0..21
              p "A" @3=20..21
            centred @5=23..50
              p "B" @5~23..50
            p "C" @7=52..53
            html "</p>\n" @9=55..59
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            centred @5=23..50
              p "B" @5~23..50
            p "C" @7=52..53
            html "</p>\n" @9=55..59
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tag is dim and nothing is centred",
    },
    Pending {
        name: "a one-block center ends an open centred p",
        source: "<p align=\"center\">\n\nA\n\n<center>B</center>\n\nC\n\n</p>\n",
        old: r#"
            centred @1=0..21
              p "A" @3=20..21
            centred @5=23..41
              p "B" @5~23..41
            p "C" @7=43..44
            html "</p>\n" @9=46..50
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            centred @5=23..41
              p "B" @5~23..41
            p "C" @7=43..44
            html "</p>\n" @9=46..50
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tag is dim and nothing is centred",
    },
    Pending {
        name: "a centred heading in an open centred p ends nothing",
        source: "<p align=\"center\">\n\nA\n\n<h1 align=\"center\">T</h1>\n\nC\n\n</p>\n",
        old: r#"
            centred @1=0..57
              p "A" @3=20..21
              centred @5=23..48
                h1 "T" @5~23..48
              p "C" @7=50..51
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            centred @5=23..48
              h1 "T" @5~23..48
            p "C" @7=50..51
            html "</p>\n" @9=53..57
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tag is dim and nothing is centred",
    },
    Pending {
        name: "an image in an open centred p ends nothing",
        source: "<p align=\"center\">\n\nA\n\n<img src=\"a.png\">\n\nC\n\n</p>\n",
        old: r#"
            centred @1=0..49
              p "A" @3=20..21
              p "![](a.png)" @5=23..40
              p "C" @7=42..43
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            p "![](a.png)" @5=23..40
            p "C" @7=42..43
            html "</p>\n" @9=45..49
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tag is dim and nothing is centred",
    },
    Pending {
        name: "a details start while a centred p is open",
        source: "<p align=\"center\">\n\nA\n\n<details>\n\nB\n\n</details>\n",
        old: r#"
            centred @1=0..21
              p "A" @3=20..21
            details "Details" @5=23..47
              p "B" @7=34..35
        "#,
        new: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            details "Details" @5=23..47
              p "B" @7=34..35
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tag is dim and nothing is centred",
    },
    Pending {
        name: "a div end while a centred p is open in it",
        source: "<div align=\"center\">\n\n<p align=\"center\">\n\nA\n\n</div>\n",
        old: r#"
            centred @1=0..51
              centred @3=22..43
                p "A" @5=42..43
        "#,
        new: r#"
            centred @1=0..51
              html "<p align=\"center\">\n" @3=22..40
              p "A" @5=42..43
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tag is dim and nothing is centred",
    },
    Pending {
        name: "a center end while a centred p is open in it",
        source: "<center>\n\n<p align=\"center\">\n\nA\n\n</center>\n",
        old: r#"
            centred @1=0..42
              centred @3=10..31
                p "A" @5=30..31
        "#,
        new: r#"
            centred @1=0..42
              html "<p align=\"center\">\n" @3=10..28
              p "A" @5=30..31
        "#,
        decision: Decision::P4,
        why: "a Markdown block ends the centred p: its tag is dim and nothing is centred",
    },
    // Pinned by tests of `parse/tests.rs` before the switch.
];
