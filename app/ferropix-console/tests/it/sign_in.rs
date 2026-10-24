// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Operator sign-in against the test OpenID Provider: the authorization
//! request `GET /login` redirects to (RFC 6749 §4.1.1, RFC 7636 §4.3), the
//! pending sign-in it keeps on the server, the callback holding the returned
//! `state` to it (RFC 6749 §10.12), the code exchanged with the PKCE
//! verifier (RFC 7636 §4.5), and the ID Token held to the provider's keys,
//! its issuer, its audience and the sign-in's `nonce` (OpenID Connect Core
//! 1.0 §3.1.3.7) before a session begins.

use std::error::Error;

use base64::Engine as _;
use ferrohealth_ui::i18n::Locale;
use ferropix_console::locale::{Msg, catalog, tr_in};
use ferropix_console::session::Occupancy;
use ferropix_testkit::oidc::{IdTokenClaims, Provider, TokenAnswer};
use http::StatusCode;
use jsonwebtoken::{Algorithm, Header};
use secrecy::ExposeSecret as _;

use crate::support::{
    CLIENT_ID, Captured, OPERATOR_NAME, OPERATOR_TOKEN, REDIRECT_URI, begin, complete,
    configuration, console, get, get_with, header, send, session_cookie, set_cookies, sign_in,
};

/// The server address of a console whose tests never reach the server.
const NO_SERVER: &str = "http://127.0.0.1:9/";

/// A provider and the console configured to sign in at it.
async fn provider_console(
    oidc: &str,
    session: &str,
) -> Result<
    (
        Provider,
        ferropix_console::server::ConsoleState,
        axum::Router,
    ),
    Box<dyn Error>,
> {
    let provider = Provider::start().await?;
    let (state, service) = console(&configuration(&provider, NO_SERVER, oidc, session))?;
    Ok((provider, state, service))
}

/// Answers the code exchange of `begun` with an ID Token of `claims`.
async fn answers(provider: &Provider, claims: &IdTokenClaims) -> Result<(), Box<dyn Error>> {
    let answer = TokenAnswer {
        expires_in: Some(300),
        id_token: Some(provider.id_token(claims)?),
        ..TokenAnswer::bearer(OPERATOR_TOKEN)
    };
    provider.answer_code(&answer).await?;
    Ok(())
}

#[tokio::test]
async fn a_sign_in_refusal_reads_its_words_from_the_catalog_in_the_requested_locale()
-> Result<(), Box<dyn Error>> {
    let (_state, service) = console("")?;
    let mut request = get("/login")?;
    request.headers_mut().insert(
        http::header::ACCEPT_LANGUAGE,
        http::HeaderValue::from_static("qaa, en;q=0.5"),
    );
    let (response, body) = send(&service, request).await?;
    assert_eq!(StatusCode::SERVICE_UNAVAILABLE, response.status());
    let english = tr_in(catalog(), &Locale::english(), Msg::SignInNotConfigured);
    assert_eq!(english, body);
    assert_eq!("sign-in is not configured on this console", body);
    Ok(())
}

#[tokio::test]
async fn sign_in_answers_unavailable_when_no_provider_is_configured() -> Result<(), Box<dyn Error>>
{
    let (state, service) = console("")?;
    let (response, _body) = send(&service, get("/login")?).await?;
    assert_eq!(StatusCode::SERVICE_UNAVAILABLE, response.status());
    assert_eq!(
        Occupancy {
            sign_ins: 0,
            sessions: 0
        },
        state.sessions().occupancy()?
    );
    Ok(())
}

#[tokio::test]
async fn sign_in_redirects_with_the_code_grant_and_an_s256_challenge() -> Result<(), Box<dyn Error>>
{
    let (provider, _state, service) =
        provider_console("scopes = [\"openid\", \"profile\"]", "").await?;
    let begun = begin(&service).await?;
    assert_eq!(
        provider.authorization_endpoint(),
        format!(
            "{}://{}{}",
            begun.location.scheme(),
            begun.location.host_str().unwrap_or_default(),
            begun
                .location
                .port()
                .map(|port| format!(":{port}"))
                .unwrap_or_default()
        ) + begun.location.path()
    );
    assert_eq!("code", begun.pair("response_type"));
    assert_eq!(CLIENT_ID, begun.pair("client_id"));
    assert_eq!(REDIRECT_URI, begun.pair("redirect_uri"));
    assert_eq!("openid profile", begun.pair("scope"));
    assert_eq!("S256", begun.pair("code_challenge_method"));
    assert_eq!(43, begun.pair("code_challenge").len());
    assert_eq!(43, begun.pair("state").len());
    assert_eq!(43, begun.pair("nonce").len());
    assert_ne!(begun.pair("state"), begun.pair("nonce"));
    assert!(!begun.query.contains_key("prompt"));
    Ok(())
}

// OpenID Connect Core 1.0 §11: `offline_access` goes with `prompt=consent`.
#[tokio::test]
async fn offline_access_asks_for_consent() -> Result<(), Box<dyn Error>> {
    let (_provider, _state, service) = provider_console("offline_access = true", "").await?;
    let begun = begin(&service).await?;
    assert_eq!("openid offline_access", begun.pair("scope"));
    assert_eq!("consent", begun.pair("prompt"));
    Ok(())
}

#[tokio::test]
async fn sign_in_holds_its_state_on_the_server_and_creates_no_session() -> Result<(), Box<dyn Error>>
{
    let (_provider, state, service) = provider_console("", "").await?;
    let (response, _body) = send(&service, get("/login")?).await?;
    let set_cookie = header(&response, "set-cookie");
    assert!(
        set_cookie.starts_with("ferropix_console_sign_in="),
        "{set_cookie}"
    );
    assert!(set_cookie.contains("HttpOnly"), "{set_cookie}");
    assert!(set_cookie.contains("SameSite=Lax"), "{set_cookie}");
    assert!(set_cookie.contains("Max-Age=300"), "{set_cookie}");
    assert_eq!("no-store", header(&response, "cache-control"));
    assert_eq!(
        Occupancy {
            sign_ins: 1,
            sessions: 0
        },
        state.sessions().occupancy()?
    );
    Ok(())
}

#[tokio::test]
async fn a_verified_sign_in_begins_a_session_and_returns_to_the_console()
-> Result<(), Box<dyn Error>> {
    let (provider, state, service) = provider_console("", "").await?;
    let begun = begin(&service).await?;
    let claims = IdTokenClaims::new(&provider.issuer(), CLIENT_ID, begun.pair("nonce"));
    answers(&provider, &claims).await?;
    let (response, body) = complete(&service, &begun).await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status(), "{body}");
    assert_eq!("/", header(&response, "location"));
    assert_eq!("no-store", header(&response, "cache-control"));
    let cookies = set_cookies(&response);
    assert!(
        cookies
            .iter()
            .any(|cookie| cookie.starts_with("ferropix_console_sign_in=;")
                && cookie.contains("Max-Age=0")),
        "{cookies:?}"
    );
    let session = session_cookie(&response)?;
    assert!(!cookies.iter().any(|cookie| cookie.contains(OPERATOR_TOKEN)));
    assert_eq!(1, state.sessions().occupancy()?.sessions);
    let id = ferropix_console::session::SessionId::from_cookie(
        session
            .split_once('=')
            .map(|(_, id)| id)
            .unwrap_or_default(),
    );
    let token = state
        .sessions()
        .access_token(&id)?
        .ok_or("a live session")?;
    assert_eq!(OPERATOR_TOKEN, token.expose_secret());
    assert_eq!(
        Some(String::from(OPERATOR_NAME)),
        state.sessions().operator(&id)?
    );
    Ok(())
}

// RFC 7636 §4.5: the token request carries the verifier whose S256 the
// authorization request carried.
#[tokio::test]
async fn the_token_request_carries_the_code_and_the_pkce_verifier() -> Result<(), Box<dyn Error>> {
    let (provider, _state, service) = provider_console("", "").await?;
    let begun = begin(&service).await?;
    let claims = IdTokenClaims::new(&provider.issuer(), CLIENT_ID, begun.pair("nonce"));
    answers(&provider, &claims).await?;
    complete(&service, &begun).await?;
    let requests = provider.token_requests().await.ok_or("recorded")?;
    let [request] = requests.as_slice() else {
        panic!("one token request, got {}", requests.len());
    };
    let form: std::collections::BTreeMap<String, String> =
        url::form_urlencoded::parse(&request.body)
            .into_owned()
            .collect();
    assert_eq!(
        Some("authorization_code"),
        form.get("grant_type").map(String::as_str)
    );
    assert_eq!(Some("synthetic-code"), form.get("code").map(String::as_str));
    assert_eq!(
        Some(REDIRECT_URI),
        form.get("redirect_uri").map(String::as_str)
    );
    assert_eq!(Some(CLIENT_ID), form.get("client_id").map(String::as_str));
    let verifier = form.get("code_verifier").ok_or("a verifier")?;
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest.as_ref());
    assert_eq!(begun.pair("code_challenge"), challenge);
    assert!(request.headers.get("authorization").is_none());
    Ok(())
}

// RFC 6749 §2.3.1: a confidential client sends its credentials as Basic and
// names no client id in the form.
#[tokio::test]
async fn a_confidential_client_authenticates_with_basic() -> Result<(), Box<dyn Error>> {
    let (provider, _state, service) =
        provider_console("client_secret = \"synthetic secret\"", "").await?;
    let begun = begin(&service).await?;
    let claims = IdTokenClaims::new(&provider.issuer(), CLIENT_ID, begun.pair("nonce"));
    answers(&provider, &claims).await?;
    complete(&service, &begun).await?;
    let requests = provider.token_requests().await.ok_or("recorded")?;
    let request = requests.first().ok_or("a token request")?;
    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{CLIENT_ID}:synthetic+secret"))
    );
    assert_eq!(
        Some(expected.as_str()),
        request
            .headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
    );
    let body = String::from_utf8_lossy(&request.body);
    assert!(!body.contains("client_id"), "{body}");
    Ok(())
}

/// Completes one sign-in at a fresh console whose provider signs an ID Token
/// of `claims`, made from the sign-in's nonce by `edit`, and returns the
/// status the callback answers and the sessions the console then holds.
async fn completed_with(
    edit: impl FnOnce(&Provider, IdTokenClaims) -> IdTokenClaims,
) -> Result<(StatusCode, usize), Box<dyn Error>> {
    let (provider, state, service) = provider_console("", "").await?;
    let begun = begin(&service).await?;
    let claims = edit(
        &provider,
        IdTokenClaims::new(&provider.issuer(), CLIENT_ID, begun.pair("nonce")),
    );
    answers(&provider, &claims).await?;
    let (response, _body) = complete(&service, &begun).await?;
    Ok((response.status(), state.sessions().occupancy()?.sessions))
}

// OpenID Connect Core 1.0 §3.1.3.7 item 11: the nonce is the sign-in's.
#[tokio::test]
async fn an_id_token_with_another_or_no_nonce_is_refused() -> Result<(), Box<dyn Error>> {
    for nonce in [Some(String::from("another-nonce")), None] {
        let refused = completed_with(|_, claims| IdTokenClaims {
            nonce: nonce.clone(),
            ..claims
        })
        .await?;
        assert_eq!((StatusCode::UNAUTHORIZED, 0), refused, "{nonce:?}");
    }
    Ok(())
}

// OpenID Connect Core 1.0 §3.1.3.7 items 2 and 3: the issuer is the
// provider's and the audience this console alone.
#[tokio::test]
async fn an_id_token_of_another_issuer_or_audience_is_refused() -> Result<(), Box<dyn Error>> {
    let issuer = completed_with(|_, claims| IdTokenClaims {
        iss: String::from("https://another-issuer.example"),
        ..claims
    })
    .await?;
    assert_eq!((StatusCode::UNAUTHORIZED, 0), issuer);
    let audience = completed_with(|_, claims| IdTokenClaims {
        aud: serde_json::json!("another-client"),
        ..claims
    })
    .await?;
    assert_eq!((StatusCode::UNAUTHORIZED, 0), audience);
    let two = completed_with(|_, claims| IdTokenClaims {
        aud: serde_json::json!([CLIENT_ID, "another-client"]),
        ..claims
    })
    .await?;
    assert_eq!((StatusCode::UNAUTHORIZED, 0), two);
    let one_in_array = completed_with(|_, claims| IdTokenClaims {
        aud: serde_json::json!([CLIENT_ID]),
        ..claims
    })
    .await?;
    assert_eq!((StatusCode::SEE_OTHER, 1), one_in_array);
    Ok(())
}

// OpenID Connect Core 1.0 §3.1.3.7 item 6: an `azp` names this console.
#[tokio::test]
async fn an_id_token_issued_to_another_party_is_refused() -> Result<(), Box<dyn Error>> {
    let refused = completed_with(|_, claims| IdTokenClaims {
        azp: Some(String::from("another-client")),
        ..claims
    })
    .await?;
    assert_eq!((StatusCode::UNAUTHORIZED, 0), refused);
    Ok(())
}

// OpenID Connect Core 1.0 §3.1.3.7 item 9: the current time is before `exp`.
#[tokio::test]
async fn an_expired_id_token_is_refused() -> Result<(), Box<dyn Error>> {
    let refused = completed_with(|_, claims| IdTokenClaims {
        exp: claims.iat - 3600,
        iat: claims.iat - 7200,
        ..claims
    })
    .await?;
    assert_eq!((StatusCode::UNAUTHORIZED, 0), refused);
    Ok(())
}

// OpenID Connect Core 1.0 §3.1.3.7 item 6: the signature verifies under the
// provider's key, so a token under a key it does not publish, or a shared
// secret, is refused.
#[tokio::test]
async fn an_id_token_not_signed_by_a_published_key_is_refused() -> Result<(), Box<dyn Error>> {
    let (provider, state, service) = provider_console("", "").await?;
    let stranger = Provider::start().await?;
    let begun = begin(&service).await?;
    let claims = IdTokenClaims::new(&provider.issuer(), CLIENT_ID, begun.pair("nonce"));
    let answer = TokenAnswer {
        id_token: Some(stranger.id_token(&claims)?),
        ..TokenAnswer::bearer(OPERATOR_TOKEN)
    };
    provider.answer_code(&answer).await?;
    let (response, _body) = complete(&service, &begun).await?;
    assert_eq!(StatusCode::UNAUTHORIZED, response.status());

    let begun = begin(&service).await?;
    let claims = IdTokenClaims::new(&provider.issuer(), CLIENT_ID, begun.pair("nonce"));
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(String::from(ferropix_testkit::oidc::KID));
    let shared = jsonwebtoken::encode(
        &header,
        &serde_json::to_value(&claims)?,
        &jsonwebtoken::EncodingKey::from_secret(b"synthetic-shared-secret"),
    )?;
    provider.reset().await?;
    provider
        .answer_code(&TokenAnswer {
            id_token: Some(shared),
            ..TokenAnswer::bearer(OPERATOR_TOKEN)
        })
        .await?;
    let (response, _body) = complete(&service, &begun).await?;
    assert_eq!(StatusCode::UNAUTHORIZED, response.status());
    assert_eq!(0, state.sessions().occupancy()?.sessions);
    Ok(())
}

#[tokio::test]
async fn an_answer_without_an_id_token_or_a_refusing_endpoint_is_a_bad_gateway()
-> Result<(), Box<dyn Error>> {
    let (provider, state, service) = provider_console("", "").await?;
    let begun = begin(&service).await?;
    provider
        .answer_code(&TokenAnswer::bearer(OPERATOR_TOKEN))
        .await?;
    let (response, _body) = complete(&service, &begun).await?;
    assert_eq!(StatusCode::BAD_GATEWAY, response.status());

    provider.reset().await?;
    provider.refuse(400, "invalid_grant").await;
    let begun = begin(&service).await?;
    let (response, _body) = complete(&service, &begun).await?;
    assert_eq!(StatusCode::BAD_GATEWAY, response.status());
    assert_eq!(0, state.sessions().occupancy()?.sessions);
    Ok(())
}

#[tokio::test]
async fn the_callback_refuses_a_state_that_is_not_the_sign_ins() -> Result<(), Box<dyn Error>> {
    let (provider, _state, service) = provider_console("", "").await?;
    let begun = begin(&service).await?;
    let (response, body) = send(
        &service,
        get_with("/auth/callback?code=c&state=forged-state", &begun.cookie)?,
    )
    .await?;
    assert_eq!(StatusCode::BAD_REQUEST, response.status());
    assert!(body.contains("state"), "{body}");
    assert_eq!(
        Some(0),
        provider.token_requests().await.map(|asked| asked.len())
    );
    Ok(())
}

#[tokio::test]
async fn the_callback_refuses_a_request_without_its_sign_in_cookie() -> Result<(), Box<dyn Error>> {
    let (_provider, _state, service) = provider_console("", "").await?;
    let begun = begin(&service).await?;
    let path = format!("/auth/callback?code=c&state={}", begun.pair("state"));
    let (response, _body) = send(&service, get(&path)?).await?;
    assert_eq!(StatusCode::BAD_REQUEST, response.status());
    Ok(())
}

#[tokio::test]
async fn a_pending_sign_in_answers_one_callback_only() -> Result<(), Box<dyn Error>> {
    let (provider, _state, service) = provider_console("", "").await?;
    let begun = begin(&service).await?;
    let claims = IdTokenClaims::new(&provider.issuer(), CLIENT_ID, begun.pair("nonce"));
    answers(&provider, &claims).await?;
    let (first, _body) = complete(&service, &begun).await?;
    assert_eq!(StatusCode::SEE_OTHER, first.status());
    let (second, _body) = complete(&service, &begun).await?;
    assert_eq!(StatusCode::BAD_REQUEST, second.status());
    Ok(())
}

// RFC 6749 §4.1.2.1: the provider answers a refused request with `error`.
#[tokio::test]
async fn a_refusal_from_the_provider_answers_unauthorized() -> Result<(), Box<dyn Error>> {
    let (_provider, _state, service) = provider_console("", "").await?;
    let begun = begin(&service).await?;
    let path = format!(
        "/auth/callback?error=access_denied&state={}",
        begun.pair("state")
    );
    let (response, _body) = send(&service, get_with(&path, &begun.cookie)?).await?;
    assert_eq!(StatusCode::UNAUTHORIZED, response.status());
    Ok(())
}

#[tokio::test]
async fn a_flood_of_sign_ins_stays_bounded_and_leaves_signed_in_sessions_alone()
-> Result<(), Box<dyn Error>> {
    let (provider, state, service) = provider_console("", "max_sign_ins = 4").await?;
    let operator = sign_in(&service, &provider, TokenAnswer::bearer(OPERATOR_TOKEN)).await?;
    let first = begin(&service).await?;
    for _ in 0..50 {
        begin(&service).await?;
        let occupancy = state.sessions().occupancy()?;
        assert!(occupancy.sign_ins <= 4, "{occupancy:?}");
        assert_eq!(1, occupancy.sessions, "{occupancy:?}");
    }
    let (dropped, _body) = complete(&service, &first).await?;
    assert_eq!(StatusCode::BAD_REQUEST, dropped.status());
    let (page, body) = send(&service, get_with("/", &operator)?).await?;
    assert_eq!(StatusCode::OK, page.status());
    assert!(body.contains("Sign out"), "{body}");
    Ok(())
}

#[tokio::test]
async fn a_new_sign_in_ends_the_session_it_replaces() -> Result<(), Box<dyn Error>> {
    let (provider, state, service) = provider_console("", "").await?;
    let old = sign_in(&service, &provider, TokenAnswer::bearer(OPERATOR_TOKEN)).await?;
    let begun = begin(&service).await?;
    let claims = IdTokenClaims::new(&provider.issuer(), CLIENT_ID, begun.pair("nonce"));
    provider.reset().await?;
    answers(&provider, &claims).await?;
    let request = http::Request::get(format!(
        "/auth/callback?code=synthetic-code&state={}",
        begun.pair("state")
    ))
    .header("cookie", format!("{}; {old}", begun.cookie))
    .body(axum::body::Body::empty())?;
    let (response, _body) = send(&service, request).await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status());
    assert_eq!(1, state.sessions().occupancy()?.sessions);
    Ok(())
}

#[tokio::test]
async fn no_token_code_or_verifier_reaches_a_log_line_or_a_page() -> Result<(), Box<dyn Error>> {
    let captured = Captured::default();
    let _guard = captured.logging();
    let (provider, _state, service) = provider_console("", "").await?;
    let begun = begin(&service).await?;
    let claims = IdTokenClaims::new(&provider.issuer(), CLIENT_ID, begun.pair("nonce"));
    let id_token = provider.id_token(&claims)?;
    answers(&provider, &claims).await?;
    let (response, _body) = complete(&service, &begun).await?;
    let session = session_cookie(&response)?;
    let (_page, page) = send(&service, get_with("/", &session)?).await?;
    let requests = provider.token_requests().await.ok_or("recorded")?;
    let form =
        String::from_utf8_lossy(&requests.first().ok_or("a token request")?.body).into_owned();
    let verifier = url::form_urlencoded::parse(form.as_bytes())
        .find(|(name, _)| name == "code_verifier")
        .map(|(_, value)| value.into_owned())
        .ok_or("a verifier")?;
    let logged = captured.text();
    for secret in [
        OPERATOR_TOKEN,
        id_token.as_str(),
        verifier.as_str(),
        "synthetic-code",
    ] {
        assert!(
            !logged.contains(secret),
            "a log line carries {secret}: {logged}"
        );
        assert!(!page.contains(secret), "the page carries {secret}");
    }
    Ok(())
}
