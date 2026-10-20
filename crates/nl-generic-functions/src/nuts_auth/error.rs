// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why an access token request has no token to give.
//!
//! No error here carries a credential, a presentation, a key, a proof or an
//! access token, and none carries a request URL. The text an authorization
//! server wrote is cut to [`TEXT_LIMIT`] characters, and a body that does not
//! parse is named by the position of the fault, never by its content.

use http::StatusCode;

use crate::nuts_auth::presentation::Mismatch;

/// The most characters of an authorization server's `error` or
/// `error_description` an error keeps (no specification governs this: our
/// own design).
pub const TEXT_LIMIT: usize = 256;

/// An argument the client refuses before anything is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidInput {
    /// The authorization server's issuer identifier is not an `http` or
    /// `https` URL without userinfo, query or fragment (RFC 8414 §2), written
    /// in its canonical form.
    #[error(
        "the authorization server is not an http(s) URL without userinfo, query or fragment (RFC 8414 §2), in canonical form"
    )]
    AuthorizationServer,
    /// The scope is empty, or a scope token holds a character RFC 6749 §3.3
    /// does not admit.
    #[error("the scope is not a space-delimited list of RFC 6749 §3.3 scope tokens")]
    Scope,
    /// The `client_id` is empty or holds a control character.
    #[error("the client_id is empty or holds a control character")]
    ClientId,
}

/// The exchange an error happened in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    /// Reading the authorization server metadata (RFC 8414 §3).
    Metadata,
    /// Reading the Presentation Definition for the scope (Nuts RFC021 §5).
    Definition,
    /// The access token request (Nuts RFC021 §3).
    Token,
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Metadata => "the authorization server metadata",
            Self::Definition => "the presentation definition endpoint",
            Self::Token => "the token endpoint",
        })
    }
}

/// How the authorization server metadata falls short of what the grant
/// needs (RFC 8414 §2 and §3.3, Nuts RFC021 §3.1 and §5, RFC 9449 §5.1).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MetadataError {
    /// The metadata's `issuer` is not the issuer it was asked of (RFC 8414
    /// §3.3).
    #[error("the metadata names another issuer (RFC 8414 §3.3)")]
    Issuer,
    /// The metadata has no `token_endpoint`, or one that is not a URL (RFC
    /// 8414 §2).
    #[error("the metadata names no token_endpoint URL")]
    TokenEndpoint,
    /// The metadata has no `presentation_definition_endpoint`, or one that is
    /// not a URL (Nuts RFC021 §5).
    #[error("the metadata names no presentation_definition_endpoint URL (Nuts RFC021 §5)")]
    DefinitionEndpoint,
    /// An endpoint is not on the issuer's origin (scheme, host and port), so
    /// the holder's credentials would go elsewhere than the issuer.
    #[error("an endpoint of the metadata is not on the issuer's origin")]
    OtherOrigin,
    /// An endpoint carries userinfo or a fragment.
    #[error("an endpoint of the metadata carries userinfo or a fragment")]
    InsecureEndpoint,
    /// The metadata's `vp_formats` does not name `jwt_vp`, or names it
    /// without the holder key's algorithm (Nuts RFC021 §3.1).
    #[error(
        "the metadata's vp_formats admits no jwt_vp signed with {algorithm} (Nuts RFC021 §3.1)"
    )]
    PresentationFormat {
        /// The holder key's algorithm.
        algorithm: String,
    },
    /// The metadata lists `dpop_signing_alg_values_supported` without the
    /// prover's algorithm (RFC 9449 §5.1).
    #[error(
        "the metadata's dpop_signing_alg_values_supported does not list {algorithm} (RFC 9449 §5.1)"
    )]
    ProofAlgorithm {
        /// The prover's algorithm.
        algorithm: String,
    },
}

/// How an answer departs from the shape its step defines.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Malformation {
    /// The answer is longer than the client reads.
    #[error("the answer exceeds {limit} bytes")]
    TooLarge {
        /// The limit, in bytes.
        limit: usize,
    },
    /// The answer's media type is not `application/json` (RFC 8414 §3.2,
    /// RFC 6749 §5.1 and §5.2).
    #[error("the answer is not application/json")]
    MediaType,
    /// The answer is not JSON.
    #[error("the answer is not JSON (line {line}, column {column})")]
    NotJson {
        /// The line of the syntax error.
        line: usize,
        /// The column of the syntax error.
        column: usize,
    },
    /// An object of the answer repeats a name, which JSON parsers read
    /// differently (RFC 8259 §4).
    #[error("an object of the answer repeats a name (line {line}, column {column})")]
    RepeatedName {
        /// The line of the fault.
        line: usize,
        /// The column of the fault.
        column: usize,
    },
    /// The answer is JSON of another shape than the step defines.
    #[error("the answer is not of the shape the step defines (line {line}, column {column})")]
    Shape {
        /// The line of the fault.
        line: usize,
        /// The column of the fault.
        column: usize,
    },
}

impl Malformation {
    /// The malformation `error` describes, by position alone: a
    /// `serde_json` message can quote the value it refused.
    pub(super) fn of(error: &serde_json::Error) -> Self {
        let (line, column) = (error.line(), error.column());
        if error.is_data() {
            Self::Shape { line, column }
        } else {
            Self::NotJson { line, column }
        }
    }
}

/// A `DPoP` proof that could not be made (RFC 9449 §4.2); the prover's own
/// error is its source.
#[derive(Debug, thiserror::Error)]
#[error("the DPoP proof could not be made")]
pub struct ProofError(#[source] Box<dyn std::error::Error + Send + Sync>);

impl ProofError {
    /// A proof error caused by `source`.
    pub fn new(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self(Box::new(source))
    }
}

/// Why an access token request did not end in a token.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NutsAuthError {
    /// No answer arrived within the timeout.
    #[error("{step} did not answer within the timeout")]
    Timeout {
        /// The step that timed out.
        step: Step,
    },
    /// The request could not be sent or the answer could not be read.
    #[error("{step} could not be reached")]
    Transport {
        /// The step.
        step: Step,
        /// The transport's error, with the request URL removed.
        #[source]
        source: reqwest::Error,
    },
    /// The step answered a status the exchange does not take: anything but
    /// `200`, and, at the token endpoint, anything but `200` or an RFC 6749
    /// §5.2 error.
    #[error("{step} answered {status}")]
    Status {
        /// The step.
        step: Step,
        /// The HTTP status.
        status: StatusCode,
    },
    /// The answer does not have the shape its step defines.
    #[error("{step} gave a malformed answer")]
    Malformed {
        /// The step.
        step: Step,
        /// How the answer departs from its shape.
        #[source]
        malformation: Malformation,
    },
    /// The authorization server metadata does not support the grant.
    #[error("the authorization server metadata does not support the grant")]
    Metadata(#[from] MetadataError),
    /// The holder's credentials do not answer the Presentation Definition,
    /// so nothing was sent to the token endpoint.
    #[error("the holder's credentials do not answer the presentation definition")]
    Definition(#[from] Mismatch),
    /// A credential has expired, so nothing was sent to the token endpoint.
    #[error("the credential answering {descriptor:?} has expired")]
    Expired {
        /// The input descriptor the credential answers.
        descriptor: String,
    },
    /// The Verifiable Presentation could not be signed.
    #[error("the verifiable presentation could not be signed")]
    Sign(#[source] jsonwebtoken::errors::Error),
    /// The presentation submission could not be written.
    #[error("the presentation submission could not be written")]
    Submission(#[source] serde_json::Error),
    /// The `DPoP` proof of the token request could not be made.
    #[error(transparent)]
    Proof(#[from] ProofError),
    /// The token endpoint refused the request with an RFC 6749 §5.2 error.
    #[error(
        "the token endpoint refused the request with {status}: {error}{}",
        description.as_deref().map(|text| format!(" ({text})")).unwrap_or_default()
    )]
    Refused {
        /// The status it answered.
        status: StatusCode,
        /// The `error` code (RFC 6749 §5.2), cut to [`TEXT_LIMIT`].
        error: String,
        /// The `error_description`, cut to [`TEXT_LIMIT`], when it sent one.
        description: Option<String>,
    },
    /// The token endpoint issued a token that is not `DPoP`-bound (RFC 9449
    /// §5; the IG's GFI-004, Access Token Response).
    #[error("the token endpoint issued a token of type {token_type}, not DPoP")]
    TokenType {
        /// The `token_type` it named, cut to [`TEXT_LIMIT`].
        token_type: String,
    },
}

impl NutsAuthError {
    /// A transport failure of `step`, with the request URL removed.
    pub(super) fn transport(step: Step, error: reqwest::Error) -> Self {
        let error = error.without_url();
        if error.is_timeout() {
            Self::Timeout { step }
        } else {
            Self::Transport {
                step,
                source: error,
            }
        }
    }
}

/// At most [`TEXT_LIMIT`] characters of `text`.
pub(super) fn bounded(text: &str) -> String {
    text.chars().take(TEXT_LIMIT).collect()
}
