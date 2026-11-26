use super::*;
use crate::typeset::model::Block;

#[test]
fn anchor_ids_are_the_id_and_the_name_of_a_tags() {
    assert_eq!(anchor_ids("<a id=\"x\">"), ["x"]);
    assert_eq!(anchor_ids("<a name='y'></a>"), ["y"]);
    assert_eq!(anchor_ids("<a id=\"x\" name=\"y\"></a>"), ["x", "y"]);
    assert_eq!(anchor_ids("<a id=\"x\" name=\"x\"></a>"), ["x"]);
    assert!(anchor_ids("<a href=\"u\">t</a>").is_empty());
    assert!(anchor_ids("<span id=\"x\"></span>").is_empty());
    assert!(anchor_ids("<a id=\"\"></a>").is_empty());
    assert_eq!(
        anchor_ids("<a name=\"a\"></a>\n<a id=\"b\"></a><a href=\"u\" id=\"c\">"),
        ["a", "b", "c"]
    );
    assert!(anchor_ids("no tags <not a tag").is_empty());
}

fn img(src: &str, alt: &str) -> Option<Img> {
    Some(Img {
        src: src.into(),
        alt: alt.into(),
        width: None,
        height: None,
        link: None,
        ids: Vec::new(),
    })
}

fn sized(width: Option<Length>, height: Option<Length>) -> Option<Img> {
    Some(Img {
        width,
        height,
        ..img("a.png", "")?
    })
}

fn linked(src: &str, href: &str) -> Option<Img> {
    Some(Img {
        link: Some(href.into()),
        ..img(src, "")?
    })
}

#[test]
fn reads_src_and_alt() {
    assert_eq!(
        inline_img(r#"<img src="a.png" alt="An image">"#),
        img("a.png", "An image")
    );
}

#[test]
fn reads_any_quoting_case_and_self_closing() {
    assert_eq!(inline_img("<IMG SRC='a.png' Alt=x />"), img("a.png", "x"));
    assert_eq!(inline_img("<img src=a.png alt=\"x\"/>"), img("a.png", "x"));
    assert_eq!(
        inline_img("<img\n  src=\"a.png\"\n  alt=\"two\nlines\">"),
        img("a.png", "two\nlines")
    );
    assert_eq!(
        inline_img(r#"<img alt="a > b" src="a.png">"#),
        img("a.png", "a > b")
    );
}

#[test]
fn an_attribute_without_a_value_is_empty() {
    assert_eq!(inline_img("<img hidden src=a.png alt>"), img("a.png", ""));
}

#[test]
fn the_first_of_repeated_attributes_counts() {
    assert_eq!(
        inline_img("<img src=a.png src=b.png alt=x>"),
        img("a.png", "x")
    );
}

#[test]
fn decodes_entities_in_values() {
    assert_eq!(
        inline_img(r#"<img src="a.png?x=1&amp;y=2" alt="&lt;Fish &amp; chips&gt;">"#),
        img("a.png?x=1&y=2", "<Fish & chips>")
    );
    assert_eq!(
        inline_img(r#"<img src=a.png alt="&quot;&apos;&nbsp;&#65;&#x42;&#X43;">"#),
        img("a.png", "\"'\u{a0}ABC")
    );
}

#[test]
fn keeps_entities_it_does_not_know() {
    assert_eq!(
        inline_img(r#"<img src=a.png alt="&foo; &amp &#; &#xZZ; &#0; a&b">"#),
        img("a.png", "&foo; &amp &#; &#xZZ; \u{fffd} a&b")
    );
}

#[test]
fn decodes_the_common_named_entities() {
    for (name, character) in [
        ("copy", '\u{a9}'),
        ("reg", '\u{ae}'),
        ("trade", '\u{2122}'),
        ("hellip", '\u{2026}'),
        ("mdash", '\u{2014}'),
        ("ndash", '\u{2013}'),
        ("lsquo", '\u{2018}'),
        ("rsquo", '\u{2019}'),
        ("ldquo", '\u{201c}'),
        ("rdquo", '\u{201d}'),
        ("sbquo", '\u{201a}'),
        ("bdquo", '\u{201e}'),
        ("laquo", '\u{ab}'),
        ("raquo", '\u{bb}'),
        ("lsaquo", '\u{2039}'),
        ("rsaquo", '\u{203a}'),
        ("bull", '\u{2022}'),
        ("middot", '\u{b7}'),
        ("times", '\u{d7}'),
        ("divide", '\u{f7}'),
        ("minus", '\u{2212}'),
        ("plusmn", '\u{b1}'),
        ("deg", '\u{b0}'),
        ("micro", '\u{b5}'),
        ("para", '\u{b6}'),
        ("sect", '\u{a7}'),
        ("dagger", '\u{2020}'),
        ("Dagger", '\u{2021}'),
        ("euro", '\u{20ac}'),
        ("pound", '\u{a3}'),
        ("yen", '\u{a5}'),
        ("cent", '\u{a2}'),
        ("larr", '\u{2190}'),
        ("rarr", '\u{2192}'),
        ("uarr", '\u{2191}'),
        ("darr", '\u{2193}'),
        ("harr", '\u{2194}'),
        ("ensp", '\u{2002}'),
        ("emsp", '\u{2003}'),
        ("thinsp", '\u{2009}'),
    ] {
        assert_eq!(entity(name), Some(character), "&{name};");
        assert_eq!(
            decode_entities(&format!("a&{name};b")),
            format!("a{character}b")
        );
    }
}

#[test]
fn named_entities_are_case_sensitive_and_need_their_semicolon() {
    for kept in ["&COPY;", "&Mdash;", "&dagger", "&mdash", "&foo;", "&;"] {
        assert_eq!(decode_entities(kept), kept);
    }
    assert_eq!(decode_entities("&amp;mdash;"), "&mdash;");
    assert_eq!(decode_entities("&mdash;&mdash;"), "\u{2014}\u{2014}");
}

#[test]
fn keeps_an_ampersand_far_from_a_semicolon() {
    let long = format!("&{};", "a".repeat(40));
    assert_eq!(
        inline_img(&format!("<img src=a.png alt=\"{long} &amp;\">")),
        img("a.png", &format!("{long} &"))
    );
}

#[test]
fn an_img_needs_a_src() {
    assert_eq!(inline_img("<img alt=x>"), None);
    assert_eq!(inline_img("<img src alt=x>"), None);
    assert_eq!(inline_img("<img src=\" \n\" alt=x>"), None);
}

#[test]
fn trims_whitespace_around_the_src() {
    assert_eq!(
        inline_img("<img src=\" a.png\n\" alt=\" x \">"),
        img("a.png", " x ")
    );
}

#[test]
fn inline_html_is_an_image_only_when_it_is_one_img_tag() {
    assert_eq!(inline_img("<span>"), None);
    assert_eq!(inline_img("</img>"), None);
    assert_eq!(inline_img("<imgx src=a.png>"), None);
    assert_eq!(inline_img("<img src=a.png"), None);
    assert_eq!(inline_img("<img src=a.png><img src=b.png>"), None);
}

#[test]
fn reads_width_and_height_in_css_pixels() {
    use Length::Pixels;
    assert_eq!(
        inline_img("<img src=a.png width=320 height=\"240\">"),
        sized(Some(Pixels(320)), Some(Pixels(240)))
    );
    assert_eq!(
        inline_img("<img src=a.png width=\" 100px\">"),
        sized(Some(Pixels(100)), None)
    );
    assert_eq!(
        inline_img("<img src=a.png height=\"80.6\">"),
        sized(None, Some(Pixels(81)))
    );
}

#[test]
fn reads_a_percentage() {
    assert_eq!(
        inline_img("<img src=a.png width=\"50%\" height=\"25.5 %\">"),
        sized(Some(Length::Percent(50)), Some(Length::Pixels(26)))
    );
    assert_eq!(
        inline_img("<img src=a.png width=\"33.4%\">"),
        sized(Some(Length::Percent(33)), None)
    );
}

#[test]
fn ignores_lengths_that_are_no_size() {
    for value in ["", "px", "-10", "0", "0.4", "0%", ".5", "auto"] {
        assert_eq!(
            inline_img(&format!(
                "<img src=a.png width=\"{value}\" height=\"{value}\">"
            )),
            sized(None, None),
            "{value:?}"
        );
    }
    assert_eq!(
        inline_img("<img src=a.png width=99999999999>"),
        sized(Some(Length::Pixels(u32::MAX)), None)
    );
}

#[test]
fn a_block_of_one_img_is_an_image() {
    assert_eq!(block_img("<img src=a.png alt=x>\n"), img("a.png", "x"));
}

#[test]
fn a_block_may_wrap_its_img() {
    let wrapped = "<p align=\"center\">\n  <img src=\"a.png\" alt=\"x\">\n</p>\n";
    assert_eq!(block_img(wrapped), img("a.png", "x"));
    let centred = "<CENTER><div><img src=a.png alt=x></div></CENTER>\n";
    assert_eq!(block_img(centred), img("a.png", "x"));
    let commented = "<!-- logo -->\n<p><img src=a.png alt=x></p>\n";
    assert_eq!(block_img(commented), img("a.png", "x"));
}

#[test]
fn a_block_whose_wrapping_tags_do_not_end_in_it_is_no_image() {
    for html in [
        "<img src=a.png>\n</div>\n",
        "</div>\n<img src=a.png>\n",
        "<a href=x><img src=a.png>\n",
        "<div><img src=a.png>\n",
        "<center><img src=a.png>\n",
    ] {
        assert_eq!(block_img(html), None, "{html:?}");
    }
    // A `<p>` may leave out its end tag, which the block or an end tag
    // round it then ends.
    for html in [
        "<p align=\"center\">\n  <img src=a.png>\n",
        "<div><p><img src=a.png></div>\n",
    ] {
        assert_eq!(block_img(html), img("a.png", ""), "{html:?}");
    }
}

#[test]
fn a_picture_shows_its_img() {
    let picture = "<picture>\n\
            \x20 <source media=\"(prefers-color-scheme: dark)\" srcset=\"dark.png\">\n\
            \x20 <img src=\"light.png\" alt=\"Logo\">\n\
            </picture>\n";
    assert_eq!(block_img(picture), img("light.png", "Logo"));
}

#[test]
fn reads_a_link_from_its_tags() {
    assert_eq!(
        link_start(r#"<a href="https://x/?a=1&amp;b=2">"#),
        Some("https://x/?a=1&b=2".into())
    );
    assert_eq!(link_start("<A HREF=x title=y>"), Some("x".into()));
    assert_eq!(link_start("<a>"), None);
    assert_eq!(link_start("<a href=x><b>"), None);
    assert_eq!(link_start("<abbr href=x>"), None);
    assert!(is_link_end("</a>"));
    assert!(is_link_end("</A >"));
    assert!(!is_link_end("<a>"));
    assert!(!is_link_end("</abbr>"));
}

#[test]
fn reads_a_line_break_in_any_spelling() {
    for br in ["<br>", "<br/>", "<br />", "<BR>", "<Br\n>", "<br class=x>"] {
        assert!(is_line_break(br), "{br:?}");
    }
    for other in ["<brx>", "<br", "<br><br>", "<b>", "</br>", "<!-- <br> -->"] {
        assert!(!is_line_break(other), "{other:?}");
    }
}

#[test]
fn reads_the_tags_of_a_kbd() {
    assert_eq!(phrase_start("<kbd>"), Some(Phrase::Kbd));
    assert_eq!(phrase_start("<KBD title=\"key\">"), Some(Phrase::Kbd));
    assert_eq!(phrase_end("</kbd>"), Some(Phrase::Kbd));
    assert_eq!(phrase_end("</Kbd >"), Some(Phrase::Kbd));
    for other in ["</kbd>", "<kbdx>", "<span>", "<kbd><kbd>", "<kbd"] {
        assert_eq!(phrase_start(other), None, "{other:?}");
    }
    for other in ["<kbd>", "</kbdx>", "</span>", "</kbd></kbd>"] {
        assert_eq!(phrase_end(other), None, "{other:?}");
    }
}

#[test]
fn reads_the_tags_of_a_sub_and_a_sup() {
    assert_eq!(phrase_start("<sub>"), Some(Phrase::Sub));
    assert_eq!(phrase_end("</SUB>"), Some(Phrase::Sub));
    assert_eq!(phrase_start("<sup class=x>"), Some(Phrase::Sup));
    assert_eq!(phrase_end("</sup >"), Some(Phrase::Sup));
    assert_eq!(phrase_start("<summary>"), None);
    assert_eq!(phrase_start("<supx>"), None);
}

#[test]
fn reads_the_tags_of_a_mark() {
    assert_eq!(phrase_start("<mark class=x>"), Some(Phrase::Mark));
    assert_eq!(phrase_end("</MARK>"), Some(Phrase::Mark));
    assert_eq!(phrase_start("<marker>"), None);
}

#[test]
fn a_block_may_link_its_img() {
    let linked_block = "<p align=center>\n  <a href=\"https://x\"><img src=a.png></a>\n</p>\n";
    assert_eq!(block_img(linked_block), linked("a.png", "https://x"));
    assert_eq!(block_img("<a><img src=a.png></a>\n"), img("a.png", ""));
    assert_eq!(
        block_img("<a href=x></a><img src=a.png>\n"),
        img("a.png", "")
    );
}

#[test]
fn a_block_with_more_than_an_img_is_not_an_image() {
    assert_eq!(block_img("<p>Text <img src=a.png></p>\n"), None);
    assert_eq!(block_img("<p><img src=a.png><img src=b.png></p>\n"), None);
    assert_eq!(block_img("<p><b><img src=a.png></b></p>\n"), None);
    assert_eq!(block_img("<p><br><img src=a.png></p>\n"), None);
    assert_eq!(block_img("<p>&nbsp;<img src=a.png></p>\n"), None);
    assert_eq!(block_img("<div>\n</div>\n"), None);
    assert_eq!(block_img("<!DOCTYPE html><img src=a.png>\n"), None);
    assert_eq!(block_img("<!-- <img src=a.png> -->\n"), None);
    assert_eq!(block_img("<img alt=x>\n"), None);
}

#[test]
fn a_block_cut_short_is_not_an_image() {
    assert_eq!(block_img("<img src=a.png>\n<p\n"), None);
    assert_eq!(block_img("<img src=a.png>\n<!-- open\n"), None);
    assert_eq!(block_img("<img src=\"a.png>\n"), None);
}
/// Odd input that the readers of HTML must meet without a panic.
fn odd_inputs() -> Vec<String> {
    let mut inputs: Vec<String> = [
        "",
        "<",
        "<h2",
        "<h2>",
        "</h2>",
        "<h2>x",
        "<h2 a=\"",
        "<!--",
        "<!-- x",
        "<pre",
        "<pre>",
        "&",
        "&;",
        "&#",
        "&#x;",
        "&#99999999999;",
        "<>",
        "</>",
        "</",
        "<h2></h2",
        "<h1><h2>a</h2></h1>",
        "\u{e9}<h2>\u{e9}</h2>\u{e9}",
        "<b",
        "<b>",
        "</b>",
        "<code",
        "<code>",
        "</code>",
        "&mdash",
        "&mdash;&",
    ]
    .map(str::to_owned)
    .into();
    inputs.push("&".repeat(100_000));
    inputs.push("&mdash".repeat(20_000));
    inputs.push("<h2>".repeat(5_000));
    inputs
}

#[test]
fn odd_input_does_not_panic() {
    for input in odd_inputs() {
        heading(&input);
        pre(&input);
        wrapper::events(&input);
        wrapper::inline_html(&input);
        block_img(&input);
        phrase_start(&input);
        phrase_end(&input);
        decode_entities(&input);
    }
    assert_eq!(heading(""), None);
    assert_eq!(heading("<h2"), None);
    assert_eq!(pre("<pre"), None);
    assert_eq!(pre("<pre>"), None);
    assert_eq!(pre("<pre>\u{e9}</pre>"), Some((None, "\u{e9}\n".into())));
    assert_eq!(decode_entities(&"&".repeat(1000)), "&".repeat(1000));
}

#[test]
fn an_a_start_tag_without_an_href_is_an_anchor_start() {
    for anchor in ["<a name=\"x\">", "<a id=x>", "<A>"] {
        assert!(is_anchor_start(anchor), "{anchor:?}");
    }
    for other in [
        "<a href=\"u\">",
        "<a name=\"x\" href=\"u\">",
        "</a>",
        "<b>",
        "<abbr>",
    ] {
        assert!(!is_anchor_start(other), "{other:?}");
    }
}

/// How the time to parse `make(n)` grows from `n` to eight times `n`
/// (`crate::testing::growth`).
fn growth(make: impl Fn(usize) -> String, n: usize) -> f64 {
    crate::testing::growth(
        |source| {
            crate::typeset::parse::parse(source);
        },
        make,
        n,
    )
}

#[test]
fn many_stray_end_tags_in_a_paragraph_are_joined_in_linear_time() {
    let alone = growth(|n| format!("text {}\n", "</b>".repeat(n)), 2_000);
    assert!(alone < 24.0, "only end tags: {alone}");
    // A start tag that never ends, far back, which each end tag of another
    // element must not search for.
    let after_start = growth(|n| format!("<b>text {}\n", "</i>".repeat(n)), 2_000);
    assert!(after_start < 24.0, "after a start tag: {after_start}");
    let in_links = growth(
        |n| format!("<a name=x>text {}\n", "</a><kbd>".repeat(n)),
        1_000,
    );
    assert!(in_links < 24.0, "start tags that never end: {in_links}");
}

/// How many elements are in each other, down the first of each.
fn depth(inlines: &[Inline]) -> usize {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Strong(inner) => 1 + depth(inner),
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

#[test]
fn start_tags_one_in_another_join_only_to_a_depth_of_100() {
    let nested = |n: usize| {
        let source = format!("{}x{}\n", "<b>".repeat(n), "</b>".repeat(n));
        match crate::typeset::parse::parse(&source).blocks.as_slice() {
            [Block::Paragraph(content)] => depth(content),
            other => panic!("not one paragraph: {other:?}"),
        }
    };
    assert_eq!(nested(50), 50);
    assert_eq!(nested(100), 100);
    assert_eq!(nested(1_000), 100);
    // So deep that the stack of a thread would not hold it.
    assert_eq!(nested(20_000), 100);
}

/// Inlines as Markdown-like text, the elements of HTML in brackets.
fn show(inlines: &[Inline]) -> String {
    let wrap = |name: &str, inner: &[Inline]| format!("<{name}>{}</{name}>", show(inner));
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Text(text) => text.clone(),
            Inline::Html(tag) => format!("[{tag}]"),
            Inline::Strong(inner) => wrap("strong", inner),
            Inline::Emphasis(inner) => wrap("em", inner),
            Inline::Underline(inner) => wrap("u", inner),
            Inline::Kbd(inner) => wrap("kbd", inner),
            Inline::Code(code) => format!("<code:{code}>"),
            Inline::Anchor(id) => format!("<anchor:{id}>"),
            Inline::Image(image) => format!("<image:{}>", image.url),
            Inline::Link { url, content, .. } => format!("<link:{url}>{}</link>", show(content)),
            other => format!("{other:?}"),
        })
        .collect()
}

fn paragraph(source: &str) -> String {
    match crate::typeset::parse::parse(source).blocks.as_slice() {
        [Block::Paragraph(content)] => show(content),
        other => panic!("not one paragraph: {other:?}"),
    }
}

/// The pairing of start and end tags in text, pinned before the join was
/// rewritten to read each tag once.
#[test]
fn start_and_end_tags_in_text_pair_as_they_did() {
    for (source, expected) in [
        // The nearest start tag of its name, and what is open inside it is
        // given up.
        ("<b><i>x</b>y</i>\n", "<strong>[<i>]x</strong>y[</i>]"),
        (
            "<b>x<b>y</b>z</b>\n",
            "<strong>x<strong>y</strong>z</strong>",
        ),
        ("<b>x</i>y</b>\n", "<strong>x[</i>]y</strong>"),
        // An end tag of another element than the start tag before it.
        ("<b>x</strong>\n", "[<b>]x[</strong>]"),
        // A `<code>` is code only around text; the pair stays as it is and
        // is closed.
        (
            "<code><b>x</b></code>\n",
            "[<code>]<strong>x</strong>[</code>]",
        ),
        ("<code>a</code>b</code>\n", "<code:a>b[</code>]"),
        ("<code><code>a</code></code>\n", "[<code>]<code:a>[</code>]"),
        // A link holds something; an empty one stays as it is and is closed.
        ("<a href=x></a>y</a>\n", "[<a href=x>][</a>]y[</a>]"),
        (
            "<a href=x></a><a href=y>t</a>\n",
            "[<a href=x>][</a>]<link:y>t</link>",
        ),
        (
            "<a href=x><b>t</b></a>\n",
            "<link:x><strong>t</strong></link>",
        ),
        (
            "<a href=x><img src=a.png></a>\n",
            "<link:x><image:a.png></link>",
        ),
        // A place that an `<a>` without an `href` names; what it holds stays,
        // and the start tags in it are open still.
        ("<a name=p>x</a>\n", "<anchor:p>x"),
        ("<a name=p><u>x</a>y</u>\n", "<anchor:p><u>xy</u>"),
        ("<u><a id=q>x</a>y</u>\n", "<u><anchor:q>xy</u>"),
        ("</a><a name=p>x</a>\n", "[</a>]<anchor:p>x"),
        // Text elements nest.
        ("<kbd><kbd>x</kbd></kbd>\n", "<kbd><kbd>x</kbd></kbd>"),
    ] {
        assert_eq!(paragraph(source), expected, "{source:?}");
    }
}
