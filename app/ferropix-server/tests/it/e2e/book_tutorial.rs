// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The book's tutorial, "Your first integration", run as written, behind
//! `FERROPIX_E2E`.
//!
//! Each `http` block after a `<!-- book-request: NAME ... -->` marker is a
//! request the reader sends, in page order, to the server of
//! `quickstart.toml`, started through the shipped `serve_settings` over
//! PostgreSQL, with the client credentials token the quickstart's issuer
//! signs for `quickstart-source`. A marker states the status the answer
//! carries (`expect=`), the JSON pointers it holds (`holds=/pointer=value`),
//! the text it contains (`contains=`) and the text it must not contain
//! (`lacks=`). A page edit that changes what the server answers, or a server
//! change that breaks a documented request, fails here.
//! No specification governs the marker format: our own design.

use std::collections::BTreeMap;
use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use ferrohealth_telemetry::section::TelemetrySection;
use ferrohealth_telemetry::telemetry::{Service, Telemetry};
use ferropix_testkit::containers::{e2e_enabled, postgres, stub_provider};

use super::app_role;

/// The tutorial page.
const PAGE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../website/book/src/tutorials/first-integration.md"
));

/// The quickstart's token step, which the page includes.
const TOUR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scripts/quickstart/tour.sh"
));

/// The issuer `quickstart.toml` names, replaced by the stub provider's.
const QUICKSTART_ISSUER: &str = "http://localhost:8180/ferropix";

/// The FHIR base the quickstart's tokens are addressed to.
const QUICKSTART_AUDIENCE: &str = "http://localhost:8080/fhir";

/// The Source client and the scopes of the page's token.
const SOURCE: &str = "quickstart-source";
const SCOPES: &str = "system/Patient.cud system/Patient.rs";

/// The fewest requests the page holds; a page that loses its steps fails.
const MIN_REQUESTS: usize = 6;

/// A request the reader sends and what its answer must hold.
#[derive(Debug, Default)]
struct Step {
    /// The marker's name.
    name: String,
    /// The status the answer carries.
    expect: u16,
    /// The JSON pointers the answer holds, each with its value as text.
    holds: Vec<(String, String)>,
    /// Text the answer's body contains.
    contains: Vec<String>,
    /// Text the answer's body must not contain.
    lacks: Vec<String>,
    /// The `http` block: the request line, the headers and the body.
    block: String,
}

/// Reads one marker's attributes and the block after it.
fn step(marker: &str, block: String) -> Result<Step, Box<dyn Error>> {
    let mut words = marker.split_whitespace();
    let name = words.next().ok_or("a request marker names its request")?;
    let mut parsed = Step {
        name: name.to_owned(),
        block,
        ..Step::default()
    };
    for word in words {
        let (key, value) = word
            .split_once('=')
            .ok_or_else(|| format!("{name}: `{word}` is no key=value"))?;
        match key {
            "expect" => parsed.expect = value.parse()?,
            "holds" => {
                let (pointer, wanted) = value
                    .split_once('=')
                    .ok_or_else(|| format!("{name}: `{value}` is no /pointer=value"))?;
                parsed.holds.push((pointer.to_owned(), wanted.to_owned()));
            }
            "contains" => parsed.contains.push(value.to_owned()),
            "lacks" => parsed.lacks.push(value.to_owned()),
            _ => return Err(format!("{name}: unknown attribute `{key}`").into()),
        }
    }
    if parsed.expect == 0 {
        return Err(format!("{name} states no expected status").into());
    }
    Ok(parsed)
}

/// Reads the page's requests, in page order.
fn steps(page: &str) -> Result<Vec<Step>, Box<dyn Error>> {
    let mut found = Vec::new();
    let mut lines = page.lines();
    while let Some(line) = lines.next() {
        let Some(marker) = line
            .trim()
            .strip_prefix("<!-- book-request:")
            .and_then(|rest| rest.strip_suffix("-->"))
        else {
            continue;
        };
        let open = lines
            .by_ref()
            .find(|next| !next.trim().is_empty())
            .ok_or_else(|| format!("{marker} is followed by no block"))?;
        if open.trim() != "```http" {
            return Err(format!("{marker} is followed by `{open}`, not an http block").into());
        }
        let mut block = String::new();
        let mut closed = false;
        for inner in lines.by_ref() {
            if inner.starts_with("```") {
                closed = true;
                break;
            }
            block.push_str(inner);
            block.push('\n');
        }
        if !closed {
            return Err(format!("the block after {marker} never closes").into());
        }
        found.push(step(marker, block)?);
    }
    Ok(found)
}

/// Sends the request of `step` to `base` with `token` and checks its answer.
#[expect(
    clippy::disallowed_types,
    reason = "the test seam: a documented answer is read as JSON to follow its pointers"
)]
async fn run(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    step: &Step,
) -> Result<(), Box<dyn Error>> {
    let name = &step.name;
    let (head, body) = step
        .block
        .split_once("\n\n")
        .unwrap_or((step.block.as_str(), ""));
    let mut head_lines = head.lines();
    let request_line = head_lines.next().ok_or("a request line")?;
    let mut parts = request_line.split_whitespace();
    let method: reqwest::Method = parts.next().ok_or("a method")?.parse()?;
    let target = parts.next().ok_or("a request target")?;
    let mut request = client.request(method, format!("{base}{target}"));
    for header in head_lines {
        let (key, value) = header
            .split_once(':')
            .ok_or_else(|| format!("{name}: `{header}` is no header"))?;
        if key.eq_ignore_ascii_case("host") {
            continue;
        }
        request = request.header(key.trim(), value.trim().replace("$token", token));
    }
    if !body.trim().is_empty() {
        request = request.body(body.to_owned());
    }
    let response = request.send().await?;
    let status = response.status().as_u16();
    let text = response.text().await?;
    assert_eq!(
        step.expect, status,
        "{name}: the answer was {status}: {text}"
    );
    for wanted in &step.contains {
        assert!(
            text.contains(wanted.as_str()),
            "{name}: the answer holds no `{wanted}`"
        );
    }
    for unwanted in &step.lacks {
        assert!(
            !text.contains(unwanted.as_str()),
            "{name}: the answer holds `{unwanted}`"
        );
    }
    if !step.holds.is_empty() {
        let json: serde_json::Value = serde_json::from_str(&text)?;
        for (pointer, wanted) in &step.holds {
            let found = json
                .pointer(pointer)
                .ok_or_else(|| format!("{name}: the answer holds no {pointer}"))?;
            let found = found
                .as_str()
                .map_or_else(|| found.to_string(), str::to_owned);
            assert_eq!(wanted, &found, "{name}: {pointer}");
        }
    }
    Ok(())
}

#[test]
fn the_page_reads_into_its_requests() {
    let found = steps(PAGE).expect("the page reads");
    assert!(
        found.len() >= MIN_REQUESTS,
        "the page holds {} requests",
        found.len()
    );
    assert!(
        PAGE.contains("{{#include ../../../../scripts/quickstart/tour.sh:token}}"),
        "the page takes its token step from the quickstart"
    );
    let token_step = TOUR
        .split("# ANCHOR: token")
        .nth(1)
        .and_then(|rest| rest.split("# ANCHOR_END: token").next())
        .expect("the tour holds the token anchor");
    assert!(token_step.contains(&format!("client_id={SOURCE}")));
    assert!(token_step.contains(&format!("scope={SCOPES}")));
    assert!(step("broken expect=200 nonsense", String::new()).is_err());
    assert!(step("unstated", String::new()).is_err());
}

#[tokio::test]
async fn every_request_of_the_tutorial_answers_as_the_page_says() {
    if !e2e_enabled() {
        return;
    }
    let found = steps(PAGE).expect("the page reads");
    let provider = stub_provider(QUICKSTART_AUDIENCE)
        .await
        .expect("the stub provider starts");
    let postgres = postgres().await.expect("PostgreSQL starts");
    let url = app_role(&postgres).await;
    let files = tempfile::tempdir().expect("a temporary directory");
    let url_file = files.path().join("db-url");
    std::fs::write(&url_file, &url).expect("the URL file is written");
    let mut key = [0_u8; 32];
    aws_lc_rs::rand::fill(&mut key).expect("random bytes");
    let key_file = files.path().join("data-key");
    std::fs::write(
        &key_file,
        base64::engine::general_purpose::STANDARD.encode(key),
    )
    .expect("the key file is written");
    let port = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        probe.local_addr().expect("an address").port()
    };
    let environment = BTreeMap::from([
        (
            String::from("FERROPIX__DB__URL_FILE"),
            url_file.display().to_string(),
        ),
        (
            String::from("FERROPIX__DB__DATA_KEY_FILE"),
            key_file.display().to_string(),
        ),
        (
            String::from("FERROPIX__SERVER__LISTEN"),
            format!("127.0.0.1:{port}"),
        ),
        (
            String::from("FERROPIX__AUDIT__SPOOL_DIR"),
            files.path().join("spool").display().to_string(),
        ),
        (
            String::from("FERROPIX__USAGE_REPORT__ENABLED"),
            String::from("false"),
        ),
    ]);
    let quickstart =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/quickstart.toml"))
            .expect("quickstart.toml is read");
    assert!(
        quickstart.contains(QUICKSTART_ISSUER),
        "the quickstart's issuer"
    );
    let settings = ferropix_server::config::Config::from_sources(
        Some(&quickstart.replace(QUICKSTART_ISSUER, &provider.issuer())),
        &environment,
    )
    .expect("the quickstart reads")
    .resolve()
    .expect("the quickstart resolves");
    let telemetry = Arc::new(
        Telemetry::new(
            &TelemetrySection::default(),
            Service::new("ferropix", "0.0.0"),
        )
        .expect("the telemetry"),
    );
    tokio::spawn(ferropix_server::serve::serve_settings(settings, telemetry));
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let ready = client
            .get(format!("{base}/health/readiness"))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success());
        if ready {
            break;
        }
        assert!(Instant::now() < deadline, "the server never became ready");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let token = provider
        .client_credentials(SOURCE, SCOPES)
        .await
        .expect("the provider signs the Source's token");
    for step in &found {
        run(&client, &base, &token, step)
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", step.name));
    }
}
