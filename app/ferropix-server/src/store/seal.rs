// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The sealing of identifier values at rest.
//!
//! An identifier value in a sensitive domain is stored as AES-256-GCM
//! ciphertext under the key `[db] data_key_file` names, bound to its domain
//! as associated data, and every identifier value is found by HMAC-SHA256
//! over its domain and its value under a second key derived from the first
//! with HKDF-SHA256 (RFC 5869). A dump therefore shows no identifier, and an
//! equality lookup is one probe of the `(domain_id, identifier_hmac)` index.
//! A [`KeyId`], derived the same way, names the key a row was written under.
//!
//! No specification governs sealing: our own design (`docs/architecture.md`
//! §4.2), after FerroEHR's sealed `national_identifier`.

use std::fmt;
use std::sync::Arc;

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use aws_lc_rs::error::Unspecified;
use aws_lc_rs::{hkdf, hmac, rand};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use secrecy::{ExposeSecret, ExposeSecretMut as _, SecretBox};
use uuid::Uuid;

/// The length of the sealing key in bytes.
pub const KEY_LEN: usize = 32;

/// The length of a [`KeyId`] in bytes.
pub const KEY_ID_LEN: usize = 16;

/// The length of an identifier HMAC in bytes.
pub const HMAC_LEN: usize = 32;

/// The first byte of every sealed value: the layout of this module.
const SEALED_FORMAT: u8 = 1;

/// The HKDF info of the HMAC key.
const HMAC_INFO: &[u8] = b"ferropix identifier hmac v1";

/// The HKDF info of the key id.
const KEY_ID_INFO: &[u8] = b"ferropix key id v1";

/// The sealing key is not the base64 of 32 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the sealing key is not the base64 of exactly 32 bytes")]
pub struct KeyFormatError;

/// The cryptographic library refused an operation, or a sealed value does
/// not open under the key.
///
/// No variant carries a byte of the key or of the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SealError {
    /// A random nonce could not be drawn, or a key could not be built.
    #[error("the cryptographic library refused the operation")]
    Crypto(#[source] Unspecified),
    /// A sealed value is shorter than its layout or names another layout.
    #[error("a sealed identifier is malformed")]
    Malformed,
    /// A sealed value does not open under the key and the domain it is read
    /// for.
    #[error("a sealed identifier does not open under the configured key")]
    Open(#[source] Unspecified),
    /// An opened value is not UTF-8.
    #[error("an opened identifier is not UTF-8")]
    Utf8(#[source] std::str::Utf8Error),
}

/// The 32-byte sealing key, as the key file holds it: its base64, trimmed.
///
/// It renders as `***` through `Debug`, and its bytes are zeroed on drop.
#[derive(Clone)]
pub struct SealingKey(Arc<SecretBox<[u8; KEY_LEN]>>);

impl SealingKey {
    /// Reads a key from the base64 text of its 32 bytes.
    ///
    /// A key is made with `openssl rand -base64 32`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ferropix_server::store::seal::SealingKey;
    ///
    /// let key = SealingKey::from_base64("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")?;
    /// assert_eq!(format!("{key:?}"), "SealingKey(***)");
    /// assert!(SealingKey::from_base64("too short").is_err());
    /// # Ok::<(), ferropix_server::store::seal::KeyFormatError>(())
    /// ```
    ///
    /// # Errors
    /// Returns [`KeyFormatError`] when `text` is not standard padded base64 or
    /// does not decode to exactly 32 bytes.
    pub fn from_base64(text: &str) -> Result<Self, KeyFormatError> {
        // NOTE: no specification governs this: our own design; the decoder's
        // error names the offending byte, which is key material, so it is dropped.
        let decoded = SecretBox::new(Box::new(
            STANDARD.decode(text.trim()).ok().ok_or(KeyFormatError)?,
        ));
        let bytes = <[u8; KEY_LEN]>::try_from(decoded.expose_secret().as_slice())
            .ok()
            .ok_or(KeyFormatError)?;
        Ok(Self(Arc::new(SecretBox::new(Box::new(bytes)))))
    }

    /// Returns the bytes, for the derivations of this module.
    fn bytes(&self) -> &[u8; KEY_LEN] {
        self.0.expose_secret()
    }
}

impl fmt::Debug for SealingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SealingKey(***)")
    }
}

/// The id of a sealing key: 16 bytes derived from it, which identify the key
/// without revealing it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId([u8; KEY_ID_LEN]);

impl KeyId {
    /// Returns the id as the bytes stored beside every row.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; KEY_ID_LEN] {
        &self.0
    }

    /// Returns the id `bytes` hold, or `None` when they are not 16 bytes.
    #[must_use]
    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(Self)
    }
}

impl fmt::Debug for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyId({self})")
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0.iter().take(4) {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// An identifier value in one domain: the value of a FHIR `Identifier` or
/// the ID of a CX, as the feed normalised it.
///
/// It has no `Display`, and its `Debug` shows only its length, so the value
/// never reaches a log line or an error message.
#[derive(Clone, PartialEq, Eq)]
pub struct IdentifierValue(String);

impl IdentifierValue {
    /// Returns the value `text` holds, or `None` for an empty one (a FHIR R4
    /// `string` holds at least one character).
    #[must_use]
    pub fn new(text: impl Into<String>) -> Option<Self> {
        let text = text.into();
        (!text.is_empty()).then_some(Self(text))
    }

    /// Returns the value, for the one place it is used.
    ///
    /// Never log or format the result.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for IdentifierValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IdentifierValue(*** {} bytes)", self.0.len())
    }
}

/// The three keys one sealing key yields: the cipher key, the HMAC key and
/// the key id.
pub struct Sealer {
    /// The AES-256-GCM key.
    cipher: LessSafeKey,
    /// The HMAC-SHA256 key, derived from the sealing key.
    hmac: hmac::Key,
    /// The id of the sealing key.
    key_id: KeyId,
}

impl fmt::Debug for Sealer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sealer")
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

/// The output length of an HKDF expansion into a fixed-size buffer.
struct Length(usize);

impl hkdf::KeyType for Length {
    fn len(&self) -> usize {
        self.0
    }
}

impl Sealer {
    /// Builds the keys `key` yields.
    ///
    /// # Errors
    /// Returns [`SealError::Crypto`] when the library refuses a derivation.
    pub fn new(key: &SealingKey) -> Result<Self, SealError> {
        let cipher = UnboundKey::new(&AES_256_GCM, key.bytes())
            .map(LessSafeKey::new)
            .map_err(SealError::Crypto)?;
        let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, &[]).extract(key.bytes());
        let mut hmac_key = SecretBox::new(Box::new([0_u8; HMAC_LEN]));
        prk.expand(&[HMAC_INFO], Length(HMAC_LEN))
            .and_then(|okm| okm.fill(hmac_key.expose_secret_mut()))
            .map_err(SealError::Crypto)?;
        let mut key_id = [0_u8; KEY_ID_LEN];
        prk.expand(&[KEY_ID_INFO], Length(KEY_ID_LEN))
            .and_then(|okm| okm.fill(&mut key_id))
            .map_err(SealError::Crypto)?;
        Ok(Self {
            cipher,
            hmac: hmac::Key::new(hmac::HMAC_SHA256, hmac_key.expose_secret()),
            key_id: KeyId(key_id),
        })
    }

    /// Returns the id of the key.
    #[must_use]
    pub fn key_id(&self) -> KeyId {
        self.key_id
    }

    /// Returns the HMAC-SHA256 of `value` in `domain`, the column an equality
    /// lookup probes.
    ///
    /// The domain id comes first, so one value in two domains yields two
    /// unrelated HMACs and a dump does not show which records share it.
    #[must_use]
    pub fn hmac(&self, domain: Uuid, value: &IdentifierValue) -> [u8; HMAC_LEN] {
        let mut context = hmac::Context::with_key(&self.hmac);
        context.update(domain.as_bytes());
        context.update(value.expose().as_bytes());
        let mut out = [0_u8; HMAC_LEN];
        out.copy_from_slice(context.sign().as_ref());
        out
    }

    /// Seals `value` for `domain`: the format byte, a random 96-bit nonce,
    /// and the ciphertext with its tag, the domain id as associated data.
    ///
    /// # Errors
    /// Returns [`SealError::Crypto`] when no nonce can be drawn or the
    /// library refuses the encryption.
    pub fn seal(&self, domain: Uuid, value: &IdentifierValue) -> Result<Vec<u8>, SealError> {
        let mut nonce = [0_u8; NONCE_LEN];
        rand::fill(&mut nonce).map_err(SealError::Crypto)?;
        let mut sealed = Vec::with_capacity(1 + NONCE_LEN + value.expose().len() + 16);
        sealed.push(SEALED_FORMAT);
        sealed.extend_from_slice(&nonce);
        let mut body = value.expose().as_bytes().to_vec();
        self.cipher
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(domain.as_bytes()),
                &mut body,
            )
            .map_err(SealError::Crypto)?;
        sealed.extend_from_slice(&body);
        Ok(sealed)
    }

    /// Opens a value [`Sealer::seal`] sealed for `domain` under this key.
    ///
    /// # Errors
    /// Returns [`SealError::Malformed`] when `sealed` does not have the layout
    /// of this module, [`SealError::Open`] when it was sealed for another
    /// domain or under another key, and [`SealError::Utf8`] when the opened
    /// bytes are not UTF-8.
    pub fn open(&self, domain: Uuid, sealed: &[u8]) -> Result<IdentifierValue, SealError> {
        let Some((&SEALED_FORMAT, rest)) = sealed.split_first() else {
            return Err(SealError::Malformed);
        };
        let (nonce, body) = rest
            .split_at_checked(NONCE_LEN)
            .ok_or(SealError::Malformed)?;
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(SealError::Open)?;
        let mut body = body.to_vec();
        let opened = self
            .cipher
            .open_in_place(nonce, Aad::from(domain.as_bytes()), &mut body)
            .map_err(SealError::Open)?;
        let text = std::str::from_utf8(opened).map_err(SealError::Utf8)?;
        IdentifierValue::new(text).ok_or(SealError::Malformed)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{IdentifierValue, KeyFormatError, SealError, Sealer, SealingKey};

    fn key(byte: u8) -> SealingKey {
        use base64::Engine as _;
        SealingKey::from_base64(&base64::engine::general_purpose::STANDARD.encode([byte; 32]))
            .unwrap()
    }

    fn value(text: &str) -> IdentifierValue {
        IdentifierValue::new(text).unwrap()
    }

    #[test]
    fn a_key_is_the_base64_of_exactly_32_bytes() {
        use base64::Engine as _;
        let engine = base64::engine::general_purpose::STANDARD;
        assert!(SealingKey::from_base64(&format!("  {}\n", engine.encode([7_u8; 32]))).is_ok());
        for bad in [
            engine.encode([7_u8; 31]),
            engine.encode([7_u8; 33]),
            String::from("not base64 at all!"),
            String::new(),
        ] {
            assert_eq!(
                Some(KeyFormatError),
                SealingKey::from_base64(&bad).err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_sealed_value_opens_under_its_key_and_domain_only() {
        let sealer = Sealer::new(&key(1)).unwrap();
        let domain = Uuid::from_u128(1);
        let ciphertext = sealer.seal(domain, &value("H-1001")).unwrap();
        assert_eq!(value("H-1001"), sealer.open(domain, &ciphertext).unwrap());
        assert!(
            !ciphertext.windows(6).any(|window| window == b"H-1001"),
            "the plain value must not show"
        );
        assert!(
            matches!(
                sealer.open(Uuid::from_u128(2), &ciphertext),
                Err(SealError::Open(_))
            ),
            "another domain"
        );
        let other = Sealer::new(&key(2)).unwrap();
        assert!(
            matches!(other.open(domain, &ciphertext), Err(SealError::Open(_))),
            "another key"
        );
        assert!(
            matches!(sealer.open(domain, &[]), Err(SealError::Malformed)),
            "empty"
        );
        assert!(
            matches!(sealer.open(domain, &[9; 40]), Err(SealError::Malformed)),
            "another layout"
        );
    }

    #[test]
    fn two_seals_of_one_value_differ_and_its_hmac_does_not() {
        let sealer = Sealer::new(&key(1)).unwrap();
        let domain = Uuid::from_u128(1);
        let first = sealer.seal(domain, &value("H-1001")).unwrap();
        let second = sealer.seal(domain, &value("H-1001")).unwrap();
        assert_ne!(first, second, "a random nonce per seal");
        assert_eq!(
            sealer.hmac(domain, &value("H-1001")),
            sealer.hmac(domain, &value("H-1001"))
        );
        assert_ne!(
            sealer.hmac(domain, &value("H-1001")),
            sealer.hmac(Uuid::from_u128(2), &value("H-1001")),
            "the domain is part of the HMAC"
        );
        assert_ne!(
            sealer.hmac(domain, &value("H-1001")),
            Sealer::new(&key(2)).unwrap().hmac(domain, &value("H-1001")),
            "the key is part of the HMAC"
        );
    }

    #[test]
    fn the_key_id_names_the_key() {
        let one = Sealer::new(&key(1)).unwrap();
        assert_eq!(one.key_id(), Sealer::new(&key(1)).unwrap().key_id());
        assert_ne!(one.key_id(), Sealer::new(&key(2)).unwrap().key_id());
    }

    #[test]
    fn no_rendering_shows_a_key_or_a_value() {
        let sealing = key(0x41);
        assert_eq!("SealingKey(***)", format!("{sealing:?}"));
        let sealer = Sealer::new(&sealing).unwrap();
        assert!(!format!("{sealer:?}").contains("AAAA"), "{sealer:?}");
        let bsn = ferropix_testkit::synthetic::bsn(0).unwrap().digits();
        let identifier = value(&bsn);
        let rendered = format!("{identifier:?}");
        assert!(!rendered.contains(&bsn), "{rendered}");
        assert_eq!(None, IdentifierValue::new(""), "an empty value");
    }
}
