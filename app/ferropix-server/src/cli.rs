// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The command line: one binary, one subcommand per job.
//!
//! `serve` runs the server, `config check` reads and resolves the
//! configuration the way `serve` would and exits, `healthcheck` asks the
//! server beside it for its readiness, `db migrate` applies the pending
//! migrations and the domain seed, `db reseal` rotates the sealing key, and
//! `matching estimate` estimates each field's `u` from the store. No
//! specification governs the command line: our own design.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use ferrohealth_server::cli::{Common, ConfigArgs, ConfigCommand};

/// The `ferropix` command line.
#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "ferropix",
    version,
    about = "The FerroPIX Master Patient Index over IHE PIXm and PDQm"
)]
pub struct Cli {
    /// `--config`, the file to read in place of the one `FERROPIX_CONFIG`
    /// names, and the `--set key=value` pairs applied last.
    #[command(flatten)]
    pub config: ConfigArgs,
    /// The job to run.
    #[command(subcommand)]
    pub command: Command,
}

/// What the binary does.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// `serve`, `config check` and `healthcheck`, the jobs every FerroHEALTH
    /// binary shares.
    #[command(flatten)]
    Common(Common),
    /// Works on the database.
    Db {
        /// The job to run.
        #[command(subcommand)]
        command: DbCommand,
    },
    /// Works on the matching.
    Matching {
        /// The job to run.
        #[command(subcommand)]
        command: MatchingCommand,
    },
}

/// The `matching` jobs.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum MatchingCommand {
    /// Estimates each field's `u` from how often its values occur in the
    /// store, and prints them as `[matching.fields]` tables.
    Estimate,
}

/// The `db` jobs.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum DbCommand {
    /// Applies the pending migrations and the `[[domain]]` seed, and exits.
    Migrate,
    /// Rewrites every sealed identifier column under the key
    /// `[db] data_key_file` names, reading the old key from its own file.
    Reseal {
        /// The file holding the key the store is written under now.
        #[arg(long, value_name = "PATH")]
        old_key_file: PathBuf,
    },
}

impl Command {
    /// Returns the tracker issue that lands this job, or `None` when it runs.
    #[must_use]
    pub const fn pending_issue(&self) -> Option<u32> {
        match self {
            Self::Common(_) | Self::Db { .. } | Self::Matching { .. } => None,
        }
    }

    /// Returns the job as it is spelled on the command line.
    #[must_use]
    pub const fn spelling(&self) -> &'static str {
        match self {
            Self::Common(Common::Serve) => "serve",
            Self::Common(Common::Config {
                command: ConfigCommand::Check,
            }) => "config check",
            Self::Common(Common::Healthcheck) => "healthcheck",
            Self::Db {
                command: DbCommand::Migrate,
            } => "db migrate",
            Self::Db {
                command: DbCommand::Reseal { .. },
            } => "db reseal",
            Self::Matching {
                command: MatchingCommand::Estimate,
            } => "matching estimate",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::{CommandFactory, Parser};

    use ferrohealth_server::cli::{Common, ConfigCommand};

    use super::{Cli, Command, DbCommand, MatchingCommand};

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_job_parses_with_and_without_a_configuration_file() {
        for (args, command) in [
            (vec!["ferropix", "serve"], Command::Common(Common::Serve)),
            (
                vec!["ferropix", "config", "check"],
                Command::Common(Common::Config {
                    command: ConfigCommand::Check,
                }),
            ),
            (
                vec!["ferropix", "healthcheck"],
                Command::Common(Common::Healthcheck),
            ),
            (
                vec!["ferropix", "db", "migrate"],
                Command::Db {
                    command: DbCommand::Migrate,
                },
            ),
            (
                vec!["ferropix", "matching", "estimate"],
                Command::Matching {
                    command: MatchingCommand::Estimate,
                },
            ),
        ] {
            let parsed = Cli::try_parse_from(&args).expect("the job parses");
            assert_eq!(command, parsed.command, "{args:?}");
            assert_eq!(None, parsed.config.config, "{args:?}");
        }
        let parsed = Cli::try_parse_from(["ferropix", "serve", "--config", "/etc/ferropix.toml"])
            .expect("--config parses after the job");
        assert_eq!(
            Some(PathBuf::from("/etc/ferropix.toml")),
            parsed.config.config
        );
    }

    #[test]
    fn a_job_is_required_and_an_unknown_one_is_refused() {
        for args in [
            vec!["ferropix"],
            vec!["ferropix", "run"],
            vec!["ferropix", "db"],
            vec!["ferropix", "config"],
            vec!["ferropix", "matching"],
        ] {
            assert!(Cli::try_parse_from(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn db_reseal_takes_the_old_key_file() {
        let parsed =
            Cli::try_parse_from(["ferropix", "db", "reseal", "--old-key-file", "/run/old"])
                .expect("db reseal parses");
        assert_eq!(
            Command::Db {
                command: DbCommand::Reseal {
                    old_key_file: PathBuf::from("/run/old")
                }
            },
            parsed.command
        );
        assert_eq!("db reseal", parsed.command.spelling());
        assert!(
            Cli::try_parse_from(["ferropix", "db", "reseal"]).is_err(),
            "the old key file is required"
        );
    }

    #[test]
    fn no_job_is_pending() {
        for command in [
            Command::Common(Common::Serve),
            Command::Common(Common::Healthcheck),
            Command::Db {
                command: DbCommand::Migrate,
            },
        ] {
            assert_eq!(None, command.pending_issue(), "{command:?}");
        }
        assert_eq!(
            "db migrate",
            Command::Db {
                command: DbCommand::Migrate
            }
            .spelling()
        );
    }
}
