// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Whom an audited exchange is made for: a user the client authenticated by
//! an OAuth 2.0 access token, or the client's own system with no user.
//!
//! An IHE transaction's audit record names the user who asked for it when one
//! is known: the BALP patterns have an `agent:user` slice, `0..1` (BALP 1.1.4
//! §3:5.7.3), which PIXm §2:3.83.5.2.1 asks to augment "to include agent
//! details from the OAuth Security token" as BALP §3:5.7.5.4 maps them, and the
//! ITI-55 audit message has a Human Requestor participant "if known" (ITI TF-2
//! §3.55.5.1.1). An exchange the system makes on its own behalf names no user.
//!
//! A [`User`] names a person or an application: `Debug` shows none of it, and a
//! client of this crate writes it into the audit record alone.

use std::fmt;

use crate::redact::REDACTED;

/// Whom an exchange is made for.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OnBehalfOf {
    /// A user the client authenticated by an OAuth 2.0 access token, who
    /// asked for the exchange: the record names the user and the client
    /// application the token was issued to.
    User(User),
    /// The client's own system, with no user: the record names none, as the
    /// BALP examples of an event no user caused do (`ex-auditBasicReadNoUser`
    /// and the others named `NoUser`).
    System,
}

impl OnBehalfOf {
    /// Returns the user, when the exchange is made for one.
    #[must_use]
    pub fn user(&self) -> Option<&User> {
        match self {
            Self::User(user) => Some(user),
            Self::System => None,
        }
    }
}

/// A user as the verified access token names them (BALP 1.1.4 §3:5.7.5.4).
///
/// `Debug` shows that a user is named and none of the values.
#[derive(Clone, PartialEq, Eq)]
pub struct User {
    issuer: String,
    subject: String,
    client_id: String,
    audience: Option<String>,
    purposes: Vec<PurposeOfUse>,
}

impl User {
    /// Returns the user `subject` (the token's `sub`) at the authorization
    /// server `issuer` (`iss`), who asked through the client application
    /// `client_id` (`client_id`).
    #[must_use]
    pub fn new(issuer: String, subject: String, client_id: String) -> Self {
        Self {
            issuer,
            subject,
            client_id,
            audience: None,
            purposes: Vec::new(),
        }
    }

    /// Returns this user, the token they presented naming `audience` in its
    /// `aud`: the alias an ATNA `UserName` begins with (IUA ITI TF-2
    /// §3.72.5.1).
    #[must_use]
    pub fn with_audience(mut self, audience: Option<String>) -> Self {
        self.audience = audience;
        self
    }

    /// Returns this user, asking for `purposes` (the IHE IUA
    /// `purpose_of_use`).
    #[must_use]
    pub fn with_purposes(mut self, purposes: Vec<PurposeOfUse>) -> Self {
        self.purposes = purposes;
        self
    }

    /// Returns the authorization server that issued the token (`iss`).
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Returns the user (`sub`).
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// Returns the client application the token was issued to
    /// (`client_id`).
    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Returns the audience the token names, when the client said which.
    #[must_use]
    pub fn audience(&self) -> Option<&str> {
        self.audience.as_deref()
    }

    /// Returns every purpose of use the token declares.
    #[must_use]
    pub fn purposes(&self) -> &[PurposeOfUse] {
        &self.purposes
    }

    /// The ATNA `UserName` of a user a JWT authenticated:
    /// `alias"<"user"@"issuer">"`, with the token's `aud` as the alias, its
    /// `sub` as the user and its `iss` as the issuer (IUA ITI TF-2
    /// §3.72.5.1); the alias is empty when the audience is not known.
    #[must_use]
    pub fn atna_user_name(&self) -> String {
        format!(
            "{}<{}@{}>",
            self.audience.as_deref().unwrap_or_default(),
            self.subject,
            self.issuer
        )
    }
}

impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("User").field(&REDACTED).finish()
    }
}

/// One purpose of use: a code and the system that defines it, the HL7 v3
/// `PurposeOfUse` coding of the IHE IUA `purpose_of_use` claim.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PurposeOfUse {
    /// The code system, when the token names one.
    pub system: Option<String>,
    /// The code.
    pub code: String,
}
