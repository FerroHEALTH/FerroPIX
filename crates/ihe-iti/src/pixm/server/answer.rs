// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-83 success answer: a `Parameters` resource with a
//! `targetIdentifier` per identifier and a `targetId` per Patient resource
//! (§2:3.83.4.2.2.1, the Query Parameters Out profile).

use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::parameters::{Parameters, ParametersParameter, ParametersParameterValue};
use fhir_types::r4::reference::Reference;
use http::StatusCode;
use secrecy::ExposeSecret;

use super::query::PixQuery;
use crate::pixm::identifier::{PatientReference, TargetIdentifier};
use crate::server::answer::Answer;

/// Returns the `200` answer to `query`: one `targetIdentifier` for each of
/// `identifiers` and one `targetId` for each of `patients`, in that order,
/// which carries no meaning (§2:3.83.4.2.2.1).
///
/// The answer holds to the request whatever the store hands in: the source
/// identifier itself is left out, since it "shall not be included in the
/// returned Response" (§2:3.83.4.2.2.1), and so is an identifier from a
/// domain the request did not ask for, when it names any
/// (§2:3.83.4.1.2.2).
#[must_use]
pub fn cross_references(
    query: &PixQuery,
    identifiers: &[TargetIdentifier],
    patients: &[PatientReference],
) -> Answer<Parameters> {
    let source = query.source();
    let asked = |identifier: &&TargetIdentifier| {
        query.targets().is_empty()
            || query
                .targets()
                .iter()
                .any(|target| target.as_str() == identifier.system())
    };
    let echoed = |identifier: &&TargetIdentifier| {
        identifier.system() == source.system()
            && identifier.value().expose_secret() == source.value().expose_secret()
    };
    let mut parameter: Vec<ParametersParameter> = identifiers
        .iter()
        .filter(asked)
        .filter(|identifier| !echoed(identifier))
        .map(|identifier| ParametersParameter {
            name: "targetIdentifier".into(),
            value: Some(ParametersParameterValue::Identifier(Box::new(Identifier {
                system: Some(identifier.system().into()),
                value: Some(identifier.value().expose_secret().into()),
                ..Identifier::default()
            }))),
            ..ParametersParameter::default()
        })
        .collect();
    parameter.extend(patients.iter().map(|patient| ParametersParameter {
        name: "targetId".into(),
        value: Some(ParametersParameterValue::Reference(Box::new(Reference {
            reference: Some(patient.reference().expose_secret().into()),
            ..Reference::default()
        }))),
        ..ParametersParameter::default()
    }));
    Answer::new(
        StatusCode::OK,
        Parameters {
            parameter,
            ..Parameters::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::cross_references;
    use crate::pixm::identifier::{PatientReference, TargetIdentifier};
    use crate::pixm::server::query::PixQuery;
    use crate::server::format::Format;

    fn identifier(system: &str, value: &str) -> TargetIdentifier {
        TargetIdentifier::new(system, SecretString::from(value)).expect("an identifier")
    }

    #[test]
    fn the_source_and_the_unasked_domains_are_left_out() {
        let query = PixQuery::from_query(
            "sourceIdentifier=urn:oid:2.999.1%7C1&targetSystem=urn:oid:2.999.2",
        )
        .expect("a query");
        let answer = cross_references(
            &query,
            &[
                identifier("urn:oid:2.999.1", "1"),
                identifier("urn:oid:2.999.2", "2"),
                identifier("urn:oid:2.999.3", "3"),
            ],
            &[PatientReference::new(SecretString::from("Patient/p")).expect("a reference")],
        );
        let json = String::from_utf8(answer.encode(Format::Json).expect("JSON")).expect("UTF-8");
        assert_eq!(
            json,
            r#"{"parameter":[{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.2","value":"2"}},{"name":"targetId","valueReference":{"reference":"Patient/p"}}],"resourceType":"Parameters"}"#,
            "one identifier from the asked domain, then the Patient"
        );
    }

    #[test]
    fn with_no_target_every_other_identifier_is_answered() {
        let query = PixQuery::from_query("sourceIdentifier=urn:oid:2.999.1%7C1").expect("a query");
        let answer = cross_references(
            &query,
            &[
                identifier("urn:oid:2.999.1", "1"),
                identifier("urn:oid:2.999.1", "1b"),
                identifier("urn:oid:2.999.3", "3"),
            ],
            &[],
        );
        assert_eq!(
            answer.resource().parameter.len(),
            2,
            "the second identifier of the source domain stays"
        );
    }
}
