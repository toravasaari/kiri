//! Why an image could not be loaded, in words for a warning.

use std::fmt;
use std::path::PathBuf;

use crate::typeset::diagrams::error::DiagramError;
use crate::typeset::images::decode::MB;

/// Why an image could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// No file at this path.
    Missing(PathBuf),
    /// The file exists but reading it failed.
    Unreadable { path: PathBuf, reason: String },
    /// A directory, device, pipe or anything else that is not a regular
    /// file.
    NotAFile(PathBuf),
    /// The file is larger than the limit, in bytes.
    FileTooLarge { path: PathBuf, limit: u64 },
    /// A compressed SVG that grows past the limit, in bytes, when it is
    /// decompressed.
    DecompressedTooLarge { limit: u64 },
    /// The URL is empty or only a query or fragment, or a `file:` URL
    /// without a path.
    NoPath,
    /// `http:` and `https:` images are not fetched.
    Remote,
    /// A URL scheme other than `http`, `https`, `data` or `file`.
    UnsupportedScheme(String),
    /// A `data:` URL without a comma or with broken base64.
    InvalidData,
    /// The bytes are not in any image format kiri knows.
    UnknownFormat,
    /// A known image format that this build cannot decode, by name.
    UnsupportedFormat(String),
    /// The data ends before the image does.
    Truncated,
    /// Too many pixels to decode safely.
    TooLarge { width: u32, height: u32 },
    /// The decoder rejected the data.
    Corrupt(String),
    /// A diagram that could not be drawn: how warnings name it, and why.
    Diagram { name: String, reason: DiagramError },
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageError::Missing(path) => write!(f, "{}: file not found", path.display()),
            ImageError::Unreadable { path, reason } => write!(f, "{}: {reason}", path.display()),
            ImageError::NotAFile(path) => write!(f, "{}: not a regular file", path.display()),
            ImageError::FileTooLarge { path, limit } => {
                write!(
                    f,
                    "{}: larger than the {} limit; see --max-image-size",
                    path.display(),
                    size(*limit)
                )
            }
            ImageError::DecompressedTooLarge { limit } => {
                write!(
                    f,
                    "decompresses to more than the {} limit; see --max-image-size",
                    size(*limit)
                )
            }
            ImageError::NoPath => f.write_str("the URL names no file"),
            ImageError::Remote => f.write_str("remote images are not loaded"),
            ImageError::UnsupportedScheme(scheme) => write!(f, "{scheme}: URLs are not loaded"),
            ImageError::InvalidData => f.write_str("invalid data: URL"),
            ImageError::UnknownFormat => f.write_str("not an image format kiri knows"),
            ImageError::UnsupportedFormat(format) => {
                write!(f, "{format} images are not supported in this build")
            }
            ImageError::Truncated => f.write_str("the image data is cut short"),
            ImageError::TooLarge { width, height } => {
                write!(f, "{width} × {height} pixels is too large to load")
            }
            ImageError::Corrupt(reason) => write!(f, "broken image: {reason}"),
            ImageError::Diagram { name, reason } => write!(f, "{name}: {reason}"),
        }
    }
}

impl ImageError {
    /// Whether the message names the file or the diagram, so that it
    /// needs no other name in front.
    pub fn names_file(&self) -> bool {
        matches!(
            self,
            ImageError::Missing(_)
                | ImageError::Unreadable { .. }
                | ImageError::NotAFile(_)
                | ImageError::FileTooLarge { .. }
                | ImageError::Diagram { .. }
        )
    }
}

impl std::error::Error for ImageError {}

/// A size limit in bytes for people, to go before a noun: whole megabytes
/// when it is that, as in "the 10 MB limit", and otherwise "the 1500-byte
/// limit".
pub(crate) fn size(bytes: u64) -> String {
    if bytes >= MB && bytes.is_multiple_of(MB) {
        format!("{} MB", bytes / MB)
    } else {
        format!("{bytes}-byte")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_errors_in_words() {
        let messages = [
            ImageError::Missing("a/b.png".into()).to_string(),
            ImageError::Remote.to_string(),
            ImageError::NotAFile("/dev/zero".into()).to_string(),
            ImageError::FileTooLarge {
                path: "big.png".into(),
                limit: 10 * MB,
            }
            .to_string(),
            ImageError::FileTooLarge {
                path: "big.png".into(),
                limit: 1500,
            }
            .to_string(),
            ImageError::DecompressedTooLarge { limit: 10 * MB }.to_string(),
            ImageError::TooLarge {
                width: 20000,
                height: 20000,
            }
            .to_string(),
        ];
        assert_eq!(
            messages,
            [
                "a/b.png: file not found",
                "remote images are not loaded",
                "/dev/zero: not a regular file",
                "big.png: larger than the 10 MB limit; see --max-image-size",
                "big.png: larger than the 1500-byte limit; see --max-image-size",
                "decompresses to more than the 10 MB limit; see --max-image-size",
                "20000 × 20000 pixels is too large to load",
            ]
        );
    }
}
