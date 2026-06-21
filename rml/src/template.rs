use crate::sources::RawRow;

/// Expand an RML template string, substituting `{column}` placeholders with
/// row values. If `encode` is true (IRI term type), substituted values are
/// percent-encoded per RFC 3986 §2.1. If any referenced column is absent or
/// empty, returns None (the triple should be skipped).
pub fn expand_template(template: &str, row: &RawRow, encode: bool) -> Option<String> {
    let mut result = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        result.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        let close = after_open.find('}')?;
        let col = &after_open[..close];
        let value = row.get(col)?;
        if value.is_empty() {
            return None;
        }
        if encode {
            result.push_str(&percent_encode(value));
        } else {
            result.push_str(value);
        }
        rest = &after_open[close + 1..];
    }
    result.push_str(rest);
    Some(result)
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
