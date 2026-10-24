// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FHIR R4 `CapabilityStatement` of `GET {base}/fhir/metadata`, as far
//! as authorization states it: the SMART service, the token endpoint and the
//! scopes the permission table enforces.

use fhir_types::r4::capability_statement::{
    CapabilityStatement, CapabilityStatementImplementation, CapabilityStatementRest,
    CapabilityStatementRestSecurity, CapabilityStatementSoftware,
};
use fhir_types::r4::codeable_concept::CodeableConcept;
use fhir_types::r4::coding::Coding;
use fhir_types::r4::extension::{Extension, ExtensionValue};

use crate::auth::Smart;

/// The `oauth-uris` extension, the form SMART App Launch 1.0.0 defined and
/// 2.x keeps for existing clients while it calls it deprecated
/// (<https://hl7.org/fhir/smart-app-launch/conformance.html>).
pub const OAUTH_URIS: &str =
    "http://fhir-registry.smarthealthit.org/StructureDefinition/oauth-uris";

/// The R4 code system of `rest.security.service`
/// (<https://hl7.org/fhir/R4/codesystem-restful-security-service.html>).
pub const SECURITY_SERVICE_SYSTEM: &str =
    "http://terminology.hl7.org/CodeSystem/restful-security-service";

/// Returns the statement of a server reached at `public_base_url`, guarded
/// by `smart`.
// TODO(#69): build the whole statement from the vendored actor statements.
#[must_use]
pub fn statement(public_base_url: &url::Url, smart: &Smart) -> CapabilityStatement {
    let base = public_base_url.as_str().trim_end_matches('/');
    CapabilityStatement {
        status: "active".into(),
        date: jiff::Timestamp::now()
            .strftime("%Y-%m-%d")
            .to_string()
            .into(),
        kind: "instance".into(),
        software: Some(CapabilityStatementSoftware {
            name: "FerroPIX".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            ..CapabilityStatementSoftware::default()
        }),
        implementation: Some(CapabilityStatementImplementation {
            description: "FerroPIX, a Master Patient Index over IHE PIXm and PDQm".into(),
            url: Some(format!("{base}/fhir").into()),
            ..CapabilityStatementImplementation::default()
        }),
        fhir_version: "4.0.1".into(),
        format: vec!["application/fhir+json".into()],
        rest: vec![CapabilityStatementRest {
            mode: "server".into(),
            security: Some(security(smart)),
            ..CapabilityStatementRest::default()
        }],
        ..CapabilityStatement::default()
    }
}

/// Returns `rest.security`: the SMART service with the token endpoint, or
/// the Basic service on a development surface of Basic users alone.
fn security(smart: &Smart) -> CapabilityStatementRestSecurity {
    let scopes = smart.scopes_supported().join(" ");
    let Some(token) = smart.token_endpoint() else {
        return CapabilityStatementRestSecurity {
            service: vec![service("Basic")],
            description: Some(
                format!(
                    "HTTP Basic users of the development profile; each request carries one. The scopes FerroPIX enforces: {scopes}"
                )
                .into(),
            ),
            ..CapabilityStatementRestSecurity::default()
        };
    };
    CapabilityStatementRestSecurity {
        extension: vec![Extension {
            url: OAUTH_URIS.into(),
            extension: vec![Extension {
                url: "token".into(),
                value: Some(ExtensionValue::Uri(token.into())),
                ..Extension::default()
            }],
            ..Extension::default()
        }],
        service: vec![service("SMART-on-FHIR")],
        description: Some(
            format!(
                "SMART App Launch 2.x backend services: each request carries a bearer access token of the authorization server whose token endpoint is named here; discovery at [base]/.well-known/smart-configuration. The scopes FerroPIX enforces: {scopes}"
            )
            .into(),
        ),
        ..CapabilityStatementRestSecurity::default()
    }
}

/// Returns the `CodeableConcept` of the security service `code`.
fn service(code: &str) -> CodeableConcept {
    CodeableConcept {
        coding: vec![Coding {
            system: Some(SECURITY_SERVICE_SYSTEM.into()),
            code: Some(code.into()),
            ..Coding::default()
        }],
        ..CodeableConcept::default()
    }
}
