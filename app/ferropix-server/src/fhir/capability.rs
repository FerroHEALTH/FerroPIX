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
    CapabilityStatementRestResourceOperation, CapabilityStatementRestResourceSearchParam,
    CapabilityStatementRestSecurity, CapabilityStatementSoftware,
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

/// The PDQm Patient Demographics Supplier actor statement (PDQm 3.2.0,
/// `CapabilityStatement-IHE.PDQm.PatientDemographicsSupplier`).
pub const PDQM_SUPPLIER: &str =
    "https://profiles.ihe.net/ITI/PDQm/CapabilityStatement/IHE.PDQm.PatientDemographicsSupplier";

/// The statement of the Supplier with the Match Operation Option (PDQm
/// 3.2.0, `CapabilityStatement-IHE.PDQm.PatientDemographicsSupplierMatch`).
pub const PDQM_SUPPLIER_MATCH: &str = "https://profiles.ihe.net/ITI/PDQm/CapabilityStatement/IHE.PDQm.PatientDemographicsSupplierMatch";

/// The `$match` operation of PDQm (PDQm 3.2.0,
/// `OperationDefinition-PDQmMatch`).
pub const PDQM_MATCH: &str = "https://profiles.ihe.net/ITI/PDQm/OperationDefinition/PDQmMatch";

/// The search parameters of the Supplier's statement the search reads, each
/// with its type and, for `mothersMaidenName`, its definition
/// (§2:3.78.4.1.2.1, §2:3.78.4.1.2.2).
// NOTE: no specification governs this: our own design; `_lastUpdated`, which
// the vendored statement lists and §2:3.78.4.1.2.1 does not, is not read.
pub const PDQM_SEARCH_PARAMETERS: [(&str, &str, Option<&str>); 22] = [
    ("_id", "token", None),
    ("active", "token", None),
    ("family", "string", None),
    ("family:exact", "string", None),
    ("given", "string", None),
    ("given:exact", "string", None),
    ("identifier", "token", None),
    ("telecom", "token", None),
    ("birthdate", "date", None),
    ("address", "string", None),
    ("address:exact", "string", None),
    ("address-city", "string", None),
    ("address-city:exact", "string", None),
    ("address-country", "string", None),
    ("address-country:exact", "string", None),
    ("address-postalcode", "string", None),
    ("address-postalcode:exact", "string", None),
    ("address-state", "string", None),
    ("address-state:exact", "string", None),
    ("gender", "token", None),
    (
        "mothersMaidenName",
        "string",
        Some(MOTHERS_MAIDEN_NAME_PARAMETER),
    ),
    (
        "mothersMaidenName:exact",
        "string",
        Some(MOTHERS_MAIDEN_NAME_PARAMETER),
    ),
];

/// The search parameter of the mother's maiden name extension, as the
/// Supplier's statement names it.
const MOTHERS_MAIDEN_NAME_PARAMETER: &str =
    "http://hl7.org/fhir/SearchParameter/patient-extensions-Patient-mothersMaidenName|5.2.0";

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
    if mounts.pdqm {
        instantiates.push(PDQM_SUPPLIER.into());
        instantiates.push(PDQM_SUPPLIER_MATCH.into());
    }
    let resource: Vec<CapabilityStatementRestResource> = patient(mounts)
        .into_iter()
        .chain(mounts.pmir.then(subscription))
        .collect();
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
    if mounts.pdqm {
        pdqm(&mut patient);
    }
    let served = !patient.interaction.is_empty() || !patient.operation.is_empty();
    served.then_some(patient)
}

/// The PMIR subscription profile ITI-94 creates against (PMIR 1.6.0
/// §2:3.94.4.1.2.1).
pub const PMIR_SUBSCRIPTION: &str =
    "https://profiles.ihe.net/ITI/PMIR/StructureDefinition/IHE.PMIR.Subscription";

/// The `Subscription` resource of ITI-94: create, read, the update that
/// enables or disables, and delete (PMIR 1.6.0 §2:3.94.4.1 to §2:3.94.4.5).
fn subscription() -> CapabilityStatementRestResource {
    CapabilityStatementRestResource {
        r#type: "Subscription".into(),
        supported_profile: vec![PMIR_SUBSCRIPTION.into()],
        interaction: vec![
            interaction("create", "ITI-94: Subscribe to Patient Updates"),
            interaction("read", "ITI-94: Get Patient Subscription"),
            interaction("update", "ITI-94: Enable or Disable Patient Subscription"),
            interaction("delete", "ITI-94: Delete Patient Subscription"),
        ],
        ..CapabilityStatementRestResource::default()
    }
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

/// Adds the PDQm Supplier with the Match Operation Option to `patient`, as
/// `CapabilityStatement-IHE.PDQm.PatientDemographicsSupplierMatch` declares
/// it: the PDQm Patient profile, the `read` and `search-type` of ITI-78
/// (§2:3.78.4.3, §2:3.78.4.1.2) with the parameters it reads, and `$match`
/// (§2:3.119.4.1.2).
fn pdqm(patient: &mut CapabilityStatementRestResource) {
    patient
        .supported_profile
        .push(crate::pdqm::patient::PDQM_PATIENT.into());
    patient.interaction.push(interaction(
        "read",
        "ITI-78: Retrieve Patient Resource, one person by its id",
    ));
    patient.interaction.push(interaction(
        "search-type",
        "ITI-78: Query Patient Resource, by GET or by POST of a form to _search",
    ));
    patient.search_param = PDQM_SEARCH_PARAMETERS
        .iter()
        .map(
            |(name, kind, definition)| CapabilityStatementRestResourceSearchParam {
                name: (*name).into(),
                r#type: (*kind).into(),
                definition: definition.map(Into::into),
                ..CapabilityStatementRestResourceSearchParam::default()
            },
        )
        .collect();
    patient
        .operation
        .push(CapabilityStatementRestResourceOperation {
            name: "match".into(),
            definition: PDQM_MATCH.into(),
            documentation: Some(
                "ITI-119: by POST of the PDQm Match Input Parameters or of a bare Patient".into(),
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
