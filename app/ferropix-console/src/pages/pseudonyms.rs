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

use ferrohealth_ui::html::Html;
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::table::view::{
    Body, Column, DataTable, Role, Sorting, cell, data_table, title_cell,
};
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::form::{Field, INPUT, SELECT, check_field, help_id};
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::status::badge;
use ferrohealth_ui::view::surface::{card, disclosure, stack};
use ferrohealth_ui::view::template::list::{ListCard, list_card, list_empty};
use ferrohealth_ui::view::time::timestamp;
use ferropix_operator_api::pseudonym::{MAX_REASON, PseudonymDomain, PseudonymDomains, Resolution};

use crate::app::PSEUDONYM_DOMAINS;
use crate::locale::messages::Msg;
use crate::markup::{h, hidden, submit, t};
use crate::model::Read;
use crate::pages::{Needs, or_dash, server_state};
use crate::visit::Visit;

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
    visit: &Visit,
    domains: &PseudonymDomains,
    written: Option<&Read<()>>,
    resolved: Option<&Read<Resolution>>,
    admin: bool,
) -> Html {
    let live: Vec<&PseudonymDomain> = domains
        .domains
        .iter()
        .filter(|domain| domain.retired_at.is_none())
        .collect();
    let editor = if admin {
        create_form(visit).then(if live.is_empty() {
            Html::empty()
        } else {
            change_form(visit, &live)
        })
    } else {
        h(format!(
            "<p class=\"fui-help\">{}</p>",
            t(visit.tr(Msg::PseudonymAdminOnly))
        ))
    };
    stack(Html::join([
        resolved_notice(visit, resolved),
        written_notice(visit, written),
        domain_table(visit, domains),
        resolve_form(visit, &live),
        editor,
    ]))
}

/// The answer to a resolve just posted: the person, linked to their page,
/// or the server's refusal.
fn resolved_notice(visit: &Visit, resolved: Option<&Read<Resolution>>) -> Html {
    match resolved {
        None => Html::empty(),
        Some(Ok(resolution)) => {
            let person = &resolution.person;
            notice(
                Tone::Ok,
                Announce::Status,
                None,
                h(format!(
                    "<p><strong>{}</strong></p><p>{} <a href=\"{}\"><code>{}</code></a></p><p class=\"fui-small fui-muted\">{}</p>",
                    t(visit.tr(Msg::ResolvedTitle)),
                    t(visit.tr(Msg::ResolvedPerson)),
                    t(format!("/patients/{person}")),
                    t(visit.tr_with(Msg::PersonId, &[("person", person.clone().into())])),
                    t(visit.tr(Msg::ResolvedDisclosure)),
                )),
            )
        }
        Some(Err(probe)) => server_state(
            visit,
            Some(probe),
            Needs::of(Msg::WhatPseudonymResolve),
            "POST /operator/pseudonyms/resolve",
        ),
    }
}

/// The outcome of a domain write just posted.
fn written_notice(visit: &Visit, written: Option<&Read<()>>) -> Html {
    match written {
        None => Html::empty(),
        Some(Ok(())) => notice(
            Tone::Ok,
            Announce::Status,
            None,
            h(format!("<p>{}</p>", t(visit.tr(Msg::DomainSaved)))),
        ),
        Some(Err(probe)) => server_state(
            visit,
            Some(probe),
            Needs::of(Msg::WhatDomainWrite),
            "POST /operator/pseudonym-domains",
        ),
    }
}

/// The table of pseudonymisation domains, by namespace as the server sends
/// them.
fn domain_table(visit: &Visit, domains: &PseudonymDomains) -> Html {
    let locale = &visit.locale;
    let card_of = |count: usize, body: Html| {
        list_card(
            locale,
            ListCard {
                title: visit.tr(Msg::PseudonymDomains),
                count: Some(u64::try_from(count).unwrap_or(u64::MAX).into()),
                order: None,
                tabs: None,
                actions: None,
                body,
                pager: None,
            },
        )
    };
    if domains.domains.is_empty() {
        return card_of(0, list_empty(visit.tr(Msg::PseudonymsEmpty), None));
    }
    let columns = [
        Column::text("name", visit.tr(Msg::ColName)).role(Role::Title),
        Column::text("namespace", visit.tr(Msg::ColNamespace)).role(Role::Sub),
        Column::text("consumer", visit.tr(Msg::ColConsumer)).role(Role::Wide),
        Column::text("pseudonyms", visit.tr(Msg::TabPseudonyms)).numeric(),
        Column::text("created", visit.tr(Msg::ColCreated)).role(Role::Quiet),
        Column::text("state", visit.tr(Msg::ColState)),
    ];
    let [_, namespace, consumer, pseudonyms, created, state] = &columns;
    let rows: Html = domains
        .domains
        .iter()
        .map(|domain| {
            let consumer_cell = domain.consumer_client_id.as_ref().map_or_else(
                || or_dash(locale, None),
                |client| h(format!("<code>{}</code>", t(client))),
            );
            let badge_view = if domain.retired_at.is_some() {
                badge(Tone::Neutral, visit.tr(Msg::Retired))
            } else {
                badge(Tone::Ok, visit.tr(Msg::StateActive))
            };
            h(format!(
                "<tr>{}{}{}{}{}{}</tr>",
                title_cell(None, t(&domain.name), None),
                cell(
                    namespace,
                    h(format!(
                        "<code class=\"fui-wrap\">{}</code>",
                        t(&domain.namespace)
                    ))
                ),
                cell(consumer, consumer_cell),
                cell(
                    pseudonyms,
                    t(visit.tr_with(Msg::Count, &[("count", domain.pseudonyms.into())]))
                ),
                cell(created, timestamp(locale, &domain.created_at)),
                cell(state, badge_view),
            ))
        })
        .collect();
    let caption = visit.tr(Msg::PseudonymDomains);
    card_of(
        domains.domains.len(),
        data_table(DataTable {
            caption: &caption,
            columns: &columns,
            sorting: Sorting::default(),
            body: Body::Rows(rows),
        }),
    )
}

/// The options of a select over the domains `live`, by namespace or by id.
fn options(live: &[&PseudonymDomain], by_id: bool) -> Html {
    live.iter()
        .map(|domain| {
            let value = if by_id { &domain.id } else { &domain.namespace };
            h(format!(
                "<option value=\"{}\">{}</option>",
                t(value),
                t(&domain.name)
            ))
        })
        .collect()
}

/// The form that resolves a pseudonym to its person, with a reason; the
/// pseudonym is posted and never drawn back.
fn resolve_form(visit: &Visit, live: &[&PseudonymDomain]) -> Html {
    let body = if live.is_empty() {
        h(format!(
            "<p class=\"fui-help\">{}</p>",
            t(visit.tr(Msg::ResolveNoDomain))
        ))
    } else {
        let namespace = Field::new("resolve-namespace", visit.tr(Msg::DetailPseudonymDomain))
            .view(h(format!(
                "<select id=\"resolve-namespace\" name=\"namespace\" class=\"{SELECT}\">{}</select>",
                options(live, false)
            )));
        let pseudonym = Field::new("resolve-pseudonym", visit.tr(Msg::ResolvePseudonym)).view(h(
            format!(
                "<input id=\"resolve-pseudonym\" name=\"pseudonym\" type=\"text\" class=\"{INPUT}\" required autocomplete=\"off\" spellcheck=\"false\">"
            ),
        ));
        let reason = Field::new("resolve-reason", visit.tr(Msg::RevealReason)).view(h(format!(
            "<input id=\"resolve-reason\" name=\"reason\" type=\"text\" class=\"{INPUT}\" required maxlength=\"{MAX_REASON}\" autocomplete=\"off\" aria-describedby=\"{}\">",
            t(help_id("resolve"))
        )));
        h(format!(
            "<form method=\"post\" action=\"{PSEUDONYM_DOMAINS}\" class=\"fui-form\">{}<p class=\"fui-help\" id=\"{}\">{}</p><div class=\"fui-form-grid\">{namespace}{pseudonym}{reason}</div><div class=\"fui-form-foot\">{}</div></form>",
            hidden(FORM, RESOLVE),
            t(help_id("resolve")),
            t(visit.tr(Msg::ResolveNote)),
            submit(
                Variant::Secondary.class(),
                &label(Some(Glyph::SEARCH), visit.tr(Msg::ResolveSubmit))
            ),
        ))
    };
    card(visit.tr(Msg::ResolveTitle), body)
}

/// A text field of a domain form.
fn text_field(
    visit: &Visit,
    id: &'static str,
    name: &'static str,
    words: Msg,
    required: bool,
) -> Html {
    Field::new(id, visit.tr(words)).view(h(format!(
        "<input id=\"{id}\" name=\"{name}\" type=\"text\" class=\"{INPUT}\" autocomplete=\"off\"{}>",
        if required { " required" } else { "" }
    )))
}

/// The administrator's form that adds a domain.
fn create_form(visit: &Visit) -> Html {
    disclosure(
        visit.tr(Msg::DomainCreate),
        h(format!(
            "<form method=\"post\" action=\"{PSEUDONYM_DOMAINS}\" class=\"fui-form\">{}<p class=\"fui-help\">{}</p><div class=\"fui-form-grid\">{}{}{}{}</div><div class=\"fui-form-foot\">{}</div></form>",
            hidden(FORM, CREATE),
            t(visit.tr(Msg::DomainAdminNote)),
            text_field(
                visit,
                "create-namespace",
                "namespace",
                Msg::ColNamespace,
                true
            ),
            text_field(visit, "create-name", "name", Msg::ColName, true),
            text_field(
                visit,
                "create-consumer",
                "consumer",
                Msg::ColConsumer,
                false
            ),
            text_field(visit, "create-reason", "reason", Msg::RevealReason, true),
            submit(
                Variant::Primary.class(),
                &label(Some(Glyph::PLUS), visit.tr(Msg::DomainCreate))
            ),
        )),
    )
}

/// The administrator's form that renames a domain of `live`, names another
/// consumer, or retires it.
fn change_form(visit: &Visit, live: &[&PseudonymDomain]) -> Html {
    let domain =
        Field::new("change-domain", visit.tr(Msg::DetailPseudonymDomain)).view(h(format!(
            "<select id=\"change-domain\" name=\"domain\" class=\"{SELECT}\">{}</select>",
            options(live, true)
        )));
    let retire = check_field(
        "change-retire",
        visit.tr(Msg::DomainRetire),
        visit.tr(Msg::PseudonymRetireHelp),
        h(format!(
            "<input id=\"change-retire\" name=\"retire\" type=\"checkbox\" value=\"on\" aria-describedby=\"{}\">",
            t(help_id("change-retire"))
        )),
    );
    disclosure(
        visit.tr(Msg::PseudonymChange),
        h(format!(
            "<form method=\"post\" action=\"{PSEUDONYM_DOMAINS}\" class=\"fui-form\">{}<p class=\"fui-help\">{}</p><div class=\"fui-form-grid\">{domain}{}{}{}</div>{retire}<div class=\"fui-form-foot\">{}</div></form>",
            hidden(FORM, CHANGE),
            t(visit.tr(Msg::DomainAdminNote)),
            text_field(visit, "change-name", "name", Msg::ColName, false),
            text_field(
                visit,
                "change-consumer",
                "consumer",
                Msg::ColConsumer,
                false
            ),
            text_field(visit, "change-reason", "reason", Msg::RevealReason, true),
            submit(
                Variant::Primary.class(),
                &label(Some(Glyph::CHECK), visit.tr(Msg::PseudonymChange))
            ),
        )),
    )
}
