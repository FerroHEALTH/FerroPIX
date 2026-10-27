// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A steward resolves a pseudonym on the pseudonymisation domains page and
//! finds the disclosure in the audit trail. The pseudonym is posted in the
//! form's body: the address never carries it and the answer never draws it
//! back. No specification governs the console: our own design.

use thirtyfour::prelude::*;

use crate::journeys::browser::with_journey;
use crate::journeys::journeys_enabled;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{FILLED_PERSON, Server, Stack};

/// A synthetic pseudonym typed into the resolve form.
const TYPED: &str = "5e0c7a91-0000-4000-8000-00000000c0de";

/// Whether the page's main content draws `words`, as `yes` or `no`.
const DRAWS: &str =
    "return document.querySelector('main').innerText.includes(arguments[0]) ? 'yes' : 'no';";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_resolve_names_the_patient_and_the_trail_shows_the_disclosure() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        journey.open(&stack.url("/pseudonym-domains")).await;
        let field = journey
            .element(By::Id("resolve-pseudonym"), "the pseudonym field")
            .await;
        field
            .send_keys(TYPED)
            .await
            .expect("the field takes the text");
        let reason = journey
            .element(By::Id("resolve-reason"), "the reason field")
            .await;
        reason
            .send_keys("Synthetic phone call")
            .await
            .expect("the field takes the text");
        journey
            .press(
                By::Css("form input[name='form'][value='resolve'] ~ .fui-form-foot button"),
                "Resolve",
            )
            .await;
        journey
            .count_becoming(
                By::Css(format!("main a[href='/patients/{FILLED_PERSON}']")),
                1,
                "the patient the pseudonym belongs to",
            )
            .await;
        let address = journey
            .address_carrying("/pseudonym-domains", "the page's own address")
            .await;
        assert!(!address.contains(TYPED), "the address carries no pseudonym");
        let drawn = journey
            .evaluate_with(DRAWS, String::from(TYPED))
            .await
            .expect("the page's words");
        assert_eq!("no", drawn, "the answer never draws the pseudonym back");
        journey
            .press(By::Css("a[href='/audit']"), "the sidebar's Audit trail")
            .await;
        journey.address_carrying("/audit", "the audit trail").await;
        let drawn = journey
            .evaluate_with(DRAWS, String::from("pseudonym-resolve"))
            .await
            .expect("the trail's words");
        assert_eq!("yes", drawn, "the trail shows the disclosure");
    })
    .await;
}
