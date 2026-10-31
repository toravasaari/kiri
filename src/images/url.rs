//! What an image URL means: the bytes in a `data:` URL, the local file that a
//! `file:` URL or a path names, and its scheme. It does no I/O; reading the
//! file is `src/load/url.rs`.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use image::ImageFormat;

use crate::images::ImageError;

/// Standard base64 that does not insist on the trailing `=`.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// The local file that `url` names, a path or a `file:` URL, as `load_url`
/// reads it; `None` for any other URL.
pub(super) fn local_file(url: &str, base: &Path) -> Option<PathBuf> {
    match scheme(url) {
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
            let path = file_url_path(&url[scheme.len() + 1..])?;
            local_path(path, base).ok()
        }
        Some(_) => None,
        None => local_path(url, base).ok(),
    }
}

/// The path of a `file:` URL, given what follows `file:`, or `None` when
/// it names another host. The host may be left out, empty or `localhost`.
/// Without `//` and a host, the path may be relative too: `file:a.png`
/// is resolved against the document, like a plain path.
pub(crate) fn file_url_path(rest: &str) -> Option<&str> {
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
pub(crate) fn local_path(url_path: &str, base: &Path) -> Result<PathBuf, ImageError> {
    let url_path = url_path.split(['?', '#']).next().unwrap_or(url_path);
    if url_path.is_empty() {
        return Err(ImageError::NoPath);
    }
    let decoded = percent_decode(url_path.as_bytes());
    Ok(base.join(String::from_utf8(decoded).as_deref().unwrap_or(url_path)))
}

/// Everything `reader` gives, or `None` when that is more than `limit`
/// bytes; then it reads no further than one byte past the limit.
pub(crate) fn read_at_most(reader: impl Read, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= limit).then_some(bytes))
}

/// The URL's scheme, such as `https`, or `None` for a path. A single letter
/// is a Windows drive, not a scheme.
pub(crate) fn scheme(url: &str) -> Option<&str> {
    let (scheme, _) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    (valid && scheme.len() > 1).then_some(scheme)
}

/// The bytes in a `data:[<media type>][;base64],<data>` URL, and the
/// format its media type names.
pub(crate) fn data_url(url: &str) -> Result<(Vec<u8>, Option<ImageFormat>), ImageError> {
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
}
