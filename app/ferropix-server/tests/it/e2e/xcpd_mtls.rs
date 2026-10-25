// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The XCPD asynchronous delivery over mutual TLS (#240), behind
//! `FERROPIX_E2E`: a reply endpoint that requires a client certificate takes
//! the answer when `[xcpd]` names the client identity and refuses the
//! handshake when it names none.
//!
//! The certificate authority, the server certificate and the client
//! certificate are generated when the case runs; no key is a fixture.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferropix_testkit::containers::e2e_enabled;
use http::StatusCode;
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::RootCertStore;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::pem::PemObject as _;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::server::WebPkiClientVerifier;

use super::pixm::{Harness, synthetic_seeds};
use super::xcpd::{FEMALE, SOAP, asynchronous, gateway_with, named, post, section, seeded};

/// The certificates of one run, as PEM text.
struct Pki {
    /// The authority's certificate.
    authority: String,
    /// The reply endpoint's certificate and key.
    server: (String, String),
    /// The gateway's client certificate and key.
    client: (String, String),
}

/// Generates an authority and the server and client certificates it signs.
fn pki() -> Pki {
    let mut params = CertificateParams::new(Vec::<String>::new()).expect("authority params");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, "Synthetic community authority");
    let authority = CertifiedIssuer::self_signed(params, KeyPair::generate().expect("a key"))
        .expect("the authority signs itself");
    let leaf = |names: Vec<String>, name: &str| {
        let mut params = CertificateParams::new(names).expect("leaf params");
        params.distinguished_name.push(DnType::CommonName, name);
        let key = KeyPair::generate().expect("a key");
        let certificate = params.signed_by(&key, &authority).expect("signed");
        (certificate.pem(), key.serialize_pem())
    };
    Pki {
        authority: authority.pem(),
        server: leaf(vec![String::from("localhost")], "Synthetic reply endpoint"),
        client: leaf(Vec::new(), "Synthetic responding gateway"),
    }
}

/// What the reply endpoint saw: per connection, whether the handshake
/// completed, and the requests it read.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<(Vec<bool>, Vec<String>)>>);

/// Starts a reply endpoint on a loopback port that requires a client
/// certificate the authority signed, answering every request `202`.
async fn endpoint(pki: &Pki) -> (SocketAddr, Seen) {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from_pem_slice(pki.authority.as_bytes()).expect("the authority"))
        .expect("a root");
    // The process may hold more than one rustls provider, so the case names its own.
    let provider = Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider());
    let verifier =
        WebPkiClientVerifier::builder_with_provider(Arc::new(roots), Arc::clone(&provider))
            .build()
            .expect("a client verifier");
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("the default protocol versions")
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            vec![CertificateDer::from_pem_slice(pki.server.0.as_bytes()).expect("a certificate")],
            PrivateKeyDer::from_pem_slice(pki.server.1.as_bytes()).expect("a key"),
        )
        .expect("a server config");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    let seen = Seen::default();
    let record = seen.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let accepted = acceptor.accept(stream).await;
            record.0.lock().unwrap().0.push(accepted.is_ok());
            let Ok(mut tls) = accepted else { continue };
            let mut bytes = vec![0_u8; 1 << 16];
            let read = tls.read(&mut bytes).await.unwrap_or(0);
            record
                .0
                .lock()
                .unwrap()
                .1
                .push(String::from_utf8_lossy(&bytes[..read]).into_owned());
            let _written = tls
                .write_all(
                    b"HTTP/1.1 202 Accepted\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                )
                .await;
            let _shut = tls.shutdown().await;
        }
    });
    (address, seen)
}

/// Waits until the endpoint has seen `count` connections.
async fn connections(seen: &Seen, count: usize) -> (Vec<bool>, Vec<String>) {
    for _ in 0..200 {
        let now = seen.0.lock().unwrap().clone();
        if now.0.len() >= count {
            return now;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the endpoint did not see {count} connections");
}

/// Writes `text` to `name` under `dir` and returns its path.
fn written(dir: &tempfile::TempDir, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, text).expect("written");
    path
}

#[tokio::test]
async fn the_delivery_presents_its_client_identity_to_a_reply_endpoint_that_requires_one() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    seeded(&harness).await;
    let pki = pki();
    let (address, seen) = endpoint(&pki).await;
    let base = format!("https://localhost:{}/", address.port());
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut xcpd = section(std::slice::from_ref(&base));
    xcpd.delivery_attempts = 1;
    xcpd.reply_trust_file = Some(written(&dir, "authority.pem", &pki.authority));
    xcpd.client_certificate_file = Some(written(&dir, "client.crt", &pki.client.0));
    xcpd.client_key_file = Some(written(&dir, "client.key", &pki.client.1));
    let app = gateway_with(
        &harness,
        &harness.matching,
        xcpd.resolve().expect("resolves").expect("on"),
    );
    let sanne = named("Sanne", "Wolderink", "19660817", FEMALE);
    let (status, _) = post(&app, SOAP, asynchronous(&format!("{base}replies"), &sanne)).await;
    assert_eq!(StatusCode::ACCEPTED, status);
    let (handshakes, requests) = connections(&seen, 1).await;
    assert_eq!(vec![true], handshakes, "the handshake completes");
    assert!(requests[0].starts_with("POST /replies"), "{}", requests[0]);
}

#[tokio::test]
async fn without_a_client_identity_the_endpoint_refuses_the_handshake() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    seeded(&harness).await;
    let pki = pki();
    let (address, seen) = endpoint(&pki).await;
    let base = format!("https://localhost:{}/", address.port());
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut xcpd = section(std::slice::from_ref(&base));
    xcpd.delivery_attempts = 1;
    xcpd.reply_trust_file = Some(written(&dir, "authority.pem", &pki.authority));
    let app = gateway_with(
        &harness,
        &harness.matching,
        xcpd.resolve().expect("resolves").expect("on"),
    );
    let sanne = named("Sanne", "Wolderink", "19660817", FEMALE);
    let (status, _) = post(&app, SOAP, asynchronous(&format!("{base}replies"), &sanne)).await;
    assert_eq!(StatusCode::ACCEPTED, status);
    let (handshakes, requests) = connections(&seen, 1).await;
    assert_eq!(
        vec![false],
        handshakes,
        "no client certificate, no handshake"
    );
    assert_eq!(
        Vec::<String>::new(),
        requests,
        "no request reached the endpoint"
    );
}
