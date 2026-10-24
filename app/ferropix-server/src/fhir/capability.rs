// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FHIR R4 `CapabilityStatement` of `GET {base}/fhir/metadata`.
//!
//! It names the interactions and operations the mounted routes serve, each
//! as the vendored IHE actor statement declares it, and the SMART service,
//! the token endpoint and the scopes the permission table enforces. "A
//! given resource can only be described once per `RESTful` mode" (FHIR R4
//! `CapabilityStatement`, the constraints on `rest`), so every profile served
//! on `Patient` adds to the one `Patient` entry (`patient`).

use fhir_types::r4::capability_statement::{
    CapabilityStatement, CapabilityStatementImplementation, CapabilityStatementRest,
    CapabilityStatementRestResource, CapabilityStatementRestResourceInteraction,
    CapabilityStatementRestResourceOperation, CapabilityStatementRestSecurity,
    CapabilityStatementSoftware,
};
use fhir_types::r4::codeable_concept::CodeableConcept;
use fhir_types::r4::coding::Coding;
use fhir_types::r4::extension::{Extension, ExtensionValue};

use crate::auth::Smart;
use crate::fhir::Mounts;

/// The PIXm Manager actor statement (PIXm 3.1.0,
/// `CapabilityStatement-IHE.PIXm.Manager`).
pub const PIXM_MANAGER: &str =
    "https://profiles.ihe.net/ITI/PIXm/CapabilityStatement/IHE.PIXm.Manager";

/// The PIXm Patient profile the Manager's statement names as a
/// `supportedProfile` (§2:3.104.4.1.2.1).
pub const PIXM_PATIENT: &str =
    "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Patient";

/// The `$ihe-pix` operation (PIXm 3.1.0, `OperationDefinition-IHE.PIXm.pix`).
pub const IHE_PIX: &str = "https://profiles.ihe.net/ITI/PIXm/OperationDefinition/IHE.PIXm.pix";

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
/// by `smart`, serving what `mounts` switches on.
#[must_use]
pub fn statement(public_base_url: &url::Url, smart: &Smart, mounts: Mounts) -> CapabilityStatement {
    let base = public_base_url.as_str().trim_end_matches('/');
    let mut instantiates = Vec::new();
    if mounts.pixm {
        instantiates.push(PIXM_MANAGER.into());
    }
    let resource: Vec<CapabilityStatementRestResource> = patient(mounts).into_iter().collect();
    CapabilityStatement {
        instantiates,
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
        format: vec![
            "application/fhir+json".into(),
            "application/fhir+xml".into(),
        ],
        rest: vec![CapabilityStatementRest {
            mode: "server".into(),
            security: Some(security(smart)),
            resource,
            ..CapabilityStatementRest::default()
        }],
        ..CapabilityStatement::default()
    }
}

/// Returns the one `Patient` entry, with what every mounted profile serves
/// on it, or `None` when none is mounted.
fn patient(mounts: Mounts) -> Option<CapabilityStatementRestResource> {
    let mut patient = CapabilityStatementRestResource {
        r#type: "Patient".into(),
        ..CapabilityStatementRestResource::default()
    };
    if mounts.pixm {
        pixm(&mut patient, mounts.remove_patient);
    }
    let served = !patient.interaction.is_empty() || !patient.operation.is_empty();
    served.then_some(patient)
}

/// Adds the PIXm Manager to `patient`, as `CapabilityStatement-IHE.PIXm.Manager`
/// declares it: the conditional update of ITI-104 (§2:3.104.4.1.3), the
/// conditional delete with the Remove Patient Option (§2:3.104.4.3.3), and
/// `$ihe-pix` (§2:3.83.4.1). Its `conditionalCreate` is left out, since no
/// `POST` creates a `Patient` here.
fn pixm(patient: &mut CapabilityStatementRestResource, remove_patient: bool) {
    patient.supported_profile.push(PIXM_PATIENT.into());
    patient.interaction.push(interaction(
        "update",
        "ITI-104: Add or Revise Patient and Resolve Duplicate Patient, by conditional update on identifier",
    ));
    patient.conditional_update = Some(true.into());
    if remove_patient {
        patient.interaction.push(interaction(
            "delete",
            "ITI-104: Remove Patient, by conditional delete on identifier (the Remove Patient Option)",
        ));
        patient.conditional_delete = Some("single".into());
    }
    // NOTE: FHIR R4 CapabilityStatement.rest.resource.operation.name, "The name does
    // not include the "$" portion that is always included in the URL".
    patient.operation.push(CapabilityStatementRestResourceOperation {
        name: "ihe-pix".into(),
        definition: IHE_PIX.into(),
        documentation: Some(
            "ITI-83: by GET with the query of PIXm 3.1.0, or by POST of a Parameters body, which keeps the identifier out of the URL"
                .into(),
        ),
        ..CapabilityStatementRestResourceOperation::default()
    });
}

/// Returns the interaction `code` with its `documentation`.
fn interaction(code: &str, documentation: &str) -> CapabilityStatementRestResourceInteraction {
    CapabilityStatementRestResourceInteraction {
        code: code.into(),
        documentation: Some(documentation.into()),
        ..CapabilityStatementRestResourceInteraction::default()
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
