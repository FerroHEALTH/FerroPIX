// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pseudonymisation domains page, drawn from `GET
//! /operator/pseudonym-domains` (`docs/architecture.md` §8 and §12.3).
//!
//! It draws the domains with their counts, the one form that resolves a
//! pseudonym to its person, and behind the administrator scope the forms
//! that add and change a domain. Every form posts to the page's own
//! address, so a pseudonym rides in a request body and never in an address,
//! and the answer names the person and never the pseudonym. No
//! specification governs the console: our own design.

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::table::view::{
    Body, Column, DataTable, Role, Sorting, cell, data_table, title_cell,
};
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::form::{Field, INPUT, SELECT, check_field, help_id};
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::status::badge;
use ferrohealth_ui::view::surface::{card, disclosure, or_none, stack};
use ferrohealth_ui::view::template::list::{ListCard, list_card, list_empty};
use ferrohealth_ui::view::time::timestamp;
use ferropix_operator_api::pseudonym::{MAX_REASON, PseudonymDomain, PseudonymDomains, Resolution};
use leptos::prelude::*;

use crate::app::PSEUDONYM_DOMAINS;
use crate::locale::messages::Msg;
use crate::locale::{tr, tr_with};
use crate::model::Read;
use crate::pages::{Needs, server_state};

/// The hidden field that names which of the page's forms was posted.
pub const FORM: &str = "form";

/// The value of [`FORM`] for the create form.
pub const CREATE: &str = "create";

/// The value of [`FORM`] for the change form.
pub const CHANGE: &str = "change";

/// The value of [`FORM`] for the resolve form.
pub const RESOLVE: &str = "resolve";

/// The page: the answer to a resolve or a write just posted, the domains,
/// the resolve form, and the administrator's forms.
#[must_use]
pub fn pseudonym_domains(
    domains: &PseudonymDomains,
    written: Option<&Read<()>>,
    resolved: Option<&Read<Resolution>>,
    admin: bool,
) -> AnyView {
    let live: Vec<&PseudonymDomain> = domains
        .domains
        .iter()
        .filter(|domain| domain.retired_at.is_none())
        .collect();
    let editor = if admin {
        view! {
            {create_form()}
            {(!live.is_empty()).then(|| change_form(&live))}
        }
        .into_any()
    } else {
        view! { <p class="fui-help">{tr(Msg::PseudonymAdminOnly)}</p> }.into_any()
    };
    stack(view! {
        {resolved_notice(resolved)}
        {written_notice(written)}
        {domain_table(domains)}
        {resolve_form(&live)}
        {editor}
    })
    .into_any()
}

/// The answer to a resolve just posted: the person, linked to their page,
/// or the server's refusal.
fn resolved_notice(resolved: Option<&Read<Resolution>>) -> Option<AnyView> {
    resolved.map(|resolved| match resolved {
        Ok(resolution) => {
            let person = resolution.person.clone();
            notice(
                Tone::Ok,
                Announce::Status,
                None,
                view! {
                    <p><strong>{tr(Msg::ResolvedTitle)}</strong></p>
                    <p>
                        {tr(Msg::ResolvedPerson)}" "
                        <a href=format!("/patients/{person}")>
                            <code>{tr_with(Msg::PersonId, &[("person", person.clone().into())])}</code>
                        </a>
                    </p>
                    <p class="fui-small fui-muted">{tr(Msg::ResolvedDisclosure)}</p>
                },
            )
            .into_any()
        }
        Err(probe) => server_state(
            Some(probe),
            Needs {
                what: Msg::WhatPseudonymResolve,
                away: false,
            },
            "POST /operator/pseudonyms/resolve",
        ),
    })
}

/// The outcome of a domain write just posted.
fn written_notice(written: Option<&Read<()>>) -> Option<AnyView> {
    written.map(|written| match written {
        Ok(()) => notice(
            Tone::Ok,
            Announce::Status,
            None,
            view! { <p>{tr(Msg::DomainSaved)}</p> },
        )
        .into_any(),
        Err(probe) => server_state(
            Some(probe),
            Needs {
                what: Msg::WhatDomainWrite,
                away: false,
            },
            "POST /operator/pseudonym-domains",
        ),
    })
}

/// The table of pseudonymisation domains, by namespace as the server sends
/// them.
fn domain_table(domains: &PseudonymDomains) -> AnyView {
    if domains.domains.is_empty() {
        return list_card(ListCard {
            title: tr(Msg::PseudonymDomains),
            count: Some(0_u64.into()),
            order: None,
            tabs: None,
            actions: None,
            body: list_empty(tr(Msg::PseudonymsEmpty), None),
            pager: None,
        });
    }
    let columns = [
        Column::text("name", tr(Msg::ColName)).role(Role::Title),
        Column::text("namespace", tr(Msg::ColNamespace)).role(Role::Sub),
        Column::text("consumer", tr(Msg::ColConsumer)).role(Role::Wide),
        Column::text("pseudonyms", tr(Msg::TabPseudonyms)).numeric(),
        Column::text("created", tr(Msg::ColCreated)).role(Role::Quiet),
        Column::text("state", tr(Msg::ColState)),
    ];
    let [_, namespace, consumer, pseudonyms, created, state] = &columns;
    let rows = domains
        .domains
        .iter()
        .map(|domain| {
            let consumer_cell = domain.consumer_client_id.clone().map_or_else(
                || or_none(String::new()),
                |client| view! { <code>{client}</code> }.into_any(),
            );
            let badge_view = if domain.retired_at.is_some() {
                badge(Tone::Neutral, tr(Msg::Retired)).into_any()
            } else {
                badge(Tone::Ok, tr(Msg::Active)).into_any()
            };
            view! {
                <tr>
                    {title_cell(None, domain.name.clone(), None)}
                    {cell(namespace, view! { <code class="fui-wrap">{domain.namespace.clone()}</code> })}
                    {cell(consumer, consumer_cell)}
                    {cell(pseudonyms, tr_with(Msg::Count, &[("count", domain.pseudonyms.into())]))}
                    {cell(created, timestamp(&domain.created_at))}
                    {cell(state, badge_view)}
                </tr>
            }
        })
        .collect_view()
        .into_any();
    list_card(ListCard {
        title: tr(Msg::PseudonymDomains),
        count: Some(
            u64::try_from(domains.domains.len())
                .unwrap_or(u64::MAX)
                .into(),
        ),
        order: None,
        tabs: None,
        actions: None,
        body: data_table(DataTable {
            caption: &tr(Msg::PseudonymDomains),
            columns: &columns,
            sorting: Sorting::default(),
            body: Body::Rows(rows),
        })
        .into_any(),
        pager: None,
    })
}

/// The options of a select over the domains `live`, by namespace or by id.
fn options(live: &[&PseudonymDomain], by_id: bool) -> AnyView {
    live.iter()
        .map(|domain| {
            let value = if by_id {
                domain.id.clone()
            } else {
                domain.namespace.clone()
            };
            view! { <option value=value>{domain.name.clone()}</option> }
        })
        .collect_view()
        .into_any()
}

/// The form that resolves a pseudonym to its person, with a reason; the
/// pseudonym is posted and never drawn back.
fn resolve_form(live: &[&PseudonymDomain]) -> AnyView {
    let body = if live.is_empty() {
        view! { <p class="fui-help">{tr(Msg::ResolveNoDomain)}</p> }.into_any()
    } else {
        view! {
            <form method="post" action=PSEUDONYM_DOMAINS class="fui-form">
                <input type="hidden" name=FORM value=RESOLVE />
                <p class="fui-help" id=help_id("resolve")>{tr(Msg::ResolveNote)}</p>
                <div class="fui-form-grid">
                    {Field::new("resolve-namespace", tr(Msg::DetailPseudonymDomain)).view(view! {
                        <select id="resolve-namespace" name="namespace" class=SELECT>{options(live, false)}</select>
                    })}
                    {Field::new("resolve-pseudonym", tr(Msg::ResolvePseudonym)).view(view! {
                        <input id="resolve-pseudonym" name="pseudonym" type="text" class=INPUT required autocomplete="off" spellcheck="false" />
                    })}
                    {Field::new("resolve-reason", tr(Msg::RevealReason)).view(view! {
                        <input id="resolve-reason" name="reason" type="text" class=INPUT required maxlength=MAX_REASON.to_string() autocomplete="off" aria-describedby=help_id("resolve") />
                    })}
                </div>
                <div class="fui-form-foot">
                    <button type="submit" class=Variant::Secondary.class()>{label(Some(Glyph::SEARCH), tr(Msg::ResolveSubmit))}</button>
                </div>
            </form>
        }
        .into_any()
    };
    card(tr(Msg::ResolveTitle), body).into_any()
}

/// A text field of a domain form.
fn text_field(id: &'static str, name: &'static str, words: Msg, required: bool) -> AnyView {
    Field::new(id, tr(words)).view(view! {
        <input id=id name=name type="text" class=INPUT autocomplete="off" required=required />
    })
}

/// The administrator's form that adds a domain.
fn create_form() -> AnyView {
    disclosure(
        tr(Msg::DomainCreate),
        view! {
            <form method="post" action=PSEUDONYM_DOMAINS class="fui-form">
                <input type="hidden" name=FORM value=CREATE />
                <p class="fui-help">{tr(Msg::DomainAdminNote)}</p>
                <div class="fui-form-grid">
                    {text_field("create-namespace", "namespace", Msg::ColNamespace, true)}
                    {text_field("create-name", "name", Msg::ColName, true)}
                    {text_field("create-consumer", "consumer", Msg::ColConsumer, false)}
                    {text_field("create-reason", "reason", Msg::RevealReason, true)}
                </div>
                <div class="fui-form-foot">
                    <button type="submit" class=Variant::Primary.class()>{label(Some(Glyph::PLUS), tr(Msg::DomainCreate))}</button>
                </div>
            </form>
        },
    )
    .into_any()
}

/// The administrator's form that renames a domain of `live`, names another
/// consumer, or retires it.
fn change_form(live: &[&PseudonymDomain]) -> AnyView {
    disclosure(
        tr(Msg::PseudonymChange),
        view! {
            <form method="post" action=PSEUDONYM_DOMAINS class="fui-form">
                <input type="hidden" name=FORM value=CHANGE />
                <p class="fui-help">{tr(Msg::DomainAdminNote)}</p>
                <div class="fui-form-grid">
                    {Field::new("change-domain", tr(Msg::DetailPseudonymDomain)).view(view! {
                        <select id="change-domain" name="domain" class=SELECT>{options(live, true)}</select>
                    })}
                    {text_field("change-name", "name", Msg::ColName, false)}
                    {text_field("change-consumer", "consumer", Msg::ColConsumer, false)}
                    {text_field("change-reason", "reason", Msg::RevealReason, true)}
                </div>
                {check_field(
                    "change-retire",
                    tr(Msg::DomainRetire),
                    tr(Msg::PseudonymRetireHelp),
                    view! { <input id="change-retire" name="retire" type="checkbox" value="on" aria-describedby=help_id("change-retire") /> },
                )}
                <div class="fui-form-foot">
                    <button type="submit" class=Variant::Primary.class()>{label(Some(Glyph::CHECK), tr(Msg::PseudonymChange))}</button>
                </div>
            </form>
        },
    )
    .into_any()
}
