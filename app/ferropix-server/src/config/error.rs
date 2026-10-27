// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why a configuration is refused.
//!
//! Every variant names the key that carries the fault and never the value, so
//! a refusal printed on stderr or in a log shows no secret; the loader's own
//! refusals quote no value either. No specification governs this: our own
//! design.

/// A configuration that does not load.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The file, the environment or a `--set` pair does not read as the
    /// configuration tree: a file that cannot be read, bad TOML, an unknown
    /// key, or a value of the wrong type.
    #[error(transparent)]
    Load(#[from] ferrohealth_config::error::ConfigError),
    /// A secret is set twice, or its `_file` sibling cannot be read or is
    /// empty.
    #[error(transparent)]
    Secret(#[from] ferrohealth_secret::file::SecretError),
    /// A secret is set under its key while `profile = "production"`.
    #[error("{key} is set inline, which profile = \"production\" refuses; set {key}_file")]
    InlineSecret {
        /// The key of the secret.
        key: String,
    },
    /// A key the server needs is not set.
    #[error("{key} is not set")]
    Missing {
        /// The key.
        key: String,
    },
    /// A socket address does not parse.
    #[error("{key} is not a socket address")]
    Listen {
        /// The key.
        key: String,
        /// What the parser reported.
        #[source]
        source: std::net::AddrParseError,
    },
    /// A URL does not parse.
    #[error("{key} is not a URL")]
    Url {
        /// The key.
        key: String,
        /// What the parser reported.
        #[source]
        source: url::ParseError,
    },
    /// A value is outside the rule its key has.
    #[error("{key} is refused: {rule}")]
    Value {
        /// The key.
        key: String,
        /// The rule the value breaks.
        rule: &'static str,
    },
    /// The `[dev]` table is set while `profile = "production"`.
    #[error("the [dev] table is set, which profile = \"production\" refuses")]
    DevUnderProduction,
    /// A setting only the development profile admits is set under production.
    #[error("{key} is admitted under profile = \"development\" only: {rule}")]
    DevelopmentOnly {
        /// The key.
        key: String,
        /// What makes the setting a development one.
        rule: &'static str,
    },
    /// A setting only the `[dev]` table admits is set without it.
    #[error("{key} is admitted only beside the [dev] table")]
    DevTableRequired {
        /// The key.
        key: String,
    },
    /// `[dev] loopback` holds and a listener binds an address that is no
    /// loopback address.
    #[error("{key} binds an address that is no loopback address, which [dev] loopback refuses")]
    NotLoopback {
        /// The key.
        key: String,
    },
    /// The `[auth]` section breaks a rule of the resource-server crate.
    #[error("the [auth] section is refused")]
    Auth {
        /// The rule.
        #[source]
        source: ferrohealth_smart::config::ConfigError,
    },
    /// A `[[domain]]` names a Source client no issuer admits.
    #[error(
        "domain[{index}].source_client_id names {client:?}, which no [[auth.issuer]] lists in its clients"
    )]
    ClientNotAdmitted {
        /// The position of the entry in the seed, from 0.
        index: usize,
        /// The client id.
        client: String,
    },
    /// Two `[[domain]]` entries name one system.
    #[error("domain[{index}].system names {system:?}, which an earlier entry seeds already")]
    DuplicateSystem {
        /// The position of the later entry, from 0.
        index: usize,
        /// The system.
        system: String,
    },
    /// Two `[[pseudonym_domain]]` entries name one namespace, or one names
    /// the system of an identifier domain.
    #[error(
        "pseudonym_domain[{index}].namespace names {namespace:?}, which an earlier entry or an identifier domain holds already"
    )]
    DuplicateNamespace {
        /// The position of the entry, from 0.
        index: usize,
        /// The namespace.
        namespace: String,
    },
    /// Two `[[hl7v2.senders]]` entries name one sending application and
    /// facility.
    #[error(
        "hl7v2.senders[{index}] names a sending application and facility an earlier entry names"
    )]
    DuplicateSender {
        /// The position of the later entry, from 0.
        index: usize,
    },
}
