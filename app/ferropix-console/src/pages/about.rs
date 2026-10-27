// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! About at `/about` (design 32): the versions, the manufacturer and the
//! server's `CapabilityStatement`, FerroUI's About page.
//!
//! The `CapabilityStatement` is the one document the server describes
//! itself with (HL7 FHIR R4, the REST interaction `capabilities`), read from
//! `GET {base}/fhir/metadata` and shown as the server sent it. While the
//! server does not send one, the card is the call's refusal and the
//! server's version is not known.

use ferrohealth_ui::highlight::Language;
use ferrohealth_ui::view::surface::{Header, card};
use ferrohealth_ui::view::template::about::{
    AboutPage as Page, Link, Manufacturer, SelfDescription, about_page,
};
use leptos::prelude::*;
use serde::Deserialize;

use crate::app::page_context;
use crate::app::shell::footer;
use crate::locale::messages::Msg;
use crate::locale::tr;
use crate::model::Capability;
use crate::pages::title;

/// The FerroUI release the console is built on, as `Cargo.toml` pins it
/// exactly (a test holds the two together).
pub const FERROUI: &str = "0.5.2";

/// The route Download fetches the `CapabilityStatement` from.
pub const METADATA_DOWNLOAD: &str = "/about/capability-statement.json";

/// The support address of the maker.
const SUPPORT: &str = "info@cadasto.com";

/// The FerroPIX book.
const BOOK: &str = "https://ferropix.eu/docs/";

/// What About reads from a `CapabilityStatement`: the software and the FHIR
/// version (HL7 FHIR R4, `CapabilityStatement.software`, `.fhirVersion`).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Declared {
    /// The software the server declares.
    software: Option<Software>,
    /// The FHIR version it declares.
    fhir_version: Option<String>,
}

/// `CapabilityStatement.software`.
#[derive(Debug, Deserialize)]
struct Software {
    /// Its name.
    name: String,
    /// Its version.
    version: Option<String>,
}

/// The About page.
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn AboutPage() -> impl IntoView {
    let capability = page_context().capability;
    let declared = match &capability {
        // NOTE: HL7 FHIR R4, RESTful API: a statement that does not read as one names no
        // version, which About then says is not known; the document is still shown.
        Some(Capability::Declared(text)) => serde_json::from_str(text).unwrap_or_default(),
        _ => Declared::default(),
    };
    let server = declared.software.map_or_else(
        || tr(Msg::NotKnown),
        |software| match software.version {
            Some(version) => format!("{} {version}", software.name),
            None => software.name,
        },
    );
    let fhir = declared.fhir_version.unwrap_or_else(|| tr(Msg::NotKnown));
    let versions = vec![
        (tr(Msg::VersionServer), server.into_any()),
        (tr(Msg::VersionFhir), fhir.into_any()),
        (
            tr(Msg::VersionFerroui),
            format!("ferrohealth-ui {FERROUI}").into_any(),
        ),
    ];
    let (self_description, cards) = match capability {
        Some(Capability::Declared(text)) => (
            Some(SelfDescription {
                title: tr(Msg::CapabilityTitle),
                line: tr(Msg::CapabilityLine),
                language: Language::Json,
                text,
                copy_id: String::from("capability-copy"),
                download: String::from(METADATA_DOWNLOAD),
            }),
            Vec::new(),
        ),
        Some(Capability::Missing(probe)) => {
            let missing = crate::pages::server_state(
                Some(&probe),
                crate::pages::Needs {
                    what: Msg::WhatCapability,
                    away: false,
                },
                &probe.route,
            );
            (
                None,
                vec![card(tr(Msg::CapabilityTitle), missing).into_any()],
            )
        }
        None => (None, Vec::new()),
    };
    let page = about_page(Page {
        header: Header {
            title: tr(Msg::About).into(),
            subtitle: Some(tr(Msg::AboutSub).into()),
            ..Header::default()
        },
        release: footer().release(),
        versions,
        manufacturer: Manufacturer {
            licence: tr(Msg::Licence),
            support: Link {
                label: String::from(SUPPORT),
                href: format!("mailto:{SUPPORT}"),
            },
            documentation: Some(Link {
                label: tr(Msg::Documentation),
                href: String::from(BOOK),
            }),
        },
        cards,
        self_description,
        replica: None,
    });
    view! {
        {title(tr(Msg::About))}
        {page}
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::FERROUI;

    #[test]
    fn the_ferroui_release_about_names_is_the_one_the_workspace_pins() {
        let manifest =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml"))
                .expect("the workspace manifest");
        assert!(
            manifest.contains(&format!("ferrohealth-ui = \"={FERROUI}\"")),
            "About names ferrohealth-ui {FERROUI}, the workspace pins another"
        );
    }
}
