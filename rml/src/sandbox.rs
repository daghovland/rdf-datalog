use std::path::{Path, PathBuf};

use crate::RmlError;

/// Resolve `untrusted` relative to `base_dir` and verify the canonical result
/// stays within `base_dir`. Rejects absolute paths and `..`-escapes.
///
/// See [#84](https://github.com/daghovland/rdf-datalog/issues/84) and
/// [#85](https://github.com/daghovland/rdf-datalog/issues/85).
pub fn confine_path(base_dir: &Path, untrusted: &Path) -> Result<PathBuf, RmlError> {
    let canonical_base = base_dir.canonicalize()?;
    if untrusted.is_absolute() {
        return Err(RmlError::PathTraversal {
            path: untrusted.to_path_buf(),
            base: canonical_base,
        });
    }
    let joined = canonical_base.join(untrusted);
    let canonical = joined.canonicalize()?;
    if !canonical.starts_with(&canonical_base) {
        return Err(RmlError::PathTraversal {
            path: canonical,
            base: canonical_base,
        });
    }
    Ok(canonical)
}
