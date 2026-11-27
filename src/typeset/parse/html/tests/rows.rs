//! The table of the reading of HTML: documents and the blocks `parse` reads from
//! them, as `outline` writes them.

use super::table::Row;

pub const ROWS: &[Row] = &[
    // One HTML block: the whole wrapper, with no blank line in it.
    Row {
        name: "a one-block div of a heading, a paragraph and links is one centred block",
        source: "<div align=\"center\">\n  <h1>Name</h1>\n  <p>A <b>short</b> line.</p>\n  <a href=\"https://x/docs\">Docs</a> &middot;\n  <a href=\"https://x/issues\">Issues</a>\n</div>\n",
        blocks: r#"
            centred @1=0..158
              h1 "Name" @2~0..158
              p "A **short** line." @3~0..158
              p "[Docs](https://x/docs) · [Issues](https://x/issues)" @4~0..158
        "#,
    },
    Row {
        name: "a one-block p holds one paragraph of an image, a break and text",
        source: "<p align=\"center\">\n  <img src=\"a.png\" alt=\"A\"><br>\n  Name\n</p>\n",
        blocks: r#"
            centred @1=0..62
              p "![A](a.png)<br>Name" @2~0..62
        "#,
    },
    Row {
        name: "center needs no align",
        source: "<center>Text</center>\n",
        blocks: r#"
            centred @1=0..21
              p "Text" @1~0..21
        "#,
    },
    Row {
        name: "center in upper case",
        source: "<CENTER>Text</center>\n",
        blocks: r#"
            centred @1=0..21
              p "Text" @1~0..21
        "#,
    },
    Row {
        name: "align without quotes",
        source: "<div align=center>Text</div>\n",
        blocks: r#"
            centred @1=0..28
              p "Text" @1~0..28
        "#,
    },
    Row {
        name: "align in upper case",
        source: "<div ALIGN=\"CENTER\">Text</div>\n",
        blocks: r#"
            centred @1=0..30
              p "Text" @1~0..30
        "#,
    },
    Row {
        name: "align in single quotes and mixed case",
        source: "<div align='Center'>Text</div>\n",
        blocks: r#"
            centred @1=0..30
              p "Text" @1~0..30
        "#,
    },
    Row {
        name: "align with blanks round it, after another attribute",
        source: "<div class=\"x\" align=\" center \">Text</div>\n",
        blocks: r#"
            centred @1=0..42
              p "Text" @1~0..42
        "#,
    },
    // P1, read by the tree: a plain div is transparent: its text is a
    // paragraph.
    Row {
        name: "a div without align is transparent: its text is a paragraph",
        source: "<div>Text</div>\n",
        blocks: r#"
            p "Text" @1=0..15
        "#,
    },
    // P1, read by the tree: a plain p is a paragraph.
    Row {
        name: "a p without align is a paragraph",
        source: "<p>Text</p>\n",
        blocks: r#"
            p "Text" @1=0..11
        "#,
    },
    // P1, read by the tree: a div aligned right draws nothing of its own:
    // transparent.
    Row {
        name: "a div aligned right is transparent",
        source: "<div align=\"right\">Text</div>\n",
        blocks: r#"
            p "Text" @1=0..29
        "#,
    },
    // P1, read by the tree: a p aligned left is a plain p: a paragraph.
    Row {
        name: "a p aligned left is a paragraph",
        source: "<p align=\"left\">Text</p>\n",
        blocks: r#"
            p "Text" @1=0..24
        "#,
    },
    // P1, read by the tree: a div centred by its style draws nothing of its
    // own: transparent.
    Row {
        name: "a div centred by its style is transparent",
        source: "<div style=\"text-align:center\">Text</div>\n",
        blocks: r#"
            p "Text" @1=0..41
        "#,
    },
    Row {
        name: "a heading with align centre alone is centred",
        source: "<h1 align=\"center\">Name</h1>\n",
        blocks: r#"
            centred @1=0..28
              h1 "Name" @1~0..28
        "#,
    },
    Row {
        name: "a heading with align centre in upper case and inline tags",
        source: "<H3 ALIGN=center>The <i>name</i></H3>\n",
        blocks: r#"
            centred @1=0..37
              h3 "The *name*" @1~0..37
        "#,
    },
    Row {
        name: "a heading without align is the heading it was",
        source: "<h1>Name</h1>\n",
        blocks: r#"
            h1 "Name" @1=0..13
        "#,
    },
    Row {
        name: "a heading aligned left is the heading it was",
        source: "<h1 align=\"left\">Name</h1>\n",
        blocks: r#"
            h1 "Name" @1=0..26
        "#,
    },
    Row {
        name: "an image in a centred p is a centred picture",
        source: "<p align=\"center\"><img src=\"a.png\" alt=\"A\"></p>\n",
        blocks: r#"
            centred @1=0..47
              p "![A](a.png)" @1~0..47
        "#,
    },
    Row {
        name: "the image of a picture in a centred p is shown",
        source: "<p align=\"center\"><picture><source srcset=\"b.png\"><img src=\"a.png\"></picture></p>\n",
        blocks: r#"
            centred @1=0..81
              p "![](a.png)" @1~0..81
        "#,
    },
    Row {
        name: "a linked image in a centred p is a link round the image",
        source: "<p align=center>\n  <a href=\"https://x\"><img src=a.png></a>\n</p>\n",
        blocks: r#"
            centred @1=0..63
              p "[![](a.png)](https://x)" @2~0..63
        "#,
    },
    // P2, read by the tree: a centred div holds a table, which it may: centred,
    // the table source for now.
    Row {
        name: "a table in a one-block wrapper is source in the centred block",
        source: "<div align=\"center\">\n<table><tr><td>x</td></tr></table>\n</div>\n",
        blocks: r#"
            centred @1=0..62
              html "<table><tr><td>x</td></tr></table>" @2~0..62
        "#,
    },
    // P2, read by the tree: a centred div holds a list, which it may: centred,
    // the list source for now.
    Row {
        name: "a list in a one-block wrapper is source in the centred block",
        source: "<div align=\"center\">\n<ul><li>x</li></ul>\n</div>\n",
        blocks: r#"
            centred @1=0..47
              html "<ul><li>x</li></ul>" @2~0..47
        "#,
    },
    // A centred div holds text with a span, which it may: centred, and the
    // span transparent (before the row of `<span>`, its tags were dim).
    Row {
        name: "a span in a one-block wrapper is transparent, its text centred",
        source: "<div align=\"center\">\n<span>x</span>\n</div>\n",
        blocks: r#"
            centred @1=0..42
              p "x" @2~0..42
        "#,
    },
    // P2, read by the tree: a centred div holds a heading and a quote, which it
    // may: centred, the quote source for now.
    Row {
        name: "a heading and a quote in a one-block wrapper: the quote is source in the centred block",
        source: "<div align=\"center\">\n<h1>Name</h1>\n<blockquote>q</blockquote>\n</div>\n",
        blocks: r#"
            centred @1=0..68
              h1 "Name" @2~0..68
              html "<blockquote>q</blockquote>" @3~0..68
        "#,
    },
    // P1, read by the tree: the plain div in a centred div is transparent.
    Row {
        name: "a plain div in a one-block wrapper is transparent",
        source: "<div align=\"center\">\n<div>inner</div>\n</div>\n",
        blocks: r#"
            centred @1=0..44
              p "inner" @2~0..44
        "#,
    },
    // P2, read by the tree: a heading may hold an unknown tag: read, the tag
    // dim.
    Row {
        name: "a heading with an unknown tag in a one-block wrapper is read, the tag dim",
        source: "<center><h1>a <foo>b</foo></h1></center>\n",
        blocks: r#"
            centred @1=0..40
              h1 "a <foo>b</foo>" @1~0..40
        "#,
    },
    // A centred heading holds a span: read and centred, the span transparent
    // (before the row of `<span>`, its tags were dim).
    Row {
        name: "a centred heading with a span is read and centred",
        source: "<h1 align=\"center\">a <span>b</span></h1>\n",
        blocks: r#"
            centred @1=0..40
              h1 "a b" @1~0..40
        "#,
    },
    // P1, read by the tree: an empty centred div draws nothing.
    Row {
        name: "an empty one-block wrapper draws nothing",
        source: "<div align=\"center\"></div>\n",
        blocks: r#"
        "#,
    },
    // P1, read by the tree: a centred div of a blank line draws nothing.
    Row {
        name: "a one-block wrapper of a blank line draws nothing",
        source: "<div align=\"center\">\n</div>\n",
        blocks: r#"
        "#,
    },
    // P1, read by the tree: the inner plain p is a paragraph; the outer p,
    // ended at once, and the stray end tag are dim.
    Row {
        name: "a p in a one-block centred p is a paragraph, the centred p dim",
        source: "<p align=\"center\"><p>x</p></p>\n",
        blocks: r#"
            html "<p align=\"center\">" @1~0..30
            p "x" @1~0..30
            html "</p>\n" @1~0..30
        "#,
    },
    // P2, read by the tree: the heading ends the centred p at once: its tag and
    // the stray end tag are dim, the heading read.
    Row {
        name: "a heading in a one-block centred p ends it: the p tags are dim",
        source: "<p align=\"center\"><h1>x</h1></p>\n",
        blocks: r#"
            html "<p align=\"center\">" @1~0..32
            h1 "x" @1~0..32
            html "</p>\n" @1~0..32
        "#,
    },
    // K5, read by the tree: the HTML after the end tag of a wrapper in its block is read after it.
    Row {
        name: "text after the end tag in a one-block wrapper keeps it source",
        source: "<div align=\"center\">\nText\n</div>\nAfter the tag.\n",
        blocks: r#"
            centred @1=0..32
              p "Text" @2~0..32
            p "After the tag." @4~0..47
        "#,
    },
    Row {
        name: "markdown inside a one-block wrapper is not read",
        source: "<p align=\"center\">[x](y) and *z*</p>\n",
        blocks: r#"
            centred @1=0..36
              p "[x](y) and *z*" @1~0..36
        "#,
    },
    Row {
        name: "the places of a one-block wrapper and of the blocks in it",
        source: "Before.\n\n<div align=\"center\">\n<h1>Name</h1>\n<p>Tag</p>\n</div>\n\nAfter.\n",
        blocks: r#"
            p "Before." @1=0..7
            centred @3=9..61
              h1 "Name" @4~9..61
              p "Tag" @5~9..61
            p "After." @8=63..69
        "#,
    },
    Row {
        name: "two centred p elements in one block are two centred blocks",
        source: "<p align=\"center\">a</p>\n<p align=\"center\">b</p>\n",
        blocks: r#"
            centred @1=0..23
              p "a" @1~0..23
            centred @2=24..47
              p "b" @2~24..47
        "#,
    },
    Row {
        name: "a centred image and a centred row of links in one block are two centred blocks",
        source: "<p align=\"center\"><img src=\"a.png\" alt=\"logo\"></p>\n<p align=\"center\"><a href=\"https://x\">Docs</a></p>\n",
        blocks: r#"
            centred @1=0..50
              p "![logo](a.png)" @1~0..50
            centred @2=51..101
              p "[Docs](https://x)" @2~51..101
        "#,
    },
    Row {
        name: "a centred heading and a centred p in one block are two centred blocks",
        source: "<h1 align=\"center\">Name</h1>\n<p align=\"center\">Tag</p>\n",
        blocks: r#"
            centred @1=0..28
              h1 "Name" @1~0..28
            centred @2=29..54
              p "Tag" @2~29..54
        "#,
    },
    // K5, read by the tree: the HTML after the end tag of a wrapper in its block is read after it.
    Row {
        name: "two centred elements with text between them in one block stay source",
        source: "<p align=\"center\">a</p>\ntext\n<p align=\"center\">b</p>\n",
        blocks: r#"
            centred @1=0..23
              p "a" @1~0..23
            p "text" @2~0..52
            centred @3=29..52
              p "b" @3~29..52
        "#,
    },
    Row {
        name: "two centred elements in the body of a one-block details are two centred blocks",
        source: "<details><summary>S</summary><p align=\"center\">a</p><p align=\"center\">b</p></details>\n",
        blocks: r#"
            details "S" @1=0..85
              centred @1=29..52
                p "a" @1~29..52
              centred @1=52..75
                p "b" @1~52..75
        "#,
    },
    Row {
        name: "two centred elements after an end tag in its block are two centred blocks",
        source: "<center>\n\nA\n\n</center>\n<p align=\"center\">B</p>\n<p align=\"center\">C</p>\n",
        blocks: r#"
            centred @1=0..22
              p "A" @3=10..11
            centred @6=23..46
              p "B" @6~23..46
            centred @7=47..70
              p "C" @7~47..70
        "#,
    },
    Row {
        name: "a one-block wrapper in a quote",
        source: "> <p align=\"center\">x</p>\n",
        blocks: r#"
            quote @1=0..25
              centred @1=2..25
                p "x" @1~2..25
        "#,
    },
    Row {
        name: "a one-block wrapper in a list item",
        source: "- <p align=\"center\">x</p>\n",
        blocks: r#"
            list @1=0..25
              item
                centred @1=2..25
                  p "x" @1~2..25
        "#,
    },
    Row {
        name: "a one-block wrapper in a details",
        source: "<details>\n<summary>S</summary>\n\n<p align=\"center\">x</p>\n\n</details>\n",
        blocks: r#"
            details "S" @1=0..67
              centred @4=32..55
                p "x" @4~32..55
        "#,
    },
    Row {
        name: "a line in a centred paragraph has no white space at the break",
        source: "<p align=\"center\">\n  Line one <br>\n\t Line two  \n</p>\n",
        blocks: r#"
            centred @1=0..52
              p "Line one<br>Line two" @2~0..52
        "#,
    },
    Row {
        name: "a heading with an id in a wrapper keeps its id",
        source: "<div align=\"center\">\n<h2 id=\"x\">Name</h2>\n</div>\n",
        blocks: r#"
            centred @1=0..48
              h2 "Name" #x @2~0..48
        "#,
    },
    Row {
        name: "an empty id is none",
        source: "<div align=\"center\">\n<h2 id=\"\">Name</h2>\n</div>\n",
        blocks: r#"
            centred @1=0..47
              h2 "Name" @2~0..47
        "#,
    },
    // Blank lines inside the wrapper: a block for each of its tags and the
    // blocks between them.
    // P4, read by the tree: a Markdown block ends the centred p: its tags are
    // dim and nothing is centred.
    Row {
        name: "a centred p that blank lines split is not centred: a Markdown block ends it",
        source: "<p align=\"center\">\n\nSome *text*.\n\n- an item\n\n</p>\n\nAfter.\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "Some *text*." @3=20..32
            list @5=34..43
              item
                p "an item" @5=36..43
            html "</p>\n" @7=45..49
            p "After." @9=51..57
        "#,
    },
    Row {
        name: "a centred div that blank lines split is one centred block",
        source: "<div align=\"center\">\n\nSome *text*.\n\n- an item\n\n</div>\n\nAfter.\n",
        blocks: r#"
            centred @1=0..53
              p "Some *text*." @3=22..34
              list @5=36..45
                item
                  p "an item" @5=38..45
            p "After." @9=55..61
        "#,
    },
    Row {
        name: "a center that blank lines split is one centred block",
        source: "<center>\n\nSome *text*.\n\n- an item\n\n</center>\n\nAfter.\n",
        blocks: r#"
            centred @1=0..44
              p "Some *text*." @3=10..22
              list @5=24..33
                item
                  p "an item" @5=26..33
            p "After." @9=46..52
        "#,
    },
    Row {
        name: "a split wrapper in upper case",
        source: "<DIV ALIGN=center>\n\nSome *text*.\n\n- an item\n\n</DIV>\n\nAfter.\n",
        blocks: r#"
            centred @1=0..51
              p "Some *text*." @3=20..32
              list @5=34..43
                item
                  p "an item" @5=36..43
            p "After." @9=53..59
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tags are
    // dim and nothing is centred.
    Row {
        name: "a centred p that Markdown and HTML headings cut is dim tags and the blocks",
        source: "<p align=\"center\">\n\n# Title\n\n<img src=\"a.png\" alt=\"A\">\n\n<h2>Sub</h2>\n\n</p>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            h1 "Title" @3=20..27
            p "![A](a.png)" @5=29..54
            h2 "Sub" @7=56..68
            html "</p>\n" @9=70..74
        "#,
    },
    Row {
        name: "the places of a split wrapper run from its start tag to its end tag",
        source: "Before.\n\n<div align=\"center\">\n\nIn *one*.\n\nIn two.\n\n</div>\n\nAfter.\n",
        blocks: r#"
            p "Before." @1=0..7
            centred @3=9..57
              p "In *one*." @5=31..40
              p "In two." @7=42..49
            p "After." @11=59..65
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tags are
    // dim and nothing is centred.
    Row {
        name: "a centred p that blank lines split, in a quote, is not centred",
        source: "> <p align=\"center\">\n>\n> x\n>\n> </p>\n",
        blocks: r#"
            quote @1=0..35
              html "<p align=\"center\">\n" @1=2..20
              p "x" @3=25..26
              html "</p>\n" @5=31..35
        "#,
    },
    Row {
        name: "a split wrapper in a list item",
        source: "- <center>\n\n  x\n\n  </center>\n",
        blocks: r#"
            list @1=0..28
              item
                centred @1=2..28
                  p "x" @3=14..15
        "#,
    },
    Row {
        name: "wrappers nest",
        source: "<div align=\"center\">\n\nOut.\n\n<center>\n\nIn.\n\n</center>\n\nBack.\n\n</div>\n",
        blocks: r#"
            centred @1=0..67
              p "Out." @3=22..26
              centred @5=28..52
                p "In." @7=38..41
              p "Back." @11=54..59
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tags are
    // dim and nothing is centred.
    Row {
        name: "a centred p that blank lines split, in a details, is not centred",
        source: "<details>\n<summary>S</summary>\n\n<p align=\"center\">\n\nIn.\n\n</p>\n\n</details>\n",
        blocks: r#"
            details "S" @1=0..73
              html "<p align=\"center\">\n" @4=32..50
              p "In." @6=52..55
              html "</p>\n" @8=57..61
        "#,
    },
    Row {
        name: "a details nests in a wrapper",
        source: "<div align=\"center\">\n\n<details>\n<summary>S</summary>\n\nIn.\n\n</details>\n\n</div>\n",
        blocks: r#"
            centred @1=0..77
              details "S" @3=22..69
                p "In." @6=54..57
        "#,
    },
    Row {
        name: "a one-block wrapper in a split one",
        source: "<div align=\"center\">\n\n<h1 align=\"center\">Name</h1>\n\nText.\n\n</div>\n",
        blocks: r#"
            centred @1=0..65
              centred @3=22..50
                h1 "Name" @3~22..50
              p "Text." @5=52..57
        "#,
    },
    Row {
        name: "an unclosed wrapper stays source with the blocks after it",
        source: "<p align=\"center\">\n\nText.\n\n# Head\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "Text." @3=20..25
            h1 "Head" @5=27..33
        "#,
    },
    Row {
        name: "an unclosed wrapper in a quote does not reach out of it",
        source: "> <p align=\"center\">\n>\n> Text.\n\nOut.\n",
        blocks: r#"
            quote @1=0..30
              html "<p align=\"center\">\n" @1=2..20
              p "Text." @3=25..30
            p "Out." @5=32..36
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its end tag is
    // a stray, dim.
    Row {
        name: "a div end tag after Markdown in a centred p is a stray, dim",
        source: "<p align=\"center\">\n\nA\n\n</div>\n\nB\n\n</p>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            html "</div>\n" @5=23..29
            p "B" @7=31..32
            html "</p>\n" @9=34..38
        "#,
    },
    Row {
        name: "a wrapper closed by another end tag only is unclosed",
        source: "<p align=\"center\">\n\nA\n\n</div>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            html "</div>\n" @5=23..29
        "#,
    },
    Row {
        name: "an end tag with no wrapper open stays source",
        source: "A\n\n</p>\n\nB\n",
        blocks: r#"
            p "A" @1=0..1
            html "</p>\n" @3=3..7
            p "B" @5=9..10
        "#,
    },
    Row {
        name: "a details end tag does not close a wrapper",
        source: "<div align=\"center\">\n\nA\n\n</details>\n\nB\n\n</div>\n",
        blocks: r#"
            centred @1=0..46
              p "A" @3=22..23
              html "</details>\n" @5=25..35
              p "B" @7=37..38
        "#,
    },
    Row {
        name: "a p end tag does not close a details",
        source: "<details>\n<summary>S</summary>\n\nA\n\n</p>\n\n</details>\n",
        blocks: r#"
            details "S" @1=0..51
              p "A" @4=32..33
              html "</p>\n" @6=35..39
        "#,
    },
    Row {
        name: "text after the end tag in its block follows the wrapper",
        source: "<div align=\"center\">\n\nA\n\n</div>\nAfter the tag.\n\nMore.\n",
        blocks: r#"
            centred @1=0..31
              p "A" @3=22..23
            p "After the tag." @6~25..46
            p "More." @8=48..53
        "#,
    },
    Row {
        name: "html after the end tag stays a block of html",
        source: "<div align=\"center\">\n\nA\n\n</div>\n<table><tr><td>x</td></tr></table>\n",
        blocks: r#"
            centred @1=0..31
              p "A" @3=22..23
            html "<table><tr><td>x</td></tr></table>\n" @6~25..66
        "#,
    },
    // P1, read by the tree: the plain div is transparent: its tags are gone.
    Row {
        name: "the end tag of a plain div ends it and its text follows it",
        source: "<div align=\"center\">\n\n<div>\n\nA\n\n</div>\nafter\n\nB\n\n</div>\n",
        blocks: r#"
            centred @1=0..55
              p "A" @5=29..30
              p "after" @8~32..44
              p "B" @10=46..47
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its end tag is
    // a stray, dim.
    Row {
        name: "a stray end tag that starts a block is cut off the text after it",
        source: "<p align=\"center\">\n\nA\n\n</div>\nAfter.\n\n</p>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            html "</div>" @5~23..36
            p "After." @6~23..36
            html "</p>\n" @8=38..42
        "#,
    },
    Row {
        name: "a wrapper never closed stays source, and a stray end tag is cut off the text after it",
        source: "<div align=\"center\">\n\nA\n\n</p>\nAfter.\n",
        blocks: r#"
            html "<div align=\"center\">\n" @1=0..20
            p "A" @3=22..23
            html "</p>" @5~25..36
            p "After." @6~25..36
        "#,
    },
    Row {
        name: "the place of a wrapper ends at its end tag and the text after it has its line",
        source: "<div align=\"center\">\n\nA\n\n</div>\nAfter the tag.\n",
        blocks: r#"
            centred @1=0..31
              p "A" @3=22..23
            p "After the tag." @6~25..46
        "#,
    },
    Row {
        name: "a centring start tag with text in its block starts a wrapper of it",
        source: "<div align=\"center\">\nText.\n\nMore.\n\n</div>\n",
        blocks: r#"
            centred @1=0..41
              p "Text." @2~0..26
              p "More." @4=28..33
        "#,
    },
    Row {
        name: "a centring start tag with an image in its block starts a wrapper of it",
        source: "<div align=\"center\">\n<img src=\"logo.png\" alt=\"logo\">\n\nSome *text*.\n\n</div>\n",
        blocks: r#"
            centred @1=0..74
              p "![logo](logo.png)" @2~0..52
              p "Some *text*." @4=54..66
        "#,
    },
    Row {
        name: "a center with an image, a break and a link in its block starts a wrapper of them",
        source: "<center>\n<img src=\"logo.png\">\n<br>\n<a href=\"https://x\">Docs</a>\n\nText\n\n</center>\n",
        blocks: r#"
            centred @1=0..80
              p "![](logo.png)<br>[Docs](https://x)" @2~0..63
              p "Text" @6=65..69
        "#,
    },
    Row {
        name: "a centring start tag with a heading in its block starts a wrapper of it",
        source: "<div align=\"center\">\n<h1>Name</h1>\n\nText\n\n</div>\n",
        blocks: r#"
            centred @1=0..48
              h1 "Name" @2~0..34
              p "Text" @4=36..40
        "#,
    },
    Row {
        name: "a wrapper started with an image in its block and never closed stays source",
        source: "<div align=\"center\">\n<img src=\"logo.png\">\n\nText\n",
        blocks: r#"
            html "<div align=\"center\">" @1~0..41
            p "![](logo.png)" @2~0..41
            p "Text" @4=43..47
        "#,
    },
    // P2, read by the tree: a centred div may hold a table (source for now):
    // the wrapper is read.
    Row {
        name: "a centring start tag with a table in its block is read, the table source",
        source: "<div align=\"center\">\n<table><tr><td>x</td></tr></table>\n\nText\n\n</div>\n",
        blocks: r#"
            centred @1=0..69
              html "<table><tr><td>x</td></tr></table>\n" @2~0..55
              p "Text" @4=57..61
        "#,
    },
    // P1, read by the tree: a plain div is transparent: its tags are gone.
    Row {
        name: "a div without align does not start a split wrapper",
        source: "<div>\n\nA\n\n</div>\n",
        blocks: r#"
            p "A" @3=7..8
        "#,
    },
    // P1, read by the tree: the plain div in a wrapper is transparent.
    Row {
        name: "a plain div inside a wrapper is closed by its own end tag",
        source: "<div align=\"center\">\n\n# Title\n\n<div>\n\ninner text\n\n</div>\n\nafter inner\n\n</div>\n",
        blocks: r#"
            centred @1=0..77
              h1 "Title" @3=22..29
              p "inner text" @7=38..48
              p "after inner" @11=58..69
        "#,
    },
    Row {
        name: "a plain p start ends an empty centred p, which stays source",
        source: "<p align=\"center\">\n\n<p>\n\nA\n\n</p>\n\nB\n\n</p>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            html "<p>\n" @3=20..23
            p "A" @5=25..26
            html "</p>\n" @7=28..32
            p "B" @9=34..35
            html "</p>\n" @11=37..41
        "#,
    },
    // P1, read by the tree: the inner plain div is transparent; the outer one,
    // without an end tag, is dim.
    Row {
        name: "a plain div ends first so the wrapper round it is never closed",
        source: "<div align=\"center\">\n\n<div>\n\nA\n\n</div>\n",
        blocks: r#"
            html "<div align=\"center\">\n" @1=0..20
            p "A" @5=29..30
        "#,
    },
    // P1, read by the tree: the plain div in a wrapper is transparent.
    Row {
        name: "a plain div and the wrapper round it are closed by their own end tags",
        source: "<div align=\"center\">\n\n<div>\n\nA\n\n</div>\n\n</div>\n\n",
        blocks: r#"
            centred @1=0..46
              p "A" @5=29..30
        "#,
    },
    Row {
        name: "a plain div never closed",
        source: "<div>\n\nA\n",
        blocks: r#"
            html "<div>\n" @1=0..5
            p "A" @3=7..8
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p in the details:
    // its tag is dim.
    Row {
        name: "a details end tag after Markdown in a centred p in the details",
        source: "<details><summary>S</summary>\n\n<p align=\"center\">\n\ncentred text\n\n</details>\n\nafter\n",
        blocks: r#"
            details "S" @1=0..75
              html "<p align=\"center\">\n" @3=31..49
              p "centred text" @5=51..63
            p "after" @9=77..82
        "#,
    },
    Row {
        name: "a details end tag closes the details round a wrapper in a plain div",
        source: "<details>\n<summary>S</summary>\n\n<div>\n\n<div align=\"center\">\n\nA\n\n</details>\n\nafter\n",
        blocks: r#"
            details "S" @1=0..74
              html "<div>\n" @4=32..37
              html "<div align=\"center\">\n" @6=39..59
              p "A" @8=61..62
            p "after" @12=76..81
        "#,
    },
    // P1, read by the tree: the plain div is transparent: its text is a
    // paragraph.
    Row {
        name: "a plain div whose start block holds text is closed by its own end tag",
        source: "<div align=\"center\">\n\n<div class=\"x\">hello\n\ntext\n\n</div>\n\nafter\n\n</div>\n",
        blocks: r#"
            centred @1=0..71
              p "hello" @3~22..42
              p "text" @5=44..48
              p "after" @9=58..63
        "#,
    },
    // P1, read by the tree: the plain p is a paragraph; the Markdown block ends
    // it, and its end tag is a stray, dim.
    Row {
        name: "a plain p whose start block holds text is closed by its own end tag",
        source: "<div align=\"center\">\n\n<p>hello\n\ntext\n\n</p>\n\nafter\n\n</div>\n",
        blocks: r#"
            centred @1=0..57
              p "hello" @3=22..30
              p "text" @5=32..36
              html "</p>\n" @7=38..42
              p "after" @9=44..49
        "#,
    },
    Row {
        name: "a centring start tag with text beside it in its block starts a wrapper of it",
        source: "<div align=\"center\">\n\n<div align=\"center\">hello\n\ntext\n\n</div>\n\nafter\n\n</div>\n",
        blocks: r#"
            centred @1=0..76
              centred @3=22..61
                p "hello" @3~22..47
                p "text" @5=49..53
              p "after" @9=63..68
        "#,
    },
    // P1, read by the tree: the plain divs are transparent: their text is a
    // paragraph.
    Row {
        name: "two start tags in one block are two elements to close",
        source: "<div align=\"center\">\n\n<div><div>hello\n\n</div>\n\n</div>\n\nafter\n\n</div>\n",
        blocks: r#"
            centred @1=0..68
              p "hello" @3~22..37
              p "after" @9=55..60
        "#,
    },
    // P1, read by the tree: the plain div and p are read: their text is
    // paragraphs.
    Row {
        name: "a start tag its block ends is balanced, and a div end ends a plain p",
        source: "<div align=\"center\">\n\n<div>a</div>\n\nA\n\n<p>open\n\n</div>\n",
        blocks: r#"
            centred @1=0..54
              p "a" @3=22..34
              p "A" @5=36..37
              p "open" @7=39..46
        "#,
    },
    // P1, read by the tree: the plain div is transparent; the centred div
    // without an end tag is dim.
    Row {
        name: "a plain div with text in a wrapper never closed is transparent, the wrapper's tag dim",
        source: "<div align=\"center\">\n\n<div class=\"x\">hello\n\ntext\n\n</div>\n",
        blocks: r#"
            html "<div align=\"center\">\n" @1=0..20
            p "hello" @3~22..42
            p "text" @5=44..48
        "#,
    },
    // An open `<p>` that does not centre ends where a browser ends it.
    Row {
        name: "an end tag run ends a plain p and then the wrapper",
        source: "<div align=\"center\">\n\n<p>\n\ntext\n\n</p></div>\n\nafter\n",
        blocks: r#"
            centred @1=0..43
              html "<p>\n" @3=22..25
              p "text" @5=27..31
              html "</p>" @7~33..43
            p "after" @9=45..50
        "#,
    },
    Row {
        name: "an end tag and the start of the next wrapper in one block",
        source: "<div align=\"center\">\n\nA\n\n</div>\n<div align=\"center\">\n\nB\n\n</div>\n",
        blocks: r#"
            centred @1=0..31
              p "A" @3=22..23
            centred @6=32..63
              p "B" @8=54..55
        "#,
    },
    // P1, read by the tree: the plain div is transparent; the p, ended by a
    // Markdown block, and the stray end tag are dim.
    Row {
        name: "three end tags end three elements",
        source: "<div align=\"center\">\n\n<div>\n\n<p>\n\nA\n\n</p></div></div>\n\nafter\n",
        blocks: r#"
            centred @1=0..53
              html "<p>\n" @5=29..32
              p "A" @7=34..35
              html "</p>" @9~37..53
            p "after" @11=55..60
        "#,
    },
    Row {
        name: "an end tag run that ends nothing is cut into its tags",
        source: "A\n\n</p></div>\n\nB\n",
        blocks: r#"
            p "A" @1=0..1
            html "</p>" @3~3..13
            html "</div>\n" @3~3..13
            p "B" @5=15..16
        "#,
    },
    Row {
        name: "a stray end tag at the start of a block is cut off, and the end tag after it ends the wrapper",
        source: "<div align=\"center\">\n\nA\n\n</p></div>\n\nafter\n",
        blocks: r#"
            centred @1=0..35
              p "A" @3=22..23
              html "</p>" @5~25..35
            p "after" @7=37..42
        "#,
    },
    Row {
        name: "an unclosed p does not stop the end tag of a center",
        source: "<center>\n\n<p>\n\n# Title\n\n</center>\n\nafter\n",
        blocks: r#"
            centred @1=0..33
              html "<p>\n" @3=10..13
              h1 "Title" @5=15..22
            p "after" @9=35..40
        "#,
    },
    Row {
        name: "an unclosed p does not stop the end tag of a centred div",
        source: "<div align=\"center\">\n\n<p>\n\n# Title\n\n</div>\n\nafter\n",
        blocks: r#"
            centred @1=0..42
              html "<p>\n" @3=22..25
              h1 "Title" @5=27..34
            p "after" @9=44..49
        "#,
    },
    Row {
        name: "an unclosed p does not stop the end tag of a wrapper in a plain div",
        source: "<div>\n\n<div align=\"center\">\n\n<p>\n\n# Title\n\n</div>\n\nafter\n",
        blocks: r#"
            html "<div>\n" @1=0..5
            centred @3=7..49
              html "<p>\n" @5=29..32
              h1 "Title" @7=34..41
            p "after" @11=51..56
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tags are
    // dim and nothing is centred.
    Row {
        name: "the start of a centred p ends a plain p",
        source: "<p>\n\nplain text\n\n<p align=\"center\">\n\ncentred text\n\n</p>\n\nafter\n",
        blocks: r#"
            html "<p>\n" @1=0..3
            p "plain text" @3=5..15
            html "<p align=\"center\">\n" @5=17..35
            p "centred text" @7=37..49
            html "</p>\n" @9=51..55
            p "after" @11=57..62
        "#,
    },
    Row {
        name: "the start of a center ends a plain p",
        source: "<p>\n\nplain text\n\n<center>\n\ncentred text\n\n</center>\n",
        blocks: r#"
            html "<p>\n" @1=0..3
            p "plain text" @3=5..15
            centred @5=17..50
              p "centred text" @7=27..39
        "#,
    },
    Row {
        name: "the start of a centred div ends a plain p",
        source: "<p>\n\nplain text\n\n<div align=\"center\">\n\ncentred text\n\n</div>\n",
        blocks: r#"
            html "<p>\n" @1=0..3
            p "plain text" @3=5..15
            centred @5=17..59
              p "centred text" @7=39..51
        "#,
    },
    Row {
        name: "a wrapper after an end tag in its block starts on its own line",
        source: "<div align=\"center\">\n\nA\n\n</div>\n<div align=\"center\">\n\nB\n\n</div>\n",
        blocks: r#"
            centred @1=0..31
              p "A" @3=22..23
            centred @6=32..63
              p "B" @8=54..55
        "#,
    },
    Row {
        name: "a wrapper after an end tag in a quote starts on its own line",
        source: "> <div align=\"center\">\n>\n> A\n>\n> </div>\n> <div align=\"center\">\n>\n> B\n>\n> </div>\n",
        blocks: r#"
            quote @1=0..79
              centred @1=2..39
                p "A" @3=27..28
              centred @6=42..79
                p "B" @8=67..68
        "#,
    },
    // `<details>`, set apart by blank lines or in one HTML block.
    Row {
        name: "a details that blank lines split",
        source: "<details>\n<summary>Open <kbd>K</kbd><br>me</summary>\n\nSome *text*.\n\n- an item\n\n</details>\n\nAfter.\n",
        blocks: r#"
            details "Open <kbd>K</kbd><br>me" @1=0..89
              p "Some *text*." @4=54..66
              list @6=68..77
                item
                  p "an item" @6=70..77
            p "After." @10=91..97
        "#,
    },
    Row {
        name: "a details in a list item",
        source: "- Item\n  <details>\n  <summary>S</summary>\n\n  In.\n\n  </details>\n",
        blocks: r#"
            list @1=0..62
              item
                p "Item" @1=2..6
                details "S" @2=9..62
                  p "In." @5=45..48
        "#,
    },
    Row {
        name: "a details in a quote",
        source: "> <details>\n> <summary>S</summary>\n>\n> In.\n>\n> </details>\n",
        blocks: r#"
            quote @1=0..57
              details "S" @1=2..57
                p "In." @4=39..42
        "#,
    },
    Row {
        name: "nested details",
        source: "<details>\n<summary>Out</summary>\n\n<details>\n<summary>In</summary>\n\nDeep.\n\n</details>\n\nBack.\n\n</details>\n",
        blocks: r#"
            details "Out" @1=0..103
              details "In" @4=34..84
                p "Deep." @7=67..72
              p "Back." @11=86..91
        "#,
    },
    // K5, read by the tree: a details is placed from its own tags, not from the HTML block it starts in.
    Row {
        name: "nested details that no blank line splits",
        source: "<details>\n<summary>Out</summary>\n<details>\n<summary>In</summary>\n\nDeep.\n\n</details>\n\nBack.\n\n</details>\n\nAfter.\n",
        blocks: r#"
            details "Out" @1=0..102
              details "In" @3=33..83
                p "Deep." @6=66..71
              p "Back." @10=85..90
            p "After." @14=104..110
        "#,
    },
    // P1, read by the tree: the plain p in the details body is a paragraph.
    Row {
        name: "a plain p in a details body is a paragraph",
        source: "<details>\n<summary>S</summary>\n<p>\nIn a paragraph.\n\n</p>\n</details>\n\nAfter.\n",
        blocks: r#"
            details "S" @1=0..67
              p "In a paragraph." @4~0..50
            p "After." @9=69..75
        "#,
    },
    // P1, read by the tree: the plain p in the details body is a paragraph.
    Row {
        name: "a plain p before a details end in its block is a paragraph",
        source: "<details>\n<summary>S</summary>\n\nIn.\n\n<p>More.</p>\n</details>\n\nAfter.\n",
        blocks: r#"
            details "S" @1=0..60
              p "In." @4=32..35
              p "More." @6~37..60
            p "After." @9=62..68
        "#,
    },
    // K5, read by the tree: a details is placed from its own tags, and the text after its end tag has its own line.
    Row {
        name: "a details ends at a block that holds more than its end",
        source: "<details>\n<summary>S</summary>\n\nIn.\n\n</details>\nAfter.\n\nLast.\n",
        blocks: r#"
            details "S" @1=0..47
              p "In." @4=32..35
            p "After." @7~37..54
            p "Last." @9=56..61
        "#,
    },
    // K5, read by the tree: each details is placed from its own tags, not from the HTML blocks it is cut from.
    Row {
        name: "a details end and the next details in one block",
        source: "<details>\n<summary>A</summary>\n\nIn A.\n\n</details>\n<details>\n<summary>B</summary>\n\nIn B.\n\n</details>\n",
        blocks: r#"
            details "A" @1=0..49
              p "In A." @4=32..37
            details "B" @7=50..99
              p "In B." @10=82..87
        "#,
    },
    // P1, read by the tree: the plain p after the details is a paragraph.
    Row {
        name: "html after a one-block details",
        source: "<details>x</details><p>y</p>\n",
        blocks: r#"
            details "Details" @1=0..20
              p "x" @1~0..28
            p "y" @1~0..28
        "#,
    },
    // P3, read by the tree: a details never closed is broken: its tag is dim
    // and the blocks after it are read as they are.
    Row {
        name: "a details never closed in a quote is dim, the blocks after it are read as they are",
        source: "> <details>\n> <summary>S</summary>\n> More text.\n>\n> In.\n\nOut.\n",
        blocks: r#"
            quote @1=0..55
              html "<details>" @1~2..47
              html "<summary>" @2~2..47
              p "S" @2~2..47
              html "</summary>" @2~2..47
              p "More text." @3~2..47
              p "In." @5=52..55
            p "Out." @7=57..61
        "#,
    },
    Row {
        name: "a details in one block holds its text and inline tags as a paragraph",
        source: "<details><summary>One</summary>Hidden &amp; <b>text</b>\non two lines</details>\n",
        blocks: r#"
            details "One" @1=0..78
              p "Hidden & **text** on two lines" @1~0..78
        "#,
    },
    Row {
        name: "a details in one block with only a blank",
        source: "<details><summary>Empty</summary> </details>\n",
        blocks: r#"
            details "Empty" @1=0..44
        "#,
    },
    Row {
        name: "a details in one block with an image",
        source: "<details><summary>Logo</summary><img src=a.png alt=x></details>\n",
        blocks: r#"
            details "Logo" @1=0..63
              p "![x](a.png)" @1~0..63
        "#,
    },
    Row {
        name: "a details without a summary is called details",
        source: "<details>\n\nText.\n\n</details>\n",
        blocks: r#"
            details "Details" @1=0..28
              p "Text." @3=11..16
        "#,
    },
    Row {
        name: "a summary after a blank line is the summary",
        source: "<details>\n\n<summary>Later</summary>\n\nText.\n\n</details>\n",
        blocks: r#"
            details "Later" @1=0..54
              p "Text." @5=37..42
        "#,
    },
    Row {
        name: "a details end without a start stays source",
        source: "Text.\n\n</details>\n",
        blocks: r#"
            p "Text." @1=0..5
            html "</details>\n" @3=7..17
        "#,
    },
    // P1, read by the tree: the plain p after the stray end tag is a paragraph.
    Row {
        name: "a details end without a start is cut off the html after it",
        source: "</details>\n<p>y</p>\n",
        blocks: r#"
            html "</details>" @1~0..19
            p "y" @2~0..19
        "#,
    },
    // P3, read by the tree: a details never closed is broken: its tag is dim;
    // the p, ended at once, is dim too.
    Row {
        name: "a details never closed after a p is dim, and the p ended at once",
        source: "<p>\n<details>\n\nText.\n",
        blocks: r#"
            html "<p>" @1~0..13
            html "<details>\n" @2~0..13
            p "Text." @4=15..20
        "#,
    },
    Row {
        name: "details and the blocks in them know their lines",
        source: "Before\n\n<details>\n<summary>Outer</summary>\n\nIn outer\n\n<details>\n<summary>Inner</summary>\n\nDeep\n\n</details>\n\n</details>\n\nAfter\n",
        blocks: r#"
            p "Before" @1=0..6
            details "Outer" @3=8..118
              p "In outer" @6=44..52
              details "Inner" @8=54..106
                p "Deep" @11=90..94
            p "After" @17=120..125
        "#,
    },
    // K5, read by the tree: the inner details is placed from its own tags.
    Row {
        name: "details in one html block start on the line of that block",
        source: "Before\n\n<details><details>x</details></details>\n",
        blocks: r#"
            p "Before" @1=0..6
            details "Details" @3=8..47
              details "Details" @3=17..37
                p "x" @3~8..47
        "#,
    },
    // K5, read by the tree: a block of a details body has its own line, not the first line of its HTML block.
    Row {
        name: "an image in a details body is read from its html block",
        source: "Before\n\n<details>\n<summary>S</summary>\n<img src=\"a.png\">\n</details>\n",
        blocks: r#"
            p "Before" @1=0..6
            details "S" @3=8..67
              p "![](a.png)" @5~8..67
        "#,
    },
    Row {
        name: "a details runs from its first tag to the end tag that closes it",
        source: "Before\n\n<details>\n<summary>S</summary>\n\nfirst\n\nsecond\n\n</details>\n\nAfter\n",
        blocks: r#"
            p "Before" @1=0..6
            details "S" @3=8..65
              p "first" @6=40..45
              p "second" @8=47..53
            p "After" @12=67..72
        "#,
    },
    // P3, read by the tree: a details never closed is broken: its tag is dim
    // and the blocks after it are read as they are.
    Row {
        name: "a details never closed is dim, its blocks are read as they are",
        source: "<details>\n<summary>S</summary>\n\nfirst\n\nsecond\n\n",
        blocks: r#"
            html "<details>" @1~0..30
            html "<summary>" @2~0..30
            p "S" @2~0..30
            html "</summary>\n" @2~0..30
            p "first" @4=32..37
            p "second" @6=39..45
        "#,
    },
    // P3, read by the tree: a details never closed is broken: its tag is dim.
    Row {
        name: "an empty details never closed is dim tags",
        source: "<details>\n<summary>S</summary>\n",
        blocks: r#"
            html "<details>" @1~0..30
            html "<summary>" @2~0..30
            p "S" @2~0..30
            html "</summary>\n" @2~0..30
        "#,
    },
    Row {
        name: "a details in a details ends with its own end tag",
        source: "<details>\n<summary>A</summary>\n\n<details>\n<summary>B</summary>\n\ninner\n\n</details>\n\nafter inner\n\n</details>\n",
        blocks: r#"
            details "A" @1=0..106
              details "B" @4=32..81
                p "inner" @7=64..69
              p "after inner" @11=83..94
        "#,
    },
    // K5, read by the tree: each details is placed from its own tags, not from the HTML blocks it is cut from.
    Row {
        name: "two details that share an html block overlap in it",
        source: "<details>\n<summary>A</summary>\n\none\n\n</details>\n<details>\n<summary>B</summary>\n\ntwo\n\n</details>\n",
        blocks: r#"
            details "A" @1=0..47
              p "one" @4=32..35
            details "B" @7=48..95
              p "two" @10=80..83
        "#,
    },
    // K5, read by the tree: a block of a details body has its own line, not the first line of its HTML block.
    Row {
        name: "the html of a details body is read as blocks of that html block",
        source: "<details>\n<summary>S</summary>\n<pre>x</pre>\n</details>\n",
        blocks: r#"
            details "S" @1=0..54
              code "x\n" @3~0..54
        "#,
    },
    // K5, read by the tree: a details is placed from its own tags, and the stray end tag has its own line.
    Row {
        name: "a second details end in a block stays source inside the block",
        source: "<details>\n<summary>S</summary>\n</details>\n</details>\n",
        blocks: r#"
            details "S" @1=0..41
            html "</details>\n" @4~0..52
        "#,
    },
    Row {
        name: "a stray details end alone is exact",
        source: "a\n\n</details>\n",
        blocks: r#"
            p "a" @1=0..1
            html "</details>\n" @3=3..13
        "#,
    },
    Row {
        name: "a pre outside a details is the html block whole",
        source: "<pre>x</pre>\n",
        blocks: r#"
            code "x\n" @1=0..12
        "#,
    },
    // Comments, line ends, summaries, stray tags and tags among text.
    Row {
        name: "comments round the tags of a wrapper",
        source: " <DIV Align=CENTER> <!-- c -->\n\nA\n\n<!-- d --> </div>\n",
        blocks: r#"
            centred @1=1..52
              p "A" @3=32..33
        "#,
    },
    Row {
        name: "a comment block in a wrapper",
        source: "<div align=\"center\">\n\n<!-- note -->\n\nA\n\n</div>\n",
        blocks: r#"
            centred @1=0..46
              html "<!-- note -->\n" @3=22..35
              p "A" @5=37..38
        "#,
    },
    Row {
        name: "a wrapper with crlf line ends",
        source: "<div align=\"center\">\r\n\r\nA\r\n\r\n</div>\r\nAfter.\r\n",
        blocks: r#"
            centred @1=0..35
              p "A" @3=24..25
            p "After." @6~29..43
        "#,
    },
    Row {
        name: "a details with crlf line ends",
        source: "<details>\r\n<summary>S</summary>\r\n\r\nIn.\r\n\r\n</details>\r\n",
        blocks: r#"
            details "S" @1=0..52
              p "In." @4=35..38
        "#,
    },
    Row {
        name: "nested details without summaries split by blank lines",
        source: "<details>\n\n<details>\n\nDeep.\n\n</details>\n\n</details>\n",
        blocks: r#"
            details "Details" @1=0..51
              details "Details" @3=11..39
                p "Deep." @5=22..27
        "#,
    },
    Row {
        name: "a summary outside a details has no effect: dim tags, its text a paragraph",
        source: "<summary>S</summary>\n\nText.\n",
        blocks: r#"
            html "<summary>" @1~0..20
            p "S" @1~0..20
            html "</summary>\n" @1~0..20
            p "Text." @3=22..27
        "#,
    },
    Row {
        name: "a second summary has no effect: dim tags, its text a paragraph",
        source: "<details>\n<summary>A</summary>\n\n<summary>B</summary>\n\nText.\n\n</details>\n",
        blocks: r#"
            details "A" @1=0..71
              html "<summary>" @4~32..52
              p "B" @4~32..52
              html "</summary>\n" @4~32..52
              p "Text." @6=54..59
        "#,
    },
    Row {
        name: "a summary after a first one set apart has no effect: dim tags, its text a paragraph",
        source: "<details>\n\n<summary>A</summary>\n\n<summary>B</summary>\n\n</details>\n",
        blocks: r#"
            details "A" @1=0..65
              html "<summary>" @5~33..53
              p "B" @5~33..53
              html "</summary>\n" @5~33..53
        "#,
    },
    // P2, read by the tree: a summary never closed is broken: its tag is dim,
    // its text read on its own, and the details read.
    Row {
        name: "a summary without an end tag is dim and the details is read",
        source: "<details>\n<summary>S\n\nText.\n\n</details>\n",
        blocks: r#"
            details "Details" @1=0..39
              html "<summary>" @2~0..20
              p "S" @2~0..20
              p "Text." @4=22..27
        "#,
    },
    // K6, read by the tree: a summary counts wherever it is in the details, as
    // in a browser, not only first in its block.
    Row {
        name: "a summary with more after it in its block is the summary",
        source: "<details>\n\n<summary>S</summary>more\n\nText.\n\n</details>\n",
        blocks: r#"
            details "S" @1=0..54
              p "more" @3~11..35
              p "Text." @5=37..42
        "#,
    },
    Row {
        name: "a summary with comments, tags and entities",
        source: "<DETAILS open>\n<summary>A  <b>bold</b>\n &amp; more <!-- c --> &mdash; &copy;</summary>\n\nIn.\n\n</DETAILS >\n",
        blocks: r#"
            details "A **bold** & more — ©" @1=0..104
              p "In." @5=88..91
        "#,
    },
    Row {
        name: "an empty summary",
        source: "<details><summary></summary>Text\n\nMore.\n\n</details>\n",
        blocks: r#"
            details "" @1=0..51
              p "Text" @1~0..32
              p "More." @3=34..39
        "#,
    },
    Row {
        name: "a stray summary end tag stays source",
        source: "Text.\n\n</summary>\n",
        blocks: r#"
            p "Text." @1=0..5
            html "</summary>\n" @3=7..17
        "#,
    },
    Row {
        name: "a stray summary end tag in a details stays source",
        source: "<details>\n<summary>S</summary>\n\n</summary>\n\n</details>\n",
        blocks: r#"
            details "S" @1=0..54
              html "</summary>\n" @4=32..42
        "#,
    },
    // P2, read by the tree: a div never closed is broken: its tag is dim and
    // the image read on its own.
    Row {
        name: "an image in a div never closed in a details body is read, the div dim",
        source: "<details><summary>s</summary>\n<div><img src=\"a.png\" alt=\"IMGONE\">\n</details>\n\nafter\n",
        blocks: r#"
            details "s" @1=0..76
              html "<div>" @2~0..76
              p "![IMGONE](a.png)" @2~0..76
            p "after" @5=78..83
        "#,
    },
    // K5, read by the tree: the HTML before a details end tag is not cut as a body part: the stray tag is its own.
    Row {
        name: "a stray end tag before a details end in its block stays source",
        source: "<details>\n<summary>s</summary>\n\ntext\n\n</div>\n</details>\n",
        blocks: r#"
            details "s" @1=0..55
              p "text" @4=32..36
              html "</div>" @6~38..55
        "#,
    },
    // P2, read by the tree: a details body may hold text with an unknown
    // element: read, its tags dim.
    Row {
        name: "a details body of text and an unknown abbr is a paragraph",
        source: "<details><summary>s</summary><abbr>x</abbr> hello <b>bold</b></details>\n",
        blocks: r#"
            details "s" @1=0..71
              p "<abbr>x</abbr> hello **bold**" @1~0..71
        "#,
    },
    // P1, read by the tree: the plain p is a paragraph (and the details, not
    // first in its block, is read: K5).
    Row {
        name: "a details not first in its block is a details after a plain p",
        source: "<p>x</p><details>\n\nIn.\n\n</details>\n",
        blocks: r#"
            p "x" @1~0..17
            details "Details" @1=8..34
              p "In." @3=19..22
        "#,
    },
    // K5, read by the tree: the inner details is placed from its own tags.
    Row {
        name: "a details in a details in one block with text between",
        source: "<details><summary>S</summary><details>In</details>Out</details>\n",
        blocks: r#"
            details "S" @1=0..63
              details "Details" @1=29..50
                p "In" @1~0..63
              p "Out" @1~0..63
        "#,
    },
    // K5, read by the tree: a details is placed from its own tags, and the text after its end tag has its own line.
    Row {
        name: "html before a details end in its block stays source, and text after it follows",
        source: "<details>\n<summary>S</summary>\n\nA\n\n</p>\n</details>\nAfter.\n",
        blocks: r#"
            details "S" @1=0..50
              p "A" @4=32..33
              html "</p>" @6~35..57
            p "After." @8~35..57
        "#,
    },
    Row {
        name: "a script block is opaque, and a tag in its text opens nothing",
        source: "<div align=\"center\">\n\n<script>\nx = \"<div>\";\n</script>\n\nA\n\n</div>\n\nafter\n",
        blocks: r#"
            centred @1=0..64
              html "<script>\nx = \"<div>\";\n</script>\n" @3=22..53
              p "A" @7=55..56
            p "after" @11=66..71
        "#,
    },
    Row {
        name: "a style block is opaque, and a tag in its text opens nothing",
        source: "<div align=\"center\">\n\n<style>\n/* <div> */\n</style>\n\nA\n\n</div>\n",
        blocks: r#"
            centred @1=0..61
              html "<style>\n/* <div> */\n</style>\n" @3=22..50
              p "A" @7=52..53
        "#,
    },
    Row {
        name: "a pre block that is no code block is opaque, and a tag in its text opens nothing",
        source: "<div align=\"center\">\n\n<pre>\nsome <div> code\n</pre>\n\nA\n\n</div>\n",
        blocks: r#"
            centred @1=0..61
              html "<pre>\nsome <div> code\n</pre>\n" @3=22..50
              p "A" @7=52..53
        "#,
    },
    Row {
        name: "a details end in the text of a pre block ends nothing",
        source: "<details><summary>S</summary>\n\n<pre>\n</details>\n</pre>\n\ninner para\n\n</details>\n",
        blocks: r#"
            details "S" @1=0..78
              html "<pre>\n</details>\n</pre>\n" @3=31..54
              p "inner para" @7=56..66
        "#,
    },
    Row {
        name: "a details start cut short is opaque",
        source: "<details\n\nText.\n",
        blocks: r#"
            html "<details\n" @1=0..8
            p "Text." @3=10..15
        "#,
    },
    Row {
        name: "a commented details is source",
        source: "<!-- <details> -->\n",
        blocks: r#"
            html "<!-- <details> -->\n" @1=0..18
        "#,
    },
    Row {
        name: "a tag that starts like details is source",
        source: "<detailsx>\n</detailsx>\n",
        blocks: r#"
            html "<detailsx>\n</detailsx>\n" @1=0..22
        "#,
    },
    // K5, read by the tree: a wrapper after an end tag in its block is placed from its own tags.
    Row {
        name: "a centred p after an end tag in its block is centred",
        source: "<center>\n\nA\n\n</center>\n<p align=\"center\">B</p>\n",
        blocks: r#"
            centred @1=0..22
              p "A" @3=10..11
            centred @6=23..46
              p "B" @6~23..46
        "#,
    },
    // K5, read by the tree: a wrapper after an end tag in its block is placed from its own tags.
    Row {
        name: "a centred heading after a wrapper end tag in its block is centred",
        source: "<div align=\"center\">\n\nA\n\n</div>\n<h2 align=\"center\">T</h2>\n",
        blocks: r#"
            centred @1=0..31
              p "A" @3=22..23
            centred @6=32..57
              h2 "T" @6~32..57
        "#,
    },
    // K5, read by the tree: a wrapper after a details end tag in its block is placed from its own tags.
    Row {
        name: "a centred heading after a details end tag in its block is centred",
        source: "<details>\n<summary>S</summary>\n\nA\n\n</details>\n<h1 align=\"center\">T</h1>\n",
        blocks: r#"
            details "S" @1=0..45
              p "A" @4=32..33
            centred @7=46..71
              h1 "T" @7~46..71
        "#,
    },
    // K5, read by the tree: a wrapper in a details body is placed from its own tags.
    Row {
        name: "a centred p in the body of a one-block details is centred",
        source: "<details><summary>S</summary><p align=\"center\">x</p></details>\n",
        blocks: r#"
            details "S" @1=0..62
              centred @1=29..52
                p "x" @1~29..52
        "#,
    },
    // K4, read by the tree: an end tag after an image in its block ends its element.
    Row {
        name: "an end tag after an image in its block does nothing, and both stay source",
        source: "<div align=\"center\">\n\nA\n\n<img src=\"b.png\" alt=\"logo\">\n</div>\n\nafter\n",
        blocks: r#"
            centred @1=0..60
              p "A" @3=22..23
              p "![logo](b.png)" @5~25..60
            p "after" @8=62..67
        "#,
    },
    Row {
        name: "an end tag before an image in its block ends the wrapper, and the image follows it",
        source: "<div align=\"center\">\n\nA\n\n</div>\n<img src=\"b.png\" alt=\"logo\">\n",
        blocks: r#"
            centred @1=0..31
              p "A" @3=22..23
            p "![logo](b.png)" @6~25..60
        "#,
    },
    Row {
        name: "a centred p with an image in its block and no end tag there ends with the block",
        source: "<p align=\"center\">\n  <img src=\"b.png\" alt=\"logo\">\n\nSome text\n\n</p>\n",
        blocks: r#"
            centred @1=0..49
              p "![logo](b.png)" @2~0..49
            p "Some text" @4=51..60
            html "</p>\n" @6=62..66
        "#,
    },
    Row {
        name: "a stray end tag before an image is cut off, and the image follows it",
        source: "A\n\n</div>\n<img src=\"b.png\" alt=\"logo\">\n",
        blocks: r#"
            p "A" @1=0..1
            html "</div>" @3~3..38
            p "![logo](b.png)" @4~3..38
        "#,
    },
    Row {
        name: "an image in a centred p whose end tag is left out is centred",
        source: "<p align=\"center\"><img src=\"logo.png\" width=\"200\">\n",
        blocks: r#"
            centred @1=0..50
              p "![](logo.png)" @1~0..50
        "#,
    },
    Row {
        name: "text in a centred p whose end tag is left out ends with its block",
        source: "<p align=\"center\">\nA centred line.\n\nAfter.\n",
        blocks: r#"
            centred @1=0..34
              p "A centred line." @2~0..34
            p "After." @4=36..42
        "#,
    },
    Row {
        name: "text in a centred p whose end tag is left out may end in a character of more bytes",
        source: "<p align=\"center\">Caf\u{e9}",
        blocks: r#"
            centred @1=0..23
              p "Café" @1~0..23
        "#,
    },
    Row {
        name: "a centred p ending in a character of more bytes after a closed one",
        source: "<p align=\"center\">a</p><p align=\"center\">\u{20ac}",
        blocks: r#"
            centred @1=0..23
              p "a" @1~0..23
            centred @1=23..44
              p "€" @1~23..44
        "#,
    },
    Row {
        name: "an anchor line before a centred p ending in a character of more bytes",
        source: "<a name=\"x\"></a>\n<p align=\"center\">\u{e9}\n",
        blocks: r#"
            centred @2=17..37
              p "Anchor(\"x\")é" @2~17..37
        "#,
    },
    Row {
        name: "an image in a plain p whose end tag is left out is an image",
        source: "<p><img src=\"a.png\">\n",
        blocks: r#"
            p "![](a.png)" @1=0..20
        "#,
    },
    Row {
        name: "a p that a div end tag ends round an image is an image",
        source: "<div><p><img src=\"a.png\"></div>\n",
        blocks: r#"
            p "![](a.png)" @1=0..31
        "#,
    },
    // P2, read by the tree: an a never closed is broken: its tag is dim and the
    // image read on its own.
    Row {
        name: "an image in an a never closed is read, the a dim",
        source: "<a href=\"u\">\n<img src=\"x.png\">\n",
        blocks: r#"
            p "<a href=\"u\"> ![](x.png)" @1=0..30
        "#,
    },
    Row {
        name: "an image in tags that wrap it in its block is an image",
        source: "<div><a href=\"https://x\"><picture><source srcset=\"b.png\"><img src=\"a.png\"></picture></a></div>\n",
        blocks: r#"
            p "[![](a.png)](https://x)" @1=0..94
        "#,
    },
    // P2, read by the tree: the text with an unknown span after the end tag is
    // read as a paragraph (and the end tag ends the wrapper: K4).
    Row {
        name: "an end tag after text with an unknown span in its block ends the wrapper",
        source: "<div align=\"center\">\n\nA\n\n<span>\nx</div>\n\nB\n\n</div>\n",
        blocks: r#"
            centred @1=0..39
              p "A" @3=22..23
              p "<span> x" @5~25..39
            p "B" @8=41..42
            html "</div>\n" @10=44..50
        "#,
    },
    Row {
        name: "a div end with a center open above the div stays source",
        source: "<div align=\"center\">\n\n<center>\n\nA\n\n</div>\n\n</center>\n",
        blocks: r#"
            html "<div align=\"center\">\n" @1=0..20
            centred @3=22..52
              p "A" @5=32..33
              html "</div>\n" @7=35..41
        "#,
    },
    Row {
        name: "an end tag in upper case with a blank",
        source: "<div align=\"center\">\n\nA\n\n</DIV >\n",
        blocks: r#"
            centred @1=0..32
              p "A" @3=22..23
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its end tag is
    // a stray, dim.
    Row {
        name: "a leading end tag in upper case with a blank, text after it",
        source: "<p align=\"center\">\n\nA\n\n</P >\ntext\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            html "</P >" @5~23..33
            p "text" @6~23..33
        "#,
    },
    Row {
        name: "a center with text in its block starts a wrapper of it",
        source: "<center>hello\n\ntext\n\n</center>\n",
        blocks: r#"
            centred @1=0..30
              p "hello" @1~0..13
              p "text" @3=15..19
        "#,
    },
    Row {
        name: "a center with text in its block in a center is a wrapper that its own end tag ends",
        source: "<center>\n\n<center>x\n\ntext\n\n</center>\n\nmore\n\n</center>\n",
        blocks: r#"
            centred @1=0..53
              centred @3=10..36
                p "x" @3~10..19
                p "text" @5=21..25
              p "more" @9=38..42
        "#,
    },
    Row {
        name: "a one-block centred div ends an empty open centred p, which stays source",
        source: "<p align=\"center\">\n\n<div align=\"center\">x</div>\n\n</p>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            centred @3=20..47
              p "x" @3~20..47
            html "</p>\n" @5=49..53
        "#,
    },
    Row {
        name: "a wrapper end inside a list item does not end a wrapper outside",
        source: "<div align=\"center\">\n\n- item\n\n  </div>\n\nAfter.\n",
        blocks: r#"
            html "<div align=\"center\">\n" @1=0..20
            list @3=22..38
              item
                p "item" @3=24..28
                html "</div>\n" @5=32..38
            p "After." @7=40..46
        "#,
    },
    Row {
        name: "a wrapper start in a list item is not ended outside it",
        source: "- <div align=\"center\">\n\n  x\n\n</div>\n",
        blocks: r#"
            list @1=0..27
              item
                html "<div align=\"center\">\n" @1=2..22
                p "x" @3=26..27
            html "</div>\n" @5=29..35
        "#,
    },
    // A centred `<p>` that another tag ends without its end tag.
    // P4, read by the tree: a Markdown block ends the centred p: its tag is dim
    // and nothing is centred.
    Row {
        name: "a centred div after a Markdown block in a centred p is centred on its own",
        source: "<p align=\"center\">\n\nA\n\n<div align=\"center\">\n\nB\n\n</div>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            centred @5=23..54
              p "B" @7=45..46
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tag is dim
    // and nothing is centred.
    Row {
        name: "a center after a Markdown block in a centred p is centred on its own",
        source: "<p align=\"center\">\n\nA\n\n<center>\n\nB\n\n</center>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            centred @5=23..45
              p "B" @7=33..34
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tag is dim
    // and nothing is centred.
    Row {
        name: "a centred p after a Markdown block in a centred p is dim tags and its text",
        source: "<p align=\"center\">\n\nA\n\n<p align=\"center\">\n\nB\n\n</p>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            html "<p align=\"center\">\n" @5=23..41
            p "B" @7=43..44
            html "</p>\n" @9=46..50
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tag is dim
    // and nothing is centred.
    Row {
        name: "a plain p after a Markdown block in a centred p is dim tags and its text",
        source: "<p align=\"center\">\n\nA\n\n<p>\n\nB\n\n</p>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            html "<p>\n" @5=23..26
            p "B" @7=28..29
            html "</p>\n" @9=31..35
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tag is dim
    // and nothing is centred.
    Row {
        name: "a one-block centred p after a Markdown block in a centred p is centred on its own",
        source: "<p align=\"center\">\n\nA\n\n<p align=\"center\">B</p>\n\nC\n\n</p>\n\nD\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            centred @5=23..46
              p "B" @5~23..46
            p "C" @7=48..49
            html "</p>\n" @9=51..55
            p "D" @11=57..58
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tag is dim
    // and nothing is centred.
    Row {
        name: "a one-block centred div after a Markdown block in a centred p is centred on its own",
        source: "<p align=\"center\">\n\nA\n\n<div align=\"center\">B</div>\n\nC\n\n</p>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            centred @5=23..50
              p "B" @5~23..50
            p "C" @7=52..53
            html "</p>\n" @9=55..59
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tag is dim
    // and nothing is centred.
    Row {
        name: "a one-block center after a Markdown block in a centred p is centred on its own",
        source: "<p align=\"center\">\n\nA\n\n<center>B</center>\n\nC\n\n</p>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            centred @5=23..41
              p "B" @5~23..41
            p "C" @7=43..44
            html "</p>\n" @9=46..50
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tag is dim
    // and nothing is centred.
    Row {
        name: "a centred heading after a Markdown block in a centred p is centred on its own",
        source: "<p align=\"center\">\n\nA\n\n<h1 align=\"center\">T</h1>\n\nC\n\n</p>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            centred @5=23..48
              h1 "T" @5~23..48
            p "C" @7=50..51
            html "</p>\n" @9=53..57
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tag is dim
    // and nothing is centred.
    Row {
        name: "an image after a Markdown block in a centred p is not centred",
        source: "<p align=\"center\">\n\nA\n\n<img src=\"a.png\">\n\nC\n\n</p>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            p "![](a.png)" @5=23..40
            p "C" @7=42..43
            html "</p>\n" @9=45..49
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tag is dim
    // and nothing is centred.
    Row {
        name: "a details after a Markdown block in a centred p is read on its own",
        source: "<p align=\"center\">\n\nA\n\n<details>\n\nB\n\n</details>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            p "A" @3=20..21
            details "Details" @5=23..47
              p "B" @7=34..35
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tag is dim
    // and nothing is centred.
    Row {
        name: "a centred p in a div, cut by a Markdown block, is dim tags and the div ends it",
        source: "<div align=\"center\">\n\n<p align=\"center\">\n\nA\n\n</div>\n",
        blocks: r#"
            centred @1=0..51
              html "<p align=\"center\">\n" @3=22..40
              p "A" @5=42..43
        "#,
    },
    // P4, read by the tree: a Markdown block ends the centred p: its tag is dim
    // and nothing is centred.
    Row {
        name: "a centred p in a center, cut by a Markdown block, is dim tags and the center ends it",
        source: "<center>\n\n<p align=\"center\">\n\nA\n\n</center>\n",
        blocks: r#"
            centred @1=0..42
              html "<p align=\"center\">\n" @3=10..28
              p "A" @5=30..31
        "#,
    },
    Row {
        name: "an empty centred p closed by a start tag",
        source: "<p align=\"center\">\n\n<div align=\"center\">\n\nB\n\n</div>\n",
        blocks: r#"
            html "<p align=\"center\">\n" @1=0..18
            centred @3=20..51
              p "B" @5=42..43
        "#,
    },
    Row {
        name: "a details start ends a plain p, and its end tag is cut off as source",
        source: "<p>\n\n<details>\n\nA\n\n</details>\n\n</p>\nafter\n",
        blocks: r#"
            html "<p>\n" @1=0..3
            details "Details" @3=5..29
              p "A" @5=16..17
            html "</p>" @9~31..41
            p "after" @10~31..41
        "#,
    },
    // Elements that are source for now across HTML blocks: as written while
    // no Markdown block is in them, each HTML block's part a block of its
    // own; dim tags and the blocks when a Markdown block cuts them.
    Row {
        name: "a table that a blank line splits stays source, a block for each part",
        source: "<table>\n<tr><td>a</td></tr>\n\n<tr><td>b</td></tr>\n</table>\n",
        blocks: r#"
            html "<table>\n<tr><td>a</td></tr>\n" @1=0..27
            html "<tr><td>b</td></tr>\n</table>\n" @4=29..57
        "#,
    },
    Row {
        name: "a list that a blank line splits stays source, a block for each part",
        source: "<ul>\n<li>one</li>\n\n<li>two</li>\n</ul>\n",
        blocks: r#"
            html "<ul>\n<li>one</li>\n" @1=0..17
            html "<li>two</li>\n</ul>\n" @4=19..37
        "#,
    },
    Row {
        name: "a table that a markdown block cuts is its tags dim and the blocks",
        source: "<table>\n<tr><td>\n\n**b**\n\n</td><td>plain</td></tr>\n</table>\n",
        blocks: r#"
            html "<table>" @1~0..16
            html "<tr><td>\n" @2~0..16
            p "**b**" @4=18..23
            p "</td><td>plain</td></tr>" @6~25..58
            html "</table>\n" @7~25..58
        "#,
    },
    // P1, read by the tree: a plain p is a paragraph.
    Row {
        name: "a plain p of text and an image is a paragraph",
        source: "<p>Text <img src=a.png></p>\n",
        blocks: r#"
            p "Text ![](a.png)" @1=0..27
        "#,
    },
    // P1, read by the tree: an empty heading draws nothing.
    Row {
        name: "an empty heading draws nothing",
        source: "<h2></h2>\n",
        blocks: r#"
        "#,
    },
    // P1, read by the tree: the plain div after the heading is transparent (and
    // the heading is read: K5).
    Row {
        name: "a plain div after a heading in its block is a paragraph",
        source: "<h2>a</h2>\n<div>b</div>\n",
        blocks: r#"
            h2 "a" @1~0..23
            p "b" @2~0..23
        "#,
    },
    // P1, read by the tree: the plain div round the heading is transparent.
    Row {
        name: "a plain div round a heading is transparent",
        source: "<div><h2>a</h2></div>\n",
        blocks: r#"
            h2 "a" @1~0..21
        "#,
    },
    // P1, read by the tree: the plain p is a paragraph that the pre ends; its
    // end tag is a stray, dim.
    Row {
        name: "a plain p in a details body that a pre ends",
        source: "<details><summary>S</summary><p>a <pre>x</pre></p></details>\n",
        blocks: r#"
            details "S" @1=0..60
              p "a" @1~0..60
              code "x\n" @1~0..60
              html "</p>" @1~0..60
        "#,
    },
    // P2, read by the tree: a heading never closed is broken: its tag is dim
    // and its text read on its own.
    Row {
        name: "a heading never closed is dim, its text a paragraph",
        source: "<h2>T\n",
        blocks: r#"
            html "<h2>" @1~0..5
            p "T" @1~0..5
        "#,
    },
    // P2, read by the tree: a heading never closed is broken: its tag and the
    // stray end tag are dim.
    Row {
        name: "a heading ended by the end tag of another level is dim, as is the end tag",
        source: "<h2>T</h3>\n",
        blocks: r#"
            html "<h2>" @1~0..10
            p "T" @1~0..10
            html "</h3>\n" @1~0..10
        "#,
    },
    // P2, read by the tree: the outer heading is never closed: its tag is dim,
    // the inner one read.
    Row {
        name: "a heading never closed before another heading is dim, the other read",
        source: "<h2>Title\n<h2>Other</h2>\n",
        blocks: r#"
            html "<h2>" @1~0..24
            p "Title" @1~0..24
            h2 "Other" @2~0..24
        "#,
    },
    // P2, read by the tree: a heading may not hold a heading: its tags are dim,
    // the inner one read.
    Row {
        name: "a heading in a heading is read, the outer tags dim",
        source: "<h1><h2>a</h2></h1>\n",
        blocks: r#"
            html "<h1>" @1~0..19
            h2 "a" @1~0..19
            html "</h1>\n" @1~0..19
        "#,
    },
    // P2, read by the tree: a pre never closed is broken: its tag is dim and
    // its text read on its own.
    Row {
        name: "a pre never closed is dim, its text a paragraph",
        source: "<pre>x\n",
        blocks: r#"
            html "<pre>" @1~0..6
            p "x" @1~0..6
        "#,
    },
    // P4: an image line that blank lines cut off stays in the centred p, as
    // the plan's P4 says (B2 holds for HTML lines in a `<p>`).
    Row {
        name: "an image line in a centred p that blank lines split is centred",
        source: "<p align=\"center\">\n\n<img src=\"a.png\" alt=\"A\">\n\n<img src=\"b.png\" alt=\"B\">\n\n</p>\n",
        blocks: r#"
            centred @1=0..78
              p "![A](a.png)" @3=20..45
              p "![B](b.png)" @5=47..72
        "#,
    },
    // `<span>` draws nothing of its own: transparent, as a plain `<div>`.
    Row {
        name: "a span in a paragraph is transparent: what it holds is read as it is",
        source: "Text <span>**bold**</span> more\n",
        blocks: r#"
            p "Text **bold** more" @1=0..31
        "#,
    },
    Row {
        name: "a span in an HTML block is transparent",
        source: "<p>A <span class=\"x\">b <i>c</i></span> d</p>\n",
        blocks: r#"
            p "A b *c* d" @1=0..44
        "#,
    },
    Row {
        name: "a span never closed stays a dim tag in its text",
        source: "Text <span>bold\n",
        blocks: r#"
            p "Text <span>bold" @1=0..15
        "#,
    },
    Row {
        name: "a span in a span is transparent twice",
        source: "<span>a <span>b</span> c</span>\n",
        blocks: r#"
            p "a b c" @1=0..31
        "#,
    },
    // `<ins>` is underlined, at both levels (GitHub's list has it).
    Row {
        name: "an ins in a paragraph is underlined",
        source: "A <ins>b</ins> c\n",
        blocks: r#"
            p "A [u]b[/u] c" @1=0..16
        "#,
    },
    Row {
        name: "an ins in an HTML block is underlined",
        source: "<p>A <ins>b</ins> c</p>\n",
        blocks: r#"
            p "A [u]b[/u] c" @1=0..23
        "#,
    },
    Row {
        name: "an ins never closed stays a dim tag in its text",
        source: "A <ins>b\n",
        blocks: r#"
            p "A <ins>b" @1=0..8
        "#,
    },
    // `<strike>` is struck through, at both levels (GitHub's list has it).
    Row {
        name: "a strike in a paragraph is struck through",
        source: "A <strike>b</strike> c\n",
        blocks: r#"
            p "A ~~b~~ c" @1=0..22
        "#,
    },
    Row {
        name: "a strike in an HTML block is struck through, as s and del are",
        source: "<p>A <strike>b</strike> <s>c</s> <del>d</del></p>\n",
        blocks: r#"
            p "A ~~b~~ ~~c~~ ~~d~~" @1=0..49
        "#,
    },
    // A tag with nothing to show, after the summary of a details, is its own
    // block; the summary is not part of it.
    Row {
        name: "a dim tag after a details summary is shown once, apart from the summary",
        source: "<details><!-- c --><summary>S</summary><span></details>\n",
        blocks: r#"
            details "S" @1=0..55
              html "<span>" @1~0..55
        "#,
    },
    // A summary outside a details has no effect: its tags are dim and its
    // text is read on its own.
    Row {
        name: "a summary outside a details is a tag with no effect: dim tags, its text a paragraph",
        source: "<summary>S</summary>\n",
        blocks: r#"
            html "<summary>" @1~0..20
            p "S" @1~0..20
            html "</summary>\n" @1~0..20
        "#,
    },
    // The level: `<u>` is a phrase at "all" only (GitHub's level has the
    // same two rows in `GITHUB_ROWS`).
    Row {
        name: "u is a phrase at the level all",
        source: "A <u>b</u> c\n",
        blocks: r#"
            p "A [u]b[/u] c" @1=0..12
        "#,
    },
    Row {
        name: "u in an HTML block is a phrase at the level all",
        source: "<p>A <u>b</u> c</p>\n",
        blocks: r#"
            p "A [u]b[/u] c" @1=0..19
        "#,
    },
    // A `<pre>` or `<textarea>` that a blank line cuts: its end tag is in
    // Markdown, and the HTML after it is read as before.
    Row {
        name: "a pre cut by a blank line in a details does not turn the rest into text",
        source: "<details>\n<summary>Log</summary>\n<pre>\nline 1\n\nline 2\n</pre>\n</details>\n\n<div align=\"center\">footer</div>\n\n<h2 align=\"center\">Next</h2>\n",
        blocks: r#"
            details "Log" @1=0..71
              html "<pre>" @3~0..45
              p "line 1" @4~0..45
              p "line 2 </pre>" @6=47..60
            centred @10=73..105
              p "footer" @10~73..105
            centred @12=107..135
              h2 "Next" @12~107..135
        "#,
    },
    Row {
        name: "a pre cut by a blank line in a centred div does not turn the rest into text",
        source: "<div align=\"center\">\n<pre>\nfoo\n\nbar\n</pre>\n</div>\n\n<p align=\"center\">after</p>\n",
        blocks: r#"
            centred @1=0..49
              html "<pre>" @2~0..30
              p "foo" @3~0..30
              p "bar </pre>" @5=32..42
            centred @9=51..78
              p "after" @9~51..78
        "#,
    },
    Row {
        name: "a textarea never ended in a div is an unpaired tag",
        source: "<div>\n<textarea>\n</div>\n\n<p align=\"center\">after</p>\n",
        blocks: r#"
            html "<div>" @1~0..23
            html "<textarea>" @2~0..23
            p "</div>" @3~0..23
            centred @5=25..52
              p "after" @5~25..52
        "#,
    },
    // An element whose end is implied is source as far as its last byte, not
    // as far as the last node: the end tags and an HTML block that cannot be
    // read are in it, and nothing is lost.
    Row {
        name: "a li with a div after a blank line keeps all the text of the div",
        source: "<li>\n\n<div>x < y SECRET</div>\n",
        blocks: r#"
            html "<li>\n" @1=0..4
            html "<div>x < y SECRET</div>\n" @3=6..29
        "#,
    },
    Row {
        name: "a div of li and ul keeps the end tags of the list",
        source: "<div><li>a<ul><li>b</li></ul></div>\n",
        blocks: r#"
            html "<li>a<ul><li>b</li></ul>" @1~0..35
        "#,
    },
    Row {
        name: "a div of dd and a table keeps the end tag of the table",
        source: "<div>\n<dd>a<table><tr><td>b</td></tr></table></div>\n",
        blocks: r#"
            html "<dd>a<table><tr><td>b</td></tr></table>" @2~0..51
        "#,
    },
    // A block of nothing but anchors is not a block: its places go to the
    // next block (the one before at the end), or nowhere.
    Row {
        name: "a block of an anchor between two paragraphs is no block",
        source: "a\n\n<a name=\"top\"></a>\n\nb\n",
        blocks: r#"
            p "a" @1=0..1
            p "Anchor(\"top\")b" @5=23..24
        "#,
    },
    Row {
        name: "an anchor line before a heading is given to the heading",
        source: "<a name=\"top\"></a>\n# H\n\nb\n",
        blocks: r#"
            h1 "Anchor(\"top\")H" @2=19..22
            p "b" @4=24..25
        "#,
    },
    Row {
        name: "an anchor at the end of a document goes to the block before",
        source: "a\n\n<a name=\"end\"></a>\n",
        blocks: r#"
            p "aAnchor(\"end\")" @1=0..1
        "#,
    },
    Row {
        name: "a document of only anchors has no block",
        source: "<a name=\"x\"></a>\n\n<a id=\"y\"></a>\n",
        blocks: "",
    },
];

/// What GitHub's level reads differently from the level "all": the tags
/// that only "all" reads are dim tags, and what they hold is read on its
/// own. The rows of `ROWS` that this list does not repeat read the same at
/// both levels.
pub const GITHUB_ROWS: &[Row] = &[
    Row {
        name: "an ins is underlined at GitHub's level too",
        source: "A <ins>b</ins> c\n",
        blocks: r#"
            p "A [u]b[/u] c" @1=0..16
        "#,
    },
    Row {
        name: "a span is read at GitHub's level too: transparent",
        source: "Text <span>bold</span> more\n",
        blocks: r#"
            p "Text bold more" @1=0..27
        "#,
    },
    Row {
        name: "center is no wrapper at GitHub's level: its tags are dim",
        source: "<center>Text</center>\n",
        blocks: r#"
            html "<center>" @1~0..21
            p "Text" @1~0..21
            html "</center>\n" @1~0..21
        "#,
    },
    Row {
        name: "u is no phrase at GitHub's level: its tags stay in the paragraph",
        source: "A <u>b</u> c\n",
        blocks: r#"
            p "A <u>b</u> c" @1=0..12
        "#,
    },
    Row {
        name: "u in an HTML block is no phrase at GitHub's level",
        source: "<p>A <u>b</u> c</p>\n",
        blocks: r#"
            p "A <u>b</u> c" @1=0..19
        "#,
    },
];

/// What `html_tags = "hide"` changes, at GitHub's level: the tags that have
/// no effect are left out, in a block of their own and in text. Source that
/// is shown as written stays: a block that is source for now, a comment
/// alone, and what the tokenizer cannot read.
pub const HIDE_ROWS: &[Row] = &[
    Row {
        name: "a dim tag between blocks is left out",
        source: "Text\n\n</div>\n\nMore\n",
        blocks: r#"
            p "Text" @1=0..4
            p "More" @5=14..18
        "#,
    },
    Row {
        name: "a start tag that nothing ends is left out",
        source: "<div align=\"center\">\n\nText\n",
        blocks: r#"
            p "Text" @3=22..26
        "#,
    },
    Row {
        name: "the tags of an element that kiri does not know are left out of text",
        source: "A <abbr>b</abbr> c\n",
        blocks: r#"
            p "A b c" @1=0..18
        "#,
    },
    Row {
        name: "a tag left out between two spaces of an HTML block leaves one",
        source: "<p>a <abbr> b</abbr></p>\n",
        blocks: r#"
            p "a b" @1=0..24
        "#,
    },
    Row {
        name: "a tag that nothing ends is left out of text",
        source: "A <b>b c\n",
        blocks: r#"
            p "A b c" @1=0..8
        "#,
    },
    Row {
        name: "an end tag that ends nothing is left out of text",
        source: "A b</i> c\n",
        blocks: r#"
            p "A b c" @1=0..9
        "#,
    },
    Row {
        name: "the dim tags of a broken heading are left out, its text stays",
        source: "<h2>T</h3>\n",
        blocks: r#"
            p "T" @1~0..10
        "#,
    },
    Row {
        name: "the tags of center are left out at GitHub's level",
        source: "<center>Text</center>\n",
        blocks: r#"
            p "Text" @1~0..21
        "#,
    },
    Row {
        name: "a block of tags with nothing to show is left out",
        source: "<abbr>\n",
        blocks: "",
    },
    Row {
        name: "a paragraph of tags and nothing else is left out",
        source: "<abbr><abbr>\n",
        blocks: "",
    },
    // A block is left out when nothing in it draws anything: line breaks,
    // and emphasis or a link with nothing in it, are not drawing.
    Row {
        name: "a paragraph of hidden tags and a line break is left out",
        source: "<abbr></abbr>\n<abbr></abbr>\n\nAfter\n",
        blocks: r#"
            p "After" @4=29..34
        "#,
    },
    Row {
        name: "a paragraph of emphasis with nothing in it is left out",
        source: "**<abbr></abbr>**\n\nAfter\n",
        blocks: r#"
            p "After" @3=19..24
        "#,
    },
    Row {
        name: "a paragraph of a link with nothing in it is left out",
        source: "[<abbr></abbr>](u)\n\nAfter\n",
        blocks: r#"
            p "After" @3=20..25
        "#,
    },
    Row {
        name: "a paragraph of hidden tags and a space is left out",
        source: "<abbr></abbr> <abbr></abbr>\n\nAfter\n",
        blocks: r#"
            p "After" @3=29..34
        "#,
    },
    Row {
        name: "a paragraph of empty emphasis, a space and hidden tags is left out",
        source: "*<abbr></abbr>* <abbr></abbr>\n\nAfter\n",
        blocks: r#"
            p "After" @3=31..36
        "#,
    },
    // Tags that are read are not hidden, so a block that held only them, and
    // nothing with no effect, was empty as written and stays.
    Row {
        name: "an empty b stays at hide, which hides nothing",
        source: "<b></b>\n\nAfter\n",
        blocks: r#"
            p "****" @1=0..7
            p "After" @3=9..14
        "#,
    },
    Row {
        name: "bold round an empty i stays at hide",
        source: "**<i></i>**\n\nAfter\n",
        blocks: r#"
            p "******" @1=0..11
            p "After" @3=13..18
        "#,
    },
    Row {
        name: "an empty kbd stays at hide",
        source: "<kbd></kbd>\n\nAfter\n",
        blocks: r#"
            p "<kbd></kbd>" @1=0..11
            p "After" @3=13..18
        "#,
    },
    Row {
        name: "an empty sup stays at hide",
        source: "<sup></sup>\n\nAfter\n",
        blocks: r#"
            p "Superscript([])" @1=0..11
            p "After" @3=13..18
        "#,
    },
    Row {
        name: "a span of spaces stays at hide, which hides nothing",
        source: "<span>  </span>\n\nAfter\n",
        blocks: r#"
            p "  " @1=0..15
            p "After" @3=17..22
        "#,
    },
    // A non-breaking space draws: it is the way to write a blank line, so
    // it is not nothing, hidden tags or not.
    Row {
        name: "a span of a non-breaking space stays at hide",
        source: "<span>&nbsp;</span>\n\nAfter\n",
        blocks: r#"
            p "\u{a0}" @1=0..19
            p "After" @3=21..26
        "#,
    },
    Row {
        name: "bold of a non-breaking space stays at hide",
        source: "<b>&nbsp;</b>\n\nAfter\n",
        blocks: r#"
            p "**\u{a0}**" @1=0..13
            p "After" @3=15..20
        "#,
    },
    Row {
        name: "hidden tags and a non-breaking space stay at hide",
        source: "<abbr></abbr>&nbsp;\n\nAfter\n",
        blocks: r#"
            p "\u{a0}" @1=0..19
            p "After" @3=21..26
        "#,
    },
    // What was empty as written stays, with no tag to hide: an empty link.
    Row {
        name: "an empty link written in Markdown stays at hide",
        source: "[](u)\n",
        blocks: r#"
            p "[](u)" @1=0..5
        "#,
    },
    Row {
        name: "bold round an empty link written in Markdown stays at hide",
        source: "**[](u)**\n",
        blocks: r#"
            p "**[](u)**" @1=0..9
        "#,
    },
    Row {
        name: "an empty link in a quote stays at hide",
        source: "> [](u)\n",
        blocks: r#"
            quote @1=0..7
              p "[](u)" @1=2..7
        "#,
    },
    Row {
        name: "an empty link in a heading stays at hide",
        source: "# [](u)\n",
        blocks: r#"
            h1 "[](u)" @1=0..7
        "#,
    },
    Row {
        name: "an empty link in a list item stays at hide",
        source: "- [](u)\n- b\n",
        blocks: r#"
            list @1=0..11
              item
                p "[](u)" @1=2..7
              item
                p "b" @2=10..11
        "#,
    },
    // What hiding empties goes with it, if it held something: a heading, a
    // list item, a quote. A cell stays a cell, and what was empty as written
    // stays.
    Row {
        name: "a heading of nothing but hidden tags is left out",
        source: "# <abbr>\n\nAfter\n",
        blocks: r#"
            p "After" @3=10..15
        "#,
    },
    Row {
        name: "a heading keeps its text when its tags are hidden",
        source: "# a <abbr>b</abbr>\n",
        blocks: r#"
            h1 "a b" @1=0..18
        "#,
    },
    Row {
        name: "a list item of nothing but hidden tags is left out",
        source: "- <abbr>\n- b\n",
        blocks: r#"
            list @1=0..12
              item
                p "b" @2=11..12
        "#,
    },
    Row {
        name: "a list of items that hiding empties is left out",
        source: "- <abbr>\n- <i>\n\nAfter\n",
        blocks: r#"
            p "After" @4=16..21
        "#,
    },
    Row {
        name: "a list item that was empty as written stays",
        source: "- a\n-\n- c\n",
        blocks: r#"
            list @1=0..9
              item
                p "a" @1=2..3
              item
              item
                p "c" @3=8..9
        "#,
    },
    Row {
        name: "a quote of nothing but hidden tags is left out",
        source: "> <abbr>\n\nAfter\n",
        blocks: r#"
            p "After" @3=10..15
        "#,
    },
    Row {
        name: "an alert of nothing but hidden tags is left out with its title",
        source: "> [!NOTE]\n> <abbr></abbr>\n\nAfter\n",
        blocks: r#"
            p "After" @4=27..32
        "#,
    },
    Row {
        name: "a quote keeps its text when its tags are hidden",
        source: "> a <abbr>b</abbr>\n",
        blocks: r#"
            quote @1=0..18
              p "a b" @1=2..18
        "#,
    },
    Row {
        name: "a table cell of nothing but hidden tags stays an empty cell",
        source: "| a | b |\n|---|---|\n| <abbr> | c |\n",
        blocks: r#"
            Table(Table { alignments: [Left, Left], cells: Parsed { header: [[Text("a")], [Text("b")]], rows: [[[], [Text("c")]]] } }) @1=0..34
        "#,
    },
    Row {
        name: "the text of a link keeps its words when its tags are hidden",
        source: "[a <abbr>b</abbr>](u)\n",
        blocks: r#"
            p "[a b](u)" @1=0..21
        "#,
    },
    Row {
        name: "a comment in text is left out like a tag",
        source: "a <!-- c -->b\n",
        blocks: r#"
            p "a b" @1=0..13
        "#,
    },
    Row {
        name: "a loose summary is a tag with no effect: hidden, its text read on its own",
        source: "<summary>S</summary>\n",
        blocks: r#"
            p "S" @1~0..20
        "#,
    },
    Row {
        name: "a block that is source for now is not hidden",
        source: "<table><tr><td>x</td></tr></table>\n",
        blocks: r#"
            html "<table><tr><td>x</td></tr></table>\n" @1=0..34
        "#,
    },
    Row {
        name: "a rule in HTML is not hidden",
        source: "<hr>\n",
        blocks: r#"
            html "<hr>\n" @1=0..4
        "#,
    },
    Row {
        name: "a comment alone is not hidden",
        source: "<!-- note -->\n",
        blocks: r#"
            html "<!-- note -->\n" @1=0..13
        "#,
    },
    // A `<pre>` or `<textarea>` that a blank line cuts: its end tag is in
    // Markdown, and the HTML after it is read as before.
    Row {
        name: "a pre cut by a blank line in a details does not turn the rest into text at hide",
        source: "<details>\n<summary>Log</summary>\n<pre>\nline 1\n\nline 2\n</pre>\n</details>\n\n<div align=\"center\">footer</div>\n\n<h2 align=\"center\">Next</h2>\n",
        blocks: r#"
            details "Log" @1=0..71
              p "line 1" @4~0..45
              p "line 2 " @6=47..60
            centred @10=73..105
              p "footer" @10~73..105
            centred @12=107..135
              h2 "Next" @12~107..135
        "#,
    },
    Row {
        name: "a pre cut by a blank line in a centred div does not turn the rest into text at hide",
        source: "<div align=\"center\">\n<pre>\nfoo\n\nbar\n</pre>\n</div>\n\n<p align=\"center\">after</p>\n",
        blocks: r#"
            centred @1=0..49
              p "foo" @3~0..30
              p "bar " @5=32..42
            centred @9=51..78
              p "after" @9~51..78
        "#,
    },
    Row {
        name: "a textarea never ended in a div is an unpaired tag at hide",
        source: "<div>\n<textarea>\n</div>\n\n<p align=\"center\">after</p>\n",
        blocks: r#"
            p "</div>" @3~0..23
            centred @5=25..52
              p "after" @5~25..52
        "#,
    },
];

/// What `html_tags = "hide"` changes at the level "all": what that level
/// reads is read, not hidden, and the tags with no effect are left out as
/// at GitHub's.
pub const HIDE_ROWS_ALL: &[Row] = &[
    Row {
        name: "center is read at all, not hidden",
        source: "<center>Text</center>\n",
        blocks: r#"
            centred @1=0..21
              p "Text" @1~0..21
        "#,
    },
    Row {
        name: "a tag of an unknown element is left out of text at all too",
        source: "A <abbr>b</abbr> <u>c</u>\n",
        blocks: r#"
            p "A b [u]c[/u]" @1=0..25
        "#,
    },
];
