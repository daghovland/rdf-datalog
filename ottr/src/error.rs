#[derive(Debug, thiserror::Error)]
pub enum OttrError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Unknown template: {0}")]
    UnknownTemplate(String),
    #[error("Template {template} called with {got} arguments, expected {expected}")]
    ArityMismatch {
        template: String,
        got: usize,
        expected: usize,
    },
    #[error("Recursive template definition: {0}")]
    RecursiveTemplate(String),
    #[error("Unbound variable: ?{0}")]
    UnboundVariable(String),
}
