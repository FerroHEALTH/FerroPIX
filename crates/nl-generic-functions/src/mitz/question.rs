// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The closed authorization question and the values it carries
//! (Implementatiehandleiding Open en gesloten autorisatievraag 3.8.2
//! §3.2.4.2, Programma van Eisen AMC AUS-TR-e0040).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use secrecy::{ExposeSecret, SecretString};

use super::error::InvalidInput;
use crate::identification::Ura;

/// The most data categories one question asks about (no specification
/// governs this: our own design; Mitz repeats only the data category in one
/// question, Programma van Eisen AMC AUS-TR-e0040).
pub const MAX_CATEGORIES: usize = 32;

/// The longest professional identification number Mitz takes
/// (Implementatiehandleiding §5, rule 3).
const PROFESSIONAL_NUMBER_LIMIT: usize = 60;

/// A patient's BSN, the patient identifier the question requires
/// (§3.2.4.2, "Verplicht, BSN").
///
/// It is a directly identifying identifier, so the value reaches text only
/// through a deliberate [`ExposeSecret::expose_secret`]; `Debug` is redacted
/// and there is no `Display`.
#[derive(Clone)]
pub struct Bsn(SecretString);

impl Bsn {
    /// Creates the BSN `value`.
    ///
    /// The register governs the value's form, so any non-empty value is
    /// taken as written.
    ///
    /// # Errors
    ///
    /// [`InvalidInput::EmptyBsn`] when `value` is empty.
    pub fn new(value: SecretString) -> Result<Self, InvalidInput> {
        if value.expose_secret().is_empty() {
            return Err(InvalidInput::EmptyBsn);
        }
        Ok(Self(value))
    }

    /// Returns the BSN.
    #[must_use]
    pub fn value(&self) -> &SecretString {
        &self.0
    }
}

impl fmt::Debug for Bsn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Bsn(***)")
    }
}

/// Defines a coded value: a non-empty code with no surrounding white space,
/// in the one code system the question fixes for it.
macro_rules! code {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Creates the code `code`.
            ///
            /// # Errors
            ///
            /// [`InvalidInput::Code`] when `code` is empty or has leading or
            /// trailing white space.
            pub fn new(code: impl Into<String>) -> Result<Self, InvalidInput> {
                let code = code.into();
                if code.is_empty() || code.trim() != code {
                    return Err(InvalidInput::Code);
                }
                Ok(Self(code))
            }

            /// Returns the code as written.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

code!(
    /// A care provider category ("zorgaanbiedercategorie"), a code of
    /// [`CARE_PROVIDER_TYPE_SYSTEM`](super::CARE_PROVIDER_TYPE_SYSTEM), such
    /// as `V6` (§3.2.4.2, §4).
    CareProviderType
);

code!(
    /// A Mitz data category ("gegevenscategorie"), a code of
    /// [`DATA_CATEGORY_SYSTEM`](super::DATA_CATEGORY_SYSTEM), such as
    /// `GGC002` (§3.2.4.2, §4).
    DataCategory
);

code!(
    /// The role of the responsible professional ("UZI rolcode"), a code of
    /// [`ROLE_SYSTEM`](super::ROLE_SYSTEM), such as `01.015` (§3.2.4.2, §4).
    RoleCode
);

/// A professional's identification number: an identifier root and an
/// alphanumeric extension of at most 60 characters (§5).
///
/// The root is the UZI register's [`UZI_ROOT`](super::UZI_ROOT), or, as §5
/// admits for now, the AGB or BIG register's OID or the OID of the
/// organisation the professional works for; the combination must lead back
/// to one natural person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfessionalId {
    root: String,
    extension: String,
}

impl ProfessionalId {
    /// Creates the identification number `extension` under the OID `root`.
    ///
    /// # Errors
    ///
    /// [`InvalidInput::Oid`] when `root` is not an ISO OID in dotted
    /// notation, and [`InvalidInput::ProfessionalNumber`] when `extension` is
    /// not 1 to 60 ASCII letters and digits.
    pub fn new(
        root: impl Into<String>,
        extension: impl Into<String>,
    ) -> Result<Self, InvalidInput> {
        let root = root.into();
        let extension = extension.into();
        if !is_oid(&root) {
            return Err(InvalidInput::Oid);
        }
        let alphanumeric = extension.chars().all(|c| c.is_ascii_alphanumeric());
        if extension.is_empty() || extension.len() > PROFESSIONAL_NUMBER_LIMIT || !alphanumeric {
            return Err(InvalidInput::ProfessionalNumber);
        }
        Ok(Self { root, extension })
    }

    /// Returns the identifier root.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Returns the identification number.
    #[must_use]
    pub fn extension(&self) -> &str {
        &self.extension
    }
}

/// Returns whether `text` is an ISO OID in dotted notation: two or more
/// arcs of decimal digits without leading zeros, the first `0`, `1` or `2`.
fn is_oid(text: &str) -> bool {
    let mut arcs = text.split('.');
    let first = arcs.next();
    let rest: Vec<&str> = arcs.collect();
    let arc = |arc: &str| {
        !arc.is_empty()
            && arc.bytes().all(|byte| byte.is_ascii_digit())
            && (arc == "0" || !arc.starts_with('0'))
    };
    matches!(first, Some("0" | "1" | "2")) && !rest.is_empty() && rest.into_iter().all(arc)
}

/// The situation the data is consulted in ("raadpleegsituatie"), a purpose
/// of use of [`PURPOSE_SYSTEM`](super::PURPOSE_SYSTEM) (§3.2.4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Purpose {
    /// `TREAT`: normal care, explicit consent required.
    Treatment,
    /// `COC`: normal care, consent presumed.
    ContinuityOfCare,
}

impl Purpose {
    /// Returns the purpose a code names, of the two the question admits.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "TREAT" => Some(Self::Treatment),
            "COC" => Some(Self::ContinuityOfCare),
            _ => None,
        }
    }

    /// Returns the purpose's code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Treatment => "TREAT",
            Self::ContinuityOfCare => "COC",
        }
    }
}

/// The organisation that holds the data ("dossierhouder"): its URA and its
/// care provider category.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DataHolder {
    organisation: Ura,
    kind: CareProviderType,
}

impl DataHolder {
    /// Creates the data holder `organisation` of category `kind`.
    #[must_use]
    pub fn new(organisation: Ura, kind: CareProviderType) -> Self {
        Self { organisation, kind }
    }

    /// Returns the organisation's URA.
    #[must_use]
    pub fn organisation(&self) -> &Ura {
        &self.organisation
    }

    /// Returns the organisation's care provider category.
    #[must_use]
    pub fn kind(&self) -> &CareProviderType {
        &self.kind
    }
}

/// The organisation that consults the data ("raadpleger").
///
/// It is named by its URA and its care provider category, with the
/// responsible professional and that professional's role, and, when someone
/// else consults under the responsible professional's mandate, that person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataUser {
    organisation: Ura,
    kind: CareProviderType,
    responsible: ProfessionalId,
    role: RoleCode,
    mandated: Option<ProfessionalId>,
}

impl DataUser {
    /// Creates the data user `organisation` of category `kind`, answering
    /// through the professional `responsible` in role `role`.
    #[must_use]
    pub fn new(
        organisation: Ura,
        kind: CareProviderType,
        responsible: ProfessionalId,
        role: RoleCode,
    ) -> Self {
        Self {
            organisation,
            kind,
            responsible,
            role,
            mandated: None,
        }
    }

    /// This data user, consulting through `mandated` under the responsible
    /// professional's mandate (§3.2.4.2, `subject:mandated`).
    #[must_use]
    pub fn mandating(mut self, mandated: ProfessionalId) -> Self {
        self.mandated = Some(mandated);
        self
    }

    /// Returns the organisation's URA.
    #[must_use]
    pub fn organisation(&self) -> &Ura {
        &self.organisation
    }

    /// Returns the organisation's care provider category.
    #[must_use]
    pub fn kind(&self) -> &CareProviderType {
        &self.kind
    }

    /// Returns the responsible professional.
    #[must_use]
    pub fn responsible(&self) -> &ProfessionalId {
        &self.responsible
    }

    /// Returns the responsible professional's role.
    #[must_use]
    pub fn role(&self) -> &RoleCode {
        &self.role
    }

    /// Returns the person consulting under mandate, if any.
    #[must_use]
    pub fn mandated(&self) -> Option<&ProfessionalId> {
        self.mandated.as_ref()
    }
}

/// One closed authorization question: may `holder` make the patient's data
/// of each category available to `user` for `purpose`?
#[derive(Debug, Clone)]
pub struct ClosedQuestion {
    patient: Bsn,
    holder: DataHolder,
    user: DataUser,
    categories: Vec<DataCategory>,
    purpose: Purpose,
}

impl ClosedQuestion {
    /// Creates the question about `patient`'s data of each of `categories`
    /// at `holder`, consulted by `user` for `purpose`.
    ///
    /// # Errors
    ///
    /// [`InvalidInput::NoCategory`] for no category,
    /// [`InvalidInput::DuplicateCategory`] for a category named twice, and
    /// [`InvalidInput::TooManyCategories`] for more than [`MAX_CATEGORIES`].
    pub fn new(
        patient: Bsn,
        holder: DataHolder,
        user: DataUser,
        categories: Vec<DataCategory>,
        purpose: Purpose,
    ) -> Result<Self, InvalidInput> {
        if categories.is_empty() {
            return Err(InvalidInput::NoCategory);
        }
        if categories.len() > MAX_CATEGORIES {
            return Err(InvalidInput::TooManyCategories {
                limit: MAX_CATEGORIES,
            });
        }
        let mut seen = BTreeSet::new();
        for category in &categories {
            if !seen.insert(category) {
                return Err(InvalidInput::DuplicateCategory(category.clone()));
            }
        }
        Ok(Self {
            patient,
            holder,
            user,
            categories,
            purpose,
        })
    }

    /// Returns the patient.
    #[must_use]
    pub fn patient(&self) -> &Bsn {
        &self.patient
    }

    /// Returns the data holder.
    #[must_use]
    pub fn holder(&self) -> &DataHolder {
        &self.holder
    }

    /// Returns the data user.
    #[must_use]
    pub fn user(&self) -> &DataUser {
        &self.user
    }

    /// Returns the data categories, in the order asked.
    #[must_use]
    pub fn categories(&self) -> &[DataCategory] {
        &self.categories
    }

    /// Returns the purpose.
    #[must_use]
    pub fn purpose(&self) -> Purpose {
        self.purpose
    }
}

/// Mitz's decision for one data category (§3.2.4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// `Permit`: the holder may make the data available to the user.
    Permit,
    /// `Deny`: the exchange may not go ahead, with or without a reason.
    Deny,
}

/// Mitz's answer to a closed authorization question: one decision per data
/// category asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedAnswer {
    decisions: BTreeMap<DataCategory, Decision>,
}

impl ClosedAnswer {
    /// The answer of `decisions`, one per category asked about.
    pub(super) fn new(decisions: BTreeMap<DataCategory, Decision>) -> Self {
        Self { decisions }
    }

    /// Returns the decision for `category`, if it was asked about.
    #[must_use]
    pub fn decision(&self, category: &DataCategory) -> Option<Decision> {
        self.decisions.get(category).copied()
    }

    /// Returns every decision, by category.
    #[must_use]
    pub fn decisions(&self) -> &BTreeMap<DataCategory, Decision> {
        &self.decisions
    }

    /// Returns whether Mitz denied every category asked about.
    #[must_use]
    pub fn denies_all(&self) -> bool {
        self.decisions
            .values()
            .all(|decision| *decision == Decision::Deny)
    }
}

#[cfg(test)]
mod tests {
    use super::is_oid;

    #[test]
    fn an_oid_is_two_or_more_dotted_decimal_arcs_under_a_root_arc() {
        for oid in ["2.16.528.1.1007.3.1", "1.2", "0.0", "2.999.40.1"] {
            assert!(is_oid(oid), "{oid}");
        }
        for not in ["", "2", "3.1", "2..1", "2.01", "2.1.", "a.1", "2.1 "] {
            assert!(!is_oid(not), "{not}");
        }
    }
}
