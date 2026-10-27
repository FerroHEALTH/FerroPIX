// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The settings the run path holds: every section of [`Config`] resolved,
//! every value checked and every secret file read, so a fault refuses the
//! start before anything is bound.

use crate::config::error::Error;
use crate::config::section::audit::AuditSettings;
use crate::config::section::auth::AuthSettings;
use crate::config::section::db::DbSettings;
use crate::config::section::dev::Dev;
use crate::config::section::domain::{self, DomainSeed};
use crate::config::section::hl7v2::Hl7v2Settings;
use crate::config::section::matching::Matching;
use crate::config::section::outbox::Retention;
use crate::config::section::profiles::{Pdqm, Pixm, Pmir};
use crate::config::section::pseudonym::{self, PseudonymSeed};
use crate::config::section::server::ServerSettings;
use crate::config::section::usage_report::UsageReportSettings;
use crate::config::section::xcpd::XcpdSettings;
use crate::config::section::{is_loopback_address, operator::Operator};
use crate::config::{Config, Profile};

/// Every section, resolved.
#[derive(Debug)]
pub struct Settings {
    /// The deployment profile.
    pub profile: Profile,
    /// The HTTP surface.
    pub server: ServerSettings,
    /// The console, the request log and the OTLP export.
    pub telemetry: ferrohealth_telemetry::section::TelemetrySection,
    /// The PostgreSQL store.
    pub db: DbSettings,
    /// The resource server.
    pub auth: AuthSettings,
    /// The identifier domains seeded at start.
    pub domains: Vec<DomainSeed>,
    /// The pseudonymisation domains seeded at start.
    pub pseudonym_domains: Vec<PseudonymSeed>,
    /// The scorer's configuration.
    pub matching: Matching,
    /// The PIXm Manager.
    pub pixm: Pixm,
    /// The PDQm Supplier.
    pub pdqm: Pdqm,
    /// The PMIR Registry.
    pub pmir: Pmir,
    /// The HL7 v2 face, when it is on.
    pub hl7v2: Option<Hl7v2Settings>,
    /// The XCPD Responding Gateway, when it is on.
    pub xcpd: Option<XcpdSettings>,
    /// The outbox's retention.
    pub outbox: Retention,
    /// The BALP audit trail.
    pub audit: AuditSettings,
    /// The operator API switch.
    pub operator: Operator,
    /// The installation report.
    pub usage_report: UsageReportSettings,
    /// The development table, under the development profile.
    pub dev: Option<Dev>,
    /// Where each setting came from: the provenance of the load, which the
    /// run path sets; the default (every key a default) until it does.
    pub provenance: ferrohealth_config::provenance::Provenance,
}

impl Config {
    /// Resolves every section, in the order the file lists them, and returns
    /// the first refusal.
    ///
    /// # Errors
    /// Returns [`Error::DevUnderProduction`] for a `[dev]` table under
    /// production, [`Error::DevTableRequired`] for Basic users without the
    /// `[dev]` table, [`Error::NotLoopback`] for a listener `[dev] loopback`
    /// refuses, and every [`Error`] each section's resolution returns.
    pub fn resolve(&self) -> Result<Settings, Error> {
        if self.dev.is_some() && self.profile == Profile::Production {
            return Err(Error::DevUnderProduction);
        }
        let server = self.server.resolve(self.profile)?;
        let db = self.db.resolve(self.profile)?;
        let auth = self.auth.resolve(self.profile)?;
        // NOTE: no specification governs this: our own design; a Basic user sends a
        // reusable password, so it rides only with the [dev] table's loopback rule.
        if self.auth.basic.is_some() && self.dev.is_none() {
            return Err(Error::DevTableRequired {
                key: String::from("auth.basic"),
            });
        }
        let domains = domain::resolve(&self.domain, &auth)?;
        let pseudonym_domains = pseudonym::resolve(&self.pseudonym_domain, &domains)?;
        let matching = self.matching.resolve()?;
        let pmir = self.pmir.resolve()?;
        let hl7v2 = self.hl7v2.resolve()?;
        let xcpd = self.xcpd.resolve()?;
        let outbox = self.outbox.resolve()?;
        let audit = self.audit.resolve(self.profile)?;
        let usage_report = self.usage_report.resolve()?;
        if self.dev.as_ref().is_some_and(|dev| dev.loopback) {
            if !is_loopback_address(server.listen) {
                return Err(Error::NotLoopback {
                    key: String::from("server.listen"),
                });
            }
            if hl7v2
                .as_ref()
                .is_some_and(|hl7v2| !is_loopback_address(hl7v2.listen))
            {
                return Err(Error::NotLoopback {
                    key: String::from("hl7v2.listen"),
                });
            }
        }
        Ok(Settings {
            profile: self.profile,
            server,
            telemetry: self.telemetry.clone(),
            db,
            auth,
            domains,
            pseudonym_domains,
            matching,
            pixm: self.pixm.clone(),
            pdqm: self.pdqm.clone(),
            pmir,
            hl7v2,
            xcpd,
            outbox,
            audit,
            operator: self.operator.clone(),
            usage_report,
            dev: self.dev.clone(),
            provenance: ferrohealth_config::provenance::Provenance::default(),
        })
    }
}
