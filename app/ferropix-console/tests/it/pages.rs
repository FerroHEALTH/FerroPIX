// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Every page of the design, signed in, against a FerroPIX server that does
//! not answer, one that answers but serves no operator API, and one that
//! declares its `CapabilityStatement`: each page draws its state from the
//! call the console made and never a row the server did not send.

use std::error::Error;

use ferrohealth_ui::icon::Glyph;
use http::StatusCode;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{
    Captured, OPERATOR_NAME, OPERATOR_TOKEN, console, get_with, offline, post_form, send,
    steward_session,
};

/// An address nothing listens on, so a call is refused at once.
const NOWHERE: &str = "http://127.0.0.1:9/";

/// Every page the console draws from the operator API, with words its
/// header carries.
const PAGES: [(&str, &str); 13] = [
    ("/", "Dashboard"),
    ("/queue", "Review queue"),
    (
        "/queue/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
        "Compare and decide",
    ),
    (
        "/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
        "Person 0192a1b2",
    ),
    (
        "/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b/merge",
        "Merge",
    ),
    (
        "/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b/unmerge",
        "Unmerge",
    ),
    ("/sources", "Sources"),
    ("/sources/lab-lis", "lab-lis"),
    ("/domains", "Identifier domains"),
    (
        "/domains/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5c",
        "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5c",
    ),
    ("/pseudonym-domains", "Pseudonymisation domains"),
    ("/configuration", "Configuration"),
    ("/audit", "Audit trail"),
];

#[tokio::test]
async fn every_page_says_the_server_refused_the_connection_with_the_way_on()
-> Result<(), Box<dyn Error>> {
    let (state, service) = console(&offline(NOWHERE))?;
    let cookie = steward_session(&state);
    for (route, words) in PAGES {
        let (response, page) = send(&service, get_with(route, &cookie)?).await?;
        assert_eq!(StatusCode::OK, response.status(), "{route}");
        assert!(page.contains(words), "{route} draws its header");
        assert!(
            page.contains("The FerroPIX server refused the connection"),
            "{route} names the true failure"
        );
        assert!(
            page.contains(r#"<span class="fui-sr-only">No answer:</span>"#),
            "{route} tells assistive technology no answer came"
        );
        assert!(page.contains("Try again"), "{route} offers Try again");
        assert!(
            page.contains("Technical detail"),
            "{route} folds the detail away"
        );
        assert!(
            page.contains("GET /health"),
            "{route} names the call it made"
        );
        assert!(
            page.contains("Unreachable"),
            "{route}'s status pill says so"
        );
        assert!(
            page.contains(OPERATOR_NAME),
            "{route}'s menu names the steward"
        );
        assert!(
            !page.contains(OPERATOR_TOKEN),
            "{route} never draws the token"
        );
    }
    Ok(())
}

#[tokio::test]
async fn the_dashboard_says_what_the_console_keeps_while_the_server_is_away()
-> Result<(), Box<dyn Error>> {
    let (state, service) = console(&offline(NOWHERE))?;
    let cookie = steward_session(&state);
    let (_, page) = send(&service, get_with("/", &cookie)?).await?;
    assert!(page.contains("While the server is away"));
    assert!(page.contains("This console keeps no patient data"));
    Ok(())
}

/// The pages that read a document of the operator API before they are
/// drawn.
const READING: [&str; 9] = [
    "/",
    "/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
    "/sources",
    "/sources/lab-lis",
    "/domains",
    "/domains/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5c",
    "/pseudonym-domains",
    "/configuration",
    "/audit",
];

#[tokio::test]
async fn a_server_whose_operator_api_refuses_a_page_is_named_and_an_unserved_page_gets_the_notice()
-> Result<(), Box<dyn Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health/liveness"))
        .and(header(
            "authorization",
            format!("Bearer {OPERATOR_TOKEN}").as_str(),
        ))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let (state, service) = console(&offline(&format!("{}/", server.uri())))?;
    let cookie = steward_session(&state);
    for (route, _) in PAGES {
        let (response, page) = send(&service, get_with(route, &cookie)?).await?;
        assert_eq!(StatusCode::OK, response.status(), "{route}");
        assert!(page.contains("Server up"), "{route}'s status pill says so");
        assert!(
            page.contains("/operator/"),
            "{route} names the route it reads"
        );
        if READING.contains(&route) {
            assert!(
                page.contains("refused the console") && page.contains("404"),
                "{route} names the refused call and its status"
            );
        } else {
            assert!(
                page.contains("serves no operator API yet"),
                "{route} says the server serves no operator API for it yet"
            );
        }
    }
    let (_, queue) = send(&service, get_with("/queue", &cookie)?).await?;
    assert!(queue.contains("GET /operator/candidates"));
    Ok(())
}

#[tokio::test]
async fn a_server_that_refuses_the_steward_is_named_with_its_status() -> Result<(), Box<dyn Error>>
{
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health/liveness"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let (state, service) = console(&offline(&format!("{}/", server.uri())))?;
    let cookie = steward_session(&state);
    let (_, page) = send(&service, get_with("/domains", &cookie)?).await?;
    assert!(page.contains("401: The FerroPIX server refused the console"));
    assert!(page.contains("Refused 401"), "the status pill says so");
    Ok(())
}

#[tokio::test]
async fn an_id_in_a_route_that_is_no_uuid_or_name_is_the_not_found_page()
-> Result<(), Box<dyn Error>> {
    let (state, service) = console(&offline(NOWHERE))?;
    let cookie = steward_session(&state);
    for route in [
        "/patients/123456789",
        "/queue/not-a-candidate",
        "/sources/a%20b",
        "/no-such-page",
        "/preferences/account",
    ] {
        let (response, page) = send(&service, get_with(route, &cookie)?).await?;
        assert_eq!(StatusCode::NOT_FOUND, response.status(), "{route}");
        assert!(page.contains("Page not found"), "{route}");
        assert!(page.contains("Back to the dashboard"), "{route}");
    }
    Ok(())
}

#[tokio::test]
async fn about_shows_the_versions_the_maker_and_the_capability_statement_as_sent()
-> Result<(), Box<dyn Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health/liveness"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let statement = r#"{"resourceType":"CapabilityStatement","status":"active","kind":"instance","software":{"name":"FerroPIX","version":"0.0.3"},"fhirVersion":"4.0.1"}"#;
    Mock::given(method("GET"))
        .and(path("/fhir/metadata"))
        .respond_with(ResponseTemplate::new(200).set_body_string(statement))
        .mount(&server)
        .await;
    let (state, service) = console(&offline(&format!("{}/", server.uri())))?;
    let cookie = steward_session(&state);
    let (_, page) = send(&service, get_with("/about", &cookie)?).await?;
    assert!(page.contains("FerroPIX 0.0.3"), "the server's version");
    assert!(page.contains("4.0.1"), "the FHIR version");
    assert!(
        page.contains(&format!(
            "ferrohealth-ui {}",
            ferropix_console::pages::about::FERROUI
        )),
        "the library"
    );
    assert!(page.contains("Cadasto B.V."), "the maker");
    assert!(page.contains("The CapabilityStatement"), "the card");
    assert!(
        page.contains("CapabilityStatement&quot;") || page.contains("CapabilityStatement\""),
        "the document"
    );
    let (download, body) = send(
        &service,
        get_with("/about/capability-statement.json", &cookie)?,
    )
    .await?;
    assert_eq!(StatusCode::OK, download.status());
    assert_eq!(statement, body, "Download hands over the document as sent");
    Ok(())
}

#[tokio::test]
async fn about_without_a_server_says_its_version_is_not_known() -> Result<(), Box<dyn Error>> {
    let (state, service) = console(&offline(NOWHERE))?;
    let cookie = steward_session(&state);
    let (_, page) = send(&service, get_with("/about", &cookie)?).await?;
    assert!(page.contains("Not known"));
    assert!(page.contains("its CapabilityStatement cannot be shown"));
    Ok(())
}

#[tokio::test]
async fn a_search_posts_its_fields_in_the_body_and_carries_nothing_typed_into_the_address()
-> Result<(), Box<dyn Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health/liveness"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/operator/search/identifier"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"total":0,"offset":0,"persons":[]}"#),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/operator/search/demographics"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"total":0,"offset":0,"persons":[]}"#),
        )
        .mount(&server)
        .await;
    let (state, service) = console(&offline(&format!("{}/", server.uri())))?;
    let cookie = steward_session(&state);
    let captured = Captured::default();
    let logging = captured.logging();
    // A synthetic identifier that fails the eleven-proof of every scheme it could resemble.
    let typed = "999999991";
    let (response, page) = send(
        &service,
        post_form(
            "/search",
            &cookie,
            &format!("segment=identifier&domain=urn%3Aoid%3A2.999.1.1&identifier={typed}"),
        )?,
    )
    .await?;
    assert_eq!(
        StatusCode::OK,
        response.status(),
        "the results are drawn in place"
    );
    assert!(
        response.headers().get("location").is_none(),
        "no redirect names a field"
    );
    assert!(page.contains("No patient matches"), "the empty results");
    assert!(
        !page.contains(typed),
        "the page does not repeat what was typed"
    );
    let (response, page) = send(
        &service,
        post_form(
            "/search/demographics",
            &cookie,
            "segment=demographics&family=Voorbeeld&born=1970-01-01",
        )?,
    )
    .await?;
    assert_eq!(StatusCode::OK, response.status());
    assert!(page.contains("No patient matches"));
    let received = server.received_requests().await.unwrap_or_default();
    for request in &received {
        assert!(
            request
                .url
                .query()
                .is_none_or(|query| !query.contains(typed)),
            "nothing typed reaches a query string"
        );
        assert!(
            !request.url.path().contains(typed),
            "nothing typed reaches a path"
        );
    }
    let identifier = received
        .iter()
        .find(|request| request.url.path() == "/operator/search/identifier")
        .expect("the identifier search reached the server");
    let body = String::from_utf8_lossy(&identifier.body);
    assert!(body.contains(typed), "the identifier travels in the body");
    let (_, page) = send(&service, get_with("/search", &cookie)?).await?;
    assert!(
        page.contains("type=\"password\""),
        "the identifier is masked as it is typed"
    );
    assert!(page.contains("method=\"post\""), "the search posts");
    drop(logging);
    let logged = captured.text();
    for (what, value) in [
        ("the identifier", typed),
        ("the family name", "Voorbeeld"),
        ("the birth date", "1970-01-01"),
    ] {
        assert!(!logged.contains(value), "a log line carries {what}");
    }
    Ok(())
}

#[tokio::test]
async fn the_shell_draws_the_navigation_the_search_and_the_menu_with_no_theme_toggle_in_the_top_bar()
-> Result<(), Box<dyn Error>> {
    let (state, service) = console(&offline(NOWHERE))?;
    let cookie = steward_session(&state);
    let (_, page) = send(&service, get_with("/queue", &cookie)?).await?;
    for words in [
        "Dashboard",
        "Review queue",
        "Search",
        "Sources",
        "Identifier domains",
        "Pseudonymisation domains",
        "Audit trail",
        "Configuration",
        "About",
        "FerroPIX",
        "Identifier, name or birth date",
        "Not known: the server did not answer",
        "Your preferences",
    ] {
        assert!(page.contains(words), "the shell draws {words}");
    }
    assert!(
        page.contains("aria-current=\"page\""),
        "the queue is marked current"
    );
    assert!(
        page.contains("action=\"/logout\""),
        "Sign out is a form post"
    );
    Ok(())
}

#[tokio::test]
async fn the_search_entry_of_the_navigation_draws_user_round_search() -> Result<(), Box<dyn Error>>
{
    let (state, service) = console(&offline(NOWHERE))?;
    let cookie = steward_session(&state);
    let (_, page) = send(&service, get_with("/queue", &cookie)?).await?;
    let start = page
        .find("href=\"/search\"")
        .ok_or("the navigation links Search")?;
    let entry = page
        .get(start..)
        .and_then(|rest| rest.split("</a>").next())
        .ok_or("the Search entry ends")?;
    assert!(
        entry.contains(Glyph::USER_ROUND_SEARCH.markup()),
        "the Search entry draws user-round-search: {entry}"
    );
    assert!(
        !entry.contains(Glyph::SEARCH.markup()),
        "the stand-in search glyph is gone: {entry}"
    );
    Ok(())
}

#[tokio::test]
async fn the_search_panel_offers_pages_and_domains_and_refuses_a_reader_with_no_session()
-> Result<(), Box<dyn Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/operator/domains"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"domains":[{"id":"0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5c","system":"urn:oid:2.999.1.1","name":"Synthetic hospital","kind":"local","sensitive":false,"source_client_id":"synthetic-source","created_at":"2026-10-20T08:00:00Z","records":1}]}"#,
        ))
        .mount(&server)
        .await;
    let (state, service) = console(&offline(&format!("{}/", server.uri())))?;
    let (response, _) = send(&service, post_form("/search/answer", "", "q=audit")?).await?;
    assert_eq!(
        StatusCode::UNAUTHORIZED,
        response.status(),
        "the panel loads the sign-in on a 401"
    );
    let cookie = steward_session(&state);
    let (response, answer) = send(
        &service,
        post_form("/search/answer", &cookie, "q=synthetic")?,
    )
    .await?;
    assert_eq!(StatusCode::OK, response.status());
    assert!(answer.contains("Synthetic hospital"), "a domain: {answer}");
    assert!(answer.contains("Identifier domains"), "its group: {answer}");
    let (_, answer) = send(&service, post_form("/search/answer", &cookie, "q=audit")?).await?;
    assert!(answer.contains("\"/audit\""), "a page: {answer}");
    Ok(())
}
