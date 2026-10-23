// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's own words, one Fluent catalog per language under `locales/`.
//!
//! Each catalog is embedded at build time and looked up in the locale the
//! request chose, so the console's words and the library's follow one
//! choice (`.claude/rules/leptos-ui.md`, FerroUI §13).
//!
//! English is the one catalog that ships and the reference every lookup
//! falls back to; a language joins as one directory with no code change.
//! No specification governs the catalog layout: our own design; the message
//! syntax is Project Fluent's.

use ferrohealth_ui::i18n::{Arg, Locale, locale};
use ferrohealth_ui::theme::Preferences;
use fluent_templates::Loader;

fluent_templates::static_loader! {
    static CATALOG = {
        locales: "./locales",
        fallback_language: "en",
        // NOTE: Fluent's guide, "Bidirectionality": the isolation marks around a
        // placeable are for mixed-direction text; the English catalog draws none.
        customise: |bundle| {
            bundle.set_use_isolating(false);
            bundle.set_formatter(Some(ferrohealth_ui::i18n::number::format_number));
        },
    };
}

/// The console's catalog, every language it holds.
#[must_use]
pub fn catalog() -> &'static (dyn Loader + Send + Sync) {
    &*CATALOG
}

/// Declares the console's messages: each variant names one message of the
/// catalog by its id.
macro_rules! messages {
    ($($(#[$meta:meta])+ $name:ident = $id:literal,)+) => {
        /// A string the console draws, by its message in the catalog.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Msg {
            $($(#[$meta])+ $name,)+
        }

        impl Msg {
            /// Every message, in declaration order.
            pub const ALL: &[Self] = &[$(Self::$name,)+];

            /// The message's id in the catalog.
            #[must_use]
            pub const fn id(self) -> &'static str {
                match self {
                    $(Self::$name => $id,)+
                }
            }
        }
    };
}

messages! {
    /// `FerroPIX`, the product.
    Product = "product",
    /// `Console`, the accented word of the lockup.
    ConsoleWord = "console-word",
    /// The console's name in the footer and the titles.
    ConsoleName = "console-name",
    /// The navigation's name.
    NavLabel = "nav-label",
    /// The Work group.
    NavWork = "nav-work",
    /// The Patients group.
    NavPatients = "nav-patients",
    /// The Registry group.
    NavRegistry = "nav-registry",
    /// The Trail group.
    NavTrail = "nav-trail",
    /// The Console group.
    NavConsole = "nav-console",
    /// Why the queue's depth is not known.
    QueueDepthUnknown = "queue-depth-unknown",
    /// The top bar's search field's name.
    SearchLabel = "search-label",
    /// The top bar's search field's placeholder.
    SearchPlaceholder = "search-placeholder",
    /// The top bar's search submit.
    SearchFind = "search-find",
    /// The status pill when the server answers.
    PillUp = "pill-up",
    /// The status pill when the server does not answer.
    PillUnreachable = "pill-unreachable",
    /// The status pill when the server refuses the console.
    PillRefused = "pill-refused",
    /// The status pill when the server answers with a fault.
    PillDegraded = "pill-degraded",
    /// The status pill's tooltip detail: the address the console reaches.
    PillDetail = "pill-detail",
    /// The user menu's line under the steward's name.
    MenuLine = "menu-line",
    /// The steward's name when the provider gave none.
    StewardUnnamed = "steward-unnamed",
    /// Dashboard.
    Dashboard = "dashboard",
    /// The dashboard's subtitle.
    DashboardSub = "dashboard-sub",
    /// Review queue.
    Queue = "queue",
    /// The queue's subtitle.
    QueueSub = "queue-sub",
    /// The compare page's title.
    Pair = "pair",
    /// The compare page's subtitle.
    PairSub = "pair-sub",
    /// Search.
    Search = "search",
    /// The search page's subtitle.
    SearchSub = "search-sub",
    /// Patient.
    Patient = "patient",
    /// The patient page's subtitle.
    PatientSub = "patient-sub",
    /// The merge page's title.
    Merge = "merge",
    /// The merge page's subtitle.
    MergeSub = "merge-sub",
    /// The unmerge page's title.
    Unmerge = "unmerge",
    /// The unmerge page's subtitle.
    UnmergeSub = "unmerge-sub",
    /// Sources.
    Sources = "sources",
    /// The sources page's subtitle.
    SourcesSub = "sources-sub",
    /// The source page's subtitle.
    SourceSub = "source-sub",
    /// Identifier domains.
    Domains = "domains",
    /// The domains page's subtitle.
    DomainsSub = "domains-sub",
    /// The domain page's subtitle.
    DomainSub = "domain-sub",
    /// Pseudonymisation domains.
    PseudonymDomains = "pseudonym-domains",
    /// The pseudonymisation domains page's subtitle.
    PseudonymDomainsSub = "pseudonym-domains-sub",
    /// Configuration.
    Configuration = "configuration",
    /// The configuration page's subtitle.
    ConfigurationSub = "configuration-sub",
    /// Audit trail.
    Audit = "audit",
    /// The audit trail's subtitle.
    AuditSub = "audit-sub",
    /// About.
    About = "about",
    /// The About page's subtitle.
    AboutSub = "about-sub",
    /// Your preferences.
    Preferences = "preferences",
    /// What the dashboard cannot show.
    WhatDashboard = "what-dashboard",
    /// What the queue cannot show.
    WhatQueue = "what-queue",
    /// What the compare page cannot show.
    WhatPair = "what-pair",
    /// What the patient page cannot show.
    WhatPatient = "what-patient",
    /// What the merge page cannot show.
    WhatMerge = "what-merge",
    /// What the unmerge page cannot show.
    WhatUnmerge = "what-unmerge",
    /// What the sources page cannot show.
    WhatSources = "what-sources",
    /// What the source page cannot show.
    WhatSource = "what-source",
    /// What the domains page cannot show.
    WhatDomains = "what-domains",
    /// What the domain page cannot show.
    WhatDomain = "what-domain",
    /// What the pseudonymisation domains page cannot show.
    WhatPseudonymDomains = "what-pseudonym-domains",
    /// What the configuration page cannot show.
    WhatConfiguration = "what-configuration",
    /// What the audit trail cannot show.
    WhatAudit = "what-audit",
    /// What a search cannot show.
    WhatSearch = "what-search",
    /// The refusal of a page whose server refused the connection.
    UnreachableConnect = "unreachable-connect",
    /// The refusal of a page whose server did not answer in time.
    UnreachableTimeout = "unreachable-timeout",
    /// The refusal of a page whose server could not be reached.
    UnreachableOther = "unreachable-other",
    /// The refusal of a page whose server refused the console's call.
    RefusedCall = "refused-call",
    /// The notice of a page whose server serves no operator API yet.
    NotServed = "not-served",
    /// The line under that notice: the route the page reads.
    NotServedRoute = "not-served-route",
    /// Try again.
    TryAgain = "try-again",
    /// When the console last called the server.
    LastTried = "last-tried",
    /// The card on the dashboard while the server is away.
    AwayTitle = "away-title",
    /// That card's sentence.
    AwayBody = "away-body",
    /// The technical detail disclosure.
    TechnicalDetail = "technical-detail",
    /// The failure row.
    DetailFailure = "detail-failure",
    /// The request id row.
    DetailRequestId = "detail-request-id",
    /// The time row.
    DetailTime = "detail-time",
    /// The route row.
    DetailRoute = "detail-route",
    /// The connection was refused.
    FailureConnect = "failure-connect",
    /// No answer in time.
    FailureTimeout = "failure-timeout",
    /// Another failure.
    FailureOther = "failure-other",
    /// The server answered with a status.
    FailureStatus = "failure-status",
    /// The person id, as the patient's subtitle names it.
    PersonId = "person-id",
    /// Find a patient, the search card's title.
    FindPatient = "find-patient",
    /// The search segments' name.
    SearchBy = "search-by",
    /// By identifier.
    ByIdentifier = "by-identifier",
    /// By demographics.
    ByDemographics = "by-demographics",
    /// Domain.
    Domain = "domain",
    /// The domain select while the domains are not known.
    DomainsNotKnown = "domains-not-known",
    /// Identifier.
    Identifier = "identifier",
    /// The identifier field's help line.
    IdentifierHelp = "identifier-help",
    /// Family name.
    FamilyName = "family-name",
    /// Given names.
    GivenNames = "given-names",
    /// Birth date.
    BirthDate = "birth-date",
    /// The birth date's format hint.
    BirthDateHint = "birth-date-hint",
    /// Gender.
    Gender = "gender",
    /// Any gender.
    GenderAny = "gender-any",
    /// Female.
    GenderFemale = "gender-female",
    /// Male.
    GenderMale = "gender-male",
    /// Other.
    GenderOther = "gender-other",
    /// Unknown.
    GenderUnknown = "gender-unknown",
    /// Postal code.
    PostalCode = "postal-code",
    /// Exact names only.
    ExactNames = "exact-names",
    /// The exact names check's help.
    ExactNamesHelp = "exact-names-help",
    /// The search form's submit.
    SearchSubmit = "search-submit",
    /// The line after a search that sent nothing.
    NothingSearched = "nothing-searched",
    /// The About page's console row.
    VersionConsole = "version-console",
    /// The About page's server row.
    VersionServer = "version-server",
    /// The About page's FHIR row.
    VersionFhir = "version-fhir",
    /// The About page's FerroUI row.
    VersionFerroui = "version-ferroui",
    /// A version the console does not know.
    NotKnown = "not-known",
    /// The licence.
    Licence = "licence",
    /// The documentation link.
    Documentation = "documentation",
    /// The `CapabilityStatement` card's title.
    CapabilityTitle = "capability-title",
    /// The `CapabilityStatement` card's line.
    CapabilityLine = "capability-line",
    /// What the About page cannot show without the server.
    WhatCapability = "what-capability",
    /// The reveal duration row.
    RevealFor = "reveal-for",
    /// The reveal duration row's description.
    RevealForDesc = "reveal-for-desc",
    /// A reveal duration choice.
    RevealSeconds = "reveal-seconds",
    /// The hide on leave row.
    HideOnLeave = "hide-on-leave",
    /// The hide on leave row's description.
    HideOnLeaveDesc = "hide-on-leave-desc",
    /// The hide on leave switch's name.
    HideOnLeaveSwitch = "hide-on-leave-switch",
    /// The theme cards' accent.
    AccentPlum = "accent-plum",
    /// The sign-in card's title.
    SignInTitle = "sign-in-title",
    /// The provider's button.
    SignInWith = "sign-in-with",
    /// The provider's button while it redirects.
    SignInTaking = "sign-in-taking",
    /// The sign-in card's caption.
    SignInCaption = "sign-in-caption",
    /// The notice after a sign-out.
    SignedOut = "signed-out",
    /// The notice after a session ended.
    SessionExpired = "session-expired",
    /// The notice when sign-in is not configured.
    SignInUnavailable = "sign-in-unavailable",
    /// The sign-in page's title.
    SignIn = "sign-in",
    /// The not-found page's title.
    NotFoundTitle = "not-found-title",
    /// The not-found page's body.
    NotFoundBody = "not-found-body",
    /// The not-found page's way back.
    NotFoundBack = "not-found-back",
    /// The not-found page's way to search.
    NotFoundSearch = "not-found-search",
    /// The answer when the session store cannot be read.
    SessionUnreadable = "session-unreadable",
    /// The answer when sign-in is not configured.
    SignInNotConfigured = "sign-in-not-configured",
    /// The answer when the sign-in redirect cannot be written.
    SignInRedirectUnwritten = "sign-in-redirect-unwritten",
    /// The answer when no sign-in is pending.
    NoSignInPending = "no-sign-in-pending",
    /// The answer when the returned `state` does not match.
    SignInStateMismatch = "sign-in-state-mismatch",
    /// The answer when the provider did not sign the steward in.
    ProviderDidNotSignIn = "provider-did-not-sign-in",
    /// The answer when the redirect carries no code.
    RedirectCarriesNoCode = "redirect-carries-no-code",
    /// The answer when the ID Token did not verify.
    IdTokenDidNotVerify = "id-token-did-not-verify",
    /// The answer when the sign-in could not be completed.
    SignInNotCompleted = "sign-in-not-completed",
    /// The answer when the session cookie cannot be written.
    SessionNotBegun = "session-not-begun",
    /// The answer when the session pool is full.
    SessionsFull = "sessions-full",
    /// The answer when a sign-in session cannot be begun.
    SignInSessionNotBegun = "sign-in-session-not-begun",
    /// The answer when a session cannot be ended.
    SessionNotEnded = "session-not-ended",
    /// The answer when the sign-out redirect cannot be written.
    SignOutRedirectUnwritten = "sign-out-redirect-unwritten",
    /// The answer to a state-changing request from outside the console's pages.
    ForeignRequestRefused = "foreign-request-refused",
    /// The answer to a preference post that names no preference the console offers.
    PreferenceRefused = "preference-refused",
}

/// The console's message `msg` in the page's locale.
#[must_use]
pub fn tr(msg: Msg) -> String {
    tr_in(catalog(), &locale(), msg)
}

/// The console's message `msg` in the page's locale, with the placeables
/// `args` filled in.
#[must_use]
pub fn tr_with(msg: Msg, args: &[(&'static str, Arg)]) -> String {
    locale().lookup_with(catalog(), msg.id(), args)
}

/// The message `msg` of `catalog` in `locale`: for a handler, where no page
/// tree is in reach.
#[must_use]
pub fn tr_in(catalog: &dyn Loader, locale: &Locale, msg: Msg) -> String {
    locale.lookup(catalog, msg.id())
}

/// The locales the language preference offers: every catalog the console
/// ships, English first.
#[must_use]
pub fn choices() -> Vec<Locale> {
    let mut tags: Vec<String> = catalog().locales().map(ToString::to_string).collect();
    tags.sort();
    // NOTE: no specification governs this: our own design; a directory under
    // `locales/` that is no language tag is a build defect the catalog test reports.
    let mut found: Vec<Locale> = tags
        .iter()
        .filter(|tag| tag.as_str() != "en")
        .filter_map(|tag| Locale::parse(tag).ok())
        .collect();
    found.insert(0, Locale::english());
    found
}

/// The locale of a request with `headers`: the best language of its
/// `Accept-Language` the catalog holds, else `configured`.
#[must_use]
pub fn negotiated(headers: &http::HeaderMap, configured: Locale) -> Locale {
    // NOTE: RFC 9110 §12.5.4: a header that is no visible ASCII names no
    // language, so the request falls to the configured one.
    let header = headers
        .get(http::header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    Locale::negotiate_with(header, catalog(), configured)
}

/// The locale a page is drawn in: the language the steward chose in Your
/// preferences when the console ships it, else the request's negotiated
/// one.
#[must_use]
pub fn chosen(preferences: &Preferences, headers: &http::HeaderMap, configured: Locale) -> Locale {
    preferences
        .language
        .tag()
        .and_then(|tag| choices().into_iter().find(|locale| locale.tag() == tag))
        .unwrap_or_else(|| negotiated(headers, configured))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::collections::HashMap;
    use std::path::Path;

    use ferrohealth_ui::i18n::Locale;
    use fluent_templates::fluent_bundle::FluentValue;

    use super::{Msg, catalog, tr_in};

    fluent_templates::static_loader! {
        static THROWAWAY = {
            locales: "./tests/locales",
            fallback_language: "qaa",
            customise: |bundle| bundle.set_use_isolating(false),
        };
    }

    /// Every placeable the catalog at `path` names, each set to a number, so
    /// a message that selects a form by one formats whole.
    fn placeables(path: &Path) -> HashMap<Cow<'static, str>, FluentValue<'static>> {
        let source = std::fs::read_to_string(path).expect("the catalog");
        source
            .split("{ $")
            .skip(1)
            .filter_map(|rest| rest.split([' ', '}']).next())
            .map(|name| (Cow::Owned(name.to_owned()), FluentValue::from(3)))
            .collect()
    }

    #[test]
    fn every_catalog_directory_is_a_language_holding_every_message_and_no_other() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let mut found: Vec<String> = std::fs::read_dir(&root)
            .expect("the locales directory")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        found.sort();
        let mut shipped: Vec<String> = catalog().locales().map(ToString::to_string).collect();
        shipped.sort();
        assert_eq!(found, shipped, "a catalog directory is a language");
        assert!(
            found.contains(&String::from("en")),
            "English is the reference"
        );
        for tag in found {
            let language = tag.parse().expect("a language");
            let path = root.join(&tag).join("ferropix-console.ftl");
            let args = placeables(&path);
            for msg in Msg::ALL {
                let words = catalog().try_lookup_with_args(&language, msg.id(), &args);
                assert!(
                    words.is_some_and(|words| !words.trim().is_empty()),
                    "{tag} holds {}",
                    msg.id()
                );
            }
            let source = std::fs::read_to_string(&path).expect("the catalog");
            let mut held: Vec<&str> = source
                .lines()
                .filter_map(|line| line.split_once(" ="))
                .map(|(id, _)| id)
                .filter(|id| id.starts_with(|c: char| c.is_ascii_lowercase()) && !id.contains(' '))
                .filter(|id| !id.starts_with("number-"))
                .collect();
            held.sort_unstable();
            let mut declared: Vec<&str> = Msg::ALL.iter().map(|msg| msg.id()).collect();
            declared.sort_unstable();
            assert_eq!(
                declared, held,
                "{tag} holds every message once and no other"
            );
            assert!(
                source.contains("number-group ="),
                "{tag} names its digit group mark"
            );
            assert!(
                source.contains("number-decimal ="),
                "{tag} names its decimal mark"
            );
        }
    }

    #[test]
    fn a_throwaway_catalog_changes_the_words_with_no_code_change() {
        let english = Locale::english();
        assert_eq!(
            "Page not found",
            tr_in(catalog(), &english, Msg::NotFoundTitle)
        );
        let throwaway = Locale::parse("qaa").expect("a private-use language");
        assert_eq!(
            "Qage qot qound",
            tr_in(&*THROWAWAY, &throwaway, Msg::NotFoundTitle)
        );
        assert_eq!(
            "Back to the dashboard",
            tr_in(catalog(), &throwaway, Msg::NotFoundBack),
            "the shipped catalog falls back to English"
        );
    }
}
