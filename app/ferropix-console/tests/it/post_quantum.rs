// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The key exchange of the console's client of the FerroPIX server: it
//! prefers X25519MLKEM768 (RFC 10024) and agrees on X25519 with a peer
//! that lacks it. The peer's certificate is trusted by no root, so each call
//! ends after the key exchange.

use std::time::Duration;

use ferropix_console::config::settings::PixSettings;
use ferropix_console::pix::{AccessToken, Pix};
use ferropix_testkit::tls::KxPeer;
use secrecy::SecretString;
use tokio_rustls::rustls::NamedGroup;
use tokio_rustls::rustls::crypto::aws_lc_rs::kx_group;
use url::Url;

/// The group the console's client agreed on with `peer` in one probe.
async fn agreed_by_the_console(peer: &KxPeer) -> Option<NamedGroup> {
    let pix = Pix::new(&PixSettings {
        base: Url::parse(&format!("https://{}/", peer.address())).expect("a base"),
        timeout: Duration::from_secs(10),
        admin_scope: String::from("system/*.cruds"),
    })
    .expect("the client builds");
    let probe = pix
        .probe(&AccessToken::new(SecretString::from("synthetic-token")))
        .await;
    assert!(!probe.is_up(), "the peer's certificate is trusted by no root");
    peer.agreed(0).expect("the connection is recorded")
}

#[tokio::test]
async fn the_console_client_prefers_x25519mlkem768() {
    let peer =
        KxPeer::start(vec![kx_group::X25519, kx_group::X25519MLKEM768]).expect("the peer starts");
    assert_eq!(
        Some(NamedGroup::X25519MLKEM768),
        agreed_by_the_console(&peer).await
    );
}

#[tokio::test]
async fn the_console_client_agrees_on_x25519_with_a_classical_only_peer() {
    let peer = KxPeer::start(vec![kx_group::X25519]).expect("the peer starts");
    assert_eq!(Some(NamedGroup::X25519), agreed_by_the_console(&peer).await);
}
