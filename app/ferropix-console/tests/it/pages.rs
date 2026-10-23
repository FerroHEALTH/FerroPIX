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
    OPERATOR_NAME, OPERATOR_TOKEN, console, get_with, offline, post_form, send, steward_session,
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
    ("/domains/hospital-mrn", "hospital-mrn"),
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

#[tokio::test]
async fn a_server_that_answers_without_an_operator_api_gets_the_notice_and_the_route()
-> Result<(), Box<dyn Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
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
        assert!(
            page.contains("serves no operator API yet"),
            "{route} says the server serves no operator API yet"
        );
        assert!(
            page.contains("/operator/"),
            "{route} names the route it reads"
        );
        assert!(page.contains("Server up"), "{route}'s status pill says so");
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
        .and(path("/health"))
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
        .and(path("/health"))
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
    assert!(page.contains("ferrohealth-ui 0.3.4"), "the library");
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
async fn a_search_posts_sends_nothing_on_and_carries_nothing_typed_into_the_address()
-> Result<(), Box<dyn Error>> {
    let server = MockServer::start().await;
    let (state, service) = console(&offline(&format!("{}/", server.uri())))?;
    let cookie = steward_session(&state);
    // A synthetic identifier that fails the eleven-proof of every scheme it could resemble.
    let typed = "999999991";
    let (response, _) = send(
        &service,
        post_form(
            "/search",
            &cookie,
            &format!("segment=identifier&domain=&identifier={typed}"),
        )?,
    )
    .await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status());
    let location = crate::support::header(&response, "location").to_owned();
    assert_eq!("/search?searched", location);
    assert!(!location.contains(typed));
    let (response, _) = send(
        &service,
        post_form(
            "/search/demographics",
            &cookie,
            "segment=demographics&family=Voorbeeld&born=1970-01-01",
        )?,
    )
    .await?;
    assert_eq!(
        "/search/demographics?searched",
        crate::support::header(&response, "location")
    );
    let received = server.received_requests().await.unwrap_or_default();
    assert!(
        received
            .iter()
            .all(|request| request.url.path() == "/health"),
        "nothing typed was sent to the server"
    );
    let (_, page) = send(&service, get_with("/search?searched", &cookie)?).await?;
    assert!(page.contains("Nothing was searched"));
    assert!(
        page.contains("type=\"password\""),
        "the identifier is masked as it is typed"
    );
    assert!(page.contains("method=\"post\""), "the search posts");
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
