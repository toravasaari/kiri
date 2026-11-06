//! The licences of the syntax definitions that kiri bundles.
//!
//! The definitions are two-face's, and two-face keeps their licences as data
//! that only its `acknowledgement::listing()` reads.

use std::collections::BTreeMap;
use std::fmt::Write;

use two_face::acknowledgement::License;

/// The licence texts that the syntax definitions ask to be shipped, each
/// under the definitions it covers. Many definitions share one text, so the
/// text is given once.
pub fn notices(licenses: &[License]) -> String {
    let mut paths_by_text: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for license in licenses.iter().filter(|l| l.needs_acknowledgement()) {
        paths_by_text
            .entry(license.text.as_str())
            .or_default()
            .push(license.rel_path.display().to_string());
    }

    let mut out = String::new();
    for (text, paths) in &mut paths_by_text {
        paths.sort();
        paths.dedup();
        out.push_str("Covers the syntax definitions in:\n\n");
        for path in paths.iter() {
            writeln!(out, "- `{path}`").expect("writing to a String cannot fail");
        }
        // Four backticks, so that a licence text with a fence in it stays whole.
        writeln!(out, "\n````text\n{}\n````\n", text.trim_end())
            .expect("writing to a String cannot fail");
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use two_face::acknowledgement::{LicenseType, listing};

    use super::*;

    fn license(ty: LicenseType, text: &str, path: &str) -> License {
        License {
            ty,
            text: text.to_string(),
            rel_path: PathBuf::from(path),
        }
    }

    #[test]
    fn a_text_shared_by_several_definitions_is_given_once() {
        let licenses = [
            license(LicenseType::Mit, "The MIT text", "b/LICENSE"),
            license(LicenseType::Mit, "The MIT text", "a/LICENSE"),
        ];
        let text = notices(&licenses);
        assert_eq!(text.matches("The MIT text").count(), 1);
        assert!(text.find("`a/LICENSE`") < text.find("`b/LICENSE`"));
    }

    #[test]
    fn a_licence_that_asks_for_no_notice_is_left_out() {
        let licenses = [license(
            LicenseType::Unlicense,
            "Public domain",
            "u/LICENSE",
        )];
        assert_eq!(notices(&licenses), "");
    }

    #[test]
    fn the_bundled_licences_are_given_as_markdown() {
        let text = notices(listing().for_syntaxes());
        assert!(text.contains("````text"));
        assert!(text.contains("Permission is hereby granted"));
        assert!(!text.contains("<details>"));
    }
}
