//! The rows that the switch to the tree of HTML (roadmap 4.10.2, step 5a)
//! changes by a decision of the plan (P1 to P4), or for a reason that none
//! of them gives: each with its document, its blocks before the switch and
//! its blocks as the tree reads them. A row of the table (`rows.rs`) stays
//! there as it was; its entry here is what it reads as now. Step 5b moves
//! the rows back, one decision at a time, and deletes this table.

/// Why a row reads otherwise after the switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// A whole element with nothing to draw is transparent; a plain `<p>` is
    /// a paragraph, and an empty one draws nothing.
    P1,
    /// An element is read when its tags pair and what it holds is allowed in
    /// it; else its tags are dim and what it holds is read on its own.
    P2,
    /// A `<details>` never closed is broken, as any other element.
    P3,
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
        name: "a div without align is no wrapper",
        source: "<div>Text</div>\n",
        old: r#"
            html "<div>Text</div>\n" @1=0..15
        "#,
        new: r#"
            p "Text" @1=0..15
        "#,
        decision: Decision::P1,
        why: "a plain div is transparent: its text is a paragraph",
    },
    Pending {
        name: "a p without align is no wrapper",
        source: "<p>Text</p>\n",
        old: r#"
            html "<p>Text</p>\n" @1=0..11
        "#,
        new: r#"
            p "Text" @1=0..11
        "#,
        decision: Decision::P1,
        why: "a plain p is a paragraph",
    },
    Pending {
        name: "a div aligned right is no wrapper",
        source: "<div align=\"right\">Text</div>\n",
        old: r#"
            html "<div align=\"right\">Text</div>\n" @1=0..29
        "#,
        new: r#"
            p "Text" @1=0..29
        "#,
        decision: Decision::P1,
        why: "a div aligned right draws nothing of its own: transparent",
    },
    Pending {
        name: "a p aligned left is no wrapper",
        source: "<p align=\"left\">Text</p>\n",
        old: r#"
            html "<p align=\"left\">Text</p>\n" @1=0..24
        "#,
        new: r#"
            p "Text" @1=0..24
        "#,
        decision: Decision::P1,
        why: "a p aligned left is a plain p: a paragraph",
    },
    Pending {
        name: "a div centred by its style is no wrapper",
        source: "<div style=\"text-align:center\">Text</div>\n",
        old: r#"
            html "<div style=\"text-align:center\">Text</div>\n" @1=0..41
        "#,
        new: r#"
            p "Text" @1=0..41
        "#,
        decision: Decision::P1,
        why: "a div centred by its style draws nothing of its own: transparent",
    },
    Pending {
        name: "a one-block wrapper of a table stays source",
        source: "<div align=\"center\">\n<table><tr><td>x</td></tr></table>\n</div>\n",
        old: r#"
            html "<div align=\"center\">\n<table><tr><td>x</td></tr></table>\n</div>\n" @1=0..62
        "#,
        new: r#"
            centred @1=0..62
              html "<table><tr><td>x</td></tr></table>" @2~0..62
        "#,
        decision: Decision::P2,
        why: "a centred div holds a table, which it may: centred, the table source for now",
    },
    Pending {
        name: "a one-block wrapper of a list stays source",
        source: "<div align=\"center\">\n<ul><li>x</li></ul>\n</div>\n",
        old: r#"
            html "<div align=\"center\">\n<ul><li>x</li></ul>\n</div>\n" @1=0..47
        "#,
        new: r#"
            centred @1=0..47
              html "<ul><li>x</li></ul>" @2~0..47
        "#,
        decision: Decision::P2,
        why: "a centred div holds a list, which it may: centred, the list source for now",
    },
    Pending {
        name: "a one-block wrapper of a span stays source",
        source: "<div align=\"center\">\n<span>x</span>\n</div>\n",
        old: r#"
            html "<div align=\"center\">\n<span>x</span>\n</div>\n" @1=0..42
        "#,
        new: r#"
            centred @1=0..42
              p "<span>x</span>" @2~0..42
        "#,
        decision: Decision::P2,
        why: "a centred div holds text with an unknown span, which it may: centred, the span tags dim",
    },
    Pending {
        name: "a one-block wrapper of a heading and a quote stays source",
        source: "<div align=\"center\">\n<h1>Name</h1>\n<blockquote>q</blockquote>\n</div>\n",
        old: r#"
            html "<div align=\"center\">\n<h1>Name</h1>\n<blockquote>q</blockquote>\n</div>\n" @1=0..68
        "#,
        new: r#"
            centred @1=0..68
              h1 "Name" @2~0..68
              html "<blockquote>q</blockquote>" @3~0..68
        "#,
        decision: Decision::P2,
        why: "a centred div holds a heading and a quote, which it may: centred, the quote source for now",
    },
    Pending {
        name: "a one-block wrapper of a div stays source",
        source: "<div align=\"center\">\n<div>inner</div>\n</div>\n",
        old: r#"
            html "<div align=\"center\">\n<div>inner</div>\n</div>\n" @1=0..44
        "#,
        new: r#"
            centred @1=0..44
              p "inner" @2~0..44
        "#,
        decision: Decision::P1,
        why: "the plain div in a centred div is transparent",
    },
    Pending {
        name: "a one-block wrapper of a heading with an unknown tag stays source",
        source: "<center><h1>a <foo>b</foo></h1></center>\n",
        old: r#"
            html "<center><h1>a <foo>b</foo></h1></center>\n" @1=0..40
        "#,
        new: r#"
            centred @1=0..40
              h1 "a <foo>b</foo>" @1~0..40
        "#,
        decision: Decision::P2,
        why: "a heading may hold an unknown tag: read, the tag dim",
    },
    Pending {
        name: "a centred heading with a span stays source",
        source: "<h1 align=\"center\">a <span>b</span></h1>\n",
        old: r#"
            h1 "a <span>b</span>" @1=0..40
        "#,
        new: r#"
            centred @1=0..40
              h1 "a <span>b</span>" @1~0..40
        "#,
        decision: Decision::P2,
        why: "a centred heading may hold an unknown span: read and centred",
    },
    Pending {
        name: "an empty one-block wrapper stays source",
        source: "<div align=\"center\"></div>\n",
        old: r#"
            html "<div align=\"center\"></div>\n" @1=0..26
        "#,
        new: r#"
        "#,
        decision: Decision::P1,
        why: "an empty centred div draws nothing",
    },
    Pending {
        name: "a one-block wrapper of a blank line stays source",
        source: "<div align=\"center\">\n</div>\n",
        old: r#"
            html "<div align=\"center\">\n</div>\n" @1=0..27
        "#,
        new: r#"
        "#,
        decision: Decision::P1,
        why: "a centred div of a blank line draws nothing",
    },
    Pending {
        name: "a p in a one-block centred p stays source",
        source: "<p align=\"center\"><p>x</p></p>\n",
        old: r#"
            html "<p align=\"center\"><p>x</p></p>\n" @1=0..30
        "#,
        new: r#"
            html "<p align=\"center\">" @1~0..30
            p "x" @1~0..30
            html "</p>\n" @1~0..30
        "#,
        decision: Decision::P1,
        why: "the inner plain p is a paragraph; the outer p, ended at once, and the stray end tag are dim",
    },
    Pending {
        name: "a heading in a one-block centred p stays source",
        source: "<p align=\"center\"><h1>x</h1></p>\n",
        old: r#"
            html "<p align=\"center\"><h1>x</h1></p>\n" @1=0..32
        "#,
        new: r#"
            html "<p align=\"center\">" @1~0..32
            h1 "x" @1~0..32
            html "</p>\n" @1~0..32
        "#,
        decision: Decision::P2,
        why: "the heading ends the centred p at once: its tag and the stray end tag are dim, the heading read",
    },
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
        name: "the end tag of a plain div ends it and its text follows it",
        source: "<div align=\"center\">\n\n<div>\n\nA\n\n</div>\nafter\n\nB\n\n</div>\n",
        old: r#"
            centred @1=0..55
              html "<div>\n" @3=22..27
              p "A" @5=29..30
              html "</div>" @7~32..44
              p "after" @8~32..44
              p "B" @10=46..47
        "#,
        new: r#"
            centred @1=0..55
              p "A" @5=29..30
              p "after" @8~32..44
              p "B" @10=46..47
        "#,
        decision: Decision::P1,
        why: "the plain div is transparent: its tags are gone",
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
        name: "a centring start tag with what kiri does not read in its block stays source",
        source: "<div align=\"center\">\n<table><tr><td>x</td></tr></table>\n\nText\n\n</div>\n",
        old: r#"
            html "<div align=\"center\">\n<table><tr><td>x</td></tr></table>\n" @1=0..55
            p "Text" @4=57..61
            html "</div>\n" @6=63..69
        "#,
        new: r#"
            centred @1=0..69
              html "<table><tr><td>x</td></tr></table>\n" @2~0..55
              p "Text" @4=57..61
        "#,
        decision: Decision::P2,
        why: "a centred div may hold a table (source for now): the wrapper is read",
    },
    Pending {
        name: "a div without align does not start a split wrapper",
        source: "<div>\n\nA\n\n</div>\n",
        old: r#"
            html "<div>\n" @1=0..5
            p "A" @3=7..8
            html "</div>\n" @5=10..16
        "#,
        new: r#"
            p "A" @3=7..8
        "#,
        decision: Decision::P1,
        why: "a plain div is transparent: its tags are gone",
    },
    Pending {
        name: "a plain div inside a wrapper is closed by its own end tag",
        source: "<div align=\"center\">\n\n# Title\n\n<div>\n\ninner text\n\n</div>\n\nafter inner\n\n</div>\n",
        old: r#"
            centred @1=0..77
              h1 "Title" @3=22..29
              html "<div>\n" @5=31..36
              p "inner text" @7=38..48
              html "</div>\n" @9=50..56
              p "after inner" @11=58..69
        "#,
        new: r#"
            centred @1=0..77
              h1 "Title" @3=22..29
              p "inner text" @7=38..48
              p "after inner" @11=58..69
        "#,
        decision: Decision::P1,
        why: "the plain div in a wrapper is transparent",
    },
    Pending {
        name: "a plain div ends first so the wrapper round it is never closed",
        source: "<div align=\"center\">\n\n<div>\n\nA\n\n</div>\n",
        old: r#"
            html "<div align=\"center\">\n" @1=0..20
            html "<div>\n" @3=22..27
            p "A" @5=29..30
            html "</div>\n" @7=32..38
        "#,
        new: r#"
            html "<div align=\"center\">\n" @1=0..20
            p "A" @5=29..30
        "#,
        decision: Decision::P1,
        why: "the inner plain div is transparent; the outer one, without an end tag, is dim",
    },
    Pending {
        name: "a plain div and the wrapper round it are closed by their own end tags",
        source: "<div align=\"center\">\n\n<div>\n\nA\n\n</div>\n\n</div>\n\n",
        old: r#"
            centred @1=0..46
              html "<div>\n" @3=22..27
              p "A" @5=29..30
              html "</div>\n" @7=32..38
        "#,
        new: r#"
            centred @1=0..46
              p "A" @5=29..30
        "#,
        decision: Decision::P1,
        why: "the plain div in a wrapper is transparent",
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
        name: "a plain div whose start block holds text is closed by its own end tag",
        source: "<div align=\"center\">\n\n<div class=\"x\">hello\n\ntext\n\n</div>\n\nafter\n\n</div>\n",
        old: r#"
            centred @1=0..71
              html "<div class=\"x\">hello\n" @3=22..42
              p "text" @5=44..48
              html "</div>\n" @7=50..56
              p "after" @9=58..63
        "#,
        new: r#"
            centred @1=0..71
              p "hello" @3~22..42
              p "text" @5=44..48
              p "after" @9=58..63
        "#,
        decision: Decision::P1,
        why: "the plain div is transparent: its text is a paragraph",
    },
    Pending {
        name: "a plain p whose start block holds text is closed by its own end tag",
        source: "<div align=\"center\">\n\n<p>hello\n\ntext\n\n</p>\n\nafter\n\n</div>\n",
        old: r#"
            centred @1=0..57
              html "<p>hello\n" @3=22..30
              p "text" @5=32..36
              html "</p>\n" @7=38..42
              p "after" @9=44..49
        "#,
        new: r#"
            centred @1=0..57
              p "hello" @3=22..30
              p "text" @5=32..36
              html "</p>\n" @7=38..42
              p "after" @9=44..49
        "#,
        decision: Decision::P1,
        why: "the plain p is a paragraph; the Markdown block ends it, and its end tag is a stray, dim",
    },
    Pending {
        name: "two start tags in one block are two elements to close",
        source: "<div align=\"center\">\n\n<div><div>hello\n\n</div>\n\n</div>\n\nafter\n\n</div>\n",
        old: r#"
            centred @1=0..68
              html "<div><div>hello\n" @3=22..37
              html "</div>\n" @5=39..45
              html "</div>\n" @7=47..53
              p "after" @9=55..60
        "#,
        new: r#"
            centred @1=0..68
              p "hello" @3~22..37
              p "after" @9=55..60
        "#,
        decision: Decision::P1,
        why: "the plain divs are transparent: their text is a paragraph",
    },
    Pending {
        name: "a start tag its block ends is balanced, and a div end ends a plain p",
        source: "<div align=\"center\">\n\n<div>a</div>\n\nA\n\n<p>open\n\n</div>\n",
        old: r#"
            centred @1=0..54
              html "<div>a</div>\n" @3=22..34
              p "A" @5=36..37
              html "<p>open\n" @7=39..46
        "#,
        new: r#"
            centred @1=0..54
              p "a" @3=22..34
              p "A" @5=36..37
              p "open" @7=39..46
        "#,
        decision: Decision::P1,
        why: "the plain div and p are read: their text is paragraphs",
    },
    Pending {
        name: "a plain div with text never closed leaves everything as source",
        source: "<div align=\"center\">\n\n<div class=\"x\">hello\n\ntext\n\n</div>\n",
        old: r#"
            html "<div align=\"center\">\n" @1=0..20
            html "<div class=\"x\">hello\n" @3=22..42
            p "text" @5=44..48
            html "</div>\n" @7=50..56
        "#,
        new: r#"
            html "<div align=\"center\">\n" @1=0..20
            p "hello" @3~22..42
            p "text" @5=44..48
        "#,
        decision: Decision::P1,
        why: "the plain div is transparent; the centred div without an end tag is dim",
    },
    Pending {
        name: "three end tags end three elements",
        source: "<div align=\"center\">\n\n<div>\n\n<p>\n\nA\n\n</p></div></div>\n\nafter\n",
        old: r#"
            centred @1=0..53
              html "<div>\n" @3=22..27
              html "<p>\n" @5=29..32
              p "A" @7=34..35
              html "</p>" @9~37..53
              html "</div>" @9~37..53
            p "after" @11=55..60
        "#,
        new: r#"
            centred @1=0..53
              html "<p>\n" @5=29..32
              p "A" @7=34..35
              html "</p>" @9~37..53
            p "after" @11=55..60
        "#,
        decision: Decision::P1,
        why: "the plain div is transparent; the p, ended by a Markdown block, and the stray end tag are dim",
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
        name: "the html that shares a block with details tags stays source when kiri does not read its tags",
        source: "<details>\n<summary>S</summary>\n<p>\nIn a paragraph.\n\n</p>\n</details>\n\nAfter.\n",
        old: r#"
            details "S" @1=0..67
              html "<p>\nIn a paragraph.\n" @1~0..50
              html "</p>\n" @6~52..67
            p "After." @9=69..75
        "#,
        new: r#"
            details "S" @1=0..67
              p "In a paragraph." @4~0..50
            p "After." @9=69..75
        "#,
        decision: Decision::P1,
        why: "the plain p in the details body is a paragraph",
    },
    Pending {
        name: "a plain p before a details end in its block stays source, as a block of its own does",
        source: "<details>\n<summary>S</summary>\n\nIn.\n\n<p>More.</p>\n</details>\n\nAfter.\n",
        old: r#"
            details "S" @1=0..60
              p "In." @4=32..35
              html "<p>More.</p>\n" @6~37..60
            p "After." @9=62..68
        "#,
        new: r#"
            details "S" @1=0..60
              p "In." @4=32..35
              p "More." @6~37..60
            p "After." @9=62..68
        "#,
        decision: Decision::P1,
        why: "the plain p in the details body is a paragraph",
    },
    Pending {
        name: "html after a one-block details",
        source: "<details>x</details><p>y</p>\n",
        old: r#"
            details "Details" @1=0..28
              p "x" @1~0..28
            html "<p>y</p>\n" @1~0..28
        "#,
        new: r#"
            details "Details" @1=0..20
              p "x" @1~0..28
            p "y" @1~0..28
        "#,
        decision: Decision::P1,
        why: "the plain p after the details is a paragraph",
    },
    Pending {
        name: "a details never closed runs to the end of its container",
        source: "> <details>\n> <summary>S</summary>\n> More text.\n>\n> In.\n\nOut.\n",
        old: r#"
            quote @1=0..55
              details "S" @1=2..55
                p "More text." @1~2..47
                p "In." @5=52..55
            p "Out." @7=57..61
        "#,
        new: r#"
            quote @1=0..55
              html "<details>" @1~2..47
              html "<summary>S</summary>" @2~2..47
              p "More text." @3~2..47
              p "In." @5=52..55
            p "Out." @7=57..61
        "#,
        decision: Decision::P3,
        why: "a details never closed is broken: its tag is dim and the blocks after it are read as they are",
    },
    Pending {
        name: "a details end without a start is cut off the html after it",
        source: "</details>\n<p>y</p>\n",
        old: r#"
            html "</details>" @1~0..19
            html "<p>y</p>\n" @2~0..19
        "#,
        new: r#"
            html "</details>" @1~0..19
            p "y" @2~0..19
        "#,
        decision: Decision::P1,
        why: "the plain p after the stray end tag is a paragraph",
    },
    Pending {
        name: "a block that does not start with details is source",
        source: "<p>\n<details>\n\nText.\n",
        old: r#"
            html "<p>\n<details>\n" @1=0..13
            p "Text." @4=15..20
        "#,
        new: r#"
            html "<p>" @1~0..13
            html "<details>\n" @2~0..13
            p "Text." @4=15..20
        "#,
        decision: Decision::P3,
        why: "a details never closed is broken: its tag is dim; the p, ended at once, is dim too",
    },
    Pending {
        name: "a details never closed runs to the end of its last block",
        source: "<details>\n<summary>S</summary>\n\nfirst\n\nsecond\n\n",
        old: r#"
            details "S" @1=0..45
              p "first" @4=32..37
              p "second" @6=39..45
        "#,
        new: r#"
            html "<details>" @1~0..30
            html "<summary>S</summary>\n" @2~0..30
            p "first" @4=32..37
            p "second" @6=39..45
        "#,
        decision: Decision::P3,
        why: "a details never closed is broken: its tag is dim and the blocks after it are read as they are",
    },
    Pending {
        name: "an empty details never closed runs to the end of its html block",
        source: "<details>\n<summary>S</summary>\n",
        old: r#"
            details "S" @1=0..30
        "#,
        new: r#"
            html "<details>" @1~0..30
            html "<summary>S</summary>\n" @2~0..30
        "#,
        decision: Decision::P3,
        why: "a details never closed is broken: its tag is dim",
    },
    Pending {
        name: "a summary without an end tag keeps the block from being a details",
        source: "<details>\n<summary>S\n\nText.\n\n</details>\n",
        old: r#"
            html "<details>\n<summary>S\n" @1=0..20
            p "Text." @4=22..27
            html "</details>\n" @6=29..39
        "#,
        new: r#"
            details "Details" @1=0..39
              html "<summary>" @2~0..20
              p "S" @2~0..20
              p "Text." @4=22..27
        "#,
        decision: Decision::P2,
        why: "a summary never closed is broken: its tag is dim, its text read on its own, and the details read",
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
        name: "an image in a tag that does not end in a details body stays source",
        source: "<details><summary>s</summary>\n<div><img src=\"a.png\" alt=\"IMGONE\">\n</details>\n\nafter\n",
        old: r#"
            details "s" @1=0..76
              html "<div><img src=\"a.png\" alt=\"IMGONE\">\n" @1~0..76
            p "after" @5=78..83
        "#,
        new: r#"
            details "s" @1=0..76
              html "<div>" @2~0..76
              p "![IMGONE](a.png)" @2~0..76
            p "after" @5=78..83
        "#,
        decision: Decision::P2,
        why: "a div never closed is broken: its tag is dim and the image read on its own",
    },
    Pending {
        name: "a details body of text and tags kiri does not read stays source",
        source: "<details><summary>s</summary><span>x</span> hello <b>bold</b></details>\n",
        old: r#"
            details "s" @1=0..71
              html "<span>x</span> hello <b>bold</b>" @1~0..71
        "#,
        new: r#"
            details "s" @1=0..71
              p "<span>x</span> hello **bold**" @1~0..71
        "#,
        decision: Decision::P2,
        why: "a details body may hold text with an unknown span: read, the span tags dim",
    },
    Pending {
        name: "a details not first in its block is no details",
        source: "<p>x</p><details>\n\nIn.\n\n</details>\n",
        old: r#"
            html "<p>x</p><details>\n" @1=0..17
            p "In." @3=19..22
            html "</details>\n" @5=24..34
        "#,
        new: r#"
            p "x" @1~0..17
            details "Details" @1=8..34
              p "In." @3=19..22
        "#,
        decision: Decision::P1,
        why: "the plain p is a paragraph (and the details, not first in its block, is read: K5)",
    },
    Pending {
        name: "an image in an a that does not end stays source",
        source: "<a href=\"u\">\n<img src=\"x.png\">\n",
        old: r#"
            html "<a href=\"u\">\n<img src=\"x.png\">\n" @1=0..30
        "#,
        new: r#"
            p "<a href=\"u\"> ![](x.png)" @1=0..30
        "#,
        decision: Decision::P2,
        why: "an a never closed is broken: its tag is dim and the image read on its own",
    },
    Pending {
        name: "an end tag after text in its block does nothing",
        source: "<div align=\"center\">\n\nA\n\n<span>\nx</div>\n\nB\n\n</div>\n",
        old: r#"
            centred @1=0..50
              p "A" @3=22..23
              html "<span>\nx</div>\n" @5=25..39
              p "B" @8=41..42
        "#,
        new: r#"
            centred @1=0..39
              p "A" @3=22..23
              p "<span> x" @5~25..39
            p "B" @8=41..42
            html "</div>\n" @10=44..50
        "#,
        decision: Decision::P2,
        why: "the text with an unknown span after the end tag is read as a paragraph (and the end tag ends the wrapper: K4)",
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
    Pending {
        name: "parse::tests::keeps_other_html_as_source: <p>Text <img src=a.png></p>",
        source: "<p>Text <img src=a.png></p>\n",
        old: r#"
            html "<p>Text <img src=a.png></p>\n" @1=0..27
        "#,
        new: r#"
            p "Text ![](a.png)" @1=0..27
        "#,
        decision: Decision::P1,
        why: "a plain p is a paragraph",
    },
    Pending {
        name: "parse::tests::keeps_other_html_around_a_heading_as_source: <h2>T",
        source: "<h2>T\n",
        old: r#"
            html "<h2>T\n" @1=0..5
        "#,
        new: r#"
            html "<h2>" @1~0..5
            p "T" @1~0..5
        "#,
        decision: Decision::P2,
        why: "a heading never closed is broken: its tag is dim and its text read on its own",
    },
    Pending {
        name: "parse::tests::keeps_other_html_around_a_heading_as_source: <h2>T</h3>",
        source: "<h2>T</h3>\n",
        old: r#"
            html "<h2>T</h3>\n" @1=0..10
        "#,
        new: r#"
            html "<h2>" @1~0..10
            p "T" @1~0..10
            html "</h3>\n" @1~0..10
        "#,
        decision: Decision::P2,
        why: "a heading never closed is broken: its tag and the stray end tag are dim",
    },
    Pending {
        name: "parse::tests::keeps_other_html_around_a_heading_as_source: <h2></h2>",
        source: "<h2></h2>\n",
        old: r#"
            html "<h2></h2>\n" @1=0..9
        "#,
        new: r#"
        "#,
        decision: Decision::P1,
        why: "an empty heading draws nothing",
    },
    Pending {
        name: "parse::tests::keeps_other_html_around_a_heading_as_source: <h2>Title",
        source: "<h2>Title\n<h2>Other</h2>\n",
        old: r#"
            html "<h2>Title\n<h2>Other</h2>\n" @1=0..24
        "#,
        new: r#"
            html "<h2>" @1~0..24
            p "Title" @1~0..24
            h2 "Other" @2~0..24
        "#,
        decision: Decision::P2,
        why: "the outer heading is never closed: its tag is dim, the inner one read",
    },
    Pending {
        name: "parse::tests::keeps_other_html_around_a_heading_as_source: <h1><h2>a</h2></h1>",
        source: "<h1><h2>a</h2></h1>\n",
        old: r#"
            html "<h1><h2>a</h2></h1>\n" @1=0..19
        "#,
        new: r#"
            html "<h1>" @1~0..19
            h2 "a" @1~0..19
            html "</h1>\n" @1~0..19
        "#,
        decision: Decision::P2,
        why: "a heading may not hold a heading: its tags are dim, the inner one read",
    },
    Pending {
        name: "parse::tests::keeps_other_html_around_a_heading_as_source: <h2>a</h2>",
        source: "<h2>a</h2>\n<div>b</div>\n",
        old: r#"
            html "<h2>a</h2>\n<div>b</div>\n" @1=0..23
        "#,
        new: r#"
            h2 "a" @1~0..23
            p "b" @2~0..23
        "#,
        decision: Decision::P1,
        why: "the plain div after the heading is transparent (and the heading is read: K5)",
    },
    Pending {
        name: "parse::tests::keeps_other_html_around_a_heading_as_source: <div><h2>a</h2></div>",
        source: "<div><h2>a</h2></div>\n",
        old: r#"
            html "<div><h2>a</h2></div>\n" @1=0..21
        "#,
        new: r#"
            h2 "a" @1~0..21
        "#,
        decision: Decision::P1,
        why: "the plain div round the heading is transparent",
    },
    Pending {
        name: "parse::tests::keeps_other_html_around_a_pre_as_source: <pre>x",
        source: "<pre>x\n",
        old: r#"
            html "<pre>x\n" @1=0..6
        "#,
        new: r#"
            html "<pre>" @1~0..6
            p "x" @1~0..6
        "#,
        decision: Decision::P2,
        why: "a pre never closed is broken: its tag is dim and its text read on its own",
    },
    Pending {
        name: "parse::tests::reads_a_heading_and_a_pre_in_the_html_of_a_details_block: <details><summary>S</summary><p>a <pre>x</pre></p></details>",
        source: "<details><summary>S</summary><p>a <pre>x</pre></p></details>\n",
        old: r#"
            details "S" @1=0..60
              html "<p>a <pre>x</pre></p>" @1~0..60
        "#,
        new: r#"
            details "S" @1=0..60
              p "a" @1~0..60
              code "x\n" @1~0..60
              html "</p>" @1~0..60
        "#,
        decision: Decision::P1,
        why: "the plain p is a paragraph that the pre ends; its end tag is a stray, dim",
    },
];
