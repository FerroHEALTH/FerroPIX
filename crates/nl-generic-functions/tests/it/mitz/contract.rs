// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What the client refuses before anything is sent: an endpoint Mitz would
//! not be reached at over TLS (§3.3), and a question the interface does not
//! define (§3.2.4.2, §5).

use nl_generic_functions::identification::Ura;
use nl_generic_functions::mitz::MitzClient;
use nl_generic_functions::mitz::error::{ClientError, InvalidInput};
use nl_generic_functions::mitz::question::{
    Bsn, CareProviderType, ClosedQuestion, DataCategory, DataHolder, DataUser, MAX_CATEGORIES,
    ProfessionalId, Purpose, RoleCode,
};
use secrecy::SecretString;
use url::Url;

use super::{HOLDER, PATIENT, USER};

fn refused(endpoint: &str) -> bool {
    let url = Url::parse(endpoint).expect("a URL");
    matches!(
        MitzClient::new(url, reqwest::Client::builder()),
        Err(ClientError::Endpoint(InvalidInput::Endpoint))
    )
}

#[test]
fn an_endpoint_without_tls_is_refused_outside_development() {
    assert!(refused("http://mitz.example.org/vraag"));
    let url = Url::parse("http://mitz.example.org/vraag").expect("a URL");
    assert!(MitzClient::unencrypted_for_development(url, reqwest::Client::builder()).is_ok());
}

#[test]
fn an_endpoint_with_userinfo_a_query_or_a_fragment_is_refused() {
    assert!(refused("https://user:Qz7secret@mitz.example.org/vraag"));
    assert!(refused("https://mitz.example.org/vraag?x=1"));
    assert!(refused("https://mitz.example.org/vraag#x"));
    assert!(refused("mailto:mitz@example.org"));
    assert!(!refused("https://mitz.example.org/vraag"));
}

#[test]
fn an_empty_bsn_is_refused() {
    assert_eq!(
        Some(InvalidInput::EmptyBsn),
        Bsn::new(SecretString::from("")).err()
    );
}

#[test]
fn a_code_must_be_non_empty_and_trimmed() {
    assert_eq!(Some(InvalidInput::Code), CareProviderType::new("").err());
    assert_eq!(Some(InvalidInput::Code), DataCategory::new(" GGC002").err());
    assert_eq!(Some(InvalidInput::Code), RoleCode::new("01.015 ").err());
    assert_eq!(
        "GGC002",
        DataCategory::new("GGC002").expect("a code").as_str()
    );
}

#[test]
fn a_professional_number_is_one_to_sixty_alphanumerics_under_an_oid() {
    assert_eq!(
        Some(InvalidInput::Oid),
        ProfessionalId::new("UZI", "professional0001").err()
    );
    for number in ["", "with-hyphen", "with space", &"9".repeat(61)] {
        assert_eq!(
            Some(InvalidInput::ProfessionalNumber),
            ProfessionalId::new("2.16.528.1.1007.3.1", number).err(),
            "{number:?}"
        );
    }
    assert!(ProfessionalId::new("2.16.528.1.1007.3.1", "9".repeat(60)).is_ok());
}

#[test]
fn only_treat_and_coc_are_purposes() {
    assert_eq!(Some(Purpose::Treatment), Purpose::from_code("TREAT"));
    assert_eq!(Some(Purpose::ContinuityOfCare), Purpose::from_code("COC"));
    assert_eq!(None, Purpose::from_code("ETREAT"));
    assert_eq!("COC", Purpose::ContinuityOfCare.code());
}

fn asking(categories: Vec<DataCategory>) -> Result<ClosedQuestion, InvalidInput> {
    let kind = CareProviderType::new("V6").expect("a category");
    let user = DataUser::new(
        Ura::new(USER).expect("a URA"),
        kind.clone(),
        ProfessionalId::new("2.999.10", "professional0001").expect("a professional"),
        RoleCode::new("01.015").expect("a role"),
    );
    ClosedQuestion::new(
        Bsn::new(SecretString::from(PATIENT)).expect("a BSN"),
        DataHolder::new(Ura::new(HOLDER).expect("a URA"), kind),
        user,
        categories,
        Purpose::Treatment,
    )
}

#[test]
fn a_question_asks_about_one_or_more_distinct_categories() {
    assert_eq!(Some(InvalidInput::NoCategory), asking(Vec::new()).err());
    let twice = vec![
        DataCategory::new("GGC002").expect("a code"),
        DataCategory::new("GGC002").expect("a code"),
    ];
    assert!(matches!(
        asking(twice),
        Err(InvalidInput::DuplicateCategory(_))
    ));
    let many = (0..=MAX_CATEGORIES)
        .map(|index| DataCategory::new(format!("GGC{index:03}")).expect("a code"))
        .collect();
    assert_eq!(
        Some(InvalidInput::TooManyCategories {
            limit: MAX_CATEGORIES
        }),
        asking(many).err()
    );
}
