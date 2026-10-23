// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's HTTP surface driven in-process: the health route, the
//! pages, the site bundle, and the security headers on each.

use axum::body::Body;
use http::header::{
    ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_SECURITY_POLICY, REFERRER_POLICY,
    VARY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use http::{Request, Response, StatusCode};
use tower::ServiceExt as _;

use ferropix_console::server::{ConsoleState, router};

use crate::support::settings;

/// A site root holding a WebAssembly file big enough to be compressed.
fn site() -> tempfile::TempDir {
    let site = tempfile::tempdir().expect("a temporary directory");
    let pkg = site.path().join("pkg");
    std::fs::create_dir_all(&pkg).expect("the pkg directory");
    std::fs::write(pkg.join("ferropix-console.wasm"), vec![0_u8; 4096]).expect("the bundle");
    site
}

/// Sends `request` to a console over the site root `site`.
async fn send(site: &tempfile::TempDir, request: Request<Body>) -> Response<Body> {
    let state =
        ConsoleState::new(settings(site.path(), "http://127.0.0.1:9/")).expect("the state builds");
    router(state)
        .oneshot(request)
        .await
        .expect("the router answers")
}

/// A `GET` of `path` with `Accept-Encoding: br, gzip`.
fn get(path: &str) -> Request<Body> {
    Request::get(path)
        .header(ACCEPT_ENCODING, "br, gzip")
        .body(Body::empty())
        .expect("a request")
}

/// The text of `response`'s header `name`.
fn header<'a>(response: &'a Response<Body>, name: &http::HeaderName) -> &'a str {
    response
        .headers()
        .get(name)
        .unwrap_or_else(|| panic!("a {name} header"))
        .to_str()
        .expect("a text header")
}

/// The body of `response` as text.
async fn text(response: Response<Body>) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body reads");
    String::from_utf8(bytes.to_vec()).expect("a UTF-8 body")
}

/// The script nonce `policy` names.
fn nonce(policy: &str) -> &str {
    policy
        .split_once("'nonce-")
        .and_then(|(_, rest)| rest.split_once('\''))
        .map(|(nonce, _)| nonce)
        .expect("the policy names a nonce")
}

/// Asserts the headers every answer carries.
fn assert_hardened(response: &Response<Body>) {
    assert_eq!(
        "nosniff",
        header(response, &X_CONTENT_TYPE_OPTIONS),
        "no media-type sniffing"
    );
    assert_eq!("DENY", header(response, &X_FRAME_OPTIONS), "never framed");
    assert_eq!(
        "no-referrer",
        header(response, &REFERRER_POLICY),
        "no referrer leaves the console"
    );
    let policy = header(response, &CONTENT_SECURITY_POLICY);
    assert!(!policy.contains("'unsafe-inline'"), "{policy}");
    assert!(!policy.contains("'unsafe-eval'"), "{policy}");
    assert!(policy.contains("frame-ancestors 'none'"), "{policy}");
}

#[tokio::test]
async fn the_health_route_answers_up_with_the_headers() {
    let site = site();
    let response = send(&site, get("/health")).await;
    assert_eq!(StatusCode::OK, response.status());
    assert_hardened(&response);
    assert_eq!("no-store", header(&response, &CACHE_CONTROL));
    assert_eq!(r#"{"status":"up"}"#, text(response).await);
}

#[tokio::test]
async fn a_page_carries_a_fresh_nonce_its_scripts_use_and_is_never_stored_or_compressed() {
    let site = site();
    let first = send(&site, get("/sign-in")).await;
    assert_eq!(StatusCode::OK, first.status());
    assert_hardened(&first);
    assert_eq!("no-store", header(&first, &CACHE_CONTROL));
    assert!(
        first.headers().get(CONTENT_ENCODING).is_none(),
        "a document is never compressed"
    );
    let policy = header(&first, &CONTENT_SECURITY_POLICY).to_owned();
    let page = text(first).await;
    let minted = nonce(&policy);
    assert!(
        page.contains(&format!("nonce=\"{minted}\"")),
        "the page's scripts carry the nonce the policy names"
    );
    assert!(
        page.contains("/pkg/ferropix-console.js") && page.contains("/pkg/ferropix-console.wasm"),
        "the page loads the bundle cargo-leptos writes"
    );
    assert!(
        page.contains("/ferrohealth-ui.css") && page.contains("/theme.css"),
        "the page links the library's stylesheet and its FerroPIX theme"
    );
    let second = send(&site, get("/sign-in")).await;
    let again = header(&second, &CONTENT_SECURITY_POLICY).to_owned();
    assert_ne!(minted, nonce(&again), "each response mints its own nonce");
}

#[tokio::test]
async fn a_page_with_no_session_sends_the_browser_to_the_sign_in_page() {
    let site = site();
    for path in ["/", "/queue", "/about", "/no-such-page"] {
        let response = send(&site, get(path)).await;
        assert_eq!(StatusCode::SEE_OTHER, response.status(), "{path}");
        assert_eq!(
            "/sign-in",
            header(&response, &http::header::LOCATION),
            "{path}"
        );
        assert_eq!("no-store", header(&response, &CACHE_CONTROL), "{path}");
    }
}

#[tokio::test]
async fn the_bundle_is_served_compressed_and_revalidated() {
    let site = site();
    let response = send(&site, get("/pkg/ferropix-console.wasm")).await;
    assert_eq!(StatusCode::OK, response.status());
    assert_hardened(&response);
    assert_eq!("br", header(&response, &CONTENT_ENCODING));
    assert!(
        header(&response, &VARY).contains("accept-encoding"),
        "the answer varies on the encoding"
    );
    assert_eq!("no-cache", header(&response, &CACHE_CONTROL));
}

#[tokio::test]
async fn the_stylesheet_the_theme_and_the_brand_files_are_the_librarys_and_the_brands() {
    let site = site();
    for (path, media, body) in [
        (
            "/ferrohealth-ui.css",
            "text/css; charset=utf-8",
            ferrohealth_ui::style::STYLESHEET,
        ),
        (
            "/theme.css",
            "text/css; charset=utf-8",
            ferrohealth_ui::style::Brand::Pix.theme(),
        ),
        (
            "/favicon.svg",
            "image/svg+xml",
            include_str!("../../../../assets/brand/favicon.svg"),
        ),
        (
            "/ferropix-icon.svg",
            "image/svg+xml",
            include_str!("../../../../assets/brand/ferropix-icon.svg"),
        ),
    ] {
        let plain = Request::get(path).body(Body::empty()).expect("a request");
        let response = send(&site, plain).await;
        assert_eq!(StatusCode::OK, response.status(), "{path}");
        assert_hardened(&response);
        assert_eq!("no-cache", header(&response, &CACHE_CONTROL), "{path}");
        assert!(
            header(&response, &http::header::CONTENT_TYPE).starts_with(media),
            "{path}"
        );
        assert_eq!(body, text(response).await, "{path}");
    }
}
