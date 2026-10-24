// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `ferropix` binary against the store: `db migrate` applies the set
//! and the seed, and `db reseal` moves the store to the key the
//! configuration names.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use base64::Engine as _;
use ferropix_testkit::containers::{e2e_enabled, postgres};

use super::app_role;

/// Writes a key generated for this run into `path`.
fn write_key(path: &Path) {
    let mut bytes = [0_u8; 32];
    aws_lc_rs::rand::fill(&mut bytes).expect("random bytes");
    std::fs::write(
        path,
        base64::engine::general_purpose::STANDARD.encode(bytes),
    )
    .expect("the key file is written");
}

/// Runs the binary on the quickstart configuration with `[db]` pointed at
/// `url_file` and `key_file`.
fn ferropix(args: &[&str], url_file: &Path, key_file: &Path) -> Output {
    let quickstart: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("quickstart.toml");
    Command::new(env!("CARGO_BIN_EXE_ferropix"))
        .args(args)
        .arg("--config")
        .arg(quickstart)
        .env_clear()
        .env("FERROPIX__DB__URL_FILE", url_file)
        .env("FERROPIX__DB__DATA_KEY_FILE", key_file)
        .env("FERROPIX__TELEMETRY__FILTER", "warn")
        .output()
        .expect("the binary runs")
}

#[tokio::test]
async fn db_migrate_and_db_reseal_run_against_the_store() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let url = app_role(&postgres).await;
    let directory = tempfile::tempdir().unwrap();
    let url_file = directory.path().join("db-url");
    std::fs::write(&url_file, &url).unwrap();
    let (old_key, new_key) = (directory.path().join("old"), directory.path().join("new"));
    write_key(&old_key);
    write_key(&new_key);

    for _ in 0..2 {
        let migrated = ferropix(&["db", "migrate"], &url_file, &old_key);
        assert_eq!(Some(0), migrated.status.code(), "{migrated:?}");
        assert!(
            String::from_utf8_lossy(&migrated.stdout)
                .contains("the migrations and the domain seed are applied"),
            "{migrated:?}"
        );
    }
    let refused = ferropix(&["db", "migrate"], &url_file, &new_key);
    assert_eq!(
        Some(1),
        refused.status.code(),
        "another key is refused: {refused:?}"
    );
    assert!(String::from_utf8_lossy(&refused.stderr).contains("written under key"));

    let old_key_arg = old_key.display().to_string();
    let resealed = ferropix(
        &["db", "reseal", "--old-key-file", &old_key_arg],
        &url_file,
        &new_key,
    );
    assert_eq!(Some(0), resealed.status.code(), "{resealed:?}");
    assert!(String::from_utf8_lossy(&resealed.stdout).contains("resealed from key"));
    let migrated = ferropix(&["db", "migrate"], &url_file, &new_key);
    assert_eq!(
        Some(0),
        migrated.status.code(),
        "the new key starts: {migrated:?}"
    );
}
