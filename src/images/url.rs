//! Where an image URL leads: `data:` URLs, and local files named by
//! `file:` URLs or paths, read up to a size limit.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use image::ImageFormat;

use crate::images::{Fonts, ImageError, Source, decode_named};

/// Standard base64 that does not insist on the trailing `=`.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// Loads the image at `url`: a `data:` URL, a local `file:` URL, or a path
/// that is absolute or relative to `base`. Paths may use URL percent
/// escapes and end in a `?query` or `#fragment`, as in a browser. Only
/// regular files of at most `max_file_size` bytes are read.
pub(super) fn load_url(
    url: &str,
    base: &Path,
    max_file_size: u64,
    fonts: &Fonts,
) -> Result<Source, ImageError> {
    match scheme(url) {
        Some(scheme) if scheme.eq_ignore_ascii_case("data") => {
            let (bytes, named) = data_url(url)?;
            decode_named(&bytes, named, max_file_size, fonts)
        }
        Some(scheme)
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") =>
        {
            Err(ImageError::Remote)
        }
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
            let path = file_url_path(&url[scheme.len() + 1..]).ok_or(ImageError::Remote)?;
            decode_file(&local_path(path, base)?, max_file_size, fonts)
        }
        Some(scheme) => Err(ImageError::UnsupportedScheme(scheme.to_ascii_lowercase())),
        None => decode_file(&local_path(url, base)?, max_file_size, fonts),
    }
}

/// The image in the file at `path`, whose extension names its format.
fn decode_file(path: &Path, max_file_size: u64, fonts: &Fonts) -> Result<Source, ImageError> {
    let bytes = read_file(path, max_file_size)?;
    let named = ImageFormat::from_path(path).ok();
    decode_named(&bytes, named, max_file_size, fonts)
}

/// The path of a `file:` URL, given what follows `file:`, or `None` when
/// it names another host. The host may be left out, empty or `localhost`.
/// Without `//` and a host, the path may be relative too: `file:a.png`
/// is resolved against the document, like a plain path.
fn file_url_path(rest: &str) -> Option<&str> {
    let Some(authority_and_path) = rest.strip_prefix("//") else {
        return Some(rest);
    };
    let (host, path) = match authority_and_path.find(['/', '?', '#']) {
        Some(end) => authority_and_path.split_at(end),
        None => (authority_and_path, ""),
    };
    (host.is_empty() || host.eq_ignore_ascii_case("localhost")).then_some(path)
}

/// The file a path in a URL names: without a query or fragment, with
/// percent escapes decoded, and relative to `base` unless it is absolute.
/// Escapes are decoded before the path is split, so `%2F` separates
/// directories like `/` does.
fn local_path(url_path: &str, base: &Path) -> Result<PathBuf, ImageError> {
    let url_path = url_path.split(['?', '#']).next().unwrap_or(url_path);
    if url_path.is_empty() {
        return Err(ImageError::NoPath);
    }
    let decoded = percent_decode(url_path.as_bytes());
    Ok(base.join(String::from_utf8(decoded).as_deref().unwrap_or(url_path)))
}

/// The contents of the regular file at `path`. Directories, devices and
/// pipes are refused before they are opened: reading `/dev/zero` never
/// ends, and opening a pipe waits for a writer. So is a file larger than
/// `limit` bytes, without reading more of it than that.
fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>, ImageError> {
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
    read_at_most(file, limit)
        .map_err(unreadable)?
        .ok_or_else(too_large)
}

/// Everything `reader` gives, or `None` when that is more than `limit`
/// bytes; then it reads no further than one byte past the limit.
pub(super) fn read_at_most(reader: impl Read, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= limit).then_some(bytes))
}

/// The URL's scheme, such as `https`, or `None` for a path. A single letter
/// is a Windows drive, not a scheme.
pub(super) fn scheme(url: &str) -> Option<&str> {
    let (scheme, _) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    (valid && scheme.len() > 1).then_some(scheme)
}

/// The bytes in a `data:[<media type>][;base64],<data>` URL, and the
/// format its media type names.
fn data_url(url: &str) -> Result<(Vec<u8>, Option<ImageFormat>), ImageError> {
    let (header, data) = url.split_once(',').ok_or(ImageError::InvalidData)?;
    let media_type = header["data:".len()..].split(';').next().unwrap_or("");
    let named = ImageFormat::from_mime_type(media_type.trim().to_ascii_lowercase());
    let data = percent_decode(data.as_bytes());
    let is_base64 = header
        .rsplit(';')
        .next()
        .is_some_and(|part| part.trim().eq_ignore_ascii_case("base64"));
    if !is_base64 {
        return Ok((data, named));
    }
    let data: Vec<u8> = data
        .into_iter()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let data = BASE64.decode(data).map_err(|_| ImageError::InvalidData)?;
    Ok((data, named))
}

/// Turns `%XX` escapes into bytes and leaves everything else as it is.
fn percent_decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let escaped = (input[i] == b'%')
            .then(|| input.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(input[i]);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    use crate::images::tests::{fixture, fixtures, open};

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
    fn reads_no_more_than_one_byte_past_the_limit() {
        assert_eq!(
            read_at_most(&b"12345"[..], 5).unwrap(),
            Some(b"12345".to_vec())
        );
        assert_eq!(read_at_most(&b"123456"[..], 5).unwrap(), None);
        // An endless stream ends at the limit.
        let mut counted = Counted(io::repeat(0), 0);
        assert_eq!(read_at_most(&mut counted, 1000).unwrap(), None);
        assert_eq!(counted.1, 1001);
    }

    /// A reader that counts the bytes read through it.
    struct Counted<R>(R, u64);

    impl<R: Read> Read for Counted<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.0.read(buf)?;
            self.1 += n as u64;
            Ok(n)
        }
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
}
