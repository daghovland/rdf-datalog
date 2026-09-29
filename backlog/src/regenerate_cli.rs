/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Pure argument parsing for the `backlog-regenerate` binary's `--out`
//! flag, split out of `src/bin/regenerate.rs` so it's unit-testable
//! without `gh`/network access (`main()` itself needs both, since it
//! shells out to `gh api` via [`crate::GhCliSource`]). See
//! [`docs/plans/WIRE_BACKLOG_SNAPSHOT_565_PLAN.md`](../../../docs/plans/WIRE_BACKLOG_SNAPSHOT_565_PLAN.md)
//! (issue [#565](https://github.com/daghovland/rdf-datalog/issues/565)):
//! the production regeneration script points `--out` at a scratch temp
//! path instead of the checked-in `backlog/examples/snapshot.ttl`, since
//! writing to a tracked file from a host-side scheduled job would leave a
//! live `git` checkout permanently dirty.

use std::path::PathBuf;

/// Resolved options for `backlog-regenerate`.
#[derive(Debug, PartialEq, Eq)]
pub struct RegenerateOptions {
    /// Where to write the regenerated snapshot. `None` means the caller
    /// should fall back to its own default (the checked-in
    /// `backlog/examples/snapshot.ttl`) -- resolving that default is left
    /// to the caller, since it depends on `CARGO_MANIFEST_DIR`, which this
    /// pure function has no business knowing about.
    pub out: Option<PathBuf>,
}

/// Parses CLI arguments as `std::env::args().skip(1)` would yield them
/// (i.e. NOT including argv[0]). Currently understands only `--out
/// <PATH>`; any other argument is rejected with an error string rather
/// than silently ignored, so a typo'd flag fails loudly instead of quietly
/// regenerating to the default location.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<RegenerateOptions, String> {
    let mut out = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--out" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--out requires a path argument".to_string())?;
                out = Some(PathBuf::from(value));
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(RegenerateOptions { out })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_out_is_none() {
        let opts = parse_args(Vec::<String>::new()).unwrap();
        assert_eq!(opts.out, None);
    }

    #[test]
    fn out_flag_sets_path() {
        let opts = parse_args(vec!["--out".to_string(), "/tmp/x.ttl".to_string()]).unwrap();
        assert_eq!(opts.out, Some(PathBuf::from("/tmp/x.ttl")));
    }

    #[test]
    fn out_flag_missing_value_errors() {
        let err = parse_args(vec!["--out".to_string()]).unwrap_err();
        assert!(err.contains("--out"), "unexpected error: {err}");
    }

    #[test]
    fn unknown_flag_errors() {
        let err = parse_args(vec!["--bogus".to_string()]).unwrap_err();
        assert!(err.contains("--bogus"), "unexpected error: {err}");
    }
}
