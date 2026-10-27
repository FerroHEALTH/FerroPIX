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
//!
//! The pseudo-locale, tagged [`PSEUDO_TAG`], draws every console string and
//! every library string accented and two fifths longer, so the browser
//! journeys see a word that skipped the catalog and a layout that does not
//! leave room for a longer translation. It ships no catalog file and the
//! language preference never offers it: only a language cookie naming it
//! chooses it (FerroUI §13, after the gallery's pseudo-locale).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;

use ferrohealth_ui::i18n::{Arg, Locale, library_catalog, locale};
use ferrohealth_ui::theme::Preferences;
use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::{LanguageIdentifier, Loader};

/// The tag of the pseudo-locale, a private-use language subtag (BCP 47,
/// `qaa` to `qtz`), so no catalog the console or the library ships serves
/// it.
pub const PSEUDO_TAG: &str = "qps";

/// The pseudo-locale's language.
static PSEUDO_LANGUAGE: LazyLock<LanguageIdentifier> =
    LazyLock::new(|| fluent_templates::langid!("qps"));

fluent_templates::static_loader! {
    static SHIPPED = {
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

/// The console's catalog: its own words in every language it ships, and in
/// the pseudo-locale the library's English and its own, stretched.
#[derive(Debug, Clone, Copy)]
pub struct Catalog;

/// The one catalog the console looks its words up in and hands the library.
static CATALOG: Catalog = Catalog;

/// The console's catalog, every language it holds.
#[must_use]
pub fn catalog() -> &'static (dyn Loader + Send + Sync) {
    &CATALOG
}

impl Loader for Catalog {
    fn lookup_complete(
        &self,
        lang: &LanguageIdentifier,
        text_id: &str,
        args: Option<&HashMap<Cow<'static, str>, FluentValue>>,
    ) -> String {
        self.try_lookup_complete(lang, text_id, args)
            .unwrap_or_else(|| text_id.to_owned())
    }

    fn try_lookup_complete(
        &self,
        lang: &LanguageIdentifier,
        text_id: &str,
        args: Option<&HashMap<Cow<'static, str>, FluentValue>>,
    ) -> Option<String> {
        if *lang != *PSEUDO_LANGUAGE {
            return SHIPPED.try_lookup_complete(lang, text_id, args);
        }
        let english = fluent_templates::langid!("en");
        SHIPPED
            .try_lookup_complete(&english, text_id, args)
            .or_else(|| library_catalog().try_lookup_complete(&english, text_id, args))
            .map(|english| stretch(&english))
    }

    // NOTE: no specification governs this: our own design; the pseudo-locale is
    // left out, so neither the language preference nor `Accept-Language` reaches it.
    fn locales(&self) -> Box<dyn Iterator<Item = &LanguageIdentifier> + '_> {
        SHIPPED.locales()
    }
}

/// `english` accented letter by letter and padded by two fifths of its
/// length (at least three), in brackets, so a cut end shows.
///
/// The padding is tildes in words of at most five, so a long message breaks
/// across lines as a translation would, where one run of tildes could not.
#[must_use]
pub fn stretch(english: &str) -> String {
    let accented: String = english.chars().map(accent).collect();
    let length = english.chars().count();
    let pad = length.saturating_mul(2).div_ceil(5).max(3);
    let words: Vec<String> = (0..pad.div_ceil(PAD_WORD))
        .map(|word| "~".repeat(PAD_WORD.min(pad.saturating_sub(word.saturating_mul(PAD_WORD)))))
        .collect();
    format!("[{accented} {}]", words.join(" "))
}

/// The longest word of tildes the pseudo-locale pads a message with.
const PAD_WORD: usize = 5;

/// The accented look-alike of `letter`, the letter itself when it has none.
const fn accent(letter: char) -> char {
    match letter {
        'a' => 'á',
        'c' => 'ç',
        'e' => 'é',
        'i' => 'í',
        'n' => 'ñ',
        'o' => 'ö',
        'u' => 'ü',
        'y' => 'ý',
        'A' => 'Å',
        'C' => 'Ç',
        'E' => 'É',
        'I' => 'Î',
        'N' => 'Ñ',
        'O' => 'Ö',
        'U' => 'Ü',
        'Y' => 'Ý',
        other => other,
    }
}

/// The pseudo-locale, with the console's catalog consulted for every
/// message, the library's included.
///
/// # Panics
///
/// Never: [`PSEUDO_TAG`] is a constant private-use language subtag, which
/// every parse accepts.
#[must_use]
pub fn pseudo() -> Locale {
    #[expect(
        clippy::expect_used,
        reason = "PSEUDO_TAG is a constant private-use language subtag, which every parse accepts"
    )]
    let language = Locale::parse(PSEUDO_TAG).expect("the pseudo-locale's tag should parse");
    language.with_catalog(catalog())
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
    /// The legend's group of the shortcuts that work on every screen.
    ShortcutsEverywhere = "shortcuts-everywhere",
    /// The legend's group of the shortcuts of a list.
    ShortcutsLists = "shortcuts-lists",
    /// The legend's group of the patient record's shortcuts.
    ShortcutsPatient = "shortcuts-patient",
    /// What `/` does.
    ShortcutSearch = "shortcut-search",
    /// What `?` does.
    ShortcutLegend = "shortcut-legend",
    /// What `j` does.
    ShortcutNext = "shortcut-next",
    /// What `k` does.
    ShortcutPrevious = "shortcut-previous",
    /// What Enter does on a row.
    ShortcutOpen = "shortcut-open",
    /// The line under a page the search panel offers.
    SuggestPage = "suggest-page",
    /// The line under a setting the search panel offers.
    SuggestSetting = "suggest-setting",
    /// The search panel's group of pages.
    SuggestPages = "suggest-pages",
    /// The search panel's group of identifier domains.
    SuggestDomains = "suggest-domains",
    /// The Appearance tab of Your preferences, as the search panel offers it.
    SettingAppearance = "setting-appearance",
    /// The Language and region tab, as the search panel offers it.
    SettingLanguage = "setting-language",
    /// The Keyboard tab, as the search panel offers it.
    SettingKeyboard = "setting-keyboard",
    /// The search panel's group of settings.
    SuggestSettings = "suggest-settings",
    /// The search panel's sentence when no page, domain or setting matches.
    SuggestNone = "suggest-none",
    /// The line under it.
    SuggestNoneLine = "suggest-none-line",
    /// Why the search panel names no identifier domain.
    SuggestDomainsUnread = "suggest-domains-unread",
    /// The search panel's last row, which searches patients for the words.
    SearchPatientsFor = "search-patients-for",
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
    /// The name of the identifier field's hold-to-show eye.
    IdentifierHold = "identifier-hold",
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
    /// The disclosure of a trail row's references.
    ActionDetail = "action-detail",
    /// The state of an active domain, person or record.
    Active = "state-active",
    /// An address.
    Address = "address",
    /// A source admitted as a Basic user.
    AdmissionBasic = "admission-basic",
    /// A source an issuer vouches for.
    AdmissionIssuer = "admission-issuer",
    /// A registered HL7 v2 sender.
    AdmissionSender = "admission-sender",
    /// The trail filter form's submit.
    ApplyFilters = "apply-filters",
    /// A built-in rule's description.
    BuiltInRule = "built-in-rule",
    /// The FHIR channel.
    ChannelFhir = "channel-fhir",
    /// The HL7 v2 channel.
    ChannelV2 = "channel-v2",
    /// The PMIR channel.
    ChannelPmir = "channel-pmir",
    /// The action column.
    ColAction = "col-action",
    /// The actor column.
    ColActor = "col-actor",
    /// The admission column.
    ColAdmission = "col-admission",
    /// The channel column.
    ColChannel = "col-channel",
    /// The creation time.
    ColCreated = "col-created",
    /// The detail column.
    ColDetail = "col-detail",
    /// The domain column.
    ColDomain = "col-domain",
    /// The domains column.
    ColDomains = "col-domains",
    /// The grade column.
    ColGrade = "col-grade",
    /// The identifiers column.
    ColIdentifiers = "col-identifiers",
    /// The kind column.
    ColKind = "col-kind",
    /// The last activity column.
    ColLastActivity = "col-last-activity",
    /// The last fed column.
    ColLastFed = "col-last-fed",
    /// The link column.
    ColLink = "col-link",
    /// The name column.
    ColName = "col-name",
    /// The namespace column.
    ColNamespace = "col-namespace",
    /// The outcome column.
    ColOutcome = "col-outcome",
    /// The person column.
    ColPerson = "col-person",
    /// The reason column.
    ColReason = "col-reason",
    /// The records column.
    ColRecords = "col-records",
    /// Whether values are sealed at rest.
    ColSensitive = "col-sensitive",
    /// The source column.
    ColSource = "col-source",
    /// The state column.
    ColState = "col-state",
    /// The system column.
    ColSystem = "col-system",
    /// The time column.
    ColTime = "col-time",
    /// The value column.
    ColValue = "col-value",
    /// The line when no configuration file was read.
    ConfigurationNoFile = "configuration-no-file",
    /// A count, formatted for the language.
    Count = "count",
    /// The action id detail.
    DetailAction = "detail-action",
    /// The candidate detail.
    DetailCandidate = "detail-candidate",
    /// The identifier domain detail.
    DetailDomain = "detail-domain",
    /// The merge detail.
    DetailMerge = "detail-merge",
    /// The pseudonymisation domain detail.
    DetailPseudonymDomain = "detail-pseudonym-domain",
    /// The record detail.
    DetailRecord = "detail-record",
    /// The domain page's definition card.
    DomainDefinition = "domain-definition",
    /// The empty domain listing.
    DomainsEmpty = "domains-empty",
    /// A steward or system action on the timeline.
    EventAction = "event-action",
    /// A record linked on the timeline.
    EventLinked = "event-linked",
    /// A merge on the timeline.
    EventMerged = "event-merged",
    /// A record unlinked on the timeline.
    EventUnlinked = "event-unlinked",
    /// A reverted merge on the timeline.
    EventUnmerged = "event-unmerged",
    /// A version on the timeline.
    EventVersion = "event-version",
    /// The trail export's submit.
    ExportCsv = "export-csv",
    /// A feed that created a record.
    FeedCreated = "feed-created",
    /// A feed that revised a record.
    FeedRevised = "feed-revised",
    /// An empty feed listing or chart.
    FeedsEmpty = "feeds-empty",
    /// The feed activity chart.
    FeedsPerHour = "feeds-per-hour",
    /// The feed chart's summary.
    FeedsSummary = "feeds-summary",
    /// The trail filter's start date.
    FilterFrom = "filter-from",
    /// The trail filter's end date.
    FilterTo = "filter-to",
    /// The trail filter card.
    Filters = "filters",
    /// A person's name.
    FullName = "full-name",
    /// The certain grade.
    GradeCertain = "grade-certain",
    /// The certainly-not grade.
    GradeCertainlyNot = "grade-certainly-not",
    /// The possible grade.
    GradePossible = "grade-possible",
    /// The probable grade.
    GradeProbable = "grade-probable",
    /// A grade with its score.
    GradeScore = "grade-score",
    /// The empty history.
    HistoryEmpty = "history-empty",
    /// The note under a cut history.
    HistoryTruncated = "history-truncated",
    /// The empty identifiers tab.
    IdentifiersEmpty = "identifiers-empty",
    /// The authoritative domain kind.
    KindAuthoritative = "kind-authoritative",
    /// The local domain kind.
    KindLocal = "kind-local",
    /// The master domain kind.
    KindMaster = "kind-master",
    /// A domain's last feeds.
    LastFeeds = "last-feeds",
    /// A source's last messages.
    LastMessages = "last-messages",
    /// The demographics the person shows.
    LeadingDemographics = "leading-demographics",
    /// A deterministic link.
    LinkDeterministic = "link-deterministic",
    /// A feed link.
    LinkFeed = "link-feed",
    /// A link a merge moved.
    LinkMerge = "link-merge",
    /// A probabilistic link.
    LinkProbabilistic = "link-probabilistic",
    /// A link the source made.
    LinkSource = "link-source",
    /// A steward's link.
    LinkSteward = "link-steward",
    /// A listener that is down.
    ListenerDown = "listener-down",
    /// The outbound feed sender.
    ListenerFeed = "listener-feed",
    /// The HTTP listener.
    ListenerHttp = "listener-http",
    /// The MLLP listener.
    ListenerMllp = "listener-mllp",
    /// A listener that is not running.
    ListenerOff = "listener-off",
    /// A listener that is up.
    ListenerUp = "listener-up",
    /// The listeners card.
    Listeners = "listeners",
    /// The state of a merged person.
    Merged = "merged",
    /// The survivor of a merged person.
    MergedInto = "merged-into",
    /// The line of a merge event.
    MergedIntoLine = "merged-into-line",
    /// The empty messages listing.
    MessagesEmpty = "messages-empty",
    /// A result without a name.
    NoName = "no-name",
    /// A domain the person has no pseudonym in.
    NoPseudonym = "no-pseudonym",
    /// The trail's next page.
    OlderActions = "older-actions",
    /// The oldest pairs card.
    OldestInQueue = "oldest-in-queue",
    /// The way to the queue.
    OpenQueue = "open-queue",
    /// The patient tabs' name.
    PatientTabs = "patient-tabs",
    /// A percentage.
    Percent = "percent",
    /// Values kept plain at rest.
    Plain = "plain",
    /// The empty pseudonyms tab.
    PseudonymsEmpty = "pseudonyms-empty",
    /// The queue depth chart.
    QueueDepth = "queue-depth",
    /// The empty queue chart.
    QueueDepthEmpty = "queue-depth-empty",
    /// The queue chart's summary.
    QueueDepthSummary = "queue-depth-summary",
    /// The empty oldest card.
    QueueEmpty = "queue-empty",
    /// The queue chart's series.
    QueueOpen = "queue-open",
    /// A record's version and channel.
    RecordLine = "record-line",
    /// The empty records tab.
    RecordsEmpty = "records-empty",
    /// A removed record.
    Removed = "removed",
    /// The line of an unmerge event.
    RestoredLine = "restored-line",
    /// The search results.
    Results = "results",
    /// The empty results.
    ResultsEmpty = "results-empty",
    /// A retired domain.
    Retired = "retired",
    /// The reveal action.
    Reveal = "reveal",
    /// When a revealed value hides.
    RevealHidden = "reveal-hidden",
    /// The reveal reason field.
    RevealReason = "reveal-reason",
    /// The note under a revealed value.
    RevealRecorded = "reveal-recorded",
    /// The revealed value notice.
    Revealed = "revealed",
    /// A score.
    Score = "score",
    /// Values sealed at rest.
    Sealed = "sealed",
    /// The blocking keys section.
    SectionBlocking = "section-blocking",
    /// The blocking section's intro.
    SectionBlockingIntro = "section-blocking-intro",
    /// The rules section.
    SectionRules = "section-rules",
    /// The rules section's intro.
    SectionRulesIntro = "section-rules-intro",
    /// The thresholds section.
    SectionThresholds = "section-thresholds",
    /// The thresholds section's intro.
    SectionThresholdsIntro = "section-thresholds-intro",
    /// The weights section.
    SectionWeights = "section-weights",
    /// The weights section's intro.
    SectionWeightsIntro = "section-weights-intro",
    /// The source page's definition card.
    SourceDefinition = "source-definition",
    /// The empty sources listing.
    SourcesEmpty = "sources-empty",
    /// The auto-link rate stat.
    StatAutoLink = "stat-auto-link",
    /// The records created beside the rate.
    StatCreated = "stat-created",
    /// The versions fed stat.
    StatFedToday = "stat-fed-today",
    /// Subscriptions in error.
    StatInError = "stat-in-error",
    /// The open queue stat.
    StatOpenQueue = "stat-open-queue",
    /// The subscriptions stat.
    StatSubscriptions = "stat-subscriptions",
    /// Pairs waiting over a week.
    StatWaitingOverWeek = "stat-waiting-over-week",
    /// A subsumed record.
    Subsumed = "subsumed",
    /// The history tab.
    TabHistory = "tab-history",
    /// The identifiers tab.
    TabIdentifiers = "tab-identifiers",
    /// The pseudonyms tab.
    TabPseudonyms = "tab-pseudonyms",
    /// The records tab.
    TabRecords = "tab-records",
    /// A contact point.
    Telecom = "telecom",
    /// The empty trail.
    TrailEmpty = "trail-empty",
    /// What the listeners card shows.
    WhatListeners = "what-listeners",
    /// What a reveal shows.
    WhatReveal = "what-reveal",
    /// A domain write saved.
    DomainSaved = "domain-saved",
    /// What a refused domain write shows.
    WhatDomainWrite = "what-domain-write",
    /// The create form's disclosure and submit.
    DomainCreate = "domain-create",
    /// The change form's disclosure and submit.
    DomainChange = "domain-change",
    /// The note that the server records a domain write with its reason.
    DomainAdminNote = "domain-admin-note",
    /// The note drawn in place of the domain forms without the
    /// administrator scope.
    DomainAdminOnly = "domain-admin-only",
    /// The link threshold.
    SettingLinkAt = "setting-link-at",
    /// The review threshold.
    SettingReviewAt = "setting-review-at",
    /// The possible threshold.
    SettingPossibleAt = "setting-possible-at",
    /// The automatic link switch.
    SettingAutoLink = "setting-auto-link",
    /// The blocking keys.
    SettingBlockingKeys = "setting-blocking-keys",
    /// A field's agreement weight.
    SettingAgreement = "setting-agreement",
    /// A field's chance agreement weight.
    SettingChance = "setting-chance",
    /// The family name field.
    FieldFamilyName = "field-family-name",
    /// The given names field.
    FieldGivenNames = "field-given-names",
    /// The birth date field.
    FieldBirthDate = "field-birth-date",
    /// The gender field.
    FieldGender = "field-gender",
    /// The postal code field.
    FieldPostalCode = "field-postal-code",
    /// The address field.
    FieldAddress = "field-address",
    /// The telephone field.
    FieldTelephone = "field-telephone",
    /// The email field.
    FieldEmail = "field-email",
    /// The mother's maiden name field.
    FieldMothersMaidenName = "field-mothers-maiden-name",
    /// The local identifier field.
    FieldLocalIdentifier = "field-local-identifier",
    /// The authoritative identifier rule.
    RuleAuthoritative = "rule-authoritative",
    /// What the authoritative identifier rule holds.
    RuleAuthoritativeText = "rule-authoritative-text",
    /// The hard conflict rule.
    RuleHardConflict = "rule-hard-conflict",
    /// What the hard conflict rule holds.
    RuleHardConflictText = "rule-hard-conflict-text",
    /// The local domains rule.
    RuleLocalDomains = "rule-local-domains",
    /// What the local domains rule holds.
    RuleLocalDomainsText = "rule-local-domains-text",
    /// A steward viewed a person.
    ActionView = "action-view",
    /// A steward exported the trail.
    ActionExport = "action-export",
    /// A steward revealed a value.
    ActionReveal = "action-reveal",
    /// An administrator added a domain.
    ActionDomainCreate = "action-domain-create",
    /// An administrator changed a domain.
    ActionDomainUpdate = "action-domain-update",
    /// A source fed a record.
    ActionFeed = "action-feed",
    /// A source removed a record.
    ActionRemove = "action-remove",
    /// A source resolved a duplicate.
    ActionResolveDuplicate = "action-resolve-duplicate",
    /// A steward merged two persons.
    ActionMerge = "action-merge",
    /// A steward undid a merge.
    ActionUnmerge = "action-unmerge",
    /// A steward decided a pair.
    ActionDecide = "action-decide",
    /// An HL7 v2 message was received.
    ActionMessage = "action-message",
    /// The sensitive switch.
    DomainSensitive = "domain-sensitive",
    /// The sensitive switch's help.
    DomainSensitiveHelp = "domain-sensitive-help",
    /// The retire switch.
    DomainRetire = "domain-retire",
    /// The retire switch's help.
    DomainRetireHelp = "domain-retire-help",
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
/// preferences when the console ships it or it is the pseudo-locale, else
/// the request's negotiated one.
#[must_use]
pub fn chosen(preferences: &Preferences, headers: &http::HeaderMap, configured: Locale) -> Locale {
    match preferences.language.tag() {
        Some(PSEUDO_TAG) => pseudo(),
        tag => tag
            .and_then(|tag| choices().into_iter().find(|locale| locale.tag() == tag))
            .unwrap_or_else(|| negotiated(headers, configured)),
    }
}

#[cfg(test)]
mod tests;
