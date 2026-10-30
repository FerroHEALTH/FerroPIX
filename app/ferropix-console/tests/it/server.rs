// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's HTTP surface driven in-process: the health route, the
//! pages, FerroUI's assets, and the security headers on each.

use axum::body::Body;
use http::header::{
    ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_SECURITY_POLICY, REFERRER_POLICY,
    VARY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use http::{Request, Response, StatusCode};
use tower::ServiceExt as _;

use ferropix_console::server::{ConsoleState, router};

use crate::support::settings;

/// Sends `request` to a console whose FerroPIX server does not answer.
async fn send(request: Request<Body>) -> Response<Body> {
    let state = ConsoleState::new(settings("http://127.0.0.1:9/")).expect("the state builds");
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

/// Where FerroUI's assets are served at the pinned release, with the
/// FerroPIX theme.
fn assets() -> ferrohealth_ui::serve::Assets {
    ferrohealth_ui::serve::Assets::of(ferrohealth_ui::style::Brand::Pix)
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
    assert!(!policy.contains("'wasm-unsafe-eval'"), "{policy}");
    assert!(!policy.contains("'nonce-"), "{policy}");
    assert!(policy.contains("script-src 'self';"), "{policy}");
    assert!(policy.contains("style-src 'self';"), "{policy}");
    assert!(policy.contains("frame-ancestors 'none'"), "{policy}");
}

#[tokio::test]
async fn the_health_route_answers_up_with_the_headers() {
    let response = send(get("/health")).await;
    assert_eq!(StatusCode::OK, response.status());
    assert_hardened(&response);
    assert_eq!("no-store", header(&response, &CACHE_CONTROL));
    assert_eq!(r#"{"state":"up"}"#, text(response).await);
}

#[tokio::test]
async fn liveness_answers_and_readiness_is_withdrawn_once_the_drain_starts() {
    let state = ConsoleState::new(settings("http://127.0.0.1:9/")).expect("the state builds");
    let lifecycle = state.lifecycle().clone();
    let app = router(state);
    let ready = app
        .clone()
        .oneshot(get("/health/readiness"))
        .await
        .expect("the console answers");
    assert_eq!(StatusCode::OK, ready.status());
    lifecycle.drain();
    let draining = app
        .clone()
        .oneshot(get("/health/readiness"))
        .await
        .expect("the console answers");
    assert_eq!(StatusCode::SERVICE_UNAVAILABLE, draining.status());
    let live = app
        .oneshot(get("/health/liveness"))
        .await
        .expect("the console answers");
    assert_eq!(StatusCode::OK, live.status(), "liveness asks nothing");
}

#[tokio::test]
async fn a_page_loads_its_scripts_and_styles_from_the_console_alone_and_is_never_stored_or_compressed()
 {
    let first = send(get("/sign-in")).await;
    assert_eq!(StatusCode::OK, first.status());
    assert_hardened(&first);
    assert_eq!("no-store", header(&first, &CACHE_CONTROL));
    assert!(
        first.headers().get(CONTENT_ENCODING).is_none(),
        "a document is never compressed"
    );
    let page = text(first).await;
    let assets = assets();
    assert!(
        page.contains(&assets.loader()),
        "the page loads FerroUI's one loader"
    );
    assert!(
        !page.contains("nonce="),
        "no script carries a nonce the policy would need to name"
    );
    assert_eq!(
        page.matches("<script").count(),
        page.matches("<script type=\"module\" src=\"").count(),
        "every script is a module the console's origin serves, never an inline one: {page}"
    );
    assert!(!page.contains(" style=\""), "no inline style");
    assert!(!page.contains("<style"), "no style element");
    assert!(
        page.contains(&assets.stylesheet()) && page.contains(&assets.theme()),
        "the page links the library's stylesheet and its FerroPIX theme"
    );
    assert!(!page.contains("/pkg/"), "no site bundle is left");
}

#[tokio::test]
async fn a_page_with_no_session_sends_the_browser_to_the_sign_in_page() {
    for path in ["/", "/queue", "/about", "/no-such-page"] {
        let response = send(get(path)).await;
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
async fn the_library_assets_are_served_compressed_and_kept_for_good() {
    let response = send(get(&assets().stylesheet())).await;
    assert_eq!(StatusCode::OK, response.status());
    assert_hardened(&response);
    assert_eq!("br", header(&response, &CONTENT_ENCODING));
    assert!(
        header(&response, &VARY).contains("accept-encoding"),
        "the answer varies on the encoding"
    );
    assert_eq!(
        "public, max-age=31536000, immutable",
        header(&response, &CACHE_CONTROL),
        "a versioned path is a new path at the next release"
    );
}

#[tokio::test]
async fn the_stylesheet_the_theme_and_the_loader_are_the_librarys() {
    let assets = assets();
    for (path, media, body) in [
        (
            assets.stylesheet(),
            "text/css; charset=utf-8",
            ferrohealth_ui::style::STYLESHEET,
        ),
        (
            assets.theme(),
            "text/css; charset=utf-8",
            ferrohealth_ui::style::Brand::Pix.theme(),
        ),
        (
            assets.loader(),
            "text/javascript; charset=utf-8",
            ferrohealth_ui::script::LOADER.text,
        ),
    ] {
        let plain = Request::get(&path).body(Body::empty()).expect("a request");
        let response = send(plain).await;
        assert_eq!(StatusCode::OK, response.status(), "{path}");
        assert_hardened(&response);
        assert!(
            header(&response, &http::header::CONTENT_TYPE).starts_with(media),
            "{path}"
        );
        assert_eq!(body, text(response).await, "{path}");
    }
}

#[tokio::test]
async fn the_brand_files_are_the_brands_and_revalidated() {
    for (path, media, body) in [
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
        let response = send(plain).await;
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
