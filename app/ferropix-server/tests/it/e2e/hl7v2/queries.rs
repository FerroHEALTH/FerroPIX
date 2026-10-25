// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The queries of the HL7 v2 face: ITI-9 beside ITI-83, ITI-21 beside
//! ITI-78 with its continuation, and ITI-22 refused.
use std::collections::BTreeSet;
use std::time::Duration;

use ferropix_server::hl7v2::cursor::Cursors;
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN, LABORATORY_DOMAIN};
use http::{Method, StatusCode};

use super::{
    V2, adt, bsn_cx, code, errors, linked, pdq_query, pid_identifiers, pix_query, pointer,
    segments, status,
};

#[tokio::test]
async fn a_pix_query_answers_what_iti_83_answers() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    linked(&v2).await;
    for (qpd4, targets) in [("", vec![]), ("^^^LAB", vec![LABORATORY_DOMAIN])] {
        let answer = v2.send(&pix_query("P-1", "H-1^^^HOSP", qpd4)).await;
        assert_eq!(code(&answer), "AA", "{answer}");
        assert_eq!(status(&answer), "OK");
        let iti_83 = v2.pix.pix_get(HOSPITAL_DOMAIN, "H-1", &targets).await;
        assert_eq!(iti_83.status, StatusCode::OK, "{}", iti_83.body);
        assert_eq!(
            pid_identifiers(&answer)
                .first()
                .cloned()
                .unwrap_or_default(),
            iti_83.identifiers(),
            "QPD-4 {qpd4:?}"
        );
        assert!(
            answer.contains("||~^^^^^^S"),
            "PID-5 is the pseudo-name: {answer}"
        );
    }
    let bsn_query = v2.send(&pix_query("P-2", &bsn_cx(0), "^^^HOSP")).await;
    assert_eq!(
        pid_identifiers(&bsn_query)
            .first()
            .cloned()
            .unwrap_or_default(),
        BTreeSet::from([format!("{HOSPITAL_DOMAIN}|H-1")])
    );
    let record = v2
        .records()
        .into_iter()
        .find(|record| record["subtype"][0]["code"] == "ITI-9")
        .expect("the ITI-9 record");
    assert_eq!(record["type"]["code"], "110112");
    assert!(
        record["entity"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|entity| entity["role"]["code"] == "24" && entity["query"].is_string()),
        "the record carries the query"
    );
}

#[tokio::test]
async fn an_unknown_identifier_domain_or_target_domain_answers_the_case_the_text_names() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    linked(&v2).await;
    let unknown = v2.send(&pix_query("U-1", "H-404^^^HOSP", "")).await;
    assert_eq!(
        (code(&unknown).as_str(), status(&unknown).as_str()),
        ("AE", "AE")
    );
    assert_eq!(
        errors(&unknown),
        vec![(String::from("QPD^1^3^1^1"), String::from("204"))]
    );
    let domain = v2.send(&pix_query("U-2", "H-1^^^NOWHERE", "")).await;
    assert_eq!(
        errors(&domain),
        vec![(String::from("QPD^1^3^1^4"), String::from("204"))]
    );
    let target = v2
        .send(&pix_query("U-3", "H-1^^^HOSP", "^^^LAB~^^^NOWHERE"))
        .await;
    assert_eq!(
        errors(&target),
        vec![(String::from("QPD^1^4^2"), String::from("204"))]
    );
    let nothing = v2.send(&pix_query("U-4", "H-1^^^HOSP", "^^^GP")).await;
    assert_eq!(
        (code(&nothing).as_str(), status(&nothing).as_str()),
        ("AA", "NF")
    );
    assert_eq!(pid_identifiers(&nothing), Vec::<BTreeSet<String>>::new());
}

/// The persons ITI-78 finds for `query`, by the hospital identifiers of
/// their Patients.
async fn iti_78(v2: &V2, query: &str) -> BTreeSet<String> {
    let answer = v2
        .pix
        .send(
            Method::GET,
            &format!("/fhir/Patient?{query}"),
            Some(&v2.pix.consumer()),
            &[],
            None,
        )
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    answer.json()["entry"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["search"]["mode"] == "match")
        .flat_map(|entry| {
            entry["resource"]["identifier"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|identifier| identifier["system"] == HOSPITAL_DOMAIN)
        .filter_map(|identifier| identifier["value"].as_str().map(str::to_owned))
        .collect()
}

/// The hospital identifiers of the PIDs of `answer`.
fn hospital_ids(answer: &str) -> BTreeSet<String> {
    pid_identifiers(answer)
        .into_iter()
        .flatten()
        .filter_map(|identifier| {
            identifier
                .strip_prefix(&format!("{HOSPITAL_DOMAIN}|"))
                .map(str::to_owned)
        })
        .collect()
}

#[tokio::test]
async fn a_demographics_query_finds_the_persons_iti_78_finds() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    for (index, (name, born)) in [(0, "19800229"), (0, "19700101"), (1, "19800229")]
        .iter()
        .enumerate()
    {
        let answer = v2
            .send(&adt(
                "A01",
                &format!("D-{index}"),
                &format!("H-{index}^^^HOSP"),
                *name,
                born,
            ))
            .await;
        assert_eq!(code(&answer), "AA", "{answer}");
    }
    let family = FAMILY_NAMES[0];
    for (qpd3, query) in [
        (format!("@PID.5.1.1^{family}"), format!("family={family}")),
        (
            format!("@PID.5.1.1^{family}~@PID.7^19800229"),
            format!("family={family}&birthdate=1980-02-29"),
        ),
        (
            String::from("@PID.3.1^H-2~@PID.3.4.1^HOSP"),
            super::super::feeds::condition(HOSPITAL_DOMAIN, "H-2"),
        ),
    ] {
        let answer = v2.send(&pdq_query("Q-1", "T-1", &qpd3, "", "", None)).await;
        assert_eq!(code(&answer), "AA", "{answer}");
        assert_eq!(hospital_ids(&answer), iti_78(&v2, &query).await, "{qpd3}");
        assert!(!hospital_ids(&answer).is_empty(), "{qpd3}");
    }
    let filtered = v2
        .send(&pdq_query(
            "Q-2",
            "T-2",
            &format!("@PID.5.1.1^{family}"),
            "^^^GP",
            "",
            None,
        ))
        .await;
    assert_eq!(
        (code(&filtered).as_str(), status(&filtered).as_str()),
        ("AA", "NF")
    );
    let unknown = v2
        .send(&pdq_query(
            "Q-3",
            "T-3",
            &format!("@PID.5.1.1^{family}"),
            "^^^HOSP~^^^NOWHERE",
            "",
            None,
        ))
        .await;
    assert_eq!(
        errors(&unknown),
        vec![(String::from("QPD^1^8^2"), String::from("204"))]
    );
    assert!(
        v2.records()
            .iter()
            .any(|record| record["subtype"][0]["code"] == "ITI-21"),
        "the record of each query"
    );
}

#[tokio::test]
async fn a_query_continues_over_three_increments_and_a_pointer_past_its_bound_is_refused() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    for index in 0..5 {
        let born = format!("1970010{}", index + 1);
        let answer = v2
            .send(
                &adt(
                    "A01",
                    &format!("C-{index}"),
                    &format!("H-{index}^^^HOSP"),
                    0,
                    &born,
                )
                .replace(
                    &format!("^{}^^^^^L", GIVEN_NAMES[0]),
                    &format!("^{}^^^^^L", GIVEN_NAMES[index % GIVEN_NAMES.len()]),
                ),
            )
            .await;
        assert_eq!(code(&answer), "AA", "{answer}");
    }
    let qpd3 = format!("@PID.5.1.1^{}", FAMILY_NAMES[0]);
    let mut seen = BTreeSet::new();
    let mut next: Option<String> = None;
    let mut increments = Vec::new();
    for page in 0..3 {
        let answer = v2
            .send(&pdq_query(
                &format!("N-{page}"),
                "T-N",
                &qpd3,
                "",
                "2^RD",
                next.as_deref(),
            ))
            .await;
        assert_eq!(code(&answer), "AA", "{answer}");
        let ids = hospital_ids(&answer);
        increments.push(ids.len());
        seen.extend(ids);
        next = pointer(&answer);
    }
    assert_eq!(increments, vec![2, 2, 1]);
    assert_eq!(seen.len(), 5);
    assert_eq!(next, None, "the last increment names no pointer");

    let short = V2::start(Cursors::with_lifetime(Duration::ZERO)).await;
    for index in 0..3 {
        let answer = short
            .send(
                &adt(
                    "A01",
                    &format!("S-{index}"),
                    &format!("H-{index}^^^HOSP"),
                    0,
                    "19700101",
                )
                .replace(
                    &format!("^{}^^^^^L", GIVEN_NAMES[0]),
                    &format!("^{}^^^^^L", GIVEN_NAMES[index % GIVEN_NAMES.len()]),
                ),
            )
            .await;
        assert_eq!(code(&answer), "AA", "{answer}");
    }
    let first = short
        .send(&pdq_query("O-1", "T-O", &qpd3, "", "1^RD", None))
        .await;
    let stale = pointer(&first).expect("a pointer");
    for control in ["O-2", "O-3"] {
        let answer = short
            .send(&pdq_query(control, "T-O", &qpd3, "", "1^RD", Some(&stale)))
            .await;
        assert_eq!(
            (code(&answer).as_str(), status(&answer).as_str()),
            ("AE", "AE")
        );
        assert_eq!(
            errors(&answer),
            vec![(String::from("DSC^1^1"), String::from("204"))]
        );
    }
}

// NOTE: ITI TF-2 §3.22.4.2.2.7 (a PV1 per patient) and Table 3.22-4 (a visit field searched):
// FerroPIX keeps no visit data, so ITI-22 is refused and ITI-21 is the demographics query.
#[tokio::test]
async fn a_demographics_and_visit_query_is_refused_as_unsupported() {
    if !e2e_enabled() {
        return;
    }
    let v2 = V2::start(Cursors::default()).await;
    let before = v2.records().len();
    for (control, qpd3) in [
        ("Z-1", "@PID.5.1.1^Proefsma"),
        ("Z-2", "@PV1.3.2^389~@PV1.3.3^2"),
    ] {
        let query = pdq_query(control, &format!("T-{control}"), qpd3, "", "", None)
            .replace("QBP^Q22^QBP_Q21", "QBP^ZV1^QBP_Q21");
        let answer = v2.send(&query).await;
        assert_eq!(code(&answer), "AR", "{answer}");
        assert_eq!(errors(&answer), vec![(String::new(), String::from("200"))]);
        assert!(
            answer.contains("ITI-21"),
            "the refusal names the query to use: {answer}"
        );
        assert!(
            !segments(&answer)
                .iter()
                .any(|segment| segment.starts_with("PID|")),
            "no patient is answered: {answer}"
        );
    }
    assert_eq!(v2.records().len(), before, "a refused query is no query");
}
