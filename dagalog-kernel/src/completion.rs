//! Plain-text completion/inspection for SPARQL cells, used by
//! `complete_request`/`inspect_request` (see docs/plans/COMPLETION_INSPECT_PLAN.md).

#[allow(dead_code)]
const KEYWORDS: &[&str] = &[
    "SELECT",
    "CONSTRUCT",
    "ASK",
    "DESCRIBE",
    "WHERE",
    "FROM",
    "FILTER",
    "OPTIONAL",
    "UNION",
    "MINUS",
    "GRAPH",
    "BIND",
    "VALUES",
    "GROUP",
    "BY",
    "HAVING",
    "ORDER",
    "ASC",
    "DESC",
    "LIMIT",
    "OFFSET",
    "DISTINCT",
    "PREFIX",
    "AS",
    "EXISTS",
    "NOT",
    "SEPARATOR",
    "UNDEF",
    "TRUE",
    "FALSE",
];

#[allow(dead_code)]
const FUNCTIONS: &[(&str, &str)] = &[
    (
        "STR",
        "STR(x) — the lexical/string form of a literal or IRI.",
    ),
    (
        "LANG",
        "LANG(x) — the language tag of a literal, or \"\" if none.",
    ),
    (
        "LANGMATCHES",
        "LANGMATCHES(lang, pattern) — whether a language tag matches a language range.",
    ),
    ("DATATYPE", "DATATYPE(x) — the datatype IRI of a literal."),
    (
        "BOUND",
        "BOUND(?var) — whether a variable is bound in the current solution.",
    ),
    ("ISIRI", "ISIRI(x) — whether the term is an IRI."),
    ("ISURI", "ISURI(x) — alias for ISIRI."),
    ("ISBLANK", "ISBLANK(x) — whether the term is a blank node."),
    ("ISLITERAL", "ISLITERAL(x) — whether the term is a literal."),
    ("STRLEN", "STRLEN(x) — the length of a string literal."),
    (
        "REGEX",
        "REGEX(text, pattern[, flags]) — whether text matches a regular expression.",
    ),
];

/// Result of a `complete_request`: candidate matches plus the span of the
/// partial word they replace.
pub struct Completion {
    pub matches: Vec<String>,
    pub cursor_start: usize,
    pub cursor_end: usize,
}

/// Find the partial token ending at `cursor_pos` in `code` and return
/// matching keywords, builtin functions, and prefixes already declared
/// earlier in `code`.
pub fn complete(_code: &str, _cursor_pos: usize) -> Completion {
    unimplemented!("see docs/plans/COMPLETION_INSPECT_PLAN.md")
}

/// Find the identifier at `cursor_pos` in `code` and return a short doc
/// string for it, if it names a recognized builtin function.
pub fn inspect(_code: &str, _cursor_pos: usize) -> Option<String> {
    unimplemented!("see docs/plans/COMPLETION_INSPECT_PLAN.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn test_complete_no_partial_word_returns_empty() {
        let c = complete("SELECT ", 7);
        assert!(c.matches.is_empty());
    }

    #[test]
    #[ignore]
    fn test_complete_keyword_prefix() {
        let c = complete("SEL", 3);
        assert!(c.matches.contains(&"SELECT".to_string()));
        assert_eq!(c.cursor_start, 0);
        assert_eq!(c.cursor_end, 3);
    }

    #[test]
    #[ignore]
    fn test_complete_function_prefix() {
        let code = "SELECT * WHERE { FILTER(reg(?x)) }";
        let cursor = code.find("reg").unwrap() + 3;
        let c = complete(code, cursor);
        assert!(c.matches.contains(&"REGEX".to_string()));
    }

    #[test]
    #[ignore]
    fn test_complete_declared_prefix() {
        let code = "PREFIX foaf: <http://xmlns.com/foaf/0.1/>\nfo";
        let c = complete(code, code.len());
        assert!(c.matches.contains(&"foaf".to_string()));
    }

    #[test]
    #[ignore]
    fn test_inspect_known_function_returns_doc() {
        let doc = inspect("REGEX", 2).expect("REGEX should be recognized");
        assert!(doc.contains("regular expression"));
    }

    #[test]
    #[ignore]
    fn test_inspect_unknown_word_returns_none() {
        assert_eq!(inspect("Alice", 2), None);
    }
}
