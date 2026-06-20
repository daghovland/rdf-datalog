use crate::sources::RawRow;

/// Expand an RML template string, substituting `{column}` placeholders with
/// row values. If `encode` is true (IRI term type), substituted values are
/// percent-encoded per RFC 3986 §2.1. If any referenced column is absent or
/// empty, returns None (the triple should be skipped).
pub fn expand_template(_template: &str, _row: &RawRow, _encode: bool) -> Option<String> {
    todo!()
}

/// Percent-encode a string for use inside an IRI per RFC 3986 §2.1.
/// Unreserved characters (A-Za-z0-9 - . _ ~) pass through unchanged;
/// everything else is encoded as %XX.
pub fn percent_encode(_value: &str) -> String {
    todo!()
}
