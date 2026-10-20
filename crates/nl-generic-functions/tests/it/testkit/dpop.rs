// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The test side of `DPoP` (RFC 9449): the check a test device makes of a
//! proof.
//!
//! [`verify`] holds a proof to RFC 9449 §4.3: typed `dpop+jwt`, signed ES256
//! or ES384 by the public key its header carries, binding the request's
//! method and URL without query and fragment, issued within a minute of
//! now, with a `jti`, and, for a request carrying an access token, the
//! token's SHA-256 in `ath` (§7.1). No specification governs the device:
//! our own design.

use aws_lc_rs::digest::{SHA256, digest};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::jwk::ThumbprintHash;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::Deserialize;

/// The header a proof travels in.
pub(crate) const HEADER: &str = "DPoP";

/// The header a server sends a nonce in.
pub(crate) const NONCE_HEADER: &str = "DPoP-Nonce";

/// How far a proof's `iat` may lie from now, in seconds.
pub(crate) const IAT_WINDOW_S: i64 = 60;

/// The claims of a proof, as a test device reads them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Proof {
    /// The proof's unique id.
    pub(crate) jti: String,
    /// The method it binds.
    pub(crate) htm: String,
    /// The URL it binds.
    pub(crate) htu: String,
    /// When it was made, in seconds since the epoch.
    pub(crate) iat: i64,
    /// The hash of the access token it binds, when it binds one.
    pub(crate) ath: Option<String>,
    /// The nonce it names, when it names one.
    pub(crate) nonce: Option<String>,
}

/// A proof that verified, with the RFC 7638 thumbprint of its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verified {
    /// The thumbprint of the key that signed it, the `jkt` of a token bound
    /// to it (RFC 9449 §6.1).
    pub(crate) jkt: String,
    /// Its claims.
    pub(crate) proof: Proof,
}

/// The `ath` of `token`: the base64url SHA-256 of its text (RFC 9449 §4.2).
#[must_use]
pub(crate) fn ath(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(digest(&SHA256, token.as_bytes()))
}

/// Verifies `proof` as a proof of a request with `method` to `htu`,
/// carrying `token` when it carries one.
///
/// # Errors
///
/// Returns the reason the proof is refused.
pub(crate) fn verify(
    proof: &str,
    method: &str,
    htu: &str,
    token: Option<&str>,
) -> Result<Verified, String> {
    let header = jsonwebtoken::decode_header(proof).map_err(|error| format!("header: {error}"))?;
    if header.typ.as_deref() != Some("dpop+jwt") {
        return Err(format!("the proof is typed {:?}", header.typ));
    }
    if !matches!(header.alg, Algorithm::ES256 | Algorithm::ES384) {
        return Err(format!("the proof is signed with {:?}", header.alg));
    }
    let jwk = header.jwk.ok_or("the proof carries no jwk")?;
    let key = DecodingKey::from_jwk(&jwk).map_err(|error| format!("jwk: {error}"))?;
    let mut validation = Validation::new(header.alg);
    validation.validate_exp = false;
    validation.validate_aud = false;
    validation.set_required_spec_claims::<&str>(&[]);
    let claims = jsonwebtoken::decode::<Proof>(proof, &key, &validation)
        .map_err(|error| format!("verification: {error}"))?
        .claims;
    if claims.htm != method {
        return Err(format!("the proof binds {} for {method}", claims.htm));
    }
    if claims.htu != htu {
        return Err(format!("the proof binds {} for {htu}", claims.htu));
    }
    let skew = jiff::Timestamp::now()
        .as_second()
        .saturating_sub(claims.iat)
        .abs();
    if skew > IAT_WINDOW_S {
        return Err(format!("the proof was made {skew} s from now"));
    }
    if claims.jti.is_empty() {
        return Err("the proof's jti is empty".to_owned());
    }
    match (token, claims.ath.as_deref()) {
        (Some(token), Some(hash)) if hash == ath(token) => {}
        (Some(_), _) => return Err("the proof's ath is not the token's hash".to_owned()),
        (None, Some(_)) => return Err("the proof binds a token the request lacks".to_owned()),
        (None, None) => {}
    }
    let jkt = jwk
        .thumbprint(ThumbprintHash::SHA256)
        .map_err(|error| format!("thumbprint: {error}"))?;
    Ok(Verified { jkt, proof: claims })
}
