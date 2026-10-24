// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Captures the git commit the usage report states as its `git_sha`.
//!
//! A build outside a git checkout records `unknown`, which the report sends
//! as the all-zero commit. No specification governs this: our own design,
//! after FerroBRIDGE's build script.

use std::process::Command;

fn main() {
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| String::from("unknown"));
    println!("cargo::rustc-env=FERROPIX_BUILD_COMMIT={commit}");
    for path in ["HEAD", "refs", "packed-refs"] {
        if let Some(watched) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo::rerun-if-changed={watched}");
        }
    }
    println!("cargo::rerun-if-changed=build.rs");
}

/// Returns what `git args` prints, trimmed, or `None` when git is absent,
/// fails or prints nothing.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (output.status.success() && !text.is_empty()).then(|| text.to_owned())
}
