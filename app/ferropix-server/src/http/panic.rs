// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A handler panic becomes a `500` with a body, never a dropped connection.
//!
//! The release profile pins `panic = "unwind"`, which is what lets
//! `std::panic::catch_unwind` turn an unwound handler into a response
//! (<https://doc.rust-lang.org/cargo/reference/profiles.html#panic>). The
//! panic message reaches neither the body nor the log, since a handler may
//! have panicked over an identifier or a demographic; the log line and the
//! body name the server's request id. No specification governs this: our own
//! design.

use std::any::Any;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use ferrohealth_telemetry::request_log::RequestId;
use http::{StatusCode, header};

/// The marker a caught panic leaves on its response.
#[derive(Debug, Clone, Copy)]
struct Panicked;

/// Renders a panicked handler as a `500` carrying the marker.
#[must_use]
pub fn caught(_payload: Box<dyn Any + Send + 'static>) -> Response {
    let mut response = body("").into_response();
    response.extensions_mut().insert(Panicked);
    response
}

/// Fills the request id into a panic response and logs the panic.
///
/// It runs outside the request id of `ferrohealth-server`, which leaves the
/// server's id on the response, so the body and the log line name the id
/// the request log wrote.
pub async fn render(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    if response.extensions().get::<Panicked>().is_none() {
        return response;
    }
    let request_id = response
        .extensions()
        .get::<RequestId>()
        .map_or_else(String::new, |id| id.id().to_owned());
    tracing::error!(
        request_id = request_id.as_str(),
        "the request handler panicked"
    );
    let mut rendered = body(&request_id).into_response();
    *rendered.headers_mut() = response.headers().clone();
    rendered.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    rendered
}

/// Returns the JSON body an internal failure is answered with.
fn body(request_id: &str) -> (StatusCode, String) {
    let id = request_id.replace(['"', '\\'], "");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!(r#"{{"error":"internal","request_id":"{id}"}}"#),
    )
}

#[cfg(test)]
mod tests {
    use super::{Panicked, caught};
    use http::StatusCode;

    #[test]
    fn a_caught_panic_is_a_500_carrying_the_marker_and_not_the_message() {
        let response = caught(Box::new("the handler gave up over a value"));
        assert_eq!(StatusCode::INTERNAL_SERVER_ERROR, response.status());
        assert!(response.extensions().get::<Panicked>().is_some());
    }
}
