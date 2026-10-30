// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The registry's pages drawn from the operator API: the sources, one
//! source, the identifier domains, one domain and the configuration.
//!
//! Every table is FerroUI's data table in its list card (T1), every page
//! of one thing a definition grid beside its last events, and the
//! configuration FerroUI's settings rows in file mode (family rule 13). A
//! feed or a message row names its time, its channel, what it did and how
//! it ended, never a value. No specification governs the console: our own
//! design.

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
use ferrohealth_ui::view::surface::{card, definitions, disclosure, stack};
use ferrohealth_ui::view::template::list::{ListCard, list_card, list_empty};
use ferrohealth_ui::view::template::settings::{
    How, Setting, ValueSource, file_line, setting_row, settings_panel,
};
use ferrohealth_ui::view::time::timestamp;
use ferropix_operator_api::configuration::{Configuration, Origin, Section};
use ferropix_operator_api::domain::{Domain, DomainPage, Domains, FeedOutcome};
use ferropix_operator_api::patient::{Channel, DomainKind};
use ferropix_operator_api::source::{Admission, Message, Source, SourcePage, Sources};

use crate::app::{DOMAINS, SOURCES};
use crate::locale::messages::Msg;
use crate::markup::{h, submit, t};
use crate::model::Read;
use crate::pages::{Needs, or_dash, server_state};
use crate::visit::Visit;

/// The book's page that says how FerroPIX is configured.
pub const CONFIGURATION_BOOK: &str = "https://ferropix.eu/docs/operate/configuration.html";

/// The words of a channel.
#[must_use]
pub fn channel(visit: &Visit, channel: Channel) -> String {
    visit.tr(match channel {
        Channel::Fhir => Msg::ChannelFhir,
        Channel::V2 => Msg::ChannelV2,
        Channel::Pmir => Msg::ChannelPmir,
    })
}

/// The words of a domain kind.
#[must_use]
pub fn kind(visit: &Visit, kind: DomainKind) -> String {
    visit.tr(match kind {
        DomainKind::Local => Msg::KindLocal,
        DomainKind::Authoritative => Msg::KindAuthoritative,
        DomainKind::Master => Msg::KindMaster,
    })
}

/// A count as the catalog formats it.
fn number(visit: &Visit, count: u64) -> String {
    visit.tr_with(Msg::Count, &[("count", count.into())])
}

/// A time, or a dash when there is none.
fn when(visit: &Visit, at: Option<&String>) -> Html {
    at.map_or_else(
        || or_dash(&visit.locale, None),
        |at| timestamp(&visit.locale, at),
    )
}

/// `text` in the code face, wrapping where it is long.
fn code_wrap(text: &str) -> Html {
    h(format!("<code class=\"fui-wrap\">{}</code>", t(text)))
}

/// `text` in the code face.
fn code(text: &str) -> Html {
    h(format!("<code>{}</code>", t(text)))
}

/// The list card titled `title` holding `body`, counting `count`.
fn listed(visit: &Visit, title: Msg, count: usize, body: Html) -> Html {
    list_card(
        &visit.locale,
        ListCard {
            title: visit.tr(title),
            count: Some(u64::try_from(count).unwrap_or(u64::MAX).into()),
            order: None,
            tabs: None,
            actions: None,
            body,
            pager: None,
        },
    )
}

/// The table of `sources`.
#[must_use]
pub fn sources(visit: &Visit, sources: &Sources) -> Html {
    if sources.sources.is_empty() {
        return listed(
            visit,
            Msg::Sources,
            0,
            list_empty(visit.tr(Msg::SourcesEmpty), None),
        );
    }
    let columns = [
        Column::text("name", visit.tr(Msg::ColName)).role(Role::Title),
        Column::text("channel", visit.tr(Msg::ColChannel)),
        Column::text("admission", visit.tr(Msg::ColAdmission)).role(Role::Sub),
        Column::text("domains", visit.tr(Msg::ColDomains)).role(Role::Wide),
        Column::text("activity", visit.tr(Msg::ColLastActivity)).role(Role::Quiet),
    ];
    let [
        _,
        channel_column,
        admission_column,
        domains_column,
        activity_column,
    ] = &columns;
    let rows: Html = sources
        .sources
        .iter()
        .map(|source| {
            h(format!(
                "<tr>{}{}{}{}{}</tr>",
                title_cell(
                    Some(&format!("{SOURCES}/{}", source.id)),
                    t(&source.name),
                    None
                ),
                cell(channel_column, t(channel(visit, source.channel))),
                cell(admission_column, t(admission(visit, &source.admission))),
                cell(domains_column, domain_list(visit, &source.domains)),
                cell(activity_column, when(visit, source.last_activity.as_ref())),
            ))
        })
        .collect();
    let caption = visit.tr(Msg::Sources);
    let table = data_table(DataTable {
        caption: &caption,
        columns: &columns,
        sorting: Sorting::default(),
        body: Body::Rows(rows),
    });
    let file = sources.file.as_ref().map_or_else(Html::empty, |file| {
        h(format!(
            "<p class=\"fui-small fui-muted\">{}</p>",
            file_line(
                &visit.locale,
                file,
                How::Page(String::from(CONFIGURATION_BOOK))
            )
        ))
    });
    stack(listed(visit, Msg::Sources, sources.sources.len(), table).then(file))
}

/// The words of how a source is admitted.
fn admission(visit: &Visit, admission: &Admission) -> String {
    match admission {
        Admission::IdentityProvider { issuer } => {
            visit.tr_with(Msg::AdmissionIssuer, &[("issuer", issuer.clone().into())])
        }
        Admission::BasicUser => visit.tr(Msg::AdmissionBasic),
        Admission::RegisteredSender {
            application,
            facility,
        } => visit.tr_with(
            Msg::AdmissionSender,
            &[
                ("application", application.clone().into()),
                ("facility", facility.clone().into()),
            ],
        ),
    }
}

/// The systems of `domains`, each as code, or a dash.
fn domain_list(visit: &Visit, domains: &[String]) -> Html {
    if domains.is_empty() {
        return or_dash(&visit.locale, None);
    }
    domains
        .iter()
        .map(|system| code_wrap(system).then(t(" ")))
        .collect()
}

/// The page of one source: its definition and its last messages.
#[must_use]
pub fn source(visit: &Visit, page: &SourcePage) -> Html {
    let source: &Source = &page.source;
    let facts = definitions(vec![
        (visit.tr(Msg::ColChannel), t(channel(visit, source.channel))),
        (
            visit.tr(Msg::ColAdmission),
            t(admission(visit, &source.admission)),
        ),
        (
            visit.tr(Msg::ColDomains),
            domain_list(visit, &source.domains),
        ),
        (
            visit.tr(Msg::ColLastActivity),
            when(visit, source.last_activity.as_ref()),
        ),
    ]);
    stack(card(visit.tr(Msg::SourceDefinition), facts).then(messages(visit, &page.messages)))
}

/// The table of a source's last messages.
fn messages(visit: &Visit, messages: &[Message]) -> Html {
    if messages.is_empty() {
        return listed(
            visit,
            Msg::LastMessages,
            0,
            list_empty(visit.tr(Msg::MessagesEmpty), None),
        );
    }
    let columns = [
        Column::text("at", visit.tr(Msg::ColTime)).role(Role::Title),
        Column::text("channel", visit.tr(Msg::ColChannel)),
        Column::text("action", visit.tr(Msg::ColAction)).role(Role::Sub),
        Column::text("outcome", visit.tr(Msg::ColOutcome)),
    ];
    let [_, channel_column, action_column, outcome_column] = &columns;
    let rows: Html = messages
        .iter()
        .map(|message| {
            h(format!(
                "<tr>{}{}{}{}</tr>",
                title_cell(None, timestamp(&visit.locale, &message.at), None),
                cell(channel_column, t(channel(visit, message.channel))),
                cell(
                    action_column,
                    t(crate::pages::action_words(&visit.locale, &message.action))
                ),
                cell(outcome_column, outcome(&message.outcome)),
            ))
        })
        .collect();
    let caption = visit.tr(Msg::LastMessages);
    listed(
        visit,
        Msg::LastMessages,
        messages.len(),
        data_table(DataTable {
            caption: &caption,
            columns: &columns,
            sorting: Sorting::default(),
            body: Body::Rows(rows),
        }),
    )
}

/// A message's outcome as a badge in its tone.
fn outcome(outcome: &str) -> Html {
    let tone = match outcome {
        "applied" | "AA" => Tone::Ok,
        "AE" => Tone::Warn,
        "AR" => Tone::Bad,
        _ => Tone::Neutral,
    };
    badge(tone, outcome.to_owned())
}

/// The outcome of a domain write just posted: saved, or the server's
/// refusal with its issues.
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
            "POST /operator/domains",
        ),
    }
}

/// A text field of a domain form.
fn domain_field(visit: &Visit, id: &'static str, words: Msg, required: bool) -> Html {
    Field::new(id, visit.tr(words)).view(h(format!(
        "<input id=\"{id}\" name=\"{id}\" type=\"text\" class=\"{INPUT}\" autocomplete=\"off\"{}>",
        if required { " required" } else { "" }
    )))
}

/// The note drawn in place of an administrator's form for a steward the
/// provider did not grant the administrator scope.
fn admin_only(visit: &Visit) -> Html {
    h(format!(
        "<p class=\"fui-help\">{}</p>",
        t(visit.tr(Msg::DomainAdminOnly))
    ))
}

/// The checkbox `id`, named as its id, posting `on`, checked when
/// `checked`, described by its help line.
fn checkbox(id: &str, checked: bool) -> Html {
    h(format!(
        "<input id=\"{id}\" name=\"{id}\" type=\"checkbox\" value=\"on\"{} aria-describedby=\"{}\">",
        if checked { " checked" } else { "" },
        t(help_id(id))
    ))
}

/// The administrator's form that creates a domain.
fn create_form(visit: &Visit) -> Html {
    let kinds: Html = [
        ("local", Msg::KindLocal),
        ("authoritative", Msg::KindAuthoritative),
    ]
    .into_iter()
    .map(|(value, words)| {
        h(format!(
            "<option value=\"{value}\">{}</option>",
            t(visit.tr(words))
        ))
    })
    .collect();
    let kind = Field::new("kind", visit.tr(Msg::ColKind)).view(h(format!(
        "<select id=\"kind\" name=\"kind\" class=\"{SELECT}\">{kinds}</select>"
    )));
    let sensitive = check_field(
        "sensitive",
        visit.tr(Msg::DomainSensitive),
        visit.tr(Msg::DomainSensitiveHelp),
        checkbox("sensitive", true),
    );
    disclosure(
        visit.tr(Msg::DomainCreate),
        h(format!(
            "<form method=\"post\" action=\"{DOMAINS}\" class=\"fui-form\"><p class=\"fui-help\">{}</p><div class=\"fui-form-grid\">{}{}{kind}{}{}</div>{sensitive}<div class=\"fui-form-foot\">{}</div></form>",
            t(visit.tr(Msg::DomainAdminNote)),
            domain_field(visit, "system", Msg::ColSystem, true),
            domain_field(visit, "name", Msg::ColName, true),
            domain_field(visit, "source", Msg::ColSource, true),
            domain_field(visit, "reason", Msg::RevealReason, true),
            submit(
                Variant::Primary.class(),
                &label(Some(Glyph::PLUS), visit.tr(Msg::DomainCreate))
            ),
        )),
    )
}

/// The administrator's form that renames a domain, gives it another
/// Source, or retires it.
fn change_form(visit: &Visit, domain: &Domain) -> Html {
    let retire = check_field(
        "retire",
        visit.tr(Msg::DomainRetire),
        visit.tr(Msg::DomainRetireHelp),
        checkbox("retire", false),
    );
    disclosure(
        visit.tr(Msg::DomainChange),
        h(format!(
            "<form method=\"post\" action=\"{}\" class=\"fui-form\"><p class=\"fui-help\">{}</p><div class=\"fui-form-grid\">{}{}{}</div>{retire}<div class=\"fui-form-foot\">{}</div></form>",
            t(format!("{DOMAINS}/{}", domain.id)),
            t(visit.tr(Msg::DomainAdminNote)),
            domain_field(visit, "name", Msg::ColName, false),
            domain_field(visit, "source", Msg::ColSource, false),
            domain_field(visit, "reason", Msg::RevealReason, true),
            submit(
                Variant::Primary.class(),
                &label(Some(Glyph::CHECK), visit.tr(Msg::DomainChange))
            ),
        )),
    )
}

/// The table of identifier domains, the create form, and the outcome of a
/// create just posted.
#[must_use]
pub fn domains(visit: &Visit, domains: &Domains, written: Option<&Read<()>>, admin: bool) -> Html {
    let editor = if admin {
        create_form(visit)
    } else {
        admin_only(visit)
    };
    stack(Html::join([
        written_notice(visit, written),
        domain_table(visit, domains),
        editor,
    ]))
}

/// Whether a domain is sealed, in words.
fn sealed(visit: &Visit, sensitive: bool) -> String {
    visit.tr(if sensitive { Msg::Sealed } else { Msg::Plain })
}

/// The table of identifier domains.
fn domain_table(visit: &Visit, domains: &Domains) -> Html {
    if domains.domains.is_empty() {
        return listed(
            visit,
            Msg::Domains,
            0,
            list_empty(visit.tr(Msg::DomainsEmpty), None),
        );
    }
    let columns = [
        Column::text("name", visit.tr(Msg::ColName)).role(Role::Title),
        Column::text("system", visit.tr(Msg::ColSystem)).role(Role::Sub),
        Column::text("kind", visit.tr(Msg::ColKind)),
        Column::text("sensitive", visit.tr(Msg::ColSensitive)).role(Role::Wide),
        Column::text("source", visit.tr(Msg::ColSource)).role(Role::Wide),
        Column::text("records", visit.tr(Msg::ColRecords)).numeric(),
        Column::text("fed", visit.tr(Msg::ColLastFed)).role(Role::Quiet),
        Column::text("state", visit.tr(Msg::ColState)),
    ];
    let [
        _,
        system,
        kind_column,
        sensitive,
        source_column,
        records,
        fed,
        state,
    ] = &columns;
    let rows: Html = domains
        .domains
        .iter()
        .map(|domain| {
            h(format!(
                "<tr>{}{}{}{}{}{}{}{}</tr>",
                title_cell(
                    Some(&format!("{DOMAINS}/{}", domain.id)),
                    t(&domain.name),
                    None
                ),
                cell(system, code_wrap(&domain.system)),
                cell(kind_column, t(kind(visit, domain.kind))),
                cell(sensitive, t(sealed(visit, domain.sensitive))),
                cell(source_column, code(&domain.source_client_id)),
                cell(records, t(number(visit, domain.records))),
                cell(fed, when(visit, domain.last_fed.as_ref())),
                cell(state, domain_state(visit, domain)),
            ))
        })
        .collect();
    let caption = visit.tr(Msg::Domains);
    listed(
        visit,
        Msg::Domains,
        domains.domains.len(),
        data_table(DataTable {
            caption: &caption,
            columns: &columns,
            sorting: Sorting::default(),
            body: Body::Rows(rows),
        }),
    )
}

/// A domain's state as a badge.
fn domain_state(visit: &Visit, domain: &Domain) -> Html {
    if domain.retired_at.is_some() {
        badge(Tone::Neutral, visit.tr(Msg::Retired))
    } else {
        badge(Tone::Ok, visit.tr(Msg::StateActive))
    }
}

/// The page of one domain: its definition, its last feeds, the change form
/// while it is not retired, and the outcome of a change just posted.
#[must_use]
pub fn domain(visit: &Visit, page: &DomainPage, written: Option<&Read<()>>, admin: bool) -> Html {
    let domain = &page.domain;
    let facts = definitions(vec![
        (visit.tr(Msg::ColSystem), code_wrap(&domain.system)),
        (visit.tr(Msg::ColKind), t(kind(visit, domain.kind))),
        (
            visit.tr(Msg::ColSensitive),
            t(sealed(visit, domain.sensitive)),
        ),
        (visit.tr(Msg::ColSource), code(&domain.source_client_id)),
        (visit.tr(Msg::ColRecords), t(number(visit, domain.records))),
        (
            visit.tr(Msg::ColCreated),
            timestamp(&visit.locale, &domain.created_at),
        ),
        (visit.tr(Msg::ColState), domain_state(visit, domain)),
    ]);
    let columns = [
        Column::text("at", visit.tr(Msg::ColTime)).role(Role::Title),
        Column::text("channel", visit.tr(Msg::ColChannel)),
        Column::text("outcome", visit.tr(Msg::ColOutcome)),
    ];
    let [_, channel_column, outcome_column] = &columns;
    let feeds = if page.feeds.is_empty() {
        list_empty(visit.tr(Msg::FeedsEmpty), None)
    } else {
        let rows: Html = page
            .feeds
            .iter()
            .map(|feed| {
                h(format!(
                    "<tr>{}{}{}</tr>",
                    title_cell(None, timestamp(&visit.locale, &feed.at), None),
                    cell(channel_column, t(channel(visit, feed.channel))),
                    cell(
                        outcome_column,
                        t(visit.tr(match feed.outcome {
                            FeedOutcome::Created => Msg::FeedCreated,
                            FeedOutcome::Revised => Msg::FeedRevised,
                        }))
                    ),
                ))
            })
            .collect();
        let caption = visit.tr(Msg::LastFeeds);
        data_table(DataTable {
            caption: &caption,
            columns: &columns,
            sorting: Sorting::default(),
            body: Body::Rows(rows),
        })
    };
    let change = if domain.retired_at.is_some() {
        Html::empty()
    } else if admin {
        change_form(visit, domain)
    } else {
        admin_only(visit)
    };
    stack(Html::join([
        written_notice(visit, written),
        card(visit.tr(Msg::DomainDefinition), facts),
        change,
        listed(visit, Msg::LastFeeds, page.feeds.len(), feeds),
    ]))
}

/// The configuration in file mode: each section a panel of setting rows,
/// each value marked where it does not come from the file.
#[must_use]
pub fn configuration(visit: &Visit, configuration: &Configuration) -> Html {
    let line = configuration.file.as_ref().map_or_else(
        || t(visit.tr(Msg::ConfigurationNoFile)),
        |file| {
            file_line(
                &visit.locale,
                file,
                How::Page(String::from(CONFIGURATION_BOOK)),
            )
        },
    );
    let panels: Html = configuration
        .sections
        .iter()
        .map(|section| card(section_title(visit, section), section_panel(visit, section)))
        .collect();
    stack(h(format!(
        "<p class=\"fui-small fui-muted\">{line}</p>{panels}"
    )))
}

/// The title of a configuration section.
fn section_title(visit: &Visit, section: &Section) -> String {
    visit.tr(match section.name.as_str() {
        "thresholds" => Msg::SectionThresholds,
        "weights" => Msg::SectionWeights,
        "blocking" => Msg::SectionBlocking,
        _ => Msg::SectionRules,
    })
}

/// The panel of one configuration section.
fn section_panel(visit: &Visit, section: &Section) -> Html {
    let intro = visit.tr(match section.name.as_str() {
        "thresholds" => Msg::SectionThresholdsIntro,
        "weights" => Msg::SectionWeightsIntro,
        "blocking" => Msg::SectionBlockingIntro,
        _ => Msg::SectionRulesIntro,
    });
    let rows = section
        .settings
        .iter()
        .map(|setting| {
            let built_in = setting.origin == Origin::BuiltIn;
            let source = match setting.origin {
                Origin::Environment => Some(ValueSource::Environment),
                Origin::Default => Some(ValueSource::Default),
                Origin::File | Origin::BuiltIn => None,
            };
            let label = setting_label(visit, &setting.key);
            setting_row(
                &visit.locale,
                Setting {
                    label,
                    description: if built_in {
                        visit.tr(Msg::BuiltInRule)
                    } else {
                        String::new()
                    },
                    value: if built_in {
                        t(rule_text(visit, &setting.key).unwrap_or_else(|| setting.value.clone()))
                    } else {
                        code(&setting.value)
                    },
                    keys: if built_in {
                        Vec::new()
                    } else {
                        vec![setting.key.clone()]
                    },
                    next_start: false,
                    source,
                },
                false,
            )
        })
        .collect();
    settings_panel(t(intro), None, rows)
}

/// The words of the setting `key` of the configuration document, or the key
/// itself for one the console does not know.
fn setting_label(visit: &Visit, key: &str) -> String {
    let words = match key {
        "matching.link_at" => Msg::SettingLinkAt,
        "matching.review_at" => Msg::SettingReviewAt,
        "matching.possible_at" => Msg::SettingPossibleAt,
        "matching.auto_link" => Msg::SettingAutoLink,
        "matching.blocking_keys" => Msg::SettingBlockingKeys,
        "deterministic.authoritative_identifier" => Msg::RuleAuthoritative,
        "deterministic.hard_conflict" => Msg::RuleHardConflict,
        "deterministic.local_domains" => Msg::RuleLocalDomains,
        _ => return weight_label(visit, key).unwrap_or_else(|| key.to_owned()),
    };
    visit.tr(words)
}

/// The words of a field weight `matching.fields.<field>.<m|u>`, when `key`
/// is one.
fn weight_label(visit: &Visit, key: &str) -> Option<String> {
    let rest = key.strip_prefix("matching.fields.")?;
    let (field, weight) = rest.rsplit_once('.')?;
    let field = visit.tr(match field {
        "family_name" => Msg::FieldFamilyName,
        "given_names" => Msg::FieldGivenNames,
        "birth_date" => Msg::FieldBirthDate,
        "gender" => Msg::FieldGender,
        "postal_code" => Msg::FieldPostalCode,
        "address" => Msg::FieldAddress,
        "telephone" => Msg::FieldTelephone,
        "email" => Msg::FieldEmail,
        "mothers_maiden_name" => Msg::FieldMothersMaidenName,
        "local_identifier" => Msg::FieldLocalIdentifier,
        _ => return None,
    });
    let words = match weight {
        "m" => Msg::SettingAgreement,
        "u" => Msg::SettingChance,
        _ => return None,
    };
    Some(visit.tr_with(words, &[("field", field.into())]))
}

/// What the built-in rule `key` holds, in the page's language, when the
/// console knows the rule.
fn rule_text(visit: &Visit, key: &str) -> Option<String> {
    let words = match key {
        "deterministic.authoritative_identifier" => Msg::RuleAuthoritativeText,
        "deterministic.hard_conflict" => Msg::RuleHardConflictText,
        "deterministic.local_domains" => Msg::RuleLocalDomainsText,
        _ => return None,
    };
    Some(visit.tr(words))
}
