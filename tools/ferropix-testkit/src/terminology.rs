// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A FHIR R4 terminology server stub answering `ConceptMap/$translate` and
//! the identifier `NamingSystem` search.
//!
//! The HL7 v2 face translates every v2 table value on a terminology server
//! loaded with the HL7 v2-to-FHIR table maps and FerroBRIDGE's value
//! package. The stub holds the rows of those maps a PID reaches, keyed by
//! the map's canonical `url` and the code asked, and answers each as an R4
//! `$translate` `Parameters` (FHIR R4 `conceptmap-operation-translate`):
//! `result` true with one `equivalent` match, or `result` false for a pair
//! it does not hold, alone or as the entries of a `batch`. It records every
//! pair asked, so a suite can assert what reached the server. The search
//! `NamingSystem?kind=identifier&value=<authority>` (FHIR R4 namingsystem.html)
//! answers a `searchset` with one identifier `NamingSystem` for each
//! assigning authority it registers, whose `uri` unique id is the system the
//! HL7 v2 face writes, and an empty `searchset` for any other.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// The base of the HL7 v2-to-FHIR 1.0.0 `ConceptMap` canonical urls.
const GUIDE: &str = "http://hl7.org/fhir/uv/v2mappings/ConceptMap/";

/// The assigning authority of the synthetic hospital's PID-3 identifiers.
const EAST: &str = "EAST";

/// The base of FerroBRIDGE's value package's `ConceptMap` canonical urls.
const VALUES: &str = "https://ferrobridge.eu/fhir/v2mappings/ConceptMap/";

/// One table map's rows: its id, the target system, and each code with its
/// target code.
type Table = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

/// The guide's table maps a PID reaches, as the package writes them; a row
/// the package marks `unmatched` is left out, so the stub answers no match.
const GUIDE_TABLES: [Table; 6] = [
    (
        "table-hl70001-to-administrative-gender",
        "http://hl7.org/fhir/administrative-gender",
        &[
            ("F", "female"),
            ("M", "male"),
            ("O", "other"),
            ("U", "unknown"),
            ("A", "other"),
            ("N", "other"),
        ],
    ),
    (
        "table-hl70200-to-name-use",
        "http://hl7.org/fhir/name-use",
        &[
            ("BAD", "old"),
            ("D", "usual"),
            ("L", "official"),
            ("M", "maiden"),
            ("MSK", "anonymous"),
            ("N", "nickname"),
            ("NAV", "temp"),
            ("R", "official"),
            ("TEMP", "temp"),
        ],
    ),
    (
        "table-hl70190-to-address-use",
        "http://hl7.org/fhir/address-use",
        &[
            ("BA", "old"),
            ("BI", "billing"),
            ("C", "temp"),
            ("B", "work"),
            ("H", "home"),
            ("O", "work"),
        ],
    ),
    (
        "table-hl70190-to-address-type",
        "http://hl7.org/fhir/address-type",
        &[("M", "postal"), ("SH", "postal")],
    ),
    (
        "table-hl70201-to-contact-point-use",
        "http://hl7.org/fhir/contact-point-use",
        &[("PRN", "home"), ("WPN", "work"), ("PRS", "mobile")],
    ),
    // NOTE: the PID inverse reads the guide's 0201 map with `reverse`, so it asks by FHIR code.
    (
        "table-hl70201-to-contact-point-use",
        "http://terminology.hl7.org/CodeSystem/v2-0201",
        &[("home", "PRN"), ("work", "WPN"), ("mobile", "PRS")],
    ),
];

/// The maps of FerroBRIDGE's value package (`terminology/hl7v2-values/`).
const VALUE_TABLES: [Table; 7] = [
    (
        "table-hl70202-equipment-to-contact-point-system",
        "http://hl7.org/fhir/contact-point-system",
        &[
            ("PH", "phone"),
            ("CP", "phone"),
            ("FX", "fax"),
            ("MD", "other"),
            ("SAT", "other"),
            ("BP", "pager"),
            ("Internet", "email"),
            ("X.400", "email"),
            ("TDD", "other"),
            ("TTY", "other"),
        ],
    ),
    (
        "table-hl70202-to-contact-point-use",
        "http://hl7.org/fhir/contact-point-use",
        &[("CP", "mobile")],
    ),
    (
        "table-address-type-to-hl70190",
        "http://terminology.hl7.org/CodeSystem/v2-0190",
        &[("postal", "M")],
    ),
    (
        "table-address-use-to-hl70190",
        "http://terminology.hl7.org/CodeSystem/v2-0190",
        &[
            ("home", "H"),
            ("work", "B"),
            ("temp", "C"),
            ("old", "BA"),
            ("billing", "BI"),
        ],
    ),
    (
        "table-administrative-gender-to-hl70001",
        "http://terminology.hl7.org/CodeSystem/v2-0001",
        &[
            ("female", "F"),
            ("male", "M"),
            ("other", "O"),
            ("unknown", "U"),
        ],
    ),
    (
        "table-contact-point-system-to-hl70202",
        "http://terminology.hl7.org/CodeSystem/v2-0202",
        &[
            ("phone", "PH"),
            ("fax", "FX"),
            ("email", "Internet"),
            ("pager", "BP"),
        ],
    ),
    (
        "table-name-use-to-hl70200",
        "http://terminology.hl7.org/CodeSystem/v2-0200",
        &[
            ("usual", "D"),
            ("official", "L"),
            ("temp", "TEMP"),
            ("nickname", "N"),
            ("anonymous", "MSK"),
            ("old", "BAD"),
            ("maiden", "M"),
        ],
    ),
];

/// One pair asked: the `ConceptMap` url and the code.
pub type Asked = (String, String);

/// The rows the stub answers, and every pair it was asked.
#[derive(Debug, Clone, Default)]
pub struct Translations {
    answers: BTreeMap<Asked, (String, String)>,
    registry: BTreeMap<String, String>,
    asked: Arc<Mutex<Vec<Asked>>>,
}

impl Translations {
    /// The rows a PID reaches, both ways: the HL7 v2-to-FHIR 1.0.0 table maps
    /// of tables 0001, 0190, 0200 and 0201, and FerroBRIDGE's value package
    /// (0202 and the FHIR-to-v2 maps the PID inverse names).
    #[must_use]
    pub fn v2_pid() -> Self {
        let mut stub = Self::default();
        for (base, tables) in [(GUIDE, &GUIDE_TABLES[..]), (VALUES, &VALUE_TABLES[..])] {
            for (id, system, rows) in tables {
                for (code, target) in *rows {
                    stub = stub.answer(&format!("{base}{id}"), code, system, target);
                }
            }
        }
        stub.register(EAST, crate::synthetic::HOSPITAL_DOMAIN)
    }

    /// Registers the assigning authority `authority` as an identifier
    /// `NamingSystem` whose `uri` unique id is `system`.
    #[must_use]
    pub fn register(mut self, authority: &str, system: &str) -> Self {
        self.registry
            .insert(authority.to_owned(), system.to_owned());
        self
    }

    /// Adds the answer to `code` through the map at `url`.
    #[must_use]
    pub fn answer(mut self, url: &str, code: &str, system: &str, target: &str) -> Self {
        self.answers.insert(
            (url.to_owned(), code.to_owned()),
            (system.to_owned(), target.to_owned()),
        );
        self
    }

    /// Returns every pair asked, in order.
    #[must_use]
    pub fn asked(&self) -> Vec<Asked> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Mounts the stub on a new server and returns it with its R4 base URL.
    ///
    /// # Errors
    /// Returns the parse error when the server's URI is no URL, which
    /// wiremock never gives.
    pub async fn serve(&self) -> Result<(MockServer, url::Url), url::ParseError> {
        let server = MockServer::start().await;
        for route in ["/r4/ConceptMap/$translate", "/r4"] {
            Mock::given(method("POST"))
                .and(path(route))
                .respond_with(self.clone())
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/r4/NamingSystem"))
            .and(query_param("kind", "identifier"))
            .respond_with(Registry(self.registry.clone()))
            .mount(&server)
            .await;
        let base = url::Url::parse(&format!("{}/r4", server.uri()))?;
        Ok((server, base))
    }
}

/// The `Parameters` a `$translate` request carries, read for its `url` and
/// `code`.
#[derive(Debug, Default, Deserialize)]
struct Asking {
    #[serde(default)]
    parameter: Vec<In>,
}

/// One input parameter.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct In {
    name: String,
    value_uri: Option<String>,
    value_code: Option<String>,
}

impl Asking {
    /// The value of the parameter `name`, of either type.
    fn value(&self, name: &str) -> String {
        self.parameter
            .iter()
            .find(|parameter| parameter.name == name)
            .and_then(|parameter| {
                parameter
                    .value_uri
                    .clone()
                    .or_else(|| parameter.value_code.clone())
            })
            .unwrap_or_default()
    }
}

/// The `Parameters` a `$translate` answers.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Answer {
    resource_type: &'static str,
    parameter: Vec<Out>,
}

/// One output parameter.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Out {
    name: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_boolean: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_string: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_coding: Option<Coding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    part: Vec<Out>,
}

impl Out {
    fn named(name: &'static str) -> Self {
        Self {
            name,
            value_boolean: None,
            value_string: None,
            value_code: None,
            value_coding: None,
            part: Vec::new(),
        }
    }
}

/// A `Coding` with its system and code.
#[derive(Debug, Serialize)]
struct Coding {
    system: String,
    code: String,
}

impl Respond for Translations {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let received: Received = serde_json::from_slice(&request.body).unwrap_or_default();
        let response =
            ResponseTemplate::new(200).insert_header("content-type", "application/fhir+json");
        if received.resource_type != "Bundle" {
            return response.set_body_json(self.reply(&Asking {
                parameter: received.parameter,
            }));
        }
        // NOTE: FHIR R4 http.html §3.1.0.11, a batch-response answers each entry in request order.
        let entry = received
            .entry
            .iter()
            .map(|entry| Answered {
                resource: self.reply(&entry.resource),
                response: Status { status: "200 OK" },
            })
            .collect();
        response.set_body_json(BatchResponse {
            resource_type: "Bundle",
            kind: "batch-response",
            entry,
        })
    }
}

impl Translations {
    /// Records the pair `asking` names and returns its `$translate` answer.
    fn reply(&self, asking: &Asking) -> Answer {
        let key = (asking.value("url"), asking.value("code"));
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(key.clone());
        let parameter = match self.answers.get(&key) {
            Some((system, code)) => vec![
                Out {
                    value_boolean: Some(true),
                    ..Out::named("result")
                },
                Out {
                    part: vec![
                        Out {
                            value_code: Some("equivalent"),
                            ..Out::named("equivalence")
                        },
                        Out {
                            value_coding: Some(Coding {
                                system: system.clone(),
                                code: code.clone(),
                            }),
                            ..Out::named("concept")
                        },
                    ],
                    ..Out::named("match")
                },
            ],
            None => vec![
                Out {
                    value_boolean: Some(false),
                    ..Out::named("result")
                },
                Out {
                    value_string: Some("no mapping for the code"),
                    ..Out::named("message")
                },
            ],
        };
        Answer {
            resource_type: "Parameters",
            parameter,
        }
    }
}

/// A request body: a `$translate` `Parameters`, or a `batch` `Bundle` of
/// them.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Received {
    #[serde(default)]
    resource_type: String,
    #[serde(default)]
    parameter: Vec<In>,
    #[serde(default)]
    entry: Vec<Entry>,
}

/// One entry of a `batch`, its `Parameters`.
#[derive(Debug, Deserialize)]
struct Entry {
    #[serde(default)]
    resource: Asking,
}

/// The `batch-response` `Bundle` of a `batch`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchResponse {
    resource_type: &'static str,
    #[serde(rename = "type")]
    kind: &'static str,
    entry: Vec<Answered>,
}

/// One entry of a `batch-response`: the answer and its status.
#[derive(Debug, Serialize)]
struct Answered {
    resource: Answer,
    response: Status,
}

/// The status of one `batch-response` entry.
#[derive(Debug, Serialize)]
struct Status {
    status: &'static str,
}

/// The identifier `NamingSystem` registry the stub answers searches from:
/// each assigning authority with the `uri` of its system.
#[derive(Debug, Clone)]
struct Registry(BTreeMap<String, String>);

/// The `searchset` a `NamingSystem` search answers.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Searchset {
    resource_type: &'static str,
    r#type: &'static str,
    total: usize,
    // NOTE: FHIR R4 json.html §2.6.2, an empty array is never written, so a search with no match carries no entry.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    entry: Vec<Found>,
}

/// One entry of the `searchset`.
#[derive(Debug, Serialize)]
struct Found {
    resource: Naming,
}

/// An identifier `NamingSystem` (FHIR R4 namingsystem.html).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Naming {
    resource_type: &'static str,
    name: String,
    status: &'static str,
    kind: &'static str,
    date: &'static str,
    unique_id: Vec<UniqueId>,
}

/// One unique id of a `NamingSystem`.
#[derive(Debug, Serialize)]
struct UniqueId {
    r#type: &'static str,
    value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    preferred: Option<bool>,
}

impl Respond for Registry {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let value = request
            .url
            .query_pairs()
            .find(|(name, _)| name == "value")
            .map(|(_, value)| value.into_owned())
            .unwrap_or_default();
        let entry: Vec<Found> = self
            .0
            .get(&value)
            .map(|system| Found {
                resource: Naming {
                    resource_type: "NamingSystem",
                    name: value.clone(),
                    status: "active",
                    kind: "identifier",
                    date: "2026-11-01",
                    unique_id: vec![
                        UniqueId {
                            r#type: "other",
                            value: value.clone(),
                            preferred: None,
                        },
                        UniqueId {
                            r#type: "uri",
                            value: system.clone(),
                            preferred: Some(true),
                        },
                    ],
                },
            })
            .into_iter()
            .collect();
        ResponseTemplate::new(200)
            .insert_header("content-type", "application/fhir+json")
            .set_body_json(Searchset {
                resource_type: "Bundle",
                r#type: "searchset",
                total: entry.len(),
                entry,
            })
    }
}
