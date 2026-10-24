// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `ferropix` binary: `config check` on the quickstart and on a refused
//! configuration, the pending `db migrate`, and `healthcheck` with nothing
//! listening.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ferropix_server::{EXIT_CONFIG, EXIT_PENDING_JOB};

use crate::support::{PASSWORD, Secrets};

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
fn db_migrate_names_the_issue_that_lands_it() {
    let secrets = Secrets::new();
    let output = ferropix(&["db", "migrate"], &secrets);
    assert_eq!(Some(i32::from(EXIT_PENDING_JOB)), output.status.code());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("#59"), "{stderr}");
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
