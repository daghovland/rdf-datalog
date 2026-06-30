use crate::sources::SourceRow;

/// Expand an RML template string, substituting `{key}` placeholders with row
/// values via `row.get_str(key)`. If `encode` is true (IRI term type),
/// substituted values are percent-encoded per RFC 3986 §2.1. Returns None if
/// any referenced key is absent or empty (the triple should be skipped).
pub fn expand_template(template: &str, row: &dyn SourceRow, encode: bool) -> Option<String> {
    let mut result = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        result.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        let close = after_open.find('}')?;
        let key = &after_open[..close];
        let value = row.get_str(key)?;
        if encode {
            result.push_str(&percent_encode(&value));
        } else {
            result.push_str(&value);
        }
        rest = &after_open[close + 1..];
    }
    result.push_str(rest);
    Some(result)
}

/// Return true if `s` begins with a valid absolute IRI scheme as defined by
/// RFC 3986 §3.1: `[a-zA-Z][a-zA-Z0-9+\-.]*:` followed by at least one
/// additional character.  Returns false for empty strings, strings with no
/// colon, or strings where no valid scheme prefix exists.
pub fn is_valid_iri_scheme(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let bytes = s.as_bytes();
    // First char must be ASCII alpha
    if !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    let mut i = 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b':' {
            // Scheme separator found; require at least one char after it
            return i + 1 < bytes.len();
        }
        if !matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'-' | b'.') {
            return false;
        }
        i += 1;
    }
    false // No ':' found
}

/// Percent-encode a string for use inside an IRI per RFC 3986 §2.1.
/// Unreserved characters (A-Za-z0-9 - . _ ~) pass through unchanged;
/// everything else is encoded as %XX.
pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push(
                    char::from_digit((byte >> 4) as u32, 16)
                        .unwrap()
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit((byte & 0xf) as u32, 16)
                        .unwrap()
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}
