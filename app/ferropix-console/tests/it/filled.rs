// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Every page drawn from a fixed operator API document: the dashboard, the
//! patient record, the sources, the domains, the configuration and the
//! audit trail, each with its document, empty, and with one part refused,
//! against a stand-in server that answers the routes each page reads.
//! Every value is synthetic, and a feed or message row is held to show no
//! identifier value.

use std::collections::BTreeMap;
use std::error::Error;

use ferropix_operator_api::actions::{Action, Actions};
use ferropix_operator_api::configuration::{Configuration, Mode, Origin, Section, Setting};
use ferropix_operator_api::domain::{Domain, DomainPage, Domains, Feed, FeedOutcome};
use ferropix_operator_api::health::{Dependencies, IndicatorState, Readiness, State};
use ferropix_operator_api::overview::{DayCount, HourFeeds, Overview, Queue, QueueEntry};
use ferropix_operator_api::patient::{
    Channel, Demographics, DomainKind, DomainRef, Grade, Masked, Name, PersonStatus,
};
use ferropix_operator_api::person::{
    HistoryEntry, HistoryEvent, IdentifierHandle, IdentifierRow, Link, LinkKind, Person,
    PseudonymRow, RecordState, SourceRecord,
};
use ferropix_operator_api::source::{Admission, Message, Source, SourcePage, Sources};
use http::StatusCode;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{console, get_with, offline, post_form, send, steward_session};

/// The person every patient case reads.
const PERSON: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";

/// The domain every domain case reads.
const DOMAIN: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5c";

/// A synthetic identifier value that never appears whole on a page.
const HELD_VALUE: &str = "SYNTH-4711";

/// A stand-in server that answers `/health` and each of `documents` by its
/// route with its JSON.
async fn server(documents: &[(&str, &str, String)]) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    for (verb, route, body) in documents {
        Mock::given(method(*verb))
            .and(path(*route))
            .respond_with(ResponseTemplate::new(200).set_body_string(body.clone()))
            .mount(&server)
            .await;
    }
    server
}

/// The page at `route` drawn against `server`.
async fn page(server: &MockServer, route: &str) -> Result<String, Box<dyn Error>> {
    let (state, service) = console(&offline(&format!("{}/", server.uri())))?;
    let cookie = steward_session(&state);
    let (response, page) = send(&service, get_with(route, &cookie)?).await?;
    assert_eq!(StatusCode::OK, response.status(), "{route}");
    Ok(page)
}

/// A synthetic domain reference.
fn hospital() -> DomainRef {
    DomainRef {
        id: String::from(DOMAIN),
        system: String::from("urn:oid:2.999.1.1"),
        name: String::from("Synthetic hospital"),
        kind: DomainKind::Local,
    }
}

/// Synthetic demographics.
fn demographics(family: &str, born: &str) -> Demographics {
    Demographics {
        names: vec![Name {
            family: Some(family.to_owned()),
            given: vec![String::from("Ada")],
        }],
        birth_date: Some(born.to_owned()),
        gender: Some(String::from("female")),
        addresses: Vec::new(),
        telecom: Vec::new(),
    }
}

/// The overview with every card filled.
fn overview() -> Overview {
    Overview {
        queue: Queue {
            open: 7,
            waiting_over_week: 2,
            deferred: 1,
        },
        records_created_24h: 10,
        records_linked_24h: 4,
        versions_fed_today: 31,
        subscriptions_in_error: 1,
        queue_depth: vec![
            DayCount {
                day: String::from("2026-10-24"),
                open: 5,
            },
            DayCount {
                day: String::from("2026-10-25"),
                open: 7,
            },
        ],
        feeds_per_hour: vec![
            HourFeeds {
                hour: String::from("2026-10-25T08:00:00Z"),
                fhir: 3,
                v2: 2,
                pmir: 0,
            },
            HourFeeds {
                hour: String::from("2026-10-25T09:00:00Z"),
                fhir: 1,
                v2: 4,
                pmir: 1,
            },
        ],
        oldest: vec![QueueEntry {
            candidate: String::from("0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5d"),
            created_at: String::from("2026-10-17T10:00:00Z"),
            score: 0.81,
            grade: Grade::Probable,
        }],
    }
}

/// The dependencies with the MLLP listener in `mllp`.
fn dependencies(mllp: State) -> Dependencies {
    let mut indicators = BTreeMap::new();
    indicators.insert(
        String::from("database"),
        IndicatorState {
            state: State::Up,
            detail: None,
        },
    );
    indicators.insert(
        String::from("hl7v2_listener"),
        IndicatorState {
            state: mllp,
            detail: (mllp == State::Down)
                .then(|| String::from("the MLLP listener is not accepting")),
        },
    );
    Dependencies {
        readiness: Readiness {
            state: mllp,
            indicators,
        },
        upstreams: Vec::new(),
    }
}

#[tokio::test]
async fn the_dashboard_draws_every_card_and_a_refused_overview_keeps_the_listeners()
-> Result<(), Box<dyn Error>> {
    let filled = server(&[
        (
            "GET",
            "/operator/overview",
            serde_json::to_string(&overview())?,
        ),
        (
            "GET",
            "/operator/dependencies",
            serde_json::to_string(&dependencies(State::Down))?,
        ),
    ])
    .await;
    let drawn = page(&filled, "/").await?;
    for words in [
        "Open in the queue",
        "2 over 7 days",
        "40 %",
        "Versions fed today",
        "Subscriptions in error",
        "Queue depth",
        "Feeds per hour",
        "Listeners",
        "MLLP",
        "Down",
        "Oldest in the queue",
        "Open the queue",
    ] {
        assert!(drawn.contains(words), "the dashboard shows {words}");
    }
    assert!(drawn.contains("<svg"), "the charts are SVG");
    let mut empty = overview();
    empty.queue = Queue {
        open: 0,
        waiting_over_week: 0,
        deferred: 0,
    };
    empty.oldest = Vec::new();
    empty.queue_depth = Vec::new();
    let quiet = server(&[
        ("GET", "/operator/overview", serde_json::to_string(&empty)?),
        (
            "GET",
            "/operator/dependencies",
            serde_json::to_string(&dependencies(State::Up))?,
        ),
    ])
    .await;
    let drawn = page(&quiet, "/").await?;
    assert!(drawn.contains("The queue is empty."));
    assert!(drawn.contains("The queue has no history yet."));
    let refused = server(&[(
        "GET",
        "/operator/dependencies",
        serde_json::to_string(&dependencies(State::Up))?,
    )])
    .await;
    let drawn = page(&refused, "/").await?;
    assert!(
        drawn.contains("refused the console"),
        "the overview's refusal in place"
    );
    assert!(
        drawn.contains("Listeners"),
        "one refusal never blanks the page"
    );
    Ok(())
}

/// A person with one record, one carried identifier, a history and a
/// pseudonym, or merged.
fn person(merged: bool) -> Person {
    Person {
        id: String::from(PERSON),
        status: if merged {
            PersonStatus::Merged
        } else {
            PersonStatus::Active
        },
        merged_into: merged.then(|| String::from("0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5e")),
        created_at: String::from("2026-10-20T08:00:00Z"),
        leading: (!merged).then(|| demographics("Proefsma", "1980-02-29")),
        identifiers: vec![IdentifierRow {
            handle: IdentifierHandle::Record(String::from("record-1")),
            domain: hospital(),
            masked: Masked(String::from("•••••••711")),
            state: RecordState::Active,
            last_fed: Some(String::from("2026-10-24T09:00:00Z")),
            link: Some(Link {
                kind: LinkKind::Feed,
                made_by: String::from("east-lab"),
                made_at: String::from("2026-10-20T08:00:00Z"),
            }),
        }],
        records: vec![SourceRecord {
            id: String::from("record-1"),
            domain: hospital(),
            version: 2,
            received_at: String::from("2026-10-24T09:00:00Z"),
            channel: Channel::V2,
            state: RecordState::Active,
            demographics: demographics("Proefsma", "1980-03-01"),
        }],
        history: vec![HistoryEntry {
            at: String::from("2026-10-24T09:00:00Z"),
            actor: Some(String::from("east-lab")),
            reason: None,
            event: HistoryEvent::Version {
                record: String::from("record-1"),
                version: 2,
                channel: Channel::V2,
            },
        }],
        history_truncated: false,
        pseudonyms: vec![PseudonymRow {
            domain: String::from("pseudonym-domain-1"),
            namespace: String::from("urn:example:ehr"),
            name: String::from("Synthetic EHR"),
            masked: Some(Masked(String::from("••••••••••c3d"))),
        }],
    }
}

#[tokio::test]
async fn the_patient_record_draws_its_four_tabs_marks_a_differing_field_and_keeps_values_masked()
-> Result<(), Box<dyn Error>> {
    let route = format!("/operator/persons/{PERSON}");
    let filled = server(&[(
        "GET",
        route.as_str(),
        serde_json::to_string(&person(false))?,
    )])
    .await;
    let base = format!("/patients/{PERSON}");
    let identifiers = page(&filled, &base).await?;
    assert!(identifiers.contains("Synthetic hospital"));
    assert!(identifiers.contains("711"), "the visible tail");
    assert!(!identifiers.contains(HELD_VALUE), "never the whole value");
    assert!(identifiers.contains("method=\"post\""), "a reveal posts");
    assert!(identifiers.contains("Reason"), "a reveal asks the reason");
    let records = page(&filled, &format!("{base}?tab=records")).await?;
    assert!(
        records.contains(">differs<"),
        "the birth date differs in words"
    );
    let history = page(&filled, &format!("{base}?tab=history")).await?;
    assert!(history.contains("Version received"));
    let pseudonyms = page(&filled, &format!("{base}?tab=pseudonyms")).await?;
    assert!(pseudonyms.contains("Synthetic EHR"));
    let merged = server(&[("GET", route.as_str(), serde_json::to_string(&person(true))?)]).await;
    let drawn = page(&merged, &base).await?;
    assert!(
        drawn.contains("Merged into"),
        "a merged person names its survivor"
    );
    Ok(())
}

#[tokio::test]
async fn a_refused_reveal_is_drawn_in_place_with_the_record() -> Result<(), Box<dyn Error>> {
    let route = format!("/operator/persons/{PERSON}");
    let filled = server(&[(
        "GET",
        route.as_str(),
        serde_json::to_string(&person(false))?,
    )])
    .await;
    let (state, service) = console(&offline(&format!("{}/", filled.uri())))?;
    let cookie = steward_session(&state);
    let (response, drawn) = send(
        &service,
        post_form(
            &format!("/patients/{PERSON}/reveal"),
            &cookie,
            "target=record&id=record-1&reason=Synthetic+check",
        )?,
    )
    .await?;
    assert_eq!(StatusCode::OK, response.status());
    assert!(
        drawn.contains("refused the console"),
        "the reveal's refusal"
    );
    assert!(
        drawn.contains("Synthetic hospital"),
        "the record is drawn beside it"
    );
    Ok(())
}

#[tokio::test]
async fn the_sources_and_a_source_draw_their_admission_and_messages_with_no_value()
-> Result<(), Box<dyn Error>> {
    let source = Source {
        id: String::from("EAST.ADT"),
        name: String::from("EAST|ADT"),
        channel: Channel::V2,
        admission: Admission::RegisteredSender {
            application: String::from("ADT"),
            facility: String::from("EAST"),
        },
        domains: vec![String::from("urn:oid:2.999.1.1")],
        last_activity: Some(String::from("2026-10-25T09:00:00Z")),
    };
    let listing = Sources {
        sources: vec![source.clone()],
        file: Some(String::from("ferropix.toml")),
    };
    let one = SourcePage {
        source,
        messages: vec![Message {
            at: String::from("2026-10-25T09:00:00Z"),
            channel: Channel::V2,
            action: String::from("message"),
            outcome: String::from("AE"),
        }],
    };
    let filled = server(&[
        ("GET", "/operator/sources", serde_json::to_string(&listing)?),
        (
            "GET",
            "/operator/sources/EAST.ADT",
            serde_json::to_string(&one)?,
        ),
    ])
    .await;
    let drawn = page(&filled, "/sources").await?;
    assert!(drawn.contains("EAST|ADT"));
    assert!(drawn.contains("HL7 v2 sender ADT at EAST"));
    assert!(
        drawn.contains("ferropix.toml"),
        "the file the sources are set in"
    );
    let drawn = page(&filled, "/sources/EAST.ADT").await?;
    assert!(drawn.contains("Last messages"));
    assert!(drawn.contains(">AE<"), "the outcome");
    let empty = server(&[(
        "GET",
        "/operator/sources",
        serde_json::to_string(&Sources {
            sources: Vec::new(),
            file: None,
        })?,
    )])
    .await;
    assert!(
        page(&empty, "/sources")
            .await?
            .contains("No source is configured")
    );
    Ok(())
}

#[tokio::test]
async fn the_domains_and_a_domain_draw_their_definition_and_feeds_with_no_value()
-> Result<(), Box<dyn Error>> {
    let domain = Domain {
        id: String::from(DOMAIN),
        system: String::from("urn:oid:2.999.1.1"),
        name: String::from("Synthetic hospital"),
        kind: DomainKind::Local,
        sensitive: false,
        source_client_id: String::from("east-lab"),
        created_at: String::from("2026-10-20T08:00:00Z"),
        retired_at: None,
        records: 12,
        last_fed: Some(String::from("2026-10-25T09:00:00Z")),
    };
    let listing = Domains {
        domains: vec![domain.clone()],
        next: None,
    };
    let one = DomainPage {
        domain,
        feeds: vec![Feed {
            at: String::from("2026-10-25T09:00:00Z"),
            channel: Channel::Fhir,
            outcome: FeedOutcome::Created,
        }],
    };
    let route = format!("/operator/domains/{DOMAIN}");
    let filled = server(&[
        ("GET", "/operator/domains", serde_json::to_string(&listing)?),
        ("GET", route.as_str(), serde_json::to_string(&one)?),
    ])
    .await;
    let drawn = page(&filled, "/domains").await?;
    assert!(drawn.contains("Synthetic hospital"));
    assert!(drawn.contains("urn:oid:2.999.1.1"));
    let drawn = page(&filled, &format!("/domains/{DOMAIN}")).await?;
    assert!(drawn.contains("Created a record"));
    assert!(!drawn.contains(HELD_VALUE), "a feed row shows no value");
    let empty = server(&[(
        "GET",
        "/operator/domains",
        serde_json::to_string(&Domains {
            domains: Vec::new(),
            next: None,
        })?,
    )])
    .await;
    assert!(
        page(&empty, "/domains")
            .await?
            .contains("No identifier domains yet")
    );
    Ok(())
}

#[tokio::test]
async fn the_configuration_marks_a_value_the_environment_set_and_one_nothing_set()
-> Result<(), Box<dyn Error>> {
    let configuration = Configuration {
        mode: Mode::File,
        file: Some(String::from("ferropix.toml")),
        sections: vec![Section {
            name: String::from("thresholds"),
            settings: vec![
                Setting {
                    key: String::from("matching.link_at"),
                    value: String::from("0.95"),
                    origin: Origin::Environment,
                },
                Setting {
                    key: String::from("matching.review_at"),
                    value: String::from("0.7"),
                    origin: Origin::Default,
                },
            ],
        }],
    };
    let filled = server(&[(
        "GET",
        "/operator/configuration",
        serde_json::to_string(&configuration)?,
    )])
    .await;
    let drawn = page(&filled, "/configuration").await?;
    assert!(drawn.contains("ferropix.toml"));
    assert!(drawn.contains("From the environment"));
    assert!(drawn.contains("Default"));
    assert!(drawn.contains("matching.link_at"), "the key hint");
    Ok(())
}

#[tokio::test]
async fn the_trail_draws_its_actions_its_filters_as_form_fields_and_its_empty_state()
-> Result<(), Box<dyn Error>> {
    let actions = Actions {
        actions: vec![Action {
            id: String::from("action-1"),
            kind: String::from("reveal"),
            actor: String::from("Synthetic Steward"),
            at: String::from("2026-10-25T09:00:00Z"),
            person: Some(String::from(PERSON)),
            record: Some(String::from("record-1")),
            candidate: None,
            merge: None,
            domain: None,
            pseudonym_domain: None,
            reason: Some(String::from("Synthetic check")),
            request_id: Some(String::from("request-1")),
        }],
        next: Some(String::from("cursor-1")),
    };
    let filled = server(&[("GET", "/operator/actions", serde_json::to_string(&actions)?)]).await;
    let drawn = page(&filled, "/audit").await?;
    assert!(drawn.contains("Synthetic Steward"));
    assert!(
        drawn.contains(&format!("/patients/{PERSON}")),
        "the person as a link"
    );
    assert!(
        drawn.contains("name=\"actor\""),
        "every filter is a form field"
    );
    assert!(drawn.contains("Older actions"), "the next page posts");
    assert!(drawn.contains("Export as CSV"));
    let empty = server(&[(
        "GET",
        "/operator/actions",
        serde_json::to_string(&Actions {
            actions: Vec::new(),
            next: None,
        })?,
    )])
    .await;
    assert!(page(&empty, "/audit").await?.contains("No action matches"));
    Ok(())
}

#[tokio::test]
async fn a_refused_domain_create_shows_the_server_s_issue_in_place() -> Result<(), Box<dyn Error>> {
    let filled = server(&[(
        "GET",
        "/operator/domains",
        serde_json::to_string(&Domains {
            domains: Vec::new(),
            next: None,
        })?,
    )])
    .await;
    Mock::given(method("POST"))
        .and(path("/operator/domains"))
        .respond_with(ResponseTemplate::new(409).set_body_string(
            r#"{"resourceType":"OperationOutcome","issue":[{"severity":"error","code":"conflict","diagnostics":"a domain with this system exists"}]}"#,
        ))
        .mount(&filled)
        .await;
    let (state, service) = console(&offline(&format!("{}/", filled.uri())))?;
    let cookie = steward_session(&state);
    let (response, drawn) = send(
        &service,
        post_form(
            "/domains",
            &cookie,
            "system=urn%3Aoid%3A2.999.1.1&name=Synthetic&kind=local&source=east-lab&reason=Synthetic",
        )?,
    )
    .await?;
    assert_eq!(StatusCode::OK, response.status());
    assert!(
        drawn.contains("a domain with this system exists"),
        "the server's issue"
    );
    assert!(drawn.contains("409"), "the status");
    assert!(drawn.contains("Add a domain"), "the form stays");
    Ok(())
}
