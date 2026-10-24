// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Operator sign-out at `POST /logout`: the session ended and its cookie
//! removed, the redirect to the provider's end-session endpoint with the
//! ID Token hint (OpenID Connect RP-Initiated Logout 1.0 §2), and the
//! same-origin check every unsafe request passes.

use std::error::Error;

use axum::body::Body;
use ferropix_testkit::oidc::{Provider, TokenAnswer};
use http::{Request, StatusCode};
use url::Url;

use crate::support::{
    CLIENT_ID, OPERATOR_TOKEN, ORIGIN, configuration, console, get, get_with, header, send,
    set_cookies, sign_in,
};

/// The server address of a console whose tests never reach the server.
const NO_SERVER: &str = "http://127.0.0.1:9/";

/// The post-logout redirect the console registers.
const AFTER: &str = "http://127.0.0.1:3000/";

/// A `POST /logout` carrying `cookie` and the request headers `from`.
fn sign_out(cookie: &str, from: &[(&str, &str)]) -> Result<Request<Body>, Box<dyn Error>> {
    let mut request = Request::post("/logout").header("cookie", cookie);
    for (name, value) in from {
        request = request.header(*name, *value);
    }
    Ok(request.body(Body::empty())?)
}

/// A provider, its console with or without the end-session endpoint, and a
/// signed-in session's cookie.
async fn signed_in(
    end_session: bool,
) -> Result<
    (
        Provider,
        ferropix_console::server::ConsoleState,
        axum::Router,
        String,
    ),
    Box<dyn Error>,
> {
    let provider = Provider::start().await?;
    let oidc = if end_session {
        format!(
            "end_session_endpoint = \"{}\"\npost_logout_redirect_uri = \"{AFTER}\"",
            provider.end_session_endpoint()
        )
    } else {
        String::new()
    };
    let (state, service) = console(&configuration(&provider, NO_SERVER, &oidc, ""))?;
    let cookie = sign_in(&service, &provider, TokenAnswer::bearer(OPERATOR_TOKEN)).await?;
    Ok((provider, state, service, cookie))
}

#[tokio::test]
async fn a_sign_out_ends_the_session_and_removes_the_cookie() -> Result<(), Box<dyn Error>> {
    let (_provider, state, service, cookie) = signed_in(false).await?;
    let (response, _body) = send(
        &service,
        sign_out(&cookie, &[("sec-fetch-site", "same-origin")])?,
    )
    .await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status());
    assert_eq!("/sign-in/signed-out", header(&response, "location"));
    assert_eq!("no-store", header(&response, "cache-control"));
    let cookies = set_cookies(&response);
    assert!(
        cookies
            .iter()
            .any(|set| set.starts_with("ferropix_console_session=;") && set.contains("Max-Age=0")),
        "{cookies:?}"
    );
    assert_eq!(0, state.sessions().occupancy()?.sessions);
    let (page, _body) = send(&service, get_with("/", &cookie)?).await?;
    assert_eq!(StatusCode::SEE_OTHER, page.status());
    let (_page, body) = send(&service, get("/sign-in/signed-out")?).await?;
    assert!(
        body.contains("You are signed out of this browser."),
        "{body}"
    );
    Ok(())
}

// OpenID Connect RP-Initiated Logout 1.0 §2: the request carries
// `id_token_hint`, `client_id` and the registered `post_logout_redirect_uri`.
#[tokio::test]
async fn a_sign_out_redirects_to_the_end_session_endpoint_with_the_id_token_hint()
-> Result<(), Box<dyn Error>> {
    let (provider, _state, service, cookie) = signed_in(true).await?;
    let (response, _body) = send(
        &service,
        sign_out(&cookie, &[("sec-fetch-site", "same-origin")])?,
    )
    .await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status());
    let location = Url::parse(header(&response, "location"))?;
    assert!(
        location
            .as_str()
            .starts_with(&provider.end_session_endpoint()),
        "{location}"
    );
    let pairs: std::collections::BTreeMap<String, String> =
        location.query_pairs().into_owned().collect();
    assert_eq!(Some(CLIENT_ID), pairs.get("client_id").map(String::as_str));
    assert_eq!(
        Some(AFTER),
        pairs.get("post_logout_redirect_uri").map(String::as_str)
    );
    let hint = pairs.get("id_token_hint").ok_or("an ID Token hint")?;
    assert_eq!(3, hint.split('.').count(), "a compact JWS");
    Ok(())
}

#[tokio::test]
async fn a_sign_out_without_a_session_reaches_the_provider_without_a_hint()
-> Result<(), Box<dyn Error>> {
    let (_provider, _state, service, _cookie) = signed_in(true).await?;
    let (response, _body) = send(
        &service,
        sign_out("other=1", &[("sec-fetch-site", "same-origin")])?,
    )
    .await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status());
    let location = header(&response, "location");
    assert!(!location.contains("id_token_hint"), "{location}");
    Ok(())
}

// The W3C Fetch Metadata Request Headers and RFC 6454 §7: an unsafe request
// from another site is refused before the session is read.
#[tokio::test]
async fn a_cross_site_sign_out_is_refused_and_the_session_lives_on() -> Result<(), Box<dyn Error>> {
    let (_provider, state, service, cookie) = signed_in(false).await?;
    for from in [
        vec![("sec-fetch-site", "cross-site")],
        vec![("sec-fetch-site", "same-site")],
        vec![("origin", "https://attacker.example.net")],
        vec![("referer", "https://attacker.example.net/page")],
        vec![("referer", "not a url")],
        Vec::new(),
    ] {
        let (response, _body) = send(&service, sign_out(&cookie, &from)?).await?;
        assert_eq!(StatusCode::FORBIDDEN, response.status(), "{from:?}");
        assert_eq!(1, state.sessions().occupancy()?.sessions, "{from:?}");
    }
    Ok(())
}

#[tokio::test]
async fn a_sign_out_from_the_consoles_own_origin_passes_without_fetch_metadata()
-> Result<(), Box<dyn Error>> {
    let (_provider, state, service, cookie) = signed_in(false).await?;
    let (response, _body) = send(&service, sign_out(&cookie, &[("origin", ORIGIN)])?).await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status());
    assert_eq!(0, state.sessions().occupancy()?.sessions);
    Ok(())
}

#[tokio::test]
async fn sign_out_is_never_a_get() -> Result<(), Box<dyn Error>> {
    let (_provider, state, service, cookie) = signed_in(false).await?;
    let (response, _body) = send(&service, get_with("/logout", &cookie)?).await?;
    assert!(response.status().is_client_error(), "{}", response.status());
    assert_eq!(1, state.sessions().occupancy()?.sessions);
    Ok(())
}

#[tokio::test]
async fn the_signed_in_page_offers_sign_out_as_a_post_form() -> Result<(), Box<dyn Error>> {
    let (_provider, _state, service, cookie) = signed_in(false).await?;
    let (_response, body) = send(&service, get_with("/", &cookie)?).await?;
    assert!(
        body.contains(r#"<form method="post" action="/logout""#),
        "{body}"
    );
    Ok(())
}

/// The `form-action` directive of `response`'s policy.
fn form_action(response: &http::Response<()>) -> String {
    header(response, "content-security-policy")
        .split(';')
        .map(str::trim)
        .find(|directive| directive.starts_with("form-action"))
        .unwrap_or_default()
        .to_owned()
}

// Content Security Policy Level 3, `form-action`: a browser holds the
// sign-out's redirect to the policy, so the provider's origin is in it.
#[tokio::test]
async fn the_policy_lets_the_sign_out_form_reach_the_end_session_endpoint()
-> Result<(), Box<dyn Error>> {
    let (provider, _state, service, cookie) = signed_in(true).await?;
    let (response, _body) = send(&service, get_with("/", &cookie)?).await?;
    let origin = Url::parse(&provider.end_session_endpoint())?
        .origin()
        .ascii_serialization();
    assert_eq!(
        format!("form-action 'self' {origin}"),
        form_action(&response)
    );
    let (_provider, _state, plain, cookie) = signed_in(false).await?;
    let (response, _body) = send(&plain, get_with("/", &cookie)?).await?;
    assert_eq!("form-action 'self'", form_action(&response));
    Ok(())
}
