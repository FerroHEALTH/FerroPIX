// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `healthcheck` job: asks the server running beside it for its
//! readiness, for a container runtime that has no HTTP client of its own.
//!
//! The job sends `GET {base}/health/readiness` to the configured listener on
//! loopback with a short timeout and exits `0` only on `200`, which is what a
//! Docker `HEALTHCHECK` reads as healthy
//! (<https://docs.docker.com/reference/dockerfile/#healthcheck>). No
//! specification governs health probes: our own design.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use http::StatusCode;

/// How long the job waits for the whole exchange, shorter than the five
/// seconds a container `HEALTHCHECK` commonly allows.
pub const TIMEOUT: Duration = Duration::from_secs(3);

/// What the server answered.
#[derive(Debug)]
pub enum Outcome {
    /// The readiness route answered `200`.
    Up,
    /// The readiness route answered with this other status.
    NotUp(StatusCode),
    /// The request failed: nothing listens, or no answer came in time.
    Failed(reqwest::Error),
}

impl Outcome {
    /// Returns whether the server is up.
    #[must_use]
    pub const fn is_up(&self) -> bool {
        matches!(self, Self::Up)
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Up => f.write_str("up"),
            Self::NotUp(status) => write!(f, "not up: the readiness route answered {status}"),
            Self::Failed(error) => write!(f, "not up: {error}"),
        }
    }
}

/// Returns the address the job connects to for a server listening on
/// `listen`: a wildcard address becomes the loopback address of its family.
#[must_use]
pub fn target(listen: SocketAddr) -> SocketAddr {
    let ip = match listen.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, listen.port())
}

/// Asks the server at `address` for the readiness of `{base}` `base_path`,
/// waiting at most `timeout`.
///
/// No proxy is consulted and no redirect is followed: the server is on this
/// host, and only its own answer counts.
pub async fn check(address: SocketAddr, base_path: &str, timeout: Duration) -> Outcome {
    let client = match reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(error) => return Outcome::Failed(error),
    };
    let url = format!(
        "http://{address}{base_path}{}",
        ferrohealth_server::health::READINESS
    );
    match client.get(url).send().await {
        Ok(response) if response.status() == StatusCode::OK => Outcome::Up,
        Ok(response) => Outcome::NotUp(response.status()),
        Err(error) => Outcome::Failed(error),
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::target;

    #[test]
    fn a_wildcard_listen_address_is_asked_on_loopback_and_any_other_as_it_is() {
        for (listen, asked) in [
            ("0.0.0.0:8080", "127.0.0.1:8080"),
            ("[::]:8080", "[::1]:8080"),
            ("10.0.0.7:8080", "10.0.0.7:8080"),
        ] {
            let listen: SocketAddr = listen.parse().expect("a socket address");
            let asked: SocketAddr = asked.parse().expect("a socket address");
            assert_eq!(asked, target(listen), "{listen}");
        }
    }
}
