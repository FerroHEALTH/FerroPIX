// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The page for an address the console does not serve (design 30): the
//! status in words before its number, what the address may mean, and the
//! ways on, FerroUI's error page in the shell.

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::view::error_page::{ErrorPage, Status, Way, error_page};
use http::StatusCode;

use crate::app::{DASHBOARD, SEARCH};
use crate::locale::messages::Msg;
use crate::markup::t;
use crate::pages::Drawn;
use crate::visit::Visit;

/// The not-found page, answered `404`.
#[must_use]
pub fn not_found(visit: &Visit) -> Drawn {
    let page = error_page(
        &visit.locale,
        ErrorPage {
            status: Status::NotFound,
            title: visit.tr(Msg::NotFoundTitle),
            body: t(visit.tr(Msg::NotFoundBody)),
            hint: None,
            ways: vec![
                Way {
                    label: visit.tr(Msg::NotFoundBack),
                    href: String::from(DASHBOARD),
                    glyph: Some(Glyph::ARROW_LEFT),
                },
                Way {
                    label: visit.tr(Msg::NotFoundSearch),
                    href: String::from(SEARCH),
                    glyph: Some(Glyph::SEARCH),
                },
            ],
            detail: None,
        },
    );
    Drawn {
        status: StatusCode::NOT_FOUND,
        ..Drawn::new(visit.tr(Msg::NotFoundTitle), page)
    }
}
