// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The verdicts of a conformance corpus and its pass list.
//!
//! A corpus test checks each case in a task of its own, so a failing check
//! becomes the case's verdict with its reason, and hands the verdicts to
//! [`record`]. That writes `<out>/<corpus>.txt` (the out directory is
//! `FERROPIX_CONFORMANCE_OUT`, or `target/conformance` at the workspace root)
//! in the form of a pass list, and then either rewrites
//! `conformance/<corpus>/pass-list.txt` when `FERROPIX_CONFORMANCE_UPDATE` is
//! `1`, or fails when a case the list records as passing no longer passes.
//! `scripts/checks/conformance.sh` compares the rest: a new pass, a moved
//! total, a changed reason.
//!
//! No specification governs the instrument: our own design, the model of
//! FerroBRIDGE's corpus tests.
#![expect(
    clippy::panic,
    clippy::expect_used,
    reason = "a test instrument: a failing record is how the corpus test fails"
)]

use std::any::Any;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::future::Future;
use std::path::{Path, PathBuf};

/// The workspace root, relative to this crate's manifest.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The environment variable that names the directory the results go to.
const OUT_VARIABLE: &str = "FERROPIX_CONFORMANCE_OUT";

/// The environment variable that, set to `1`, rewrites the pass lists.
const UPDATE_VARIABLE: &str = "FERROPIX_CONFORMANCE_UPDATE";

/// The prefix of a pass list line that records a failing example.
const FAIL: &str = "fail ";

/// The prefix of the one pass list line that records the example count.
const TOTAL: &str = "total ";

/// The longest reason a pass list records, in characters.
const REASON_LIMIT: usize = 200;

/// The verdict on one example.
#[derive(Debug)]
pub struct Case {
    /// The example's path under its package, such as `example/Patient-ex-patient.json`.
    id: String,
    /// Why the example fails, or `None` when it passes.
    failure: Option<String>,
    /// How a passing example was handled when that is not the plain read and
    /// write: the refusal the specification requires, with its citation.
    note: Option<String>,
}

/// Runs `check` on the example `id` in a task of its own: the example passes
/// when the check returns, and fails with the first line of its panic.
pub async fn verdict<F>(id: String, check: F) -> Case
where
    F: Future<Output = ()> + Send + 'static,
{
    verdict_noted(id, async move {
        check.await;
        None
    })
    .await
}

/// Runs `check` as [`verdict`] does, keeping the note a passing check
/// returns: how it handled the example, such as the refusal it asserted.
pub async fn verdict_noted<F>(id: String, check: F) -> Case
where
    F: Future<Output = Option<String>> + Send + 'static,
{
    let (failure, note) = match tokio::spawn(check).await {
        Ok(note) => (None, note),
        Err(error) => (
            Some(match error.try_into_panic() {
                Ok(payload) => reason(payload.as_ref()),
                Err(error) => format!("the check did not finish: {error}"),
            }),
            None,
        ),
    };
    Case { id, failure, note }
}

/// The first line of a panic, cut to [`REASON_LIMIT`] characters.
fn reason(payload: &(dyn Any + Send)) -> String {
    let text = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("the check panicked");
    text.lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(REASON_LIMIT)
        .collect()
}

/// The pass list of `cases` under the heading `title`.
fn render(title: &str, cases: &[Case]) -> String {
    let mut list = format!(
        "# SPDX-FileCopyrightText: Cadasto B.V.\n\
         # SPDX-License-Identifier: BUSL-1.1\n\
         #\n\
         # {title}.\n\
         # One example per line: a passing example is its path, followed by\n\
         # `: <how>` when the pass is a refusal the specification requires, and\n\
         # a failing one is `fail <path>: <reason>`; the last line is the total.\n\
         # The corpus test writes it under scripts/checks/conformance.sh\n\
         # --update, and fails when a listed pass stops passing.\n"
    );
    for case in cases {
        match (&case.failure, &case.note) {
            (None, None) => writeln!(list, "{}", case.id),
            (None, Some(note)) => writeln!(list, "{}: {note}", case.id),
            (Some(failure), _) => writeln!(list, "{FAIL}{}: {failure}", case.id),
        }
        .expect("a String takes any text");
    }
    writeln!(list, "{TOTAL}{}", cases.len()).expect("a String takes any text");
    list
}

/// The passing examples a pass list records.
fn passes(list: &str) -> BTreeSet<&str> {
    list.lines()
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && !line.starts_with(FAIL)
                && !line.starts_with(TOTAL)
        })
        .map(|line| line.split_once(": ").map_or(line, |(id, _note)| id))
        .collect()
}

/// Writes the result of `corpus`, then rewrites its pass list under
/// `FERROPIX_CONFORMANCE_UPDATE=1` or holds the verdicts to it.
///
/// # Panics
/// When an example the pass list records as passing fails or is gone, and
/// when the list cannot be read or a file cannot be written.
pub fn record(corpus: &str, title: &str, mut cases: Vec<Case>) {
    cases.sort_by(|left, right| left.id.cmp(&right.id));
    let list = render(title, &cases);
    let out = std::env::var_os(OUT_VARIABLE)
        .map_or_else(|| Path::new(ROOT).join("target/conformance"), PathBuf::from);
    std::fs::create_dir_all(&out).expect("the result directory");
    std::fs::write(out.join(format!("{corpus}.txt")), &list).expect("the result");
    let committed = Path::new(ROOT)
        .join("conformance")
        .join(corpus)
        .join("pass-list.txt");
    if std::env::var(UPDATE_VARIABLE).is_ok_and(|update| update == "1") {
        std::fs::create_dir_all(committed.parent().expect("a directory"))
            .expect("the list directory");
        std::fs::write(&committed, &list).expect("the pass list");
        return;
    }
    let text = std::fs::read_to_string(&committed).unwrap_or_else(|error| {
        panic!(
            "{}: {error}; run scripts/checks/conformance.sh --update",
            committed.display()
        )
    });
    let listed = passes(&text);
    let mut regressed: Vec<String> = cases
        .iter()
        .filter(|case| listed.contains(case.id.as_str()))
        .filter_map(|case| {
            let failure = case.failure.as_ref()?;
            Some(format!("{}: {failure}", case.id))
        })
        .collect();
    regressed.extend(
        listed
            .iter()
            .filter(|id| !cases.iter().any(|case| case.id == **id))
            .map(|id| format!("{id}: the corpus no longer holds it")),
    );
    assert!(
        regressed.is_empty(),
        "{corpus}: examples the pass list records no longer pass:\n{}",
        regressed.join("\n")
    );
}
