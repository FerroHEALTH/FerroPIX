// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The command line: one binary, one subcommand per job.
//!
//! `serve` runs the console, `config check` reads and resolves the
//! configuration the way `serve` would and exits, and `healthcheck` asks the
//! console on this host for its readiness, for a container runtime with no
//! HTTP client of its own. No specification governs the command line: our own
//! design.

use clap::Parser;
use ferrohealth_server::cli::{Common, ConfigArgs};

/// The `ferropix-console` command line.
#[derive(Debug, Parser, PartialEq, Eq)]
#[command(name = "ferropix-console", version, about = "The FerroPIX Console")]
pub struct Cli {
    /// `--config`, the file to read in place of the one
    /// `FERROPIX_CONSOLE_CONFIG` names, and the `--set key=value` pairs
    /// applied last.
    #[command(flatten)]
    pub config: ConfigArgs,
    /// The job to run: `serve`, `config check` or `healthcheck`, the jobs
    /// every FerroHEALTH binary shares.
    #[command(subcommand)]
    pub command: Common,
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::{CommandFactory, Parser};
    use ferrohealth_server::cli::{Common, ConfigCommand};

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_job_parses_with_and_without_a_configuration_file() {
        for (args, command) in [
            (vec!["ferropix-console", "serve"], Common::Serve),
            (
                vec!["ferropix-console", "config", "check"],
                Common::Config {
                    command: ConfigCommand::Check,
                },
            ),
            (vec!["ferropix-console", "healthcheck"], Common::Healthcheck),
        ] {
            let parsed = Cli::try_parse_from(&args).expect("the job parses");
            assert_eq!(command, parsed.command, "{args:?}");
            assert_eq!(None, parsed.config.config, "{args:?}");
        }
        let parsed = Cli::try_parse_from(["ferropix-console", "serve", "--config", "console.toml"])
            .expect("the job parses");
        assert_eq!(Some("console.toml".into()), parsed.config.config);
    }
}
