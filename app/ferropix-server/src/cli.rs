// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The command line: one binary, one subcommand per job.
//!
//! `serve` runs the server, `config check` reads and resolves the
//! configuration the way `serve` would and exits, `healthcheck` asks the
//! server beside it for its readiness, and `db migrate` applies the pending
//! migrations. No specification governs the command line: our own design.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// The `ferropix` command line.
#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "ferropix",
    version,
    about = "The FerroPIX Master Patient Index over IHE PIXm and PDQm"
)]
pub struct Cli {
    /// The configuration file to read, overriding `FERROPIX_CONFIG`.
    #[arg(long, value_name = "PATH", global = true)]
    pub config: Option<PathBuf>,
    /// The job to run.
    #[command(subcommand)]
    pub command: Command,
}

/// What the binary does.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Serves the server until the process is asked to stop.
    Serve,
    /// Works on the configuration.
    Config {
        /// The job to run.
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Asks the server on this host for its readiness, and exits 0 only when
    /// it answers 200.
    Healthcheck,
    /// Works on the database.
    Db {
        /// The job to run.
        #[command(subcommand)]
        command: DbCommand,
    },
}

/// The `config` jobs.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum ConfigCommand {
    /// Reads and resolves the configuration, secrets included, and reports
    /// whether the server would start on it.
    Check,
}

/// The `db` jobs.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum DbCommand {
    /// Applies the pending migrations and exits.
    Migrate,
}

impl Command {
    /// Returns the tracker issue that lands this job, or `None` when it runs.
    #[must_use]
    pub const fn pending_issue(&self) -> Option<u32> {
        match self {
            Self::Serve | Self::Config { .. } | Self::Healthcheck => None,
            Self::Db {
                command: DbCommand::Migrate,
            } => Some(59),
        }
    }

    /// Returns the job as it is spelled on the command line.
    #[must_use]
    pub const fn spelling(&self) -> &'static str {
        match self {
            Self::Serve => "serve",
            Self::Config {
                command: ConfigCommand::Check,
            } => "config check",
            Self::Healthcheck => "healthcheck",
            Self::Db {
                command: DbCommand::Migrate,
            } => "db migrate",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::{CommandFactory, Parser};

    use super::{Cli, Command, ConfigCommand, DbCommand};

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_job_parses_with_and_without_a_configuration_file() {
        for (args, command) in [
            (vec!["ferropix", "serve"], Command::Serve),
            (
                vec!["ferropix", "config", "check"],
                Command::Config {
                    command: ConfigCommand::Check,
                },
            ),
            (vec!["ferropix", "healthcheck"], Command::Healthcheck),
            (
                vec!["ferropix", "db", "migrate"],
                Command::Db {
                    command: DbCommand::Migrate,
                },
            ),
        ] {
            let parsed = Cli::try_parse_from(&args).expect("the job parses");
            assert_eq!(command, parsed.command, "{args:?}");
            assert_eq!(None, parsed.config, "{args:?}");
        }
        let parsed = Cli::try_parse_from(["ferropix", "serve", "--config", "/etc/ferropix.toml"])
            .expect("--config parses after the job");
        assert_eq!(Some(PathBuf::from("/etc/ferropix.toml")), parsed.config);
    }

    #[test]
    fn a_job_is_required_and_an_unknown_one_is_refused() {
        for args in [
            vec!["ferropix"],
            vec!["ferropix", "run"],
            vec!["ferropix", "db"],
            vec!["ferropix", "config"],
        ] {
            assert!(Cli::try_parse_from(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn a_pending_job_names_the_issue_that_lands_it() {
        let migrate = Command::Db {
            command: DbCommand::Migrate,
        };
        assert_eq!(Some(59), migrate.pending_issue());
        assert_eq!("db migrate", migrate.spelling());
        assert_eq!(None, Command::Serve.pending_issue());
        assert_eq!(None, Command::Healthcheck.pending_issue());
    }
}
