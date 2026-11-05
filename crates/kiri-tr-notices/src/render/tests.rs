use std::collections::BTreeMap;

use super::*;
use crate::{about, by_hand, metadata};

struct Fake(BTreeMap<PathBuf, String>);

impl Files for Fake {
    fn read(&self, path: &Path) -> Result<String, String> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| format!("no file {}", path.display()))
    }

    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, String> {
        Ok(self
            .0
            .keys()
            .filter(|p| p.starts_with(dir))
            .cloned()
            .collect())
    }
}

fn fake(files: &[(&str, &str)]) -> Fake {
    Fake(
        files
            .iter()
            .map(|(p, t)| (PathBuf::from(p), t.to_string()))
            .collect(),
    )
}

fn about_json(licenses: &str) -> About {
    about::parse(&format!(r#"{{"licenses": [{licenses}]}}"#)).unwrap()
}

fn license(name: &str, text: &str, used_by: &[(&str, &str)]) -> String {
    let used: Vec<String> = used_by
        .iter()
        .map(|(n, v)| format!(r#"{{"crate": {{"name": "{n}", "version": "{v}"}}}}"#))
        .collect();
    format!(
        r#"{{"name": "{name}", "text": "{text}", "used_by": [{}]}}"#,
        used.join(",")
    )
}

fn metadata_json() -> Metadata {
    metadata::parse(
        r#"{"packages": [
            {"name": "kiri-the-reader", "version": "0.3.0", "manifest_path": "/w/kiri/Cargo.toml"},
            {"name": "libm", "version": "0.2.16", "manifest_path": "/reg/libm-0.2.16/Cargo.toml"},
            {"name": "serde", "version": "1.0.1", "manifest_path": "/reg/serde-1.0.1/Cargo.toml"}
        ]}"#,
    )
    .unwrap()
}

fn inputs(licenses: &str, by_hand_toml: &str) -> Inputs {
    Inputs {
        about: about_json(licenses),
        metadata: metadata_json(),
        by_hand: by_hand::parse(by_hand_toml).unwrap(),
        header: "# Notices for kiri {version}\n".to_string(),
        extra: "Text.\n@include licenses/x.txt\n".to_string(),
        syntax_notices: "SYNTAX\n".to_string(),
    }
}

fn make(inputs: &Inputs, files: &Fake) -> Result<String, String> {
    notices(inputs, files, Path::new("notices"))
}

#[test]
fn the_header_names_the_version_and_the_parts_follow_in_order() {
    let text = make(
        &inputs(
            &license("MIT License", "MIT text", &[("serde", "1.0.1")]),
            "",
        ),
        &fake(&[("notices/licenses/x.txt", "X licence")]),
    )
    .unwrap();
    assert!(text.starts_with("# Notices for kiri 0.3.0\n\n## Rust libraries"));
    let order: Vec<usize> = [
        "## Rust libraries",
        "## Syntax definitions",
        "## Other material",
    ]
    .iter()
    .map(|heading| text.find(heading).unwrap())
    .collect();
    assert!(order.is_sorted());
    assert!(text.contains("- serde 1.0.1, source at <https://crates.io/crates/serde/1.0.1>"));
    assert!(text.contains("````text\nMIT text\n````"));
    assert!(text.contains("Text.\n````text\nX licence\n````"));
}

#[test]
fn a_library_that_got_the_placeholder_stops_the_notices() {
    let json = license(
        "MIT License",
        "Copyright (c) <year> <copyright holders>",
        &[("libm", "0.2.16")],
    );
    let error = make(&inputs(&json, ""), &fake(&[])).unwrap_err();
    assert!(error.contains("libm"), "{error}");
    assert!(error.contains("by-hand.toml"), "{error}");
}

#[test]
fn an_unreadable_library_is_given_by_hand_and_left_out_of_the_list() {
    let json = [
        license(
            "MIT License",
            "Copyright (c) <year> <copyright holders>",
            &[("libm", "0.2.16")],
        ),
        license("MIT License", "Serde MIT", &[("serde", "1.0.1")]),
    ]
    .join(",");
    let by_hand = r#"
        [[unreadable]]
        title = "libm"
        note = "No readable file."
        crates = ["libm"]
        crate_files = ["LICENSE.txt"]
    "#;
    let text = make(
        &inputs(&json, by_hand),
        &fake(&[
            ("/reg/libm-0.2.16/LICENSE.txt", "libm by rust-lang"),
            ("notices/licenses/x.txt", "X"),
        ]),
    )
    .unwrap();
    assert!(!text.contains("<copyright holders>"));
    assert!(text.contains("### libm\n\nNo readable file.\n\nUsed by:\n\n- libm 0.2.16"));
    assert!(text.contains("`libm 0.2.16: LICENSE.txt`\n\n````text\nlibm by rust-lang\n````"));
    // Only the by-hand section names libm: the generic list does not.
    assert_eq!(text.matches("- libm 0.2.16").count(), 1);
}

#[test]
fn a_carried_library_stays_in_the_list_and_gets_its_directory_of_notices() {
    let json = license("MIT License", "Serde MIT", &[("serde", "1.0.1")]);
    let by_hand = r#"
        [[carried]]
        title = "serde"
        crates = ["serde"]
        crate_dirs = ["LICENSES"]
    "#;
    let text = make(
        &inputs(&json, by_hand),
        &fake(&[
            ("/reg/serde-1.0.1/LICENSES/b/COPYING", "B"),
            ("/reg/serde-1.0.1/LICENSES/a", "A"),
            ("/reg/serde-1.0.1/Cargo.toml", "not a notice"),
            ("notices/licenses/x.txt", "X"),
        ]),
    )
    .unwrap();
    assert_eq!(text.matches("- serde 1.0.1").count(), 2);
    assert!(!text.contains("not a notice"));
    let a = text.find("`serde 1.0.1: LICENSES/a`").unwrap();
    let b = text.find("`serde 1.0.1: LICENSES/b/COPYING`").unwrap();
    assert!(a < b);
}

#[test]
fn files_of_the_repository_are_read_from_the_notices_directory() {
    let json = license("MIT License", "Serde MIT", &[("serde", "1.0.1")]);
    let by_hand = r#"
        [[carried]]
        title = "serde"
        crates = ["serde"]
        files = ["licenses/serde.txt"]
    "#;
    let text = make(
        &inputs(&json, by_hand),
        &fake(&[
            ("notices/licenses/serde.txt", "Serde copy"),
            ("notices/licenses/x.txt", "X"),
        ]),
    )
    .unwrap();
    assert!(text.contains("`licenses/serde.txt`\n\n````text\nSerde copy\n````"));
}

#[test]
fn an_entry_for_a_library_that_is_not_used_any_more_is_an_error() {
    let json = license("MIT License", "Serde MIT", &[("serde", "1.0.1")]);
    let gone = r#"
        [[carried]]
        title = "gone"
        crates = ["gone"]
        files = ["licenses/x.txt"]
    "#;
    let error = make(
        &inputs(&json, gone),
        &fake(&[("notices/licenses/x.txt", "X")]),
    )
    .unwrap_err();
    assert!(
        error.contains("gone") && error.contains("does not use"),
        "{error}"
    );

    let unlisted = r#"
        [[unreadable]]
        title = "libm"
        crates = ["libm"]
        files = ["licenses/x.txt"]
    "#;
    let error = make(
        &inputs(&json, unlisted),
        &fake(&[("notices/licenses/x.txt", "X")]),
    )
    .unwrap_err();
    assert!(error.contains("does not list it"), "{error}");
}

#[test]
fn an_entry_without_files_is_an_error() {
    let json = license("MIT License", "Serde MIT", &[("serde", "1.0.1")]);
    let empty = r#"
        [[carried]]
        title = "serde"
        crates = ["serde"]
    "#;
    let error = make(&inputs(&json, empty), &fake(&[])).unwrap_err();
    assert!(error.contains("has no files"), "{error}");
}

#[test]
fn a_licence_text_with_a_four_backtick_fence_is_an_error() {
    let json = license("MIT License", "bad ```` text", &[("serde", "1.0.1")]);
    assert!(make(&inputs(&json, ""), &fake(&[])).is_err());
}

#[test]
fn a_missing_file_names_itself() {
    let error = make(
        &inputs(&license("MIT License", "t", &[("serde", "1.0.1")]), ""),
        &fake(&[]),
    )
    .unwrap_err();
    assert!(error.contains("licenses/x.txt"), "{error}");
}

fn mit_license(text: &str, used_by: &[(&str, &str)]) -> String {
    license("MIT License", text, used_by).replacen(r#"{"name""#, r#"{"id": "MIT", "name""#, 1)
}

const MIT_BODY: &str =
    "Permission is hereby granted, free of charge.\\nThe above copyright notice shall be included.";

#[test]
fn mit_texts_that_differ_in_their_copyright_lines_are_one_text_with_the_lines_listed() {
    let json = [
        mit_license(
            &format!("The MIT License\\n\\nCopyright (c) 2014 Ann\\nCopyright (c) 2015 Bob\\n\\n{MIT_BODY}"),
            &[("serde", "1.0.1")],
        ),
        mit_license(
            &format!("The MIT License\\n\\nCopyright 2020 Cy\\n\\n{MIT_BODY}"),
            &[("libm", "0.2.16")],
        ),
    ]
    .join(",");
    let text = make(
        &inputs(&json, ""),
        &fake(&[("notices/licenses/x.txt", "X")]),
    )
    .unwrap();
    assert_eq!(text.matches("Permission is hereby granted").count(), 1);
    assert!(text.contains(
        "- libm 0.2.16, source at <https://crates.io/crates/libm/0.2.16>\n  - Copyright 2020 Cy\n"
    ));
    assert!(text.contains(
        "- serde 1.0.1, source at <https://crates.io/crates/serde/1.0.1>\n  - Copyright (c) 2014 Ann\n  - Copyright (c) 2015 Bob\n"
    ));
    assert!(text.contains("The copyright notices above go with this licence text:"));
    let fence = text.split("````text\n").nth(1).unwrap();
    assert!(!fence.contains("Copyright (c)"), "{fence}");
}

#[test]
fn a_line_of_the_licence_that_starts_with_copyright_notice_is_kept() {
    let json = mit_license(
        "Copyright (c) 2014 Ann\\n\\nIncluded in all copies.\\nCopyright notice and this permission notice follow.",
        &[("serde", "1.0.1")],
    );
    let text = make(
        &inputs(&json, ""),
        &fake(&[("notices/licenses/x.txt", "X")]),
    )
    .unwrap();
    assert!(text.contains("Copyright notice and this permission notice follow."));
    assert!(text.contains("  - Copyright (c) 2014 Ann"));
}

#[test]
fn other_licence_texts_that_differ_only_in_white_space_are_one_text() {
    let json = [
        license("Zlib License", "Zlib  text\\nhere", &[("a", "1.0.0")]),
        license("Zlib License", "Zlib text\\n\\nhere\\n", &[("b", "2.0.0")]),
    ]
    .join(",");
    let text = make(
        &inputs(&json, ""),
        &fake(&[("notices/licenses/x.txt", "X")]),
    )
    .unwrap();
    assert_eq!(text.matches("### Zlib License").count(), 1);
    assert!(text.contains("- a 1.0.0") && text.contains("- b 2.0.0"));
}

#[test]
fn files_of_an_entry_with_the_same_text_are_given_once_under_all_their_names() {
    let json = license("MIT License", "Serde MIT", &[("serde", "1.0.1")]);
    let by_hand = r#"
        [[carried]]
        title = "serde"
        crates = ["serde"]
        crate_dirs = ["LICENSES"]
    "#;
    let text = make(
        &inputs(&json, by_hand),
        &fake(&[
            ("/reg/serde-1.0.1/LICENSES/a/LICENSE", "Same  text"),
            ("/reg/serde-1.0.1/LICENSES/b/LICENSE", "Same text\n"),
            ("/reg/serde-1.0.1/LICENSES/c/LICENSE", "Another"),
            ("notices/licenses/x.txt", "X"),
        ]),
    )
    .unwrap();
    assert!(text.contains(
        "`serde 1.0.1: LICENSES/a/LICENSE`, `serde 1.0.1: LICENSES/b/LICENSE`\n\n````text\nSame  text\n````"
    ));
    assert_eq!(text.matches("Same").count(), 1);
    assert!(text.contains("`serde 1.0.1: LICENSES/c/LICENSE`"));
}
