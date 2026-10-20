// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Synthetic signing keys, generated in PKCS#8 PEM at run time so no key is
//! ever committed. No specification governs this: our own design, carried
//! over from FerroFED's testkit with the crate.

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{
    ECDSA_P256_SHA256_FIXED_SIGNING, ECDSA_P384_SHA384_FIXED_SIGNING, EcdsaKeyPair,
    EcdsaSigningAlgorithm,
};

/// A key could not be generated.
#[derive(Debug, thiserror::Error)]
#[error("a synthetic signing key could not be generated")]
pub(crate) struct KeyGenError;

/// A fresh ES384 (P-384) private key in PKCS#8 PEM.
pub(crate) fn es384_pem() -> Result<String, KeyGenError> {
    generated(&ECDSA_P384_SHA384_FIXED_SIGNING)
}

/// A fresh P-256 private key in PKCS#8 PEM, a key ES384 cannot sign with.
pub(crate) fn p256_pem() -> Result<String, KeyGenError> {
    generated(&ECDSA_P256_SHA256_FIXED_SIGNING)
}

fn generated(algorithm: &'static EcdsaSigningAlgorithm) -> Result<String, KeyGenError> {
    let document = EcdsaKeyPair::generate_pkcs8(algorithm, &SystemRandom::new())
        .map_err(|_unspecified| KeyGenError)?;
    Ok(pem::encode(&pem::Pem::new(
        "PRIVATE KEY",
        document.as_ref().to_vec(),
    )))
}
