// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-104 from the Patient Identity Source (feature `pixm-source`): the
//! conditional update and delete of PIXm 3.1.0 §2:3.104.4 as sent, each
//! answer FHIR R4's conditional interactions define read into its typed
//! outcome, the refusals made before anything is sent, and, with
//! `pixm-server`, each message read back by this crate's own Manager.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the requests and the vendored examples are read as JSON values"
)]

use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::patient::Patient;
use http::StatusCode;
use ihe_atna::user::OnBehalfOf;
use ihe_iti::outcome::IssueType;
use ihe_iti::pixm::identifier::SourceIdentifier;
use ihe_iti::pixm::source::error::{InvalidFeed, SourceError};
use ihe_iti::pixm::source::{Fed, PixmSourceClient, Removed};
use secrecy::{ExposeSecret as _, SecretString};
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{BASE, BLUE, FHIR_JSON, PROMPT, RED, RED_VALUE, outcome, vendored};
use crate::outbound::outbound;

/// The `[base]/Patient` path the conditional interactions go to.
const PATIENTS: &str = "/fhir/Patient";

/// The query of the IG's example condition, `identifier=<red>|IHERED-994`,
/// percent-encoded as an RFC 3986 query component.
const CONDITION: &str = "identifier=urn%3Aoid%3A1.3.6.1.4.1.21367.13.20.1000%7CIHERED-994";

/// The subsumed identifier of the IG's Resolve Duplicate Patient example.
const MAIDEN_VALUE: &str = "IHERED-m94";

/// Text of a Manager's `OperationOutcome` that no error may carry.
const MANAGER_TEXT: &str = "Patient IHERED-994 MOHR refused";

/// A Source client for `server`.
pub(crate) fn source(server: &MockServer) -> PixmSourceClient {
    PixmSourceClient::new(outbound(&format!("{}{BASE}", server.uri()))).expect("a client")
}

/// A Source client for a FHIR base nothing listens on.
pub(crate) fn unreachable_source() -> PixmSourceClient {
    PixmSourceClient::new(outbound(&format!("http://127.0.0.1:0{BASE}"))).expect("a client")
}

/// The identifier `value` in the red domain.
pub(crate) fn red(value: &str) -> SourceIdentifier {
    SourceIdentifier::new(RED, SecretString::from(value)).expect("an identifier")
}

/// The vendored example Patient `file`, under `package/example/`.
pub(crate) fn example(file: &str) -> Patient {
    serde_json::from_str(&vendored(&format!("example/{file}"))).expect("an R4 Patient")
}

/// The IG's Add Patient example, Alissa Mohr in the red domain.
pub(crate) fn alissa() -> Patient {
    example("Patient-Patient-MohrAlissa-Red.json")
}

/// The IG's subsumed patient, Maiden Alice in the red domain.
pub(crate) fn maiden() -> Patient {
    example("Patient-Patient-MaidenAlice-Red.json")
}

/// A stub Manager that answers every `method` on `[base]/Patient` with
/// `response`.
pub(crate) async fn manager(verb: &str, response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method(verb))
        .and(path(PATIENTS))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

/// The one request `server` received.
async fn received(server: &MockServer) -> wiremock::Request {
    let requests = server.received_requests().await.expect("requests");
    let [request] = requests.as_slice() else {
        panic!("one request, got {}", requests.len());
    };
    request.clone()
}

/// Checks no rendering of `error` carries an identifier, a demographic or
/// the Manager's text.
fn names_nothing(error: &SourceError) {
    let shown = format!("{error:?} {error}");
    for value in [RED_VALUE, MAIDEN_VALUE, "MOHR", MANAGER_TEXT] {
        assert!(!shown.contains(value), "{shown}");
    }
}

#[tokio::test]
async fn an_add_patient_is_the_conditional_update_the_transaction_names() {
    let server = manager("PUT", ResponseTemplate::new(201)).await;
    source(&server)
        .feed(&red(RED_VALUE), &alissa(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("applied");
    let request = received(&server).await;
    assert_eq!(request.url.path(), PATIENTS, "PUT [base]/Patient");
    assert_eq!(request.url.query(), Some(CONDITION), "§2:3.104.4.1.2");
    for header in ["content-type", "accept"] {
        assert_eq!(
            request
                .headers
                .get(header)
                .and_then(|value| value.to_str().ok()),
            Some(FHIR_JSON),
            "{header}"
        );
    }
    let sent: Value = serde_json::from_slice(&request.body).expect("JSON");
    let expected: Value =
        serde_json::from_str(&vendored("example/Patient-Patient-MohrAlissa-Red.json"))
            .expect("JSON");
    assert_eq!(sent["resourceType"], "Patient");
    assert_eq!(sent["identifier"], expected["identifier"]);
    assert_eq!(sent["name"], expected["name"]);
    assert!(
        sent.get("link").is_none(),
        "an Add Patient carries no replaced-by link"
    );
}

#[tokio::test]
async fn a_created_and_an_updated_answer_are_each_their_outcome() {
    let server = manager(
        "PUT",
        ResponseTemplate::new(201).insert_header("Location", "Patient/p-1/_history/1"),
    )
    .await;
    let fed = source(&server)
        .feed(&red(RED_VALUE), &alissa(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("applied");
    let Fed::Created(receipt) = &fed else {
        panic!("a 201 is a create: {fed:?}");
    };
    assert_eq!(
        receipt
            .location()
            .expect("the Location")
            .reference()
            .expose_secret(),
        "Patient/p-1/_history/1"
    );
    assert!(
        !format!("{fed:?}").contains("p-1"),
        "Debug redacts the location"
    );
    let server = manager(
        "PUT",
        ResponseTemplate::new(200).set_body_raw(
            r#"{"resourceType":"OperationOutcome","issue":[{"severity":"information","code":"informational"}]}"#,
            FHIR_JSON,
        ),
    )
    .await;
    let fed = source(&server)
        .feed(&red(RED_VALUE), &alissa(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("applied");
    assert!(
        matches!(fed, Fed::Updated(_)),
        "a 200 is an update: {fed:?}"
    );
    assert!(fed.receipt().location().is_none());
}

#[tokio::test]
async fn a_criterion_matching_several_resources_is_not_selective() {
    let server = manager(
        "PUT",
        ResponseTemplate::new(412)
            .set_body_raw(outcome("multiple-matches", MANAGER_TEXT), FHIR_JSON),
    )
    .await;
    let error = source(&server)
        .feed(&red(RED_VALUE), &alissa(), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("not applied");
    let SourceError::NotSelective { issues } = &error else {
        panic!("a 412: {error:?}");
    };
    assert_eq!(issues, &[IssueType::MultipleMatches]);
    names_nothing(&error);
}

#[tokio::test]
async fn every_other_answer_is_a_rejection_with_its_status_and_issues() {
    for (status, code) in [
        (400, "invalid"),
        (401, "login"),
        (403, "forbidden"),
        (404, "not-found"),
        (409, "conflict"),
        (422, "business-rule"),
        (500, "exception"),
        (202, "informational"),
        (204, "informational"),
        (302, "informational"),
    ] {
        let server = manager(
            "PUT",
            ResponseTemplate::new(status).set_body_raw(outcome(code, MANAGER_TEXT), FHIR_JSON),
        )
        .await;
        let error = source(&server)
            .feed(&red(RED_VALUE), &alissa(), &OnBehalfOf::System, PROMPT)
            .await
            .expect_err("not applied");
        let SourceError::Rejected {
            status: answered,
            issues,
        } = &error
        else {
            panic!("{status}: {error:?}");
        };
        assert_eq!(answered.as_u16(), status);
        if status != 204 {
            assert_eq!(issues, &[IssueType::from_code(code)], "{status}");
        }
        names_nothing(&error);
    }
}

#[tokio::test]
async fn a_rejection_without_an_operation_outcome_carries_no_issues() {
    let server = manager(
        "PUT",
        ResponseTemplate::new(400).set_body_raw("not FHIR", "text/plain"),
    )
    .await;
    let error = source(&server)
        .feed(&red(RED_VALUE), &alissa(), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("not applied");
    assert!(
        matches!(&error, SourceError::Rejected { status, issues } if *status == StatusCode::BAD_REQUEST && issues.is_empty()),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_resolve_duplicate_links_the_subsumed_patient_to_the_survivor() {
    let server = manager("PUT", ResponseTemplate::new(200)).await;
    source(&server)
        .resolve_duplicate(
            &red(MAIDEN_VALUE),
            &red(RED_VALUE),
            &maiden(),
            &OnBehalfOf::System,
            PROMPT,
        )
        .await
        .expect("applied");
    let request = received(&server).await;
    assert_eq!(
        request.url.query(),
        Some("identifier=urn%3Aoid%3A1.3.6.1.4.1.21367.13.20.1000%7CIHERED-m94"),
        "the update is on the subsumed identifier (§2:3.104.4.2.2)"
    );
    let sent: Value = serde_json::from_slice(&request.body).expect("JSON");
    let resolved: Value = serde_json::from_str(&vendored(
        "example/Patient-Patient-MohrMaidenResolvedByMohrMalice-Red.json",
    ))
    .expect("JSON");
    assert_eq!(
        sent["link"], resolved["link"],
        "the link the IG's example carries"
    );
    assert_eq!(sent["identifier"], resolved["identifier"]);
}

#[tokio::test]
async fn a_message_the_transaction_does_not_admit_is_never_sent() {
    let server = manager("PUT", ResponseTemplate::new(201)).await;
    let client = source(&server);
    let resolved = example("Patient-Patient-MohrMaidenResolvedByMohrMalice-Red.json");
    let mut anonymous = alissa();
    anonymous.identifier.clear();
    let mut implicit = alissa();
    implicit.implicit_rules = Some("https://rules.example.org".into());
    let mut incomplete = alissa();
    incomplete.identifier.push(Identifier {
        system: Some(BLUE.into()),
        ..Identifier::default()
    });
    for (identifier, patient, refusal) in [
        (red(RED_VALUE), maiden(), InvalidFeed::IdentifierNotCarried),
        (red(RED_VALUE), anonymous, InvalidFeed::Profile),
        (red(RED_VALUE), implicit, InvalidFeed::Profile),
        (red(RED_VALUE), incomplete, InvalidFeed::Profile),
        (red(MAIDEN_VALUE), resolved.clone(), InvalidFeed::ReplacedBy),
    ] {
        let error = client
            .feed(&identifier, &patient, &OnBehalfOf::System, PROMPT)
            .await
            .expect_err("refused");
        assert!(
            matches!(error, SourceError::Invalid(found) if found == refusal),
            "{refusal:?}: {error:?}"
        );
    }
    let blue = SourceIdentifier::new(BLUE, SecretString::from(RED_VALUE)).expect("an identifier");
    for (survivor, patient, refusal) in [
        (blue, maiden(), InvalidFeed::SurvivorInAnotherDomain),
        (red(MAIDEN_VALUE), maiden(), InvalidFeed::SurvivorIsSubsumed),
        (red(RED_VALUE), resolved, InvalidFeed::ReplacedBy),
        (red(RED_VALUE), alissa(), InvalidFeed::IdentifierNotCarried),
    ] {
        let error = client
            .resolve_duplicate(
                &red(MAIDEN_VALUE),
                &survivor,
                &patient,
                &OnBehalfOf::System,
                PROMPT,
            )
            .await
            .expect_err("refused");
        assert!(
            matches!(error, SourceError::Invalid(found) if found == refusal),
            "{refusal:?}: {error:?}"
        );
        names_nothing(&error);
    }
    assert!(
        server
            .received_requests()
            .await
            .expect("requests")
            .is_empty(),
        "nothing was sent"
    );
}

#[tokio::test]
async fn a_remove_patient_is_the_conditional_delete_and_each_answer_its_outcome() {
    for (status, expected) in [
        (200, Removed::Deleted),
        (204, Removed::Deleted),
        (202, Removed::Accepted),
    ] {
        let server = manager("DELETE", ResponseTemplate::new(status)).await;
        let removed = source(&server)
            .remove(&red(RED_VALUE), &OnBehalfOf::System, PROMPT)
            .await
            .expect("applied");
        assert_eq!(removed, expected, "{status}");
        let request = received(&server).await;
        assert_eq!(request.url.path(), PATIENTS, "DELETE [base]/Patient");
        assert_eq!(request.url.query(), Some(CONDITION), "§2:3.104.4.3.2");
        assert!(request.body.is_empty(), "a delete has no body");
    }
    for (status, code) in [(405, "not-supported"), (409, "conflict")] {
        let server = manager(
            "DELETE",
            ResponseTemplate::new(status).set_body_raw(outcome(code, MANAGER_TEXT), FHIR_JSON),
        )
        .await;
        let error = source(&server)
            .remove(&red(RED_VALUE), &OnBehalfOf::System, PROMPT)
            .await
            .expect_err("not applied");
        assert!(
            matches!(&error, SourceError::Rejected { status: answered, .. } if answered.as_u16() == status),
            "{error:?}"
        );
        names_nothing(&error);
    }
    let server = manager(
        "DELETE",
        ResponseTemplate::new(412)
            .set_body_raw(outcome("multiple-matches", MANAGER_TEXT), FHIR_JSON),
    )
    .await;
    let error = source(&server)
        .remove(&red(RED_VALUE), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("not applied");
    assert!(
        matches!(error, SourceError::NotSelective { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn no_answer_is_never_an_applied_message() {
    let client = unreachable_source();
    let error = client
        .feed(&red(RED_VALUE), &alissa(), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("no Manager");
    assert!(matches!(error, SourceError::Transport(_)), "{error:?}");
    let error = client
        .remove(&red(RED_VALUE), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("no Manager");
    assert!(matches!(error, SourceError::Transport(_)), "{error:?}");
}

#[test]
fn the_client_shows_its_base_without_credentials() {
    let client = PixmSourceClient::new(outbound("https://source:s3cret@pix.example.org/fhir"))
        .expect("a client");
    assert_eq!(
        client.endpoint().as_str(),
        "https://source:s3cret@pix.example.org/fhir/Patient"
    );
    let shown = format!("{client:?}");
    assert!(!shown.contains("s3cret"), "{shown}");
}

/// The Manager side of this crate answering each message the client sends.
#[cfg(feature = "pixm-server")]
mod against_the_manager {
    use ihe_atna::user::OnBehalfOf;
    use ihe_iti::pixm::server::feed::{Feed, Removal};
    use ihe_iti::pixm::source::{Fed, Removed};
    use ihe_iti::server::format::Format;
    use secrecy::ExposeSecret as _;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    use super::{MAIDEN_VALUE, PATIENTS, alissa, maiden, red, source};
    use crate::pixm::{FHIR_JSON, PROMPT, RED_VALUE};

    /// What the Manager read.
    #[derive(Debug, PartialEq, Eq)]
    enum Read {
        Revised(String),
        Merged(String, String),
        Removed(String),
    }

    /// A Manager that reads each request with the crate's ITI-104 readers,
    /// answers `201` for an update and `200` for a delete, and refuses what
    /// they refuse, keeping what it read.
    #[derive(Clone, Default)]
    struct Manager(std::sync::Arc<std::sync::Mutex<Vec<Read>>>);

    impl Respond for Manager {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let query = request.url.query().unwrap_or_default();
            let read = if request.method.as_str() == "DELETE" {
                Removal::from_delete(query).map(|removal| {
                    Read::Removed(removal.identifier().value().expose_secret().to_owned())
                })
            } else {
                Feed::from_update(query, Format::Json, &request.body).map(|feed| match feed {
                    Feed::Revise(revision) => {
                        Read::Revised(revision.identifier().value().expose_secret().to_owned())
                    }
                    Feed::ResolveDuplicate(merge) => Read::Merged(
                        merge.subsumed().value().expose_secret().to_owned(),
                        merge.survivor().value().expose_secret().to_owned(),
                    ),
                    other => panic!("an ITI-104 message: {other:?}"),
                })
            };
            match read {
                Ok(read) => {
                    let status = if matches!(read, Read::Removed(_)) {
                        200
                    } else {
                        201
                    };
                    self.0.lock().expect("the reads").push(read);
                    ResponseTemplate::new(status)
                }
                Err(error) => {
                    let answer = error.refusal();
                    ResponseTemplate::new(answer.status().as_u16())
                        .set_body_raw(answer.encode(Format::Json).expect("JSON"), FHIR_JSON)
                }
            }
        }
    }

    async fn served(manager: &Manager) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(path(PATIENTS))
            .respond_with(manager.clone())
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn the_manager_reads_every_message_the_source_sends() {
        let manager = Manager::default();
        let server = served(&manager).await;
        let client = source(&server);
        let fed = client
            .feed(&red(RED_VALUE), &alissa(), &OnBehalfOf::System, PROMPT)
            .await
            .expect("applied");
        assert!(matches!(fed, Fed::Created(_)), "{fed:?}");
        client
            .resolve_duplicate(
                &red(MAIDEN_VALUE),
                &red(RED_VALUE),
                &maiden(),
                &OnBehalfOf::System,
                PROMPT,
            )
            .await
            .expect("applied");
        let removed = client
            .remove(&red(RED_VALUE), &OnBehalfOf::System, PROMPT)
            .await
            .expect("applied");
        assert_eq!(removed, Removed::Deleted);
        assert_eq!(
            *manager.0.lock().expect("the reads"),
            [
                Read::Revised(RED_VALUE.to_owned()),
                Read::Merged(MAIDEN_VALUE.to_owned(), RED_VALUE.to_owned()),
                Read::Removed(RED_VALUE.to_owned()),
            ]
        );
    }

    #[tokio::test]
    async fn a_value_with_the_characters_search_gives_a_meaning_reads_back_whole() {
        let manager = Manager::default();
        let server = served(&manager).await;
        let value = "R,1|2$3\\4 5+6\u{e9}";
        let mut patient = alissa();
        patient.identifier[0].value = Some(value.into());
        source(&server)
            .feed(&red(value), &patient, &OnBehalfOf::System, PROMPT)
            .await
            .expect("applied");
        source(&server)
            .remove(&red(value), &OnBehalfOf::System, PROMPT)
            .await
            .expect("applied");
        assert_eq!(
            *manager.0.lock().expect("the reads"),
            [
                Read::Revised(value.to_owned()),
                Read::Removed(value.to_owned())
            ]
        );
    }
}
