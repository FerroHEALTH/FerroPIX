// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! GF-Consent through Mitz (feature `mitz`): the closed authorization
//! question ("gesloten autorisatievraag").
//!
//! The IG's Consent page leaves the question to the Mitz afsprakenstelsel:
//! Mitz takes a data user organisation of one category, a patient and a data
//! holder organisation of another, and answers allow or deny from the
//! consent the patient recorded; the consents themselves cannot be read. The
//! wire is the VZVZ *Implementatiehandleiding Open en gesloten
//! autorisatievraag* 3.8.2 (§2.3, §3.2, §3.3), with the attributes and the
//! error handling of the *Programma van Eisen AMC Aansluiting Mitz-connector* 3.8.1.ad1
//! (AUS-TR-e0040, AUS-TR-e0050, AUS-TR-e0900):
//!
//! - a SOAP 1.2 request under ITI TF-2x Appendix V, its body one XACML 3.0
//!   `XACMLAuthzDecisionQuery` whose `Request` names the patient by BSN, the
//!   data holder by URA and care provider category, one action per data
//!   category, the responsible professional with their role, the data user
//!   by URA and category, and the purpose (§3.2.4.2, §3.2.4.4);
//! - an XACML 3.0 `Response` with one `Result` per data category, each a
//!   `Permit` or a `Deny` (§3.2.4.6, §3.2.5);
//! - mutual TLS under IHE ATNA (§3.3), and an `X-Request-Id` header on every
//!   request (§6).
//!
//! [`MitzClient::ask`] sends one [`ClosedQuestion`] and reads the answer into
//! a [`ClosedAnswer`], a decision per category, or a [`MitzError`]. Only
//! `Permit` and `Deny` are decisions: `Indeterminate`, `NotApplicable`, a
//! fault, a status other than `200`, a timeout and an answer that does not
//! hold to the question are each an error, so a caller never reads a failure
//! as a permit. The XACML 2.0 variant of §3.2.4.5, which carries the
//! attributes in a signed SAML assertion, is not offered; the message token
//! of the *Implementatiehandleiding Berichtauthenticatie* is one a connector
//! opts into at admission (Programma van Eisen AMC AUS-TR-e0250), and the client does not.
//!
//! The BSN travels only in the request to Mitz, which is the transaction's
//! purpose: [`Bsn`](question::Bsn) redacts it in `Debug`, no error carries it, a request,
//! an answer or text Mitz wrote, and the client follows no redirect.
//!
//! # Examples
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use ferrohealth_smart::client::outbound::OutboundClient;
//! use ferrohealth_smart::config::TlsBuilder;
//! use ferrohealth_smart::config::endpoint::EndpointSection;
//! use nl_generic_functions::identification::Ura;
//! use nl_generic_functions::mitz::MitzClient;
//! use nl_generic_functions::mitz::question::{
//!     Bsn, CareProviderType, ClosedQuestion, DataCategory, DataHolder, DataUser, ProfessionalId,
//!     Purpose, RoleCode,
//! };
//! use secrecy::SecretString;
//!
//! # fn configured() -> (EndpointSection, &'static dyn TlsBuilder) {
//! #     unimplemented!("the product's configuration and its reader of TLS material")
//! # }
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! // The `[mitz.endpoint]` table, whose `tls` names the connector's client
//! // certificate (§3.3) and whose `url` is the question's endpoint.
//! let (endpoint, tls) = configured();
//! let client = MitzClient::new(OutboundClient::build(&endpoint, "mitz.endpoint", tls)?)?;
//! let kind = CareProviderType::new("V6")?;
//! let responsible = ProfessionalId::new("2.999.10", "professional0001")?;
//! let role = RoleCode::new("01.015")?;
//! let user = DataUser::new(Ura::new("ura-test-0100")?, kind.clone(), responsible, role);
//! let holder = DataHolder::new(Ura::new("ura-test-0001")?, kind);
//! let patient = Bsn::new(SecretString::from("bsn-synthetic-0001"))?;
//! let categories = vec![DataCategory::new("GGC002")?];
//! let question = ClosedQuestion::new(patient, holder, user, categories, Purpose::Treatment)?;
//! let answer = client.ask(&question, Duration::from_secs(2)).await?;
//! let _denied = answer.denies_all();
//! # Ok(())
//! # }
//! # fn main() {
//! #     let _pending = run();
//! # }
//! ```

pub mod error;
pub mod question;
mod request;
mod response;

use std::fmt;
use std::time::Duration;

use ferrohealth_smart::client::outbound::OutboundClient;
use http::header::HeaderName;
use url::Url;
use uuid::Uuid;

use error::{InvalidInput, MitzError};
use ihe_iti::soap;
use question::{ClosedAnswer, ClosedQuestion};

/// The XACML 3.0 core namespace of the `Request` and the `Response`
/// (§3.2.4.4, §3.2.5.4).
const XACML: &str = "urn:oasis:names:tc:xacml:3.0:core:schema:wd-17";

/// The namespace of the SAML 2.0 profile of XACML 3.0 that carries the
/// `XACMLAuthzDecisionQuery` (§3.2.4.4).
const XACML_SAML_PROTOCOL: &str =
    "urn:oasis:names:tc:xacml:3.0:profile:saml2.0:v2:schema:protocol:wd-14";

/// The HL7 V3 namespace of the identifiers and coded values.
const HL7: &str = "urn:hl7-org:v3";

/// The WS-Addressing action of the question (§3.2.4.4).
pub const ACTION: &str = "XACMLAuthorizationDecisionQueryRequest";

/// The OID of the BSN, the root of the patient's identifier (§4): the dotted
/// OID of [`BSN_SYSTEMS`](crate::identification::BSN_SYSTEMS).
pub const BSN_ROOT: &str = crate::identification::BSN_OID;

/// The OID of the URA register, the root of an organisation's identifier
/// (§4).
pub const URA_ROOT: &str = "2.16.528.1.1007.3.3";

/// The OID of the UZI register, the root of a professional's UZI number
/// (§4).
pub const UZI_ROOT: &str = "2.16.528.1.1007.3.1";

/// The code system of the care provider categories (§4,
/// "Zorgaanbiedercategorie").
pub const CARE_PROVIDER_TYPE_SYSTEM: &str = "2.16.840.1.113883.2.4.15.1060";

/// The code system of the Mitz data categories (§4, "Mitz
/// gegevenscategorie").
pub const DATA_CATEGORY_SYSTEM: &str = "2.16.840.1.113883.2.4.3.111.5.10.1";

/// The code system of the UZI role codes (§4, "UZI rolcode").
pub const ROLE_SYSTEM: &str = "2.16.840.1.113883.2.4.15.111";

/// The code system of the purpose of use (§4).
pub const PURPOSE_SYSTEM: &str = "2.16.840.1.113883.1.11.20448";

/// The header that names each request (§6, required).
pub const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// The SOAP 1.2 media type of the request, with the action parameter of the
/// SOAP 1.2 HTTP binding (RFC 3902).
const SOAP_REQUEST: &str =
    "application/soap+xml; charset=UTF-8; action=\"XACMLAuthorizationDecisionQueryRequest\"";

/// The data user side of the closed authorization question, bound to one
/// Mitz endpoint.
///
/// `Debug` shows the endpoint and whether the client carries a credential,
/// never the credential itself.
#[derive(Clone)]
pub struct MitzClient {
    endpoint: Url,
    client: OutboundClient,
}

impl MitzClient {
    /// Creates a client for the Mitz endpoint `client`'s URL names, an
    /// `https` URL, reached through `client`.
    ///
    /// `client` carries the transport its endpoint table names: the mutual
    /// TLS §3.3 requires, the trust roots, any credential and the retry
    /// table. The request body holds the BSN, and `client` follows no
    /// redirect: a `3xx` is an answer like any other unexpected status.
    ///
    /// # Errors
    ///
    /// [`InvalidInput::Endpoint`] when the URL is not an `https` URL with no
    /// userinfo, query or fragment.
    pub fn new(client: OutboundClient) -> Result<Self, InvalidInput> {
        // NOTE: Implementatiehandleiding §3.3: both sides communicate over TLS under IHE
        // ATNA, so a plain http endpoint is refused outside the development path.
        if client.url().scheme() != "https" {
            return Err(InvalidInput::Endpoint);
        }
        Self::unencrypted_for_development(client)
    }

    /// Creates a client for the endpoint `client`'s URL names, `http` or
    /// `https`, for development and tests only.
    ///
    /// Over `http` the BSN crosses the network in clear text, which Mitz
    /// never permits (§3.3). A caller offers this only where its own
    /// configuration is marked for development.
    ///
    /// # Errors
    ///
    /// [`InvalidInput::Endpoint`] when the URL is not an `http` or `https`
    /// URL with no userinfo, query or fragment.
    pub fn unencrypted_for_development(client: OutboundClient) -> Result<Self, InvalidInput> {
        let endpoint = client.url().clone();
        let bare = endpoint.username().is_empty()
            && endpoint.password().is_none()
            && endpoint.query().is_none()
            && endpoint.fragment().is_none();
        if !matches!(endpoint.scheme(), "http" | "https") || endpoint.cannot_be_a_base() || !bare {
            return Err(InvalidInput::Endpoint);
        }
        Ok(Self { endpoint, client })
    }

    /// Returns the endpoint the client posts to.
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Asks Mitz `question`, and reads the decision for each data category it
    /// asks about.
    ///
    /// `timeout` bounds the whole exchange, from connecting until the answer
    /// is read.
    ///
    /// # Errors
    ///
    /// A [`MitzError`] for every answer that is not a `Permit` or a `Deny`
    /// for each category asked about, about the patient and the data holder
    /// asked about, and for a failure to get an answer at all.
    pub async fn ask(
        &self,
        question: &ClosedQuestion,
        timeout: Duration,
    ) -> Result<ClosedAnswer, MitzError> {
        let message = Uuid::new_v4();
        let body =
            request::envelope(question, &self.endpoint, message).map_err(MitzError::Encode)?;
        let id = message.to_string();
        let answer = soap::post::<MitzError>(
            &self.client,
            &self.endpoint,
            SOAP_REQUEST,
            &body,
            timeout,
            response::LIMIT,
            |request| request.header(REQUEST_ID, id.as_str()),
        )
        .await?;
        response::read(
            answer.status,
            answer.media.as_deref(),
            &answer.body,
            question,
        )
    }
}

impl fmt::Debug for MitzClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MitzClient")
            .field("endpoint", &self.endpoint.as_str())
            .field("authorization", &self.client.authorization().is_some())
            .finish_non_exhaustive()
    }
}
