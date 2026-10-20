// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The token endpoint of the harness Nuts node: the `vp_token-bearer` grant
//! (Nuts RFC021 §3), its `DPoP` proof (RFC 9449), the presentation (RFC021
//! §4.2) and every credential the submission maps. No specification governs
//! the device: our own design.

use std::sync::Arc;

use jsonwebtoken::{DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use wiremock::{Request, Respond, ResponseTemplate};

use crate::testkit::dpop;
use crate::testkit::nuts::{
    GRANT_TYPE, PRESENTATION_LIFETIME_S, SKEW_S, Shared, State, Trusted, Verdict, refused,
};

/// The members of a Presentation Definition the device reads.
#[derive(Deserialize)]
struct Definition {
    id: String,
    input_descriptors: Vec<Descriptor>,
}

#[derive(Deserialize)]
struct Descriptor {
    id: String,
}

/// The members of a Presentation Submission the device reads.
#[derive(Deserialize)]
struct Submission {
    definition_id: String,
    descriptor_map: Vec<Mapping>,
}

#[derive(Deserialize)]
struct Mapping {
    id: String,
    format: String,
    path: String,
}

/// The claims of a presentation the device reads.
#[derive(Deserialize)]
struct PresentationClaims {
    iss: String,
    sub: String,
    aud: String,
    nbf: i64,
    exp: i64,
    nonce: Option<String>,
    vp: Presentation,
}

#[derive(Deserialize)]
struct Presentation {
    #[serde(rename = "type")]
    types: Vec<String>,
    #[serde(rename = "verifiableCredential")]
    credentials: Vec<String>,
}

/// The claims of a credential the device reads.
#[derive(Deserialize)]
struct HeldClaims {
    sub: String,
}

#[derive(Serialize)]
struct TokenBody<'a> {
    access_token: &'a str,
    token_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_in: Option<u64>,
    scope: &'a str,
}

/// The answer of the token endpoint.
pub(super) struct Tokens(pub(super) Arc<Shared>);

impl Respond for Tokens {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let form: Vec<(String, String)> = url::form_urlencoded::parse(&request.body)
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        let proof = request
            .headers
            .get(dpop::HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let mut state = self.0.lock();
        state.forms.push(form.clone());
        let delay = state.delay;
        let answer = self.answer(&mut state, &form, proof.as_deref());
        match delay {
            Some(delay) => answer.set_delay(delay),
            None => answer,
        }
    }
}

impl Tokens {
    fn answer(
        &self,
        state: &mut State,
        form: &[(String, String)],
        proof: Option<&str>,
    ) -> ResponseTemplate {
        if let Some(refusal) = state.refusal.clone() {
            state.verdicts.push(Verdict::Refused(refusal.error.clone()));
            return refused(refusal.status, &refusal.error, &refusal.description);
        }
        let jkt = match self.proven(state, proof) {
            Ok(jkt) => jkt,
            Err(answer) => return *answer,
        };
        if let Err(reason) = self.granted(state, form) {
            state.verdicts.push(Verdict::Refused(reason.clone()));
            return refused(400, "invalid_request", &reason);
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        state.accepted.insert(token.clone());
        state.bound.insert(token.clone(), jkt);
        state.verdicts.push(Verdict::Issued);
        ResponseTemplate::new(200).set_body_json(TokenBody {
            access_token: &token,
            token_type: if state.bearer { "Bearer" } else { "DPoP" },
            expires_in: state.expires_in,
            scope: &self.0.scope,
        })
    }

    /// The thumbprint of the proof's key, or the answer to a request whose
    /// proof is refused.
    fn proven(
        &self,
        state: &mut State,
        proof: Option<&str>,
    ) -> Result<String, Box<ResponseTemplate>> {
        let Some(proof) = proof else {
            state
                .verdicts
                .push(Verdict::Refused("no DPoP proof".to_owned()));
            return Err(Box::new(refused(
                400,
                "invalid_dpop_proof",
                "no DPoP proof",
            )));
        };
        let verified = match dpop::verify(proof, "POST", &self.0.token_url, None) {
            Ok(verified) => verified,
            Err(reason) => {
                state.verdicts.push(Verdict::Refused(reason.clone()));
                return Err(Box::new(refused(400, "invalid_dpop_proof", &reason)));
            }
        };
        if let Some(nonce) = state.nonce.clone()
            && verified.proof.nonce.as_ref() != Some(&nonce)
        {
            state
                .verdicts
                .push(Verdict::Refused("use_dpop_nonce".to_owned()));
            return Err(Box::new(
                refused(400, "use_dpop_nonce", "a nonce is required")
                    .insert_header(dpop::NONCE_HEADER, nonce.as_str()),
            ));
        }
        if !state.seen.insert(format!("dpop:{}", verified.proof.jti)) {
            state.verdicts.push(Verdict::Refused(
                "the proof's jti was used before".to_owned(),
            ));
            return Err(Box::new(refused(400, "invalid_dpop_proof", "jti")));
        }
        Ok(verified.jkt)
    }

    /// Checks the grant `form` carries, or says why it is refused.
    fn granted(&self, state: &mut State, form: &[(String, String)]) -> Result<(), String> {
        let field = |name: &str| {
            form.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        if field("grant_type").as_deref() != Some(GRANT_TYPE) {
            return Err("grant_type".to_owned());
        }
        if field("scope").as_deref() != Some(self.0.scope.as_str()) {
            return Err("scope".to_owned());
        }
        if let Some(expected) = &state.client_id
            && field("client_id").as_deref() != Some(expected.as_str())
        {
            return Err("client_id".to_owned());
        }
        let definition: Definition = state
            .definition
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok())
            .ok_or("no definition")?;
        let submission: Submission = field("presentation_submission")
            .and_then(|text| serde_json::from_str(&text).ok())
            .ok_or("presentation_submission")?;
        if submission.definition_id != definition.id {
            return Err("the submission answers another definition".to_owned());
        }
        let assertion = field("assertion").ok_or("no assertion")?;
        let holder = state.holder.clone().ok_or("no trusted holder")?;
        let claims = presentation(&assertion, &holder, &self.0.issuer)?;
        let nonce = claims
            .nonce
            .clone()
            .ok_or("the presentation has no nonce")?;
        if !state.seen.insert(format!("vp:{nonce}")) {
            return Err("the presentation nonce was used before".to_owned());
        }
        for descriptor in &definition.input_descriptors {
            if !submission
                .descriptor_map
                .iter()
                .any(|m| m.id == descriptor.id)
            {
                return Err(format!("descriptor {} is not answered", descriptor.id));
            }
        }
        let issuer = state.issuer.clone().ok_or("no trusted credential issuer")?;
        for mapping in &submission.descriptor_map {
            if mapping.format != "jwt_vc" {
                return Err(format!("format {}", mapping.format));
            }
            let index: usize = mapping
                .path
                .strip_prefix("$.verifiableCredential[")
                .and_then(|rest| rest.strip_suffix(']'))
                .and_then(|index| index.parse().ok())
                .ok_or_else(|| format!("path {}", mapping.path))?;
            let held = claims
                .vp
                .credentials
                .get(index)
                .ok_or_else(|| format!("no credential at {}", mapping.path))?;
            credential_of(held, &issuer, &holder.did)?;
        }
        Ok(())
    }
}

/// The verified claims of `assertion`, a presentation `holder` signed for
/// `audience` (Nuts RFC021 §4.2).
fn presentation(
    assertion: &str,
    holder: &Trusted,
    audience: &str,
) -> Result<PresentationClaims, String> {
    let header = jsonwebtoken::decode_header(assertion).map_err(|e| format!("header: {e}"))?;
    if header.kid.as_deref() != Some(holder.kid.as_str()) {
        return Err("the presentation names another key".to_owned());
    }
    let key = DecodingKey::from_jwk(&holder.jwk).map_err(|e| format!("jwk: {e}"))?;
    let mut validation = Validation::new(header.alg);
    validation.validate_exp = false;
    validation.validate_aud = false;
    validation.set_required_spec_claims::<&str>(&[]);
    let claims = jsonwebtoken::decode::<PresentationClaims>(assertion, &key, &validation)
        .map_err(|e| format!("verification: {e}"))?
        .claims;
    if claims.iss != holder.did || claims.sub != holder.did {
        return Err("iss and sub are not the holder".to_owned());
    }
    if claims.aud != audience {
        return Err("aud is not the issuer".to_owned());
    }
    let now = jiff::Timestamp::now().as_second();
    if claims.exp.saturating_sub(claims.nbf) > PRESENTATION_LIFETIME_S
        || claims.nbf > now.saturating_add(SKEW_S)
        || claims.exp < now.saturating_sub(SKEW_S)
    {
        return Err("the presentation is not valid now".to_owned());
    }
    if !claims
        .vp
        .types
        .iter()
        .any(|kind| kind == "VerifiablePresentation")
    {
        return Err("not a VerifiablePresentation".to_owned());
    }
    Ok(claims)
}

/// Verifies `held` as a credential `issuer` signed for `holder`.
fn credential_of(held: &str, issuer: &Trusted, holder: &str) -> Result<(), String> {
    let header =
        jsonwebtoken::decode_header(held).map_err(|e| format!("credential header: {e}"))?;
    if header.kid.as_deref() != Some(issuer.kid.as_str()) {
        return Err("the credential names another issuer key".to_owned());
    }
    let key = DecodingKey::from_jwk(&issuer.jwk).map_err(|e| format!("jwk: {e}"))?;
    let mut validation = Validation::new(header.alg);
    validation.validate_aud = false;
    validation.set_issuer(&[issuer.did.as_str()]);
    validation.set_required_spec_claims(&["exp", "iss", "sub"]);
    let claims = jsonwebtoken::decode::<HeldClaims>(held, &key, &validation)
        .map_err(|e| format!("credential: {e}"))?
        .claims;
    if claims.sub != holder {
        return Err("the credential is not the holder's".to_owned());
    }
    Ok(())
}
