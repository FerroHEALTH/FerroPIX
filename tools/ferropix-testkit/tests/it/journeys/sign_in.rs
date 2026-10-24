// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Sign-in and sign-out in a real browser: a visitor is sent to the sign-in
//! page, the provider's button signs the steward in through the
//! authorization code flow with PKCE (RFC 6749 §4.1, RFC 7636), the shell
//! names the steward, and Sign out in the user menu ends the session at the
//! console and at the provider (OpenID Connect RP-Initiated Logout 1.0 §2)
//! and lands on the sign-in page that says so.

use thirtyfour::prelude::*;

use crate::journeys::browser::{Journey, with_journey};
use crate::journeys::journeys_enabled;
use crate::journeys::stack::{STEWARD, Stack};

/// The console's session cookie over plain HTTP.
const SESSION_COOKIE: &str = "ferropix_console_session";

/// The provider's button on the sign-in page, a link to the console's
/// sign-in route.
const SIGN_IN_BUTTON: &str = "a[href='/login']";

/// The user menu's button in the top bar, which shows the steward's name.
const USER_MENU: &str = "button.fui-operator";

/// Signs the steward in from the sign-in page and waits for the dashboard
/// to name them.
pub(crate) async fn sign_in(journey: &Journey, stack: &Stack) {
    journey.open(&stack.url("/")).await;
    journey
        .address_carrying("/sign-in", "a visitor to be sent to the sign-in page")
        .await;
    journey
        .press(By::Css(SIGN_IN_BUTTON), "the provider's button")
        .await;
    let menu = journey
        .element(By::Css(USER_MENU), "the user menu naming the steward")
        .await;
    let named = menu.text().await.unwrap_or_default();
    assert!(
        named.contains(STEWARD),
        "the shell names {named:?}, not the steward the ID Token names"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_steward_signs_in_through_the_provider_and_signs_out_again() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start().await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        let session = journey
            .cookie(SESSION_COOKIE)
            .await
            .expect("the browser reads its cookies");
        assert!(
            session.is_some_and(|cookie| cookie.http_only == Some(true)),
            "the session is one HttpOnly cookie"
        );

        journey
            .press(By::Css(USER_MENU), "the user menu's button")
            .await;
        journey
            .press(
                By::Css("form[action='/logout'] button[type='submit']"),
                "Sign out in the user menu",
            )
            .await;
        journey
            .address_carrying(
                "/sign-in/signed-out",
                "the provider to send the browser to the signed-out page",
            )
            .await;
        journey
            .element(
                By::Css(SIGN_IN_BUTTON),
                "the sign-in page offering to sign in again",
            )
            .await;

        journey.open(&stack.url("/audit")).await;
        journey
            .address_carrying(
                "/sign-in",
                "a page after sign-out to send the browser to sign in",
            )
            .await;
    })
    .await;
}
