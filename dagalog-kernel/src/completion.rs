//! Plain-text completion/inspection for SPARQL cells, used by
//! `complete_request`/`inspect_request` (see docs/plans/COMPLETION_INSPECT_PLAN.md).
//!
//! Keyword/function lists cover the **full SPARQL 1.1 grammar** — the engine
//! may not execute everything listed, but completion exposes what the language
//! should support so gaps become visible during tab-completion.

const KEYWORDS: &[&str] = &[
    // Query forms
    "SELECT",
    "CONSTRUCT",
    "ASK",
    "DESCRIBE",
    // Dataset
    "FROM",
    "NAMED",
    // Solution modifiers
    "WHERE",
    "DISTINCT",
    "REDUCED",
    "ORDER",
    "BY",
    "ASC",
    "DESC",
    "LIMIT",
    "OFFSET",
    "GROUP",
    "HAVING",
    // Graph patterns
    "FILTER",
    "OPTIONAL",
    "UNION",
    "MINUS",
    "GRAPH",
    "SERVICE",
    "SILENT",
    "BIND",
    "VALUES",
    "UNDEF",
    // Aggregates (grammar keywords)
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "SAMPLE",
    "GROUP_CONCAT",
    "SEPARATOR",
    // Expression keywords
    "NOT",
    "EXISTS",
    "IN",
    "AS",
    // Prefix / base
    "PREFIX",
    "BASE",
    // Literals
    "TRUE",
    "FALSE",
];

const FUNCTIONS: &[(&str, &str)] = &[
    // Term construction
    (
        "STR",
        "STR(x) — the lexical/string form of a literal or IRI.",
    ),
    (
        "IRI",
        "IRI(x) — construct an IRI from a string or expand a prefixed name.",
    ),
    ("URI", "URI(x) — alias for IRI(x)."),
    (
        "BNODE",
        "BNODE() / BNODE(x) — construct a blank node, optionally with a label string.",
    ),
    (
        "STRDT",
        "STRDT(str, datatype) — construct a typed literal from a string and a datatype IRI.",
    ),
    (
        "STRLANG",
        "STRLANG(str, lang) — construct a language-tagged literal.",
    ),
    // Language / datatype inspection
    (
        "LANG",
        "LANG(x) — the language tag of a literal, or \"\" if none.",
    ),
    (
        "LANGMATCHES",
        "LANGMATCHES(lang, pattern) — whether a language tag matches a language range.",
    ),
    ("DATATYPE", "DATATYPE(x) — the datatype IRI of a literal."),
    // Type testing
    (
        "BOUND",
        "BOUND(?var) — whether a variable is bound in the current solution.",
    ),
    ("ISIRI", "ISIRI(x) — whether the term is an IRI."),
    ("ISURI", "ISURI(x) — alias for ISIRI."),
    ("ISBLANK", "ISBLANK(x) — whether the term is a blank node."),
    ("ISLITERAL", "ISLITERAL(x) — whether the term is a literal."),
    (
        "ISNUMERIC",
        "ISNUMERIC(x) — whether the term has a numeric datatype.",
    ),
    (
        "SAMETERM",
        "SAMETERM(x, y) — whether two RDF terms are identical (same term, not just same value).",
    ),
    // String functions
    ("STRLEN", "STRLEN(x) — the length of a string literal."),
    (
        "SUBSTR",
        "SUBSTR(str, start[, len]) — a substring, 1-indexed.",
    ),
    ("UCASE", "UCASE(x) — uppercase a string literal."),
    ("LCASE", "LCASE(x) — lowercase a string literal."),
    (
        "STRSTARTS",
        "STRSTARTS(str, prefix) — whether a string starts with the given prefix.",
    ),
    (
        "STRENDS",
        "STRENDS(str, suffix) — whether a string ends with the given suffix.",
    ),
    (
        "CONTAINS",
        "CONTAINS(str, substr) — whether a string contains the given substring.",
    ),
    (
        "STRBEFORE",
        "STRBEFORE(str, substr) — the part of str before the first occurrence of substr.",
    ),
    (
        "STRAFTER",
        "STRAFTER(str, substr) — the part of str after the first occurrence of substr.",
    ),
    (
        "ENCODE_FOR_URI",
        "ENCODE_FOR_URI(str) — percent-encode a string for use in a URI.",
    ),
    ("CONCAT", "CONCAT(str, ...) — concatenate string literals."),
    (
        "REPLACE",
        "REPLACE(str, pattern, replacement[, flags]) — regex-replace within a string.",
    ),
    (
        "REGEX",
        "REGEX(text, pattern[, flags]) — whether text matches a regular expression.",
    ),
    // Numeric functions
    ("ABS", "ABS(x) — absolute value."),
    (
        "ROUND",
        "ROUND(x) — round to nearest integer (half-to-even).",
    ),
    ("CEIL", "CEIL(x) — ceiling (round up)."),
    ("FLOOR", "FLOOR(x) — floor (round down)."),
    ("RAND", "RAND() — a random double in [0, 1)."),
    // Date / time functions
    ("NOW", "NOW() — the current date/time as an xsd:dateTime."),
    ("YEAR", "YEAR(dateTime) — the year component."),
    ("MONTH", "MONTH(dateTime) — the month component (1-12)."),
    ("DAY", "DAY(dateTime) — the day-of-month component (1-31)."),
    ("HOURS", "HOURS(dateTime) — the hours component (0-23)."),
    (
        "MINUTES",
        "MINUTES(dateTime) — the minutes component (0-59).",
    ),
    (
        "SECONDS",
        "SECONDS(dateTime) — the seconds component (xsd:decimal).",
    ),
    (
        "TIMEZONE",
        "TIMEZONE(dateTime) — the timezone offset as an xsd:dayTimeDuration.",
    ),
    (
        "TZ",
        "TZ(dateTime) — the timezone as a plain string (e.g. \"+05:30\").",
    ),
    // Hash / UUID functions
    ("MD5", "MD5(str) — MD5 hash as a lowercase hex string."),
    ("SHA1", "SHA1(str) — SHA-1 hash as a lowercase hex string."),
    (
        "SHA256",
        "SHA256(str) — SHA-256 hash as a lowercase hex string.",
    ),
    (
        "SHA384",
        "SHA384(str) — SHA-384 hash as a lowercase hex string.",
    ),
    (
        "SHA512",
        "SHA512(str) — SHA-512 hash as a lowercase hex string.",
    ),
    ("UUID", "UUID() — a fresh IRI built from a UUID v4."),
    (
        "STRUUID",
        "STRUUID() — a fresh string (plain literal) that is a UUID v4.",
    ),
    // Logic / control
    (
        "COALESCE",
        "COALESCE(expr, ...) — the first expression that evaluates without error.",
    ),
    (
        "IF",
        "IF(cond, then, else) — return then if cond is true, else otherwise.",
    ),
];

/// Result of a `complete_request`: candidate matches plus the span of the
/// partial word they replace.
pub struct Completion {
    pub matches: Vec<String>,
    pub cursor_start: usize,
    pub cursor_end: usize,
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Index of the first char of the word that ends at `cursor_pos`.
fn word_start(chars: &[char], cursor_pos: usize) -> usize {
    let pos = cursor_pos.min(chars.len());
    let mut start = pos;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    start
}

/// [start, end) span of the whole word that contains `cursor_pos`.
fn word_span(chars: &[char], cursor_pos: usize) -> (usize, usize) {
    let start = word_start(chars, cursor_pos);
    let pos = cursor_pos.min(chars.len());
    let mut end = pos;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }
    (start, end)
}

/// Names of all `PREFIX name:` declarations that appear in `code`.
fn declared_prefixes(code: &str) -> Vec<String> {
    let mut prefixes = Vec::new();
    for line in code.lines() {
        let trimmed = line.trim();
        if trimmed.len() >= 6 && trimmed[..6].eq_ignore_ascii_case("PREFIX") {
            let rest = trimmed[6..].trim_start();
            if let Some(colon) = rest.find(':') {
                let name = rest[..colon].trim().to_string();
                if !name.is_empty() {
                    prefixes.push(name);
                }
            }
        }
    }
    prefixes
}

/// Find the partial token ending at `cursor_pos` in `code` and return
/// matching keywords, builtin functions, and prefixes already declared
/// earlier in `code`.
pub fn complete(code: &str, cursor_pos: usize) -> Completion {
    let chars: Vec<char> = code.chars().collect();
    let start = word_start(&chars, cursor_pos);
    let end = cursor_pos.min(chars.len());

    if start == end {
        return Completion {
            matches: Vec::new(),
            cursor_start: cursor_pos,
            cursor_end: cursor_pos,
        };
    }

    let partial: String = chars[start..end].iter().collect();
    let partial_upper = partial.to_ascii_uppercase();

    let mut matches: Vec<String> = Vec::new();

    for &kw in KEYWORDS {
        if kw.starts_with(partial_upper.as_str()) {
            matches.push(kw.to_string());
        }
    }
    for &(name, _) in FUNCTIONS {
        if name.starts_with(partial_upper.as_str()) && !matches.contains(&name.to_string()) {
            matches.push(name.to_string());
        }
    }
    for prefix in declared_prefixes(code) {
        let prefix_upper = prefix.to_ascii_uppercase();
        if prefix_upper.starts_with(partial_upper.as_str()) && !matches.contains(&prefix) {
            matches.push(prefix);
        }
    }

    Completion {
        matches,
        cursor_start: start,
        cursor_end: end,
    }
}

/// Find the identifier at `cursor_pos` in `code` and return a short doc
/// string for it, if it names a recognized builtin function.
pub fn inspect(code: &str, cursor_pos: usize) -> Option<String> {
    let chars: Vec<char> = code.chars().collect();
    let (start, end) = word_span(&chars, cursor_pos);
    if start == end {
        return None;
    }
    let word: String = chars[start..end].iter().collect();
    let word_upper = word.to_ascii_uppercase();

    FUNCTIONS
        .iter()
        .find(|&&(name, _)| name == word_upper)
        .map(|&(_, doc)| doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complete_no_partial_word_returns_empty() {
        let c = complete("SELECT ", 7);
        assert!(c.matches.is_empty());
    }

    #[test]
    fn test_complete_keyword_prefix() {
        let c = complete("SEL", 3);
        assert!(c.matches.contains(&"SELECT".to_string()));
        assert_eq!(c.cursor_start, 0);
        assert_eq!(c.cursor_end, 3);
    }

    #[test]
    fn test_complete_function_prefix() {
        let code = "SELECT * WHERE { FILTER(reg(?x)) }";
        let cursor = code.find("reg").unwrap() + 3;
        let c = complete(code, cursor);
        assert!(c.matches.contains(&"REGEX".to_string()));
    }

    #[test]
    fn test_complete_declared_prefix() {
        let code = "PREFIX foaf: <http://xmlns.com/foaf/0.1/>\nfo";
        let c = complete(code, code.len());
        assert!(c.matches.contains(&"foaf".to_string()));
    }

    #[test]
    fn test_inspect_known_function_returns_doc() {
        let doc = inspect("REGEX", 2).expect("REGEX should be recognized");
        assert!(doc.contains("regular expression"));
    }

    #[test]
    fn test_inspect_unknown_word_returns_none() {
        assert_eq!(inspect("Alice", 2), None);
    }
}
