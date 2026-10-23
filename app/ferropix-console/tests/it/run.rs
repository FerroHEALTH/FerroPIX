// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The run path's exit codes: `config check` over a good and a refused
//! configuration, and `healthcheck` against a console that is not there.

use std::process::ExitCode;

use ferropix_console::command::{EXIT_CONFIG, run};

fn args(config: &std::path::Path, job: &[&str]) -> Vec<String> {
    let mut args = vec![
        String::from("ferropix-console"),
        String::from("--config"),
        config.to_string_lossy().into_owned(),
    ];
    args.extend(job.iter().map(|word| (*word).to_owned()));
    args
}

#[test]
fn config_check_accepts_a_valid_configuration() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let config = dir.path().join("console.toml");
    std::fs::write(&config, "[pix]\nbase_url = \"https://pix.example/\"\n")
        .expect("the configuration file");
    assert_eq!(ExitCode::SUCCESS, run(args(&config, &["config", "check"])));
}

#[test]
fn config_check_refuses_an_invalid_configuration_with_ex_config() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let config = dir.path().join("console.toml");
    std::fs::write(&config, "[pix]\ntimeout_ms = 0\n").expect("the configuration file");
    assert_eq!(
        ExitCode::from(EXIT_CONFIG),
        run(args(&config, &["config", "check"]))
    );
}

#[test]
fn an_unreadable_configuration_file_is_ex_config() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let config = dir.path().join("absent.toml");
    assert_eq!(
        ExitCode::from(EXIT_CONFIG),
        run(args(&config, &["config", "check"]))
    );
}

#[test]
fn healthcheck_fails_when_nothing_listens() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    let port = listener.local_addr().expect("its address").port();
    drop(listener);
    let dir = tempfile::tempdir().expect("a temporary directory");
    let config = dir.path().join("console.toml");
    std::fs::write(
        &config,
        format!("[server]\nlisten = \"127.0.0.1:{port}\"\n"),
    )
    .expect("the configuration file");
    assert_eq!(ExitCode::FAILURE, run(args(&config, &["healthcheck"])));
}
