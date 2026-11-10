// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The outbound clients the tests build every client from: an endpoint table
//! as a product writes one, built by `ferrohealth-smart` with no TLS material.

use ferrohealth_config::units::Duration as Span;
use ferrohealth_secret::secret_url::SecretUrl;
use ferrohealth_smart::client::outbound::OutboundClient;
use ferrohealth_smart::config::ServerTls;
use ferrohealth_smart::config::endpoint::{EndpointSection, RetrySection};

/// The endpoint table at `url`, tried once, with an engine timeout longer
/// than any timeout a test passes a call.
pub(crate) fn endpoint(url: &str) -> EndpointSection {
    EndpointSection {
        url: SecretUrl::new(url),
        timeout: Span::from_secs(60),
        retry: RetrySection {
            max_attempts: 1,
            initial_backoff: Span::from_millis(10),
            max_backoff: Span::from_millis(10),
        },
        ..EndpointSection::default()
    }
}

/// The outbound client `endpoint` builds.
pub(crate) fn build(endpoint: &EndpointSection) -> OutboundClient {
    OutboundClient::build(endpoint, "test.endpoint", &ServerTls).expect("the outbound client")
}

/// The outbound client of the table at `url`.
pub(crate) fn outbound(url: &str) -> OutboundClient {
    build(&endpoint(url))
}
