//! Path-traversal security tests for Jupyter cell magic path arguments.
//!
//! Covered issue:
//! - [#85](https://github.com/daghovland/rdf-datalog/issues/85) Path traversal in %%rml / %%load / %%validate / %%ottr

mod support;

use serial_test::serial;
use support::KernelHarness;

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("dagalog-kernel has a parent directory")
        .to_path_buf()
}

// ── Unit tests for the check_path_safe helper ────────────────────────────────

/// An absolute path (e.g. `/etc/passwd`) must be rejected.
///
/// See [#85](https://github.com/daghovland/rdf-datalog/issues/85).
#[test]
fn absolute_path_is_rejected() {
    use dagalog_kernel::cell::check_path_safe;
    use std::path::Path;

    let result = check_path_safe(Path::new("/etc/passwd"));
    assert!(result.is_err(), "absolute path must be rejected");
}

/// A path with `..` traversal components (e.g. `../../.ssh/id_rsa`) must be
/// rejected.
///
/// See [#85](https://github.com/daghovland/rdf-datalog/issues/85).
#[test]
fn traversal_path_is_rejected() {
    use dagalog_kernel::cell::check_path_safe;
    use std::path::Path;

    let cases = [
        "../../.ssh/id_rsa",
        "../sibling",
        "data/../../../etc/passwd",
        "..",
    ];
    for case in cases {
        let result = check_path_safe(Path::new(case));
        assert!(result.is_err(), "traversal path {case:?} must be rejected");
    }
}

/// A well-formed relative path that stays within the working directory must
/// pass the check.
///
/// Note: whether the file *exists* is irrelevant — the check only validates
/// the path structure.
///
/// See [#85](https://github.com/daghovland/rdf-datalog/issues/85).
#[test]
fn normal_relative_path_is_accepted() {
    use dagalog_kernel::cell::check_path_safe;
    use std::path::Path;

    let cases = [
        "data/people.ttl",
        "mapping.ttl",
        "subdir/nested/file.rq",
        "shapes/person_shape.ttl",
    ];
    for case in cases {
        let result = check_path_safe(Path::new(case));
        assert!(
            result.is_ok(),
            "safe relative path {case:?} must be accepted, got: {:?}",
            result
        );
    }
}

/// The error message returned for a rejected path must NOT include the full
/// absolute path (neither the input path nor the resolved base directory).
/// This prevents leaking filesystem layout — see issue #90.
///
/// See [#85](https://github.com/daghovland/rdf-datalog/issues/85).
#[test]
fn rejection_error_does_not_include_absolute_path() {
    use dagalog_kernel::cell::check_path_safe;
    use std::path::Path;

    let abs_err =
        check_path_safe(Path::new("/etc/passwd")).expect_err("absolute path must be rejected");
    let trav_err =
        check_path_safe(Path::new("../../secret")).expect_err("traversal path must be rejected");

    // The process cwd (the notebook's working directory) must not appear in
    // error messages — it is an absolute path the user should not see.
    let cwd = std::env::current_dir().expect("must have a cwd");
    let cwd_str = cwd.to_string_lossy();

    assert!(
        !abs_err.contains(cwd_str.as_ref()),
        "absolute-path rejection must not contain cwd in message: {abs_err}"
    );
    assert!(
        !trav_err.contains(cwd_str.as_ref()),
        "traversal rejection must not contain cwd in message: {trav_err}"
    );

    // The sensitive input path component must not be echoed back.
    assert!(
        !abs_err.contains("/etc"),
        "error must not echo the input path: {abs_err}"
    );
}

// ── Integration tests via KernelHarness (test dispatch_cell, not just the helper) ─

/// `%%rml /etc/passwd` must be rejected *before* the RML handler opens any
/// file — the cell must return status "error".
///
/// See [#85](https://github.com/daghovland/rdf-datalog/issues/85).
#[tokio::test]
#[serial]
async fn rml_cell_with_absolute_path_is_rejected() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    let outcome = kernel.execute("%%rml /etc/passwd").await;
    assert_eq!(
        outcome.status, "error",
        "absolute path in %%rml must produce error status"
    );
    kernel.shutdown().await;
}

/// `%%load ../../.ssh/id_rsa` must be rejected with status "error".
///
/// See [#85](https://github.com/daghovland/rdf-datalog/issues/85).
#[tokio::test]
#[serial]
async fn load_cell_with_traversal_path_is_rejected() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    let outcome = kernel.execute("%%load ../../.ssh/id_rsa").await;
    assert_eq!(
        outcome.status, "error",
        "traversal path in %%load must produce error status"
    );
    kernel.shutdown().await;
}
