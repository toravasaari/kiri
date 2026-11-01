//! Loading the image at a URL: the local file a path or a `file:` URL
//! names, opened and read up to a size limit, and a `data:` URL decoded.
//!
//! What a URL means is worked out in `src/typeset/images/url.rs`, which does no I/O.

use std::io;
use std::path::Path;

use image::ImageFormat;

use crate::images::{Fonts, ImageError, Source};
use crate::session::state::Look;
use crate::typeset::images::decode::decode_named;
use crate::typeset::images::url::{data_url, file_url_path, local_path, read_at_most, scheme};

/// What was read of an image before it is decoded.
#[derive(Debug)]
pub(crate) struct Fetched {
    /// The bytes of the file or `data:` URL.
    pub(crate) bytes: Vec<u8>,
    /// The format its name or media type names, if one does.
    pub(crate) named: Option<ImageFormat>,
    /// How the file looked when it was read; `None` for a `data:` URL.
    pub(crate) look: Option<Look>,
}

/// Loads the image at `url`: a `data:` URL, a local `file:` URL, or a path
/// that is absolute or relative to `base`. Paths may use URL percent
/// escapes and end in a `?query` or `#fragment`, as in a browser. Only
/// regular files of at most `max_file_size` bytes are read.
pub(crate) fn load_url(
    url: &str,
    base: &Path,
    max_file_size: u64,
    fonts: &Fonts,
) -> Result<Source, ImageError> {
    let fetched = fetch_url(url, base, max_file_size)?;
    decode_named(&fetched.bytes, fetched.named, max_file_size, fonts)
}

/// Reads the image at `url` as [`load_url`] does before it decodes it: the
/// bytes of a `data:` URL, or of a local file, with how the file looked.
pub(crate) fn fetch_url(url: &str, base: &Path, max_file_size: u64) -> Result<Fetched, ImageError> {
    match scheme(url) {
        Some(scheme) if scheme.eq_ignore_ascii_case("data") => {
            let (bytes, named) = data_url(url)?;
            Ok(Fetched {
                bytes,
                named,
                look: None,
            })
        }
        Some(scheme)
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") =>
        {
            Err(ImageError::Remote)
        }
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
            let path = file_url_path(&url[scheme.len() + 1..]).ok_or(ImageError::Remote)?;
            read_file(&local_path(path, base)?, max_file_size)
        }
        Some(scheme) => Err(ImageError::UnsupportedScheme(scheme.to_ascii_lowercase())),
        None => read_file(&local_path(url, base)?, max_file_size),
    }
}

/// The contents of the regular file at `path`, with its format by its
/// name and how it looked. Directories, devices and pipes are refused
/// before they are opened: reading `/dev/zero` never ends, and opening a
/// pipe waits for a writer. So is a file larger than `limit` bytes,
/// without reading more of it than that.
fn read_file(path: &Path, limit: u64) -> Result<Fetched, ImageError> {
    let unreadable = |error: io::Error| match error.kind() {
        io::ErrorKind::NotFound => ImageError::Missing(path.to_owned()),
        _ => ImageError::Unreadable {
            path: path.to_owned(),
            reason: error.to_string(),
        },
    };
    let metadata = std::fs::metadata(path).map_err(unreadable)?;
    if !metadata.is_file() {
        return Err(ImageError::NotAFile(path.to_owned()));
    }
    let too_large = || ImageError::FileTooLarge {
        path: path.to_owned(),
        limit,
    };
    if metadata.len() > limit {
        return Err(too_large());
    }
    // The file may have grown since.
    let file = std::fs::File::open(path).map_err(unreadable)?;
    let bytes = read_at_most(file, limit)
        .map_err(unreadable)?
        .ok_or_else(too_large)?;
    Ok(Fetched {
        bytes,
        named: ImageFormat::from_path(path).ok(),
        look: Some(Look {
            modified: metadata.modified().ok(),
            length: metadata.len(),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    use crate::images::DEFAULT_MAX_FILE_SIZE;
    use crate::load::tests::{fixture, fixtures, open};

    #[test]
    fn loads_base64_data_urls() {
        let bytes = std::fs::read(fixtures().join("images/pattern.png")).unwrap();
        let url = format!("data:image/png;base64,{}", STANDARD.encode(&bytes));
        let loaded = open(&url, Path::new("/nonexistent"));
        assert_eq!(loaded, fixture("pattern.png"));
        assert!(loaded.is_ok());
    }

    #[test]
    fn loads_percent_encoded_data_urls() {
        let bytes = std::fs::read(fixtures().join("images/pattern.png")).unwrap();
        let escaped: String = bytes.iter().map(|byte| format!("%{byte:02X}")).collect();
        let loaded = open(&format!("data:image/png,{escaped}"), Path::new("/"));
        assert_eq!(loaded, fixture("pattern.png"));
        assert!(loaded.is_ok());
    }

    #[test]
    fn rejects_broken_data_urls() {
        let base = Path::new("/");
        assert_eq!(
            open("data:image/png;base64,no*base64", base),
            Err(ImageError::InvalidData)
        );
        assert_eq!(
            open("data:image/png;base64", base),
            Err(ImageError::InvalidData)
        );
    }

    /// `pattern.tga` as a `data:` URL with the media type `media_type`.
    fn tga_data_url(media_type: &str) -> String {
        let bytes = std::fs::read(fixtures().join("images/pattern.tga")).unwrap();
        format!("data:{media_type};base64,{}", STANDARD.encode(&bytes))
    }

    #[cfg(feature = "full")]
    #[test]
    fn recognises_tga_by_its_file_name_or_media_type() {
        // TGA files have no signature to recognise them by.
        let pattern = fixture("pattern.png");
        assert!(pattern.is_ok());
        let path = fixtures().join("images/pattern.tga");
        let file_url = format!("file://{}?v=2", path.to_str().unwrap());
        for url in ["pattern.tga", "pattern.tga#top", "pattern%2Etga", &file_url] {
            assert_eq!(fixture(url), pattern, "{url}");
        }
        let elsewhere = Path::new("/nonexistent");
        for media_type in ["image/x-tga", "image/x-targa", "IMAGE/X-TGA;charset=x"] {
            let url = tga_data_url(media_type);
            assert_eq!(open(&url, elsewhere), pattern, "{media_type}");
        }
    }

    #[cfg(feature = "full")]
    #[test]
    fn decodes_a_colour_mapped_tga() {
        // It starts with 00 01 01 00; an icon starts with 00 00 01 00.
        let bytes = std::fs::read(fixtures().join("images/pattern-palette.tga")).unwrap();
        assert_eq!(bytes[..4], [0, 1, 1, 0], "no longer colour-mapped");
        let pattern = fixture("pattern.png");
        assert!(pattern.is_ok());
        assert_eq!(fixture("pattern-palette.tga"), pattern);
    }

    #[test]
    fn does_not_take_other_data_for_tga() {
        let elsewhere = Path::new("/nonexistent");
        for media_type in ["", "image/png", "application/octet-stream"] {
            let url = tga_data_url(media_type);
            assert_eq!(
                open(&url, elsewhere),
                Err(ImageError::UnknownFormat),
                "{media_type}"
            );
        }
    }

    #[cfg(not(feature = "full"))]
    #[test]
    fn names_tga_when_the_build_leaves_it_out() {
        let tga = Err(ImageError::UnsupportedFormat("TGA".into()));
        assert_eq!(fixture("pattern.tga"), tga);
        assert_eq!(open(&tga_data_url("image/x-tga"), Path::new("/")), tga);
    }

    #[test]
    fn resolves_paths_against_the_base_and_keeps_absolute_ones() {
        let base = fixtures();
        assert!(open("images/pattern.png", &base).is_ok());
        let absolute = base.join("images/pattern.png");
        assert!(open(absolute.to_str().unwrap(), Path::new("/nonexistent")).is_ok());
    }

    #[test]
    fn decodes_percent_escapes_in_paths() {
        assert!(fixture("pattern%2Epng").is_ok());
        // An escaped slash separates directories too.
        assert!(fixture("pngsuite%2Fbasn0g16.png").is_ok());
    }

    #[test]
    fn leaves_the_query_and_fragment_out_of_paths() {
        let pattern = fixture("pattern.png");
        assert!(pattern.is_ok());
        for url in ["pattern.png?v=2", "pattern.png#top", "pattern.png?a=1#b?c"] {
            assert_eq!(fixture(url), pattern, "{url}");
        }
        let base = fixtures().join("images");
        assert_eq!(
            fixture("no-such.png?v=2"),
            Err(ImageError::Missing(base.join("no-such.png")))
        );
        // Escaped, they are part of the name, as in a browser.
        assert_eq!(
            fixture("pattern%3Fv=2.png"),
            Err(ImageError::Missing(base.join("pattern?v=2.png")))
        );
        assert_eq!(
            fixture("pattern%23top.png"),
            Err(ImageError::Missing(base.join("pattern#top.png")))
        );
    }

    #[test]
    fn loads_file_urls_as_local_files() {
        let pattern = fixture("pattern.png");
        assert!(pattern.is_ok());
        let path = fixtures().join("images/pattern.png");
        let path = path.to_str().unwrap();
        let elsewhere = Path::new("/nonexistent");
        for url in [
            format!("file://{path}"),
            format!("file://localhost{path}"),
            format!("FILE://LocalHost{path}"),
            // RFC 8089 allows leaving out the empty host.
            format!("file:{path}"),
            format!("file://{path}?v=2#top"),
            format!("file://{}", path.replace(".png", "%2Epng")),
        ] {
            assert_eq!(open(&url, elsewhere), pattern, "{url}");
        }
        assert_eq!(
            open("file:///no/such.png", elsewhere),
            Err(ImageError::Missing("/no/such.png".into()))
        );
    }

    #[test]
    fn reports_a_url_without_a_path() {
        for base in [Path::new(""), &fixtures()] {
            for url in [
                "file://",
                "file://?x",
                "file://#top",
                "file:",
                "file://localhost",
                "",
                "?v=2",
                "#top",
            ] {
                assert_eq!(
                    open(url, base),
                    Err(ImageError::NoPath),
                    "{url:?} in {base:?}"
                );
            }
        }
    }

    #[test]
    fn does_not_load_file_urls_from_other_hosts() {
        assert_eq!(
            open("file://server/share/a.png", Path::new("/")),
            Err(ImageError::Remote)
        );
    }

    #[test]
    fn reports_a_missing_file_with_its_resolved_path() {
        let base = fixtures().join("images");
        assert_eq!(
            open("no-such.png", &base),
            Err(ImageError::Missing(base.join("no-such.png")))
        );
    }

    #[test]
    fn reads_only_regular_files() {
        let images = fixtures().join("images");
        assert_eq!(
            fixture("pngsuite"),
            Err(ImageError::NotAFile(images.join("pngsuite")))
        );
        // A device that never ends.
        assert_eq!(
            open("/dev/zero", Path::new("/")),
            Err(ImageError::NotAFile("/dev/zero".into()))
        );
    }

    #[cfg(unix)]
    #[test]
    fn does_not_wait_on_a_named_pipe() {
        let dir = crate::testing::Scratch::new("url-fifo");
        let fifo = dir.join("image.png");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        // Opening a pipe that nobody writes to would block.
        let loaded = open("image.png", &dir);
        assert_eq!(loaded, Err(ImageError::NotAFile(fifo)));
    }

    #[test]
    fn refuses_a_file_above_the_size_limit() {
        let base = fixtures().join("images");
        let size = std::fs::metadata(base.join("pattern.png")).unwrap().len();
        let fonts = Fonts::none();
        assert!(load_url("pattern.png", &base, size, &fonts).is_ok());
        assert_eq!(
            load_url("pattern.png", &base, size - 1, &fonts),
            Err(ImageError::FileTooLarge {
                path: base.join("pattern.png"),
                limit: size - 1
            })
        );
    }

    #[test]
    fn does_not_fetch_remote_images() {
        let base = fixtures();
        assert_eq!(
            open("https://example.com/a.png", &base),
            Err(ImageError::Remote)
        );
        assert_eq!(
            open("HTTP://example.com/a.png", &base),
            Err(ImageError::Remote)
        );
        assert_eq!(
            open("ftp://example.com/a.png", &base),
            Err(ImageError::UnsupportedScheme("ftp".into()))
        );
    }

    #[test]
    fn fetching_a_file_gives_its_bytes_its_format_and_how_it_looked() {
        let scratch = crate::testing::Scratch::new("fetch-file");
        let bytes = std::fs::read(fixtures().join("images/pattern.png")).unwrap();
        std::fs::write(scratch.join("p.png"), &bytes).unwrap();
        let metadata = std::fs::metadata(scratch.join("p.png")).unwrap();
        let look = Look {
            modified: metadata.modified().ok(),
            length: bytes.len() as u64,
        };
        let file_url = format!("file://{}", scratch.join("p.png").to_str().unwrap());
        for url in ["p.png", "p.png?v=2", &file_url] {
            let fetched = fetch_url(url, &scratch, DEFAULT_MAX_FILE_SIZE).unwrap();
            assert_eq!(fetched.bytes, bytes, "{url}");
            assert_eq!(fetched.named, Some(ImageFormat::Png), "{url}");
            assert_eq!(fetched.look, Some(look), "{url}");
        }
    }

    #[test]
    fn fetching_a_data_url_gives_its_bytes_and_no_look() {
        let bytes = std::fs::read(fixtures().join("images/pattern.png")).unwrap();
        let url = format!("data:image/png;base64,{}", STANDARD.encode(&bytes));
        let fetched = fetch_url(&url, Path::new("/nonexistent"), DEFAULT_MAX_FILE_SIZE).unwrap();
        assert_eq!(fetched.bytes, bytes);
        assert_eq!(fetched.named, Some(ImageFormat::Png));
        assert_eq!(fetched.look, None);
    }

    #[test]
    fn fetching_what_cannot_be_read_fails_as_loading_does() {
        let base = Path::new("/nonexistent");
        let fetch = |url: &str| fetch_url(url, base, DEFAULT_MAX_FILE_SIZE).map(|_| ());
        assert_eq!(fetch("https://example.org/a.png"), Err(ImageError::Remote));
        assert_eq!(
            fetch("ftp://example.org/a.png"),
            Err(ImageError::UnsupportedScheme("ftp".to_owned()))
        );
        assert_eq!(fetch("data:image/png;base64"), Err(ImageError::InvalidData));
        assert_eq!(fetch("a.png"), Err(ImageError::Missing(base.join("a.png"))));
    }
}
