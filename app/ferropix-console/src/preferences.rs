// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The steward's personal preferences, each its own cookie.
//!
//! They are the library's six, read with `theme::Preferences::read`, and
//! the console's own two, how long a revealed identifier stays and whether
//! leaving the page hides it (FerroUI family rule 9).
//!
//! Every preference is a plain form post to [`PREFERENCE_ROUTE`], which
//! keeps it in its cookie and sends the browser back to the page it came
//! from, so the page works with no WebAssembly. The server bounds a reveal
//! whatever the cookie says. No specification governs the preferences: our
//! own design.

#[cfg(not(target_arch = "wasm32"))]
use axum::Extension;
#[cfg(not(target_arch = "wasm32"))]
use axum::extract::Form;
#[cfg(not(target_arch = "wasm32"))]
use axum::response::{IntoResponse, Redirect, Response};
#[cfg(not(target_arch = "wasm32"))]
use ferrohealth_ui::theme::Preferences;
use ferrohealth_ui::theme::{
    Density, Hints, Language, Motion, Preference as _, Theme, WeekStart, cookie,
};
#[cfg(not(target_arch = "wasm32"))]
use http::header::{CACHE_CONTROL, SET_COOKIE};
#[cfg(not(target_arch = "wasm32"))]
use http::{HeaderMap, HeaderValue, StatusCode};

#[cfg(not(target_arch = "wasm32"))]
use crate::locale::{Msg, catalog, negotiated, tr_in};
#[cfg(not(target_arch = "wasm32"))]
use crate::server::ConsoleState;

/// The route every preference posts to, its value under its cookie's name.
pub const PREFERENCE_ROUTE: &str = "/preference";

/// The route Reset to defaults posts to.
pub const RESET_ROUTE: &str = "/preferences/reset";

/// The route the sidebar's Collapse and Expand post to.
pub const SIDEBAR_ROUTE: &str = "/sidebar";

/// The name of the reveal duration's cookie, after the preference prefix.
pub const REVEAL: &str = "reveal";

/// The name of the hide on leave switch's cookie, after the preference
/// prefix.
pub const HIDE: &str = "hide";

/// The name of the sidebar's cookie, after the preference prefix.
pub const SIDEBAR: &str = "sidebar";

/// The console's own preferences, which Reset to defaults clears beside the
/// library's.
pub const OWN: [&str; 2] = [REVEAL, HIDE];

/// The reveal durations a steward may choose, in seconds; the server caps
/// every reveal at the longest.
pub const REVEAL_CHOICES: [&str; 3] = ["10", "15", "30"];

/// The reveal duration when none is chosen.
pub const REVEAL_DEFAULT: &str = "15";

/// The value of the preference `name` as its cookie keeps it, when `value`
/// is one the console offers for it, and `None` for anything else.
#[must_use]
pub fn kept_value(name: &str, value: &str) -> Option<String> {
    let kept = |choice: Option<&str>| choice.map(str::to_owned);
    if name == Theme::COOKIE {
        kept(Theme::from_cookie(value).map(Theme::key))
    } else if name == Density::COOKIE {
        kept(Density::from_cookie(value).map(Density::key))
    } else if name == Motion::COOKIE {
        kept(Motion::from_cookie(value).map(Motion::key))
    } else if name == WeekStart::COOKIE {
        kept(WeekStart::from_cookie(value).map(WeekStart::key))
    } else if name == Hints::COOKIE {
        kept(Hints::from_cookie(value).map(Hints::key))
    } else if name == Language::COOKIE {
        let offered = |tag: &str| {
            crate::locale::choices()
                .iter()
                .any(|locale| locale.tag() == tag)
        };
        Language::from_cookie(value)
            .filter(|language| language.tag().is_none_or(offered))
            .map(|language| language.cookie_value().to_owned())
    } else if name == REVEAL {
        kept(REVEAL_CHOICES.into_iter().find(|seconds| *seconds == value))
    } else if name == HIDE {
        kept(["on", "off"].into_iter().find(|state| *state == value))
    } else {
        None
    }
}

/// The page `back` names when it is one of the console's own, plain path
/// segments of visible ASCII, and `/` for anything else, so a post never
/// redirects off the console.
#[must_use]
pub fn return_path(back: &str) -> &str {
    let plain = back.starts_with('/')
        && !back.starts_with("//")
        && back
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '\\' | '?' | '#'))
        && !back.split('/').any(|segment| segment == "..");
    if plain { back } else { "/" }
}

/// Where a preference post returns: the page `back` names, and on Your
/// preferences the same page saying which preference it kept.
#[must_use]
pub fn preference_return(back: &str, name: &str) -> String {
    let page = return_path(back);
    if page.starts_with("/preferences") {
        format!("{page}?saved={name}")
    } else {
        page.to_owned()
    }
}

/// The console's own preference `name` the `Cookie` header value
/// `cookies` keeps under `prefix`, when it keeps one the console offers.
#[must_use]
pub fn own_in(cookies: &str, prefix: &str, name: &str) -> Option<String> {
    cookie(cookies, &format!("{prefix}{name}")).and_then(|value| kept_value(name, value))
}

/// Whether the `Cookie` header value `cookies` asks for the sidebar's rail.
#[must_use]
pub fn rail_in(cookies: &str, prefix: &str) -> bool {
    cookie(cookies, &format!("{prefix}{SIDEBAR}")) == Some("rail")
}

/// The preference cookies of `headers` alone, each named `prefix` then a
/// preference's or the sidebar's name, joined as one `Cookie` header value;
/// the session's cookie shares the prefix and is never among them.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn kept_cookies(headers: &HeaderMap, prefix: &str) -> String {
    // NOTE: RFC 9113 §8.2.3: a request may split its cookies over several headers,
    // which read as one list joined by `; `.
    headers
        .get_all(http::header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .map(str::trim)
        .filter(|pair| {
            pair.split_once('=').is_some_and(|(name, _value)| {
                name.strip_prefix(prefix).is_some_and(|name| {
                    !prefix.is_empty()
                        && (name == SIDEBAR
                            || OWN.contains(&name)
                            || Preferences::COOKIES.contains(&name))
                })
            })
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(not(target_arch = "wasm32"))]
/// The preferences the request with `headers` keeps, read under `prefix`.
#[must_use]
pub fn read(headers: &HeaderMap, prefix: &str) -> Preferences {
    Preferences::read(&cookies(headers), prefix)
}

#[cfg(not(target_arch = "wasm32"))]
/// The console's own preference `name` the request with `headers` keeps,
/// when it keeps one the console offers.
#[must_use]
pub fn own(headers: &HeaderMap, prefix: &str, name: &str) -> Option<String> {
    let cookies = cookies(headers);
    cookie(&cookies, &format!("{prefix}{name}")).and_then(|value| kept_value(name, value))
}

#[cfg(not(target_arch = "wasm32"))]
/// Whether the request with `headers` asks for the sidebar's rail.
#[must_use]
pub fn rail(headers: &HeaderMap, prefix: &str) -> bool {
    cookie(&cookies(headers), &format!("{prefix}{SIDEBAR}")) == Some("rail")
}

#[cfg(not(target_arch = "wasm32"))]
/// Every `Cookie` header of a request, joined as one header value.
fn cookies(headers: &HeaderMap) -> String {
    headers
        .get_all(http::header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(not(target_arch = "wasm32"))]
/// Serves a preference post: keeps the one preference it chose in its
/// cookie and sends the browser back to the page it came from.
///
/// The post carries `back` and one preference under its cookie's name; a
/// switch posts `off` before its `on`, and the last value wins.
pub async fn choose(
    Extension(state): Extension<ConsoleState>,
    headers: HeaderMap,
    Form(fields): Form<Vec<(String, String)>>,
) -> Response {
    let back = fields
        .iter()
        .rev()
        .find(|(name, _)| name == "back")
        .map_or("/", |(_, back)| back.as_str());
    let chosen = fields
        .iter()
        .filter(|(name, _)| name != "back")
        .collect::<Vec<_>>();
    let one = chosen
        .last()
        .filter(|(name, _)| chosen.iter().all(|(other, _)| other == name));
    let Some(value) = one.and_then(|(name, value)| Some((name, kept_value(name, value)?))) else {
        let locale = negotiated(&headers, state.settings().locale.clone());
        return (
            StatusCode::BAD_REQUEST,
            [(CACHE_CONTROL, "no-store")],
            tr_in(catalog(), &locale, Msg::PreferenceRefused),
        )
            .into_response();
    };
    let (name, value) = value;
    let cookie = state.sessions().preference_cookie(name, &value).to_string();
    redirect_with(&preference_return(back, name), &[cookie])
}

#[cfg(not(target_arch = "wasm32"))]
/// What Reset to defaults posts.
#[derive(Debug, serde::Deserialize)]
pub struct Reset {
    /// The page to return to.
    back: String,
}

#[cfg(not(target_arch = "wasm32"))]
/// Serves Reset to defaults: clears every preference cookie, the console's
/// own among them, and sends the browser back to the page it came from.
pub async fn reset(
    Extension(state): Extension<ConsoleState>,
    Form(reset): Form<Reset>,
) -> Response {
    let cleared = Preferences::COOKIES
        .into_iter()
        .chain(OWN)
        .map(|name| state.sessions().preference_removal(name).to_string())
        .collect::<Vec<_>>();
    redirect_with(return_path(&reset.back), &cleared)
}

#[cfg(not(target_arch = "wasm32"))]
/// What the sidebar's Collapse or Expand posts.
#[derive(Debug, serde::Deserialize)]
pub struct SidebarChoice {
    /// `rail` to fold the navigation, `full` to unfold it.
    sidebar: String,
    /// The page to return to.
    back: String,
}

#[cfg(not(target_arch = "wasm32"))]
/// Serves the sidebar's Collapse and Expand: keeps the state in its cookie
/// and sends the browser back to the page it came from.
pub async fn sidebar(
    Extension(state): Extension<ConsoleState>,
    Form(choice): Form<SidebarChoice>,
) -> Response {
    if !matches!(choice.sidebar.as_str(), "rail" | "full") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let cookie = state
        .sessions()
        .preference_cookie(SIDEBAR, &choice.sidebar)
        .to_string();
    redirect_with(return_path(&choice.back), &[cookie])
}

#[cfg(not(target_arch = "wasm32"))]
/// The redirect to `location` that sets each of `cookies`.
fn redirect_with(location: &str, cookies: &[String]) -> Response {
    let mut response = Redirect::to(location).into_response();
    for cookie in cookies {
        match HeaderValue::from_str(cookie) {
            Ok(value) => {
                response.headers_mut().append(SET_COOKIE, value);
            }
            Err(_invalid) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::{kept_value, preference_return, return_path};

    #[test]
    fn a_preference_post_keeps_only_a_value_the_console_offers() {
        assert_eq!(Some(String::from("dark")), kept_value("theme", "dark"));
        assert_eq!(None, kept_value("theme", "sepia"));
        assert_eq!(
            Some(String::from("compact")),
            kept_value("density", "compact")
        );
        assert_eq!(
            Some(String::from("browser")),
            kept_value("language", "browser")
        );
        assert_eq!(Some(String::from("en")), kept_value("language", "en"));
        assert_eq!(
            None,
            kept_value("language", "nl"),
            "a language the console lacks"
        );
        assert_eq!(Some(String::from("30")), kept_value("reveal", "30"));
        assert_eq!(
            None,
            kept_value("reveal", "300"),
            "the server bounds a reveal"
        );
        assert_eq!(Some(String::from("off")), kept_value("hide", "off"));
        assert_eq!(None, kept_value("sidebar", "rail"), "no preference");
    }

    #[test]
    fn a_preference_post_returns_saying_what_it_kept_only_on_your_preferences() {
        assert_eq!(
            "/preferences/keyboard?saved=hints",
            preference_return("/preferences/keyboard", "hints")
        );
        assert_eq!("/queue", preference_return("/queue", "theme"));
        assert_eq!(
            "/",
            preference_return("//evil.example/preferences", "theme")
        );
    }

    #[test]
    fn a_return_path_never_leaves_the_console() {
        assert_eq!("/domains", return_path("/domains"));
        for hostile in [
            "//evil.example",
            "https://evil.example",
            "/a/../b",
            "/a?b",
            "",
            "/a b",
        ] {
            assert_eq!("/", return_path(hostile), "{hostile}");
        }
    }
}
