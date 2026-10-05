// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! No credential, presentation, key or token in a `Debug` rendering or an
//! error.

use std::error::Error;
use std::fmt::Write as _;
use std::time::Duration;

use secrecy::ExposeSecret;

use super::{Fixture, PROMPT};

/// The rendering of `error` with its whole source chain, in `Display` and
/// `Debug`.
fn rendered(error: &(dyn Error + 'static)) -> String {
    let mut text = format!("{error} {error:?}");
    let mut source = error.source();
    while let Some(cause) = source {
        write!(text, " {cause} {cause:?}").expect("a String takes every write");
        source = cause.source();
    }
    text
}

/// The parts of the compact JWS `jwt` long enough to identify it.
fn parts(jwt: &str) -> Vec<&str> {
    jwt.split('.').filter(|part| part.len() > 8).collect()
}

#[tokio::test]
async fn the_holder_and_the_token_render_no_secret() {
    let fixture = Fixture::start().await;
    let token = Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect("a token");
    let holder = format!("{:?}", fixture.holder);
    for part in parts(&fixture.credential) {
        assert!(!holder.contains(part), "the holder renders its credential");
    }
    assert!(!holder.contains("PRIVATE KEY"));
    let pem_body: String = fixture
        .holder_pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    assert!(!holder.contains(&pem_body), "the holder renders its key");
    let rendered_token = format!("{token:?}");
    assert!(!rendered_token.contains(token.token().expose_secret()));
}

#[tokio::test]
async fn no_error_carries_the_presentation_or_a_credential() {
    let fixture = Fixture::start().await;
    fixture
        .node
        .refuse(400, "invalid_request", "the presentation was refused");
    let refused = Fixture::client()
        .request_access_token(&fixture.grant, &fixture.holder, &fixture.prover, PROMPT)
        .await
        .expect_err("refused");
    fixture.node.delay(Duration::from_secs(3));
    let timed_out = Fixture::client()
        .request_access_token(
            &fixture.grant,
            &fixture.holder,
            &fixture.prover,
            Duration::from_millis(300),
        )
        .await
        .expect_err("timed out");
    let forms = fixture.node.forms();
    let assertions: Vec<String> = forms
        .iter()
        .filter_map(|form| {
            form.iter()
                .find(|(key, _)| key == "assertion")
                .map(|(_, value)| value.clone())
        })
        .collect();
    assert!(!assertions.is_empty());
    for error in [&refused, &timed_out] {
        let text = rendered(error);
        for part in parts(&fixture.credential) {
            assert!(
                !text.contains(part),
                "an error carries the credential: {text}"
            );
        }
        for assertion in &assertions {
            for part in parts(assertion) {
                assert!(!text.contains(part), "an error carries the presentation");
            }
        }
        assert!(
            !text.contains("/token"),
            "an error carries a request URL: {text}"
        );
    }
}
