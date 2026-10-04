// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-83 request: `GET [base]/Patient/$ihe-pix?sourceIdentifier=…
//! {&targetSystem=…}` (§2:3.83.4.1.2), or the same input parameters posted to
//! `[base]/Patient/$ihe-pix` in a `Parameters` body (FHIR R4 Operations,
//! §3.2.0.1, <http://hl7.org/fhir/R4/operations.html#executing>).

use fhir_types::r4::parameters::{Parameters, ParametersParameter, ParametersParameterValue};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::search::{self, escape};

use super::Invocation;
use super::error::InvalidInput;
use super::identifier::{SourceIdentifier, TargetSystem};

/// The `$ihe-pix` operation on the Manager's `Patient` type, relative to the
/// FHIR base (the `OperationDefinition`: `resource` `Patient`, `type` level).
const OPERATION: &str = "Patient/$ihe-pix";

/// The `[base]/Patient/$ihe-pix` URL for the FHIR base `base`.
pub(super) fn endpoint(base: Url) -> Result<Url, InvalidInput> {
    search::under_base(base, OPERATION).ok_or(InvalidInput::Base)
}

/// One ITI-83 request as the client sends it.
///
/// Both forms hold the source identifier, so a request is handed to the HTTP
/// client and to the audit record, and never kept, logged or put into an
/// error.
pub(super) enum Request {
    /// `GET` of this URL, whose query holds the input parameters.
    Get(Url),
    /// `POST` to the operation's endpoint of this `Parameters` resource, as
    /// FHIR JSON; the URL holds no input parameter.
    Post(SecretString),
}

/// The request that asks about `source` in the domains `targets` at
/// `endpoint`, in the form `invocation` names.
///
/// # Errors
/// The JSON writer's error when the `Parameters` resource of a `POST` cannot
/// be written.
pub(super) fn request(
    endpoint: &Url,
    invocation: Invocation,
    source: &SourceIdentifier,
    targets: &[TargetSystem],
) -> Result<Request, serde_json::Error> {
    match invocation {
        Invocation::Get => Ok(Request::Get(query(endpoint, source, targets))),
        Invocation::Post => serde_json::to_string(&parameters(source, targets))
            .map(|json| Request::Post(SecretString::from(json))),
    }
}

/// The `sourceIdentifier` value: `<system>|<value>`, a token whose parts
/// escape the characters FHIR search gives a meaning to (§2:3.83.4.1.2.1).
fn source_token(source: &SourceIdentifier) -> SecretString {
    SecretString::from(format!(
        "{}|{}",
        escape(source.system()),
        escape(source.value().expose_secret())
    ))
}

/// The request URL: one `sourceIdentifier` and one `targetSystem` per domain
/// asked about (§2:3.83.4.1.2.1, §2:3.83.4.1.2.2).
fn query(endpoint: &Url, source: &SourceIdentifier, targets: &[TargetSystem]) -> Url {
    let mut url = endpoint.clone();
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("sourceIdentifier", source_token(source).expose_secret());
        for target in targets {
            pairs.append_pair("targetSystem", &escape(target.as_str()));
        }
    }
    url
}

/// The `Parameters` resource of a `POST`: the same input parameters as the
/// URL of a `GET`, each a `valueString`, as the Query Parameters In profile
/// and the IG's request example give them.
fn parameters(source: &SourceIdentifier, targets: &[TargetSystem]) -> Parameters {
    let string = |name: &str, value: &str| ParametersParameter {
        name: name.into(),
        value: Some(ParametersParameterValue::String(value.into())),
        ..ParametersParameter::default()
    };
    let mut parameter = vec![string(
        "sourceIdentifier",
        source_token(source).expose_secret(),
    )];
    parameter.extend(
        targets
            .iter()
            .map(|target| string("targetSystem", &escape(target.as_str()))),
    );
    Parameters {
        parameter,
        ..Parameters::default()
    }
}

#[cfg(test)]
mod tests {
    use fhir_types::codec::{Json, Path, Value};
    use fhir_types::r4::parameters::{Parameters, ParametersParameterValue};
    use secrecy::{ExposeSecret, SecretString};
    use url::Url;

    use super::{Request, endpoint, request};
    use crate::pixm::Invocation;
    use crate::pixm::error::InvalidInput;
    use crate::pixm::identifier::{SourceIdentifier, TargetSystem};

    #[test]
    fn the_endpoint_is_the_type_level_operation_under_the_base() {
        for base in [
            "https://pix.example.org/fhir",
            "https://pix.example.org/fhir/",
        ] {
            let url = endpoint(Url::parse(base).expect("a URL")).expect("an endpoint");
            assert_eq!(
                url.as_str(),
                "https://pix.example.org/fhir/Patient/$ihe-pix",
                "{base}"
            );
        }
    }

    #[test]
    fn a_base_with_a_query_or_another_scheme_is_refused() {
        for base in [
            "https://pix.example.org/fhir?x=1",
            "https://pix.example.org/fhir#top",
            "ftp://pix.example.org/fhir",
            "urn:oid:2.999.1",
        ] {
            assert_eq!(
                endpoint(Url::parse(base).expect("a URL")).err(),
                Some(InvalidInput::Base),
                "{base} is no FHIR base"
            );
        }
    }

    fn asked() -> (Url, SourceIdentifier, [TargetSystem; 2]) {
        let source = SourceIdentifier::new("urn:oid:2.999.1", SecretString::from("x|1"))
            .expect("a source identifier");
        let targets = [
            TargetSystem::new("urn:oid:2.999.2").expect("a target"),
            TargetSystem::new("urn:oid:2.999.3").expect("a target"),
        ];
        let base = endpoint(Url::parse("https://pix.example.org/fhir/").expect("a URL"))
            .expect("an endpoint");
        (base, source, targets)
    }

    #[test]
    fn the_query_names_the_source_once_and_each_target() {
        let (base, source, targets) = asked();
        let Ok(Request::Get(url)) = request(&base, Invocation::Get, &source, &targets) else {
            panic!("a GET");
        };
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert_eq!(
            pairs,
            [
                (
                    "sourceIdentifier".to_owned(),
                    r"urn:oid:2.999.1|x\|1".to_owned()
                ),
                ("targetSystem".to_owned(), "urn:oid:2.999.2".to_owned()),
                ("targetSystem".to_owned(), "urn:oid:2.999.3".to_owned()),
            ],
            "the value's own | is escaped, the separator is not"
        );
    }

    #[test]
    fn the_posted_parameters_name_the_source_once_and_each_target() {
        let (base, source, targets) = asked();
        let Ok(Request::Post(body)) = request(&base, Invocation::Post, &source, &targets) else {
            panic!("a POST");
        };
        let value: Value = serde_json::from_str(body.expose_secret()).expect("JSON");
        let object = value.as_object().expect("an object");
        assert_eq!(
            object.get("resourceType").and_then(Value::as_str),
            Some("Parameters")
        );
        let parameters =
            Parameters::from_json(object, &mut Path::root("Parameters")).expect("an R4 Parameters");
        let sent: Vec<(Option<&str>, Option<&str>)> = parameters
            .parameter
            .iter()
            .map(|parameter| {
                let value = match &parameter.value {
                    Some(ParametersParameterValue::String(text)) => text.value.as_deref(),
                    _ => None,
                };
                (parameter.name.value.as_deref(), value)
            })
            .collect();
        assert_eq!(
            sent,
            [
                (Some("sourceIdentifier"), Some(r"urn:oid:2.999.1|x\|1")),
                (Some("targetSystem"), Some("urn:oid:2.999.2")),
                (Some("targetSystem"), Some("urn:oid:2.999.3")),
            ],
            "the values of a GET, each a valueString in the body"
        );
    }
}
