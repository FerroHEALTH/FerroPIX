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

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::table::view::{
    Body, Column, DataTable, Role, Sorting, cell, data_table, title_cell,
};
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::form::{Field, INPUT, SELECT, check_field, help_id};
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::status::badge;
use ferrohealth_ui::view::surface::{card, definitions, disclosure, or_none, stack};
use ferrohealth_ui::view::template::list::{ListCard, list_card, list_empty};
use ferrohealth_ui::view::template::settings::{
    How, Setting, ValueSource, file_line, setting_row, settings_panel,
};
use ferrohealth_ui::view::time::timestamp;
use ferropix_operator_api::configuration::{Configuration, Origin, Section};
use ferropix_operator_api::domain::{Domain, DomainPage, Domains, FeedOutcome};
use ferropix_operator_api::patient::{Channel, DomainKind};
use ferropix_operator_api::source::{Admission, Message, Source, SourcePage, Sources};
use leptos::prelude::*;

use crate::app::{DOMAINS, SOURCES};
use crate::document::Read;
use crate::locale::{Msg, tr, tr_with};
use crate::pages::{Needs, server_state};

/// The book's page that says how FerroPIX is configured.
pub const CONFIGURATION_BOOK: &str = "https://ferropix.eu/docs/operate/configuration.html";

/// The words of a channel.
#[must_use]
pub fn channel(channel: Channel) -> String {
    tr(match channel {
        Channel::Fhir => Msg::ChannelFhir,
        Channel::V2 => Msg::ChannelV2,
        Channel::Pmir => Msg::ChannelPmir,
    })
}

/// The words of a domain kind.
#[must_use]
pub fn kind(kind: DomainKind) -> String {
    tr(match kind {
        DomainKind::Local => Msg::KindLocal,
        DomainKind::Authoritative => Msg::KindAuthoritative,
        DomainKind::Master => Msg::KindMaster,
    })
}

/// A count as the catalog formats it.
fn number(count: u64) -> String {
    tr_with(Msg::Count, &[("count", count.into())])
}

/// A time, or a dash when there is none.
fn when(at: Option<&String>) -> AnyView {
    at.map_or_else(|| or_none(String::new()), |at| timestamp(at).into_any())
}

/// The table of `sources`.
#[must_use]
pub fn sources(sources: &Sources) -> AnyView {
    if sources.sources.is_empty() {
        return list_card(ListCard {
            title: tr(Msg::Sources),
            count: Some(0_u64.into()),
            order: None,
            tabs: None,
            actions: None,
            body: list_empty(tr(Msg::SourcesEmpty), None),
            pager: None,
        });
    }
    let columns = [
        Column::text("name", tr(Msg::ColName)).role(Role::Title),
        Column::text("channel", tr(Msg::ColChannel)),
        Column::text("admission", tr(Msg::ColAdmission)).role(Role::Sub),
        Column::text("domains", tr(Msg::ColDomains)).role(Role::Wide),
        Column::text("activity", tr(Msg::ColLastActivity)).role(Role::Quiet),
    ];
    let [
        _,
        channel_column,
        admission_column,
        domains_column,
        activity_column,
    ] = &columns;
    let rows = sources
        .sources
        .iter()
        .map(|source| {
            view! {
                <tr>
                    {title_cell(Some(&format!("{SOURCES}/{}", source.id)), source.name.clone(), None)}
                    {cell(channel_column, channel(source.channel))}
                    {cell(admission_column, admission(&source.admission))}
                    {cell(domains_column, domain_list(&source.domains))}
                    {cell(activity_column, when(source.last_activity.as_ref()))}
                </tr>
            }
        })
        .collect_view()
        .into_any();
    let table = data_table(DataTable {
        caption: &tr(Msg::Sources),
        columns: &columns,
        sorting: Sorting::default(),
        body: Body::Rows(rows),
    })
    .into_any();
    let file = sources.file.clone().map(|file| {
        view! { <p class="fui-small fui-muted">{file_line(file, How::Page(String::from(CONFIGURATION_BOOK)))}</p> }
    });
    stack(view! {
        {list_card(ListCard {
            title: tr(Msg::Sources),
            count: Some(u64::try_from(sources.sources.len()).unwrap_or(u64::MAX).into()),
            order: None,
            tabs: None,
            actions: None,
            body: table,
            pager: None,
        })}
        {file}
    })
    .into_any()
}

/// The words of how a source is admitted.
fn admission(admission: &Admission) -> String {
    match admission {
        Admission::IdentityProvider { issuer } => {
            tr_with(Msg::AdmissionIssuer, &[("issuer", issuer.clone().into())])
        }
        Admission::BasicUser => tr(Msg::AdmissionBasic),
        Admission::RegisteredSender {
            application,
            facility,
        } => tr_with(
            Msg::AdmissionSender,
            &[
                ("application", application.clone().into()),
                ("facility", facility.clone().into()),
            ],
        ),
    }
}

/// The systems of `domains`, each as code, or a dash.
fn domain_list(domains: &[String]) -> AnyView {
    if domains.is_empty() {
        return or_none(String::new());
    }
    domains
        .iter()
        .map(|system| view! { <code class="fui-wrap">{system.clone()}</code> " " })
        .collect_view()
        .into_any()
}

/// The page of one source: its definition and its last messages.
#[must_use]
pub fn source(page: &SourcePage) -> AnyView {
    let source: &Source = &page.source;
    let facts = definitions(vec![
        (tr(Msg::ColChannel), channel(source.channel).into_any()),
        (
            tr(Msg::ColAdmission),
            admission(&source.admission).into_any(),
        ),
        (tr(Msg::ColDomains), domain_list(&source.domains)),
        (
            tr(Msg::ColLastActivity),
            when(source.last_activity.as_ref()),
        ),
    ]);
    stack(view! {
        {card(tr(Msg::SourceDefinition), facts)}
        {messages(&page.messages)}
    })
    .into_any()
}

/// The table of a source's last messages.
fn messages(messages: &[Message]) -> AnyView {
    if messages.is_empty() {
        return list_card(ListCard {
            title: tr(Msg::LastMessages),
            count: Some(0_u64.into()),
            order: None,
            tabs: None,
            actions: None,
            body: list_empty(tr(Msg::MessagesEmpty), None),
            pager: None,
        });
    }
    let columns = [
        Column::text("at", tr(Msg::ColTime)).role(Role::Title),
        Column::text("channel", tr(Msg::ColChannel)),
        Column::text("action", tr(Msg::ColAction)).role(Role::Sub),
        Column::text("outcome", tr(Msg::ColOutcome)),
    ];
    let [_, channel_column, action_column, outcome_column] = &columns;
    let rows = messages
        .iter()
        .map(|message| {
            view! {
                <tr>
                    {title_cell(None, timestamp(&message.at), None)}
                    {cell(channel_column, channel(message.channel))}
                    {cell(action_column, crate::pages::action_words(&message.action))}
                    {cell(outcome_column, outcome(&message.outcome))}
                </tr>
            }
        })
        .collect_view()
        .into_any();
    list_card(ListCard {
        title: tr(Msg::LastMessages),
        count: Some(u64::try_from(messages.len()).unwrap_or(u64::MAX).into()),
        order: None,
        tabs: None,
        actions: None,
        body: data_table(DataTable {
            caption: &tr(Msg::LastMessages),
            columns: &columns,
            sorting: Sorting::default(),
            body: Body::Rows(rows),
        })
        .into_any(),
        pager: None,
    })
}

/// A message's outcome as a badge in its tone.
fn outcome(outcome: &str) -> AnyView {
    let tone = match outcome {
        "applied" | "AA" => Tone::Ok,
        "AE" => Tone::Warn,
        "AR" => Tone::Bad,
        _ => Tone::Neutral,
    };
    badge(tone, outcome.to_owned()).into_any()
}

/// The outcome of a domain write just posted: saved, or the server's
/// refusal with its issues.
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
            "POST /operator/domains",
        ),
    })
}

/// A text field of a domain form.
fn domain_field(id: &'static str, words: Msg, required: bool) -> AnyView {
    Field::new(id, tr(words)).view(view! {
        <input id=id name=id type="text" class=INPUT autocomplete="off" required=required />
    })
}

/// The note drawn in place of an administrator's form for a steward the
/// provider did not grant the administrator scope.
fn admin_only() -> AnyView {
    view! { <p class="fui-help">{tr(Msg::DomainAdminOnly)}</p> }.into_any()
}

/// The administrator's form that creates a domain.
fn create_form() -> AnyView {
    let kinds = [
        ("local", Msg::KindLocal),
        ("authoritative", Msg::KindAuthoritative),
    ]
    .into_iter()
    .map(|(value, words)| view! { <option value=value>{tr(words)}</option> })
    .collect_view();
    disclosure(
        tr(Msg::DomainCreate),
        view! {
            <form method="post" action=DOMAINS class="fui-form">
                <p class="fui-help">{tr(Msg::DomainAdminNote)}</p>
                <div class="fui-form-grid">
                    {domain_field("system", Msg::ColSystem, true)}
                    {domain_field("name", Msg::ColName, true)}
                    {Field::new("kind", tr(Msg::ColKind)).view(view! {
                        <select id="kind" name="kind" class=SELECT>{kinds}</select>
                    })}
                    {domain_field("source", Msg::ColSource, true)}
                    {domain_field("reason", Msg::RevealReason, true)}
                </div>
                {check_field(
                    "sensitive",
                    tr(Msg::DomainSensitive),
                    tr(Msg::DomainSensitiveHelp),
                    view! { <input id="sensitive" name="sensitive" type="checkbox" value="on" checked aria-describedby=help_id("sensitive") /> },
                )}
                <div class="fui-form-foot">
                    <button type="submit" class=Variant::Primary.class()>{label(Some(Glyph::PLUS), tr(Msg::DomainCreate))}</button>
                </div>
            </form>
        },
    )
    .into_any()
}

/// The administrator's form that renames a domain, gives it another
/// Source, or retires it.
fn change_form(domain: &Domain) -> AnyView {
    disclosure(
        tr(Msg::DomainChange),
        view! {
            <form method="post" action=format!("{DOMAINS}/{}", domain.id) class="fui-form">
                <p class="fui-help">{tr(Msg::DomainAdminNote)}</p>
                <div class="fui-form-grid">
                    {domain_field("name", Msg::ColName, false)}
                    {domain_field("source", Msg::ColSource, false)}
                    {domain_field("reason", Msg::RevealReason, true)}
                </div>
                {check_field(
                    "retire",
                    tr(Msg::DomainRetire),
                    tr(Msg::DomainRetireHelp),
                    view! { <input id="retire" name="retire" type="checkbox" value="on" aria-describedby=help_id("retire") /> },
                )}
                <div class="fui-form-foot">
                    <button type="submit" class=Variant::Primary.class()>{label(Some(Glyph::CHECK), tr(Msg::DomainChange))}</button>
                </div>
            </form>
        },
    )
    .into_any()
}

/// The table of identifier domains, the create form, and the outcome of a
/// create just posted.
#[must_use]
pub fn domains(domains: &Domains, written: Option<&Read<()>>, admin: bool) -> AnyView {
    let editor = if admin { create_form() } else { admin_only() };
    stack(view! {
        {written_notice(written)}
        {domain_table(domains)}
        {editor}
    })
    .into_any()
}

/// The table of identifier domains.
fn domain_table(domains: &Domains) -> AnyView {
    if domains.domains.is_empty() {
        return list_card(ListCard {
            title: tr(Msg::Domains),
            count: Some(0_u64.into()),
            order: None,
            tabs: None,
            actions: None,
            body: list_empty(tr(Msg::DomainsEmpty), None),
            pager: None,
        });
    }
    let columns = [
        Column::text("name", tr(Msg::ColName)).role(Role::Title),
        Column::text("system", tr(Msg::ColSystem)).role(Role::Sub),
        Column::text("kind", tr(Msg::ColKind)),
        Column::text("sensitive", tr(Msg::ColSensitive)).role(Role::Wide),
        Column::text("source", tr(Msg::ColSource)).role(Role::Wide),
        Column::text("records", tr(Msg::ColRecords)).numeric(),
        Column::text("fed", tr(Msg::ColLastFed)).role(Role::Quiet),
        Column::text("state", tr(Msg::ColState)),
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
    let rows = domains
        .domains
        .iter()
        .map(|domain| {
            view! {
                <tr>
                    {title_cell(Some(&format!("{DOMAINS}/{}", domain.id)), domain.name.clone(), None)}
                    {cell(system, view! { <code class="fui-wrap">{domain.system.clone()}</code> })}
                    {cell(kind_column, kind(domain.kind))}
                    {cell(sensitive, tr(if domain.sensitive { Msg::Sealed } else { Msg::Plain }))}
                    {cell(source_column, view! { <code>{domain.source_client_id.clone()}</code> })}
                    {cell(records, number(domain.records))}
                    {cell(fed, when(domain.last_fed.as_ref()))}
                    {cell(state, domain_state(domain))}
                </tr>
            }
        })
        .collect_view()
        .into_any();
    list_card(ListCard {
        title: tr(Msg::Domains),
        count: Some(
            u64::try_from(domains.domains.len())
                .unwrap_or(u64::MAX)
                .into(),
        ),
        order: None,
        tabs: None,
        actions: None,
        body: data_table(DataTable {
            caption: &tr(Msg::Domains),
            columns: &columns,
            sorting: Sorting::default(),
            body: Body::Rows(rows),
        })
        .into_any(),
        pager: None,
    })
}

/// A domain's state as a badge.
fn domain_state(domain: &Domain) -> AnyView {
    if domain.retired_at.is_some() {
        badge(Tone::Neutral, tr(Msg::Retired)).into_any()
    } else {
        badge(Tone::Ok, tr(Msg::Active)).into_any()
    }
}

/// The page of one domain: its definition, its last feeds, the change form
/// while it is not retired, and the outcome of a change just posted.
#[must_use]
pub fn domain(page: &DomainPage, written: Option<&Read<()>>, admin: bool) -> AnyView {
    let domain = &page.domain;
    let facts = definitions(vec![
        (
            tr(Msg::ColSystem),
            view! { <code class="fui-wrap">{domain.system.clone()}</code> }.into_any(),
        ),
        (tr(Msg::ColKind), kind(domain.kind).into_any()),
        (
            tr(Msg::ColSensitive),
            tr(if domain.sensitive {
                Msg::Sealed
            } else {
                Msg::Plain
            })
            .into_any(),
        ),
        (
            tr(Msg::ColSource),
            view! { <code>{domain.source_client_id.clone()}</code> }.into_any(),
        ),
        (tr(Msg::ColRecords), number(domain.records).into_any()),
        (
            tr(Msg::ColCreated),
            timestamp(&domain.created_at).into_any(),
        ),
        (tr(Msg::ColState), domain_state(domain)),
    ]);
    let columns = [
        Column::text("at", tr(Msg::ColTime)).role(Role::Title),
        Column::text("channel", tr(Msg::ColChannel)),
        Column::text("outcome", tr(Msg::ColOutcome)),
    ];
    let [_, channel_column, outcome_column] = &columns;
    let feeds = if page.feeds.is_empty() {
        list_empty(tr(Msg::FeedsEmpty), None)
    } else {
        let rows = page
            .feeds
            .iter()
            .map(|feed| {
                view! {
                    <tr>
                        {title_cell(None, timestamp(&feed.at), None)}
                        {cell(channel_column, channel(feed.channel))}
                        {cell(outcome_column, tr(match feed.outcome { FeedOutcome::Created => Msg::FeedCreated, FeedOutcome::Revised => Msg::FeedRevised }))}
                    </tr>
                }
            })
            .collect_view()
            .into_any();
        data_table(DataTable {
            caption: &tr(Msg::LastFeeds),
            columns: &columns,
            sorting: Sorting::default(),
            body: Body::Rows(rows),
        })
        .into_any()
    };
    let change = domain.retired_at.is_none().then(|| {
        if admin {
            change_form(domain)
        } else {
            admin_only()
        }
    });
    stack(view! {
        {written_notice(written)}
        {card(tr(Msg::DomainDefinition), facts)}
        {change}
        {list_card(ListCard {
            title: tr(Msg::LastFeeds),
            count: Some(u64::try_from(page.feeds.len()).unwrap_or(u64::MAX).into()),
            order: None,
            tabs: None,
            actions: None,
            body: feeds,
            pager: None,
        })}
    })
    .into_any()
}

/// The configuration in file mode: each section a panel of setting rows,
/// each value marked where it does not come from the file.
#[must_use]
pub fn configuration(configuration: &Configuration) -> AnyView {
    let line = configuration.file.clone().map_or_else(
        || tr(Msg::ConfigurationNoFile).into_any(),
        |file| file_line(file, How::Page(String::from(CONFIGURATION_BOOK))),
    );
    let panels = configuration
        .sections
        .iter()
        .map(|section| card(section_title(section), section_panel(section)))
        .collect_view();
    stack(view! {
        <p class="fui-small fui-muted">{line}</p>
        {panels}
    })
    .into_any()
}

/// The title of a configuration section.
fn section_title(section: &Section) -> String {
    tr(match section.name.as_str() {
        "thresholds" => Msg::SectionThresholds,
        "weights" => Msg::SectionWeights,
        "blocking" => Msg::SectionBlocking,
        _ => Msg::SectionRules,
    })
}

/// The panel of one configuration section.
fn section_panel(section: &Section) -> AnyView {
    let intro = tr(match section.name.as_str() {
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
            let label = setting_label(&setting.key);
            setting_row(
                Setting {
                    label,
                    description: if built_in {
                        tr(Msg::BuiltInRule)
                    } else {
                        String::new()
                    },
                    value: if built_in {
                        rule_text(&setting.key)
                            .unwrap_or_else(|| setting.value.clone())
                            .into_any()
                    } else {
                        view! { <code>{setting.value.clone()}</code> }.into_any()
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
    settings_panel(intro.into_any(), None, rows)
}

/// The words of the setting `key` of the configuration document, or the key
/// itself for one the console does not know.
fn setting_label(key: &str) -> String {
    let words = match key {
        "matching.link_at" => Msg::SettingLinkAt,
        "matching.review_at" => Msg::SettingReviewAt,
        "matching.possible_at" => Msg::SettingPossibleAt,
        "matching.auto_link" => Msg::SettingAutoLink,
        "matching.blocking_keys" => Msg::SettingBlockingKeys,
        "deterministic.authoritative_identifier" => Msg::RuleAuthoritative,
        "deterministic.hard_conflict" => Msg::RuleHardConflict,
        "deterministic.local_domains" => Msg::RuleLocalDomains,
        _ => return weight_label(key).unwrap_or_else(|| key.to_owned()),
    };
    tr(words)
}

/// The words of a field weight `matching.fields.<field>.<m|u>`, when `key`
/// is one.
fn weight_label(key: &str) -> Option<String> {
    let rest = key.strip_prefix("matching.fields.")?;
    let (field, weight) = rest.rsplit_once('.')?;
    let field = tr(match field {
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
    Some(tr_with(words, &[("field", field.into())]))
}

/// What the built-in rule `key` holds, in the page's language, when the
/// console knows the rule.
fn rule_text(key: &str) -> Option<String> {
    let words = match key {
        "deterministic.authoritative_identifier" => Msg::RuleAuthoritativeText,
        "deterministic.hard_conflict" => Msg::RuleHardConflictText,
        "deterministic.local_domains" => Msg::RuleLocalDomainsText,
        _ => return None,
    };
    Some(tr(words))
}
