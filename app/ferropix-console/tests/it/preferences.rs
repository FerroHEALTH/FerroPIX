// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Your preferences: each preference its own cookie, read when the page is
//! drawn, so the first paint is right (FerroUI family rule 9).

use std::error::Error;

use http::StatusCode;

use crate::support::{console, get_with, offline, post_form, send, set_cookies, steward_session};

/// An address nothing listens on.
const NOWHERE: &str = "http://127.0.0.1:9/";

#[tokio::test]
async fn a_preference_post_keeps_its_cookie_and_the_next_page_is_drawn_in_it()
-> Result<(), Box<dyn Error>> {
    let (state, service) = console(&offline(NOWHERE))?;
    let cookie = steward_session(&state);
    let (response, _) = send(
        &service,
        post_form("/preference", &cookie, "back=%2Fpreferences&theme=dark")?,
    )
    .await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status());
    assert_eq!(
        "/preferences?saved=theme",
        crate::support::header(&response, "location")
    );
    let kept = set_cookies(&response);
    let theme = kept
        .iter()
        .find(|cookie| cookie.starts_with("ferropix_console_theme=dark"))
        .expect("the theme's cookie");
    assert!(
        theme.contains("HttpOnly") && theme.contains("SameSite=Lax"),
        "{theme}"
    );
    assert!(theme.contains("Max-Age=31536000"), "kept one year: {theme}");
    let both = format!("{cookie}; ferropix_console_theme=dark; ferropix_console_density=compact");
    let (_, page) = send(&service, get_with("/preferences?saved=theme", &both)?).await?;
    assert!(
        page.contains("data-theme=\"dark\""),
        "the first paint is dark"
    );
    assert!(page.contains("data-density=\"compact\""), "and compact");
    assert!(
        page.contains("Revealed identifiers stay for"),
        "the console's own rows"
    );
    Ok(())
}

#[tokio::test]
async fn a_value_the_console_does_not_offer_is_refused() -> Result<(), Box<dyn Error>> {
    let (state, service) = console(&offline(NOWHERE))?;
    let cookie = steward_session(&state);
    for body in [
        "back=%2F&theme=sepia",
        "back=%2F&reveal=300",
        "back=%2F&language=nl",
        "back=%2F",
    ] {
        let (response, _) = send(&service, post_form("/preference", &cookie, body)?).await?;
        assert_eq!(StatusCode::BAD_REQUEST, response.status(), "{body}");
    }
    Ok(())
}

#[tokio::test]
async fn reset_clears_every_preference_and_the_consoles_own() -> Result<(), Box<dyn Error>> {
    let (state, service) = console(&offline(NOWHERE))?;
    let cookie = steward_session(&state);
    let (response, _) = send(
        &service,
        post_form("/preferences/reset", &cookie, "back=%2Fpreferences")?,
    )
    .await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status());
    let cleared = set_cookies(&response);
    for name in [
        "theme",
        "density",
        "motion",
        "language",
        "week-start",
        "hints",
        "reveal",
        "hide",
    ] {
        assert!(
            cleared.iter().any(
                |cookie| cookie.starts_with(&format!("ferropix_console_{name}=;"))
                    && cookie.contains("Max-Age=0")
            ),
            "{name} is cleared"
        );
    }
    Ok(())
}

#[tokio::test]
async fn each_tab_is_its_own_route() -> Result<(), Box<dyn Error>> {
    let (state, service) = console(&offline(NOWHERE))?;
    let cookie = steward_session(&state);
    for route in [
        "/preferences",
        "/preferences/language",
        "/preferences/keyboard",
    ] {
        let (response, page) = send(&service, get_with(route, &cookie)?).await?;
        assert_eq!(StatusCode::OK, response.status(), "{route}");
        assert!(page.contains("Only for you, in this browser."), "{route}");
    }
    Ok(())
}
