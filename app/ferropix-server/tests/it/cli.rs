// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `ferropix` binary: `config check` on the quickstart and on a refused
//! configuration, `db migrate` and `db reseal` with no store to reach, and
//! `healthcheck` with nothing listening.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ferropix_server::EXIT_CONFIG;

use crate::support::{DATA_KEY, PASSWORD, Secrets};

/// The quickstart configuration beside the crate's manifest.
fn quickstart() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("quickstart.toml")
}

/// Runs the binary with `args` and only the environment `secrets` points at.
fn ferropix(args: &[&str], secrets: &Secrets) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferropix"))
        .args(args)
        .env_clear()
        .envs(secrets.environment())
        .output()
        .expect("the binary runs")
}

#[test]
fn config_check_accepts_the_quickstart_configuration() {
    let secrets = Secrets::new();
    let quickstart = quickstart();
    let output = ferropix(
        &[
            "config",
            "check",
            "--config",
            quickstart.to_str().expect("a UTF-8 path"),
        ],
        &secrets,
    );
    assert_eq!(
        Some(0),
        output.status.code(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("accepted"), "{stdout}");
}

#[test]
fn config_check_prints_every_key_with_its_source_and_masks_every_secret() {
    let secrets = Secrets::new();
    let quickstart = quickstart();
    let mut environment = secrets.environment();
    environment.insert(String::from("RUST_LOG"), String::from("debug"));
    let output = Command::new(env!("CARGO_BIN_EXE_ferropix"))
        .args([
            "config",
            "check",
            "--config",
            quickstart.to_str().expect("a UTF-8 path"),
            "--set",
            "db.pool_size=7",
        ])
        .env_clear()
        .envs(environment)
        .output()
        .expect("the binary runs");
    assert_eq!(
        Some(0),
        output.status.code(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("# file: "), "{stdout}");
    assert!(
        stdout.contains("profile = \"production\"  # file"),
        "{stdout}"
    );
    assert!(
        stdout.contains("db.pool_size = 7  # set: --set db.pool_size"),
        "the --set pair wins and is named: {stdout}"
    );
    assert!(
        stdout.contains("telemetry.filter = \"debug\"  # environment: RUST_LOG"),
        "RUST_LOG sets telemetry.filter: {stdout}"
    );
    assert!(
        stdout.contains("db.url_file = ")
            && stdout.contains("# environment: FERROPIX__DB__URL_FILE"),
        "{stdout}"
    );
    assert!(!stdout.contains(PASSWORD), "{stdout}");
    assert!(!stdout.contains(DATA_KEY), "{stdout}");
}

#[test]
fn an_unknown_override_variable_refuses_the_start_and_names_the_key() {
    let secrets = Secrets::new();
    let quickstart = quickstart();
    let mut environment = secrets.environment();
    environment.insert(
        String::from("FERROPIX__DB__PASWORD"),
        String::from(PASSWORD),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_ferropix"))
        .args([
            "config",
            "check",
            "--config",
            quickstart.to_str().expect("a UTF-8 path"),
        ])
        .env_clear()
        .envs(environment)
        .output()
        .expect("the binary runs");
    assert_eq!(Some(i32::from(EXIT_CONFIG)), output.status.code());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("db.pasword"), "{stderr}");
    assert!(stderr.contains("FERROPIX__DB__PASWORD"), "{stderr}");
    assert!(
        !stderr.contains(PASSWORD),
        "the value is never quoted: {stderr}"
    );
}

#[test]
fn config_check_names_the_first_error_and_exits_with_ex_config() {
    let secrets = Secrets::new();
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("ferropix.toml");
    let text = std::fs::read_to_string(quickstart())
        .expect("the quickstart is read")
        .replacen("[server]\n", "[server]\nlisten_port = 8080\n", 1);
    std::fs::write(&path, text).expect("the configuration is written");
    let output = ferropix(
        &[
            "config",
            "check",
            "--config",
            path.to_str().expect("a UTF-8 path"),
        ],
        &secrets,
    );
    assert_eq!(Some(i32::from(EXIT_CONFIG)), output.status.code());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("listen_port"), "{stderr}");
    assert_eq!(
        1,
        stderr.lines().count(),
        "one line names one error: {stderr}"
    );
}

#[test]
fn config_check_never_prints_a_secret_it_refuses() {
    let secrets = Secrets::new();
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("ferropix.toml");
    let text = std::fs::read_to_string(quickstart())
        .expect("the quickstart is read")
        .replacen(
            "url_file = \"/run/secrets/ferropix-db-url\"",
            &format!("url = \"postgres://ferropix:{PASSWORD}@db.invalid/pix\""),
            1,
        );
    std::fs::write(&path, text).expect("the configuration is written");
    let mut environment = secrets.environment();
    environment.remove("FERROPIX__DB__URL_FILE");
    let output = Command::new(env!("CARGO_BIN_EXE_ferropix"))
        .args([
            "config",
            "check",
            "--config",
            path.to_str().expect("a UTF-8 path"),
        ])
        .env_clear()
        .envs(environment)
        .output()
        .expect("the binary runs");
    assert_eq!(Some(i32::from(EXIT_CONFIG)), output.status.code());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("db.url"), "{stderr}");
    assert!(!stderr.contains(PASSWORD), "{stderr}");
}

#[test]
fn db_migrate_with_no_store_to_reach_fails_without_printing_a_secret() {
    let secrets = Secrets::new();
    let output = ferropix(
        &[
            "db",
            "migrate",
            "--config",
            &quickstart().display().to_string(),
        ],
        &secrets,
    );
    assert_eq!(Some(1), output.status.code(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot connect to the store"), "{stderr}");
    assert!(!stderr.contains(PASSWORD), "{stderr}");
    assert!(!stderr.contains(DATA_KEY), "{stderr}");
}

#[test]
fn db_reseal_refuses_an_old_key_that_is_no_key_and_never_prints_it() {
    let secrets = Secrets::new();
    let directory = tempfile::tempdir().expect("a temporary directory");
    let old = directory.path().join("old-key");
    std::fs::write(
        &old,
        "Qz7-not-a-key
",
    )
    .expect("the old key file is written");
    let output = ferropix(
        &[
            "db",
            "reseal",
            "--old-key-file",
            &old.display().to_string(),
            "--config",
            &quickstart().display().to_string(),
        ],
        &secrets,
    );
    assert_eq!(
        Some(i32::from(EXIT_CONFIG)),
        output.status.code(),
        "{output:?}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("old_key_file"), "{stderr}");
    assert!(!stderr.contains("Qz7-not-a-key"), "{stderr}");
}

#[test]
fn healthcheck_exits_one_when_nothing_listens() {
    let secrets = Secrets::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    let port = listener.local_addr().expect("an address").port();
    drop(listener);
    let quickstart = quickstart();
    let output = Command::new(env!("CARGO_BIN_EXE_ferropix"))
        .args([
            "healthcheck",
            "--config",
            quickstart.to_str().expect("a UTF-8 path"),
        ])
        .env_clear()
        .envs(secrets.environment())
        .env("FERROPIX__SERVER__LISTEN", format!("127.0.0.1:{port}"))
        .output()
        .expect("the binary runs");
    assert_eq!(Some(1), output.status.code());
    assert!(String::from_utf8_lossy(&output.stdout).contains("not up"));
}
