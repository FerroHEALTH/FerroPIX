// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The connection to an Audit Record Repository: syslog over TLS (RFC 5425),
//! the transport ITI TF-2 §3.20.4.1.2.1.1 recommends.
//!
//! A repository is addressed as `tls://host[:port]`, the port defaulting to
//! 6514. Plain TCP (`tcp://host:port`, RFC 6587 octet counting) exists only
//! through [`Repository::unencrypted_for_development`]: an audit message
//! names the patient, so it never crosses a network in clear text outside a
//! development deployment.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use rustls::ClientConfig;
use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls_platform_verifier::Verifier;
use secrecy::{ExposeSecret, SecretSlice, SecretString};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use url::Url;

use super::syslog::TLS_PORT;

/// Why a repository address or its TLS settings were refused.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RepositoryError {
    /// The address is not `tls://host[:port]`, or `tcp://host:port` for
    /// development.
    #[error("the audit repository address is not tls://host[:port]")]
    Address,
    /// The address has a path, a query, a fragment or userinfo.
    #[error("the audit repository address carries more than a host and a port")]
    Extra,
    /// The host is no TLS server name.
    #[error("the audit repository host is not a TLS server name")]
    ServerName(#[source] rustls::pki_types::InvalidDnsNameError),
    /// The trust roots do not read as PEM certificates.
    #[error("the audit repository trust roots are not PEM certificates")]
    Roots(#[source] rustls::pki_types::pem::Error),
    /// The client certificate and key do not read as PEM.
    #[error("the audit repository client certificate and key are not PEM")]
    Identity(#[source] rustls::pki_types::pem::Error),
    /// The client identity holds a key and no certificate.
    #[error("the audit repository client identity holds no certificate")]
    NoCertificate,
    /// rustls refused the configuration.
    #[error("the TLS configuration for the audit repository could not be built")]
    Tls(#[source] rustls::Error),
}

/// Why a frame was not written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SendError {
    /// The connection could not be opened.
    #[error("the audit repository could not be reached")]
    Connect(#[source] std::io::Error),
    /// The TLS handshake failed.
    #[error("the TLS handshake with the audit repository failed")]
    Handshake(#[source] std::io::Error),
    /// A step did not finish within its timeout.
    #[error("the audit repository did not finish the {0} within its timeout")]
    Timeout(Step),
    /// The frame could not be written.
    #[error("the audit message could not be written to the audit repository")]
    Write(#[source] std::io::Error),
}

/// A step of sending a frame, each bounded by a timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    /// Opening the TCP connection.
    Connect,
    /// The TLS handshake.
    Handshake,
    /// Writing the frame.
    Write,
    /// Flushing it.
    Flush,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Connect => "connection",
            Self::Handshake => "TLS handshake",
            Self::Write => "write",
            Self::Flush => "flush",
        })
    }
}

/// How long each step of sending a frame may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// Opening the TCP connection.
    pub connect: Duration,
    /// The TLS handshake, and each write and flush of a frame.
    pub send: Duration,
}

/// How a connection is secured.
#[derive(Clone)]
enum Transport {
    Tls(TlsConnector, ServerName<'static>),
    Plain,
}

/// An Audit Record Repository the sender writes to.
#[derive(Clone)]
pub struct Repository {
    host: String,
    port: u16,
    transport: Transport,
    timeouts: Timeouts,
}

impl fmt::Debug for Repository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Repository")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &matches!(self.transport, Transport::Tls(..)))
            .finish_non_exhaustive()
    }
}

/// The TLS settings of a repository connection.
#[derive(Debug, Clone, Default)]
pub struct TlsSettings {
    /// PEM certificates trusted beside the platform's roots.
    pub roots: Option<Vec<u8>>,
    /// The PEM client certificate chain and private key the gateway
    /// authenticates with, when the repository asks for one.
    pub identity: Option<SecretString>,
}

impl Repository {
    /// The repository at `address`, `tls://host[:port]`, reached over TLS
    /// 1.2 or later with `tls`, each step bounded by `timeouts`.
    ///
    /// # Errors
    ///
    /// A [`RepositoryError`] for an address of another form, and for trust
    /// roots or an identity that do not read as PEM.
    pub fn tls(
        address: &Url,
        tls: &TlsSettings,
        timeouts: Timeouts,
    ) -> Result<Self, RepositoryError> {
        if address.scheme() != "tls" {
            return Err(RepositoryError::Address);
        }
        let (host, port) = host_port(address, Some(TLS_PORT))?;
        let name = ServerName::try_from(host.clone()).map_err(RepositoryError::ServerName)?;
        let connector = TlsConnector::from(Arc::new(client_config(tls)?));
        Ok(Self {
            host,
            port,
            transport: Transport::Tls(connector, name),
            timeouts,
        })
    }

    /// The repository at `address`, `tcp://host:port`, reached without
    /// encryption: for a development deployment only, since every message
    /// names a patient.
    ///
    /// # Errors
    ///
    /// [`RepositoryError::Address`] for an address of another form.
    pub fn unencrypted_for_development(
        address: &Url,
        timeouts: Timeouts,
    ) -> Result<Self, RepositoryError> {
        if address.scheme() != "tcp" {
            return Err(RepositoryError::Address);
        }
        let (host, port) = host_port(address, None)?;
        Ok(Self {
            host,
            port,
            transport: Transport::Plain,
            timeouts,
        })
    }

    /// Opens a connection: the TCP connection within the connect timeout,
    /// then the TLS handshake within the send timeout.
    ///
    /// # Errors
    ///
    /// A [`SendError`] when the repository cannot be reached, fails the TLS
    /// handshake, or does not finish a step within its timeout.
    pub async fn connect(&self) -> Result<Connection, SendError> {
        let stream = bounded(
            self.timeouts.connect,
            Step::Connect,
            TcpStream::connect((self.host.as_str(), self.port)),
        )
        .await?
        .map_err(SendError::Connect)?;
        let stream: Box<dyn Stream> = match &self.transport {
            Transport::Plain => Box::new(stream),
            Transport::Tls(connector, name) => Box::new(
                bounded(
                    self.timeouts.send,
                    Step::Handshake,
                    connector.connect(name.clone(), stream),
                )
                .await?
                .map_err(SendError::Handshake)?,
            ),
        };
        Ok(Connection {
            stream,
            send_timeout: self.timeouts.send,
        })
    }
}

/// `step`, given at most `limit` to finish.
async fn bounded<T>(
    limit: Duration,
    step: Step,
    future: impl Future<Output = T>,
) -> Result<T, SendError> {
    tokio::time::timeout(limit, future)
        .await
        .map_err(|_elapsed| SendError::Timeout(step))
}

/// A connection's byte stream, plain or TLS.
trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

/// An open connection to a repository.
pub struct Connection {
    stream: Box<dyn Stream>,
    send_timeout: Duration,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection").finish_non_exhaustive()
    }
}

impl Connection {
    /// Whether the repository has closed the connection, or it has failed.
    ///
    /// A repository sends nothing on a syslog connection (RFC 5425 §4.3), so
    /// a read that ends or fails at once means the connection is gone. A
    /// write to a connection closed after this check can still be lost:
    /// syslog has no acknowledgement.
    pub async fn is_closed(&mut self) -> bool {
        let mut probe = [0_u8; 1];
        match tokio::time::timeout(Duration::ZERO, self.stream.read(&mut probe)).await {
            Ok(Ok(0) | Err(_)) => true,
            Ok(Ok(_)) | Err(_) => false,
        }
    }

    /// Writes `frame` and flushes it, each within the send timeout, so a
    /// repository that stops reading cannot hold the sender.
    ///
    /// # Errors
    ///
    /// [`SendError::Write`] when the connection fails, and
    /// [`SendError::Timeout`] when the write or the flush does not finish in
    /// time; the connection is not used again after either.
    pub async fn send(&mut self, frame: &SecretSlice<u8>) -> Result<(), SendError> {
        let limit = self.send_timeout;
        bounded(
            limit,
            Step::Write,
            self.stream.write_all(frame.expose_secret()),
        )
        .await?
        .map_err(SendError::Write)?;
        bounded(limit, Step::Flush, self.stream.flush())
            .await?
            .map_err(SendError::Write)
    }
}

/// The host and port of `address`, the port `default` when it names none.
fn host_port(address: &Url, default: Option<u16>) -> Result<(String, u16), RepositoryError> {
    let clean = address.path().is_empty()
        && address.query().is_none()
        && address.fragment().is_none()
        && address.username().is_empty()
        && address.password().is_none();
    if !clean {
        return Err(RepositoryError::Extra);
    }
    let host = match address.host() {
        Some(url::Host::Domain(name)) => name.to_owned(),
        Some(url::Host::Ipv4(ip)) => ip.to_string(),
        Some(url::Host::Ipv6(ip)) => ip.to_string(),
        None => return Err(RepositoryError::Address),
    };
    let port = address.port().or(default).ok_or(RepositoryError::Address)?;
    Ok((host, port))
}

/// The client configuration: rustls over aws-lc-rs at TLS 1.2 or later,
/// verifying the repository against the platform's roots and `tls.roots`.
fn client_config(tls: &TlsSettings) -> Result<ClientConfig, RepositoryError> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let roots = match &tls.roots {
        Some(pem) => CertificateDer::pem_slice_iter(pem)
            .collect::<Result<Vec<_>, _>>()
            .map_err(RepositoryError::Roots)?,
        None => Vec::new(),
    };
    let verifier = Verifier::new_with_extra_roots(roots, Arc::clone(&provider))
        .map_err(RepositoryError::Tls)?;
    // NOTE: ITI TF-2 §3.20.4.1.2.1.1: the transport must be TLS, 1.2 recommended.
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(RepositoryError::Tls)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier));
    match &tls.identity {
        None => Ok(builder.with_no_client_auth()),
        Some(pem) => {
            let pem = pem.expose_secret().as_bytes();
            let chain = CertificateDer::pem_slice_iter(pem)
                .collect::<Result<Vec<_>, _>>()
                .map_err(RepositoryError::Identity)?;
            let key = PrivateKeyDer::from_pem_slice(pem).map_err(RepositoryError::Identity)?;
            if chain.is_empty() {
                return Err(RepositoryError::NoCertificate);
            }
            builder
                .with_client_auth_cert(chain, key)
                .map_err(RepositoryError::Tls)
        }
    }
}
