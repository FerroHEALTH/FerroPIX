# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# The words of the FerroPIX Console, in English, the reference catalog.

# The marks a number is grouped and written with in this language.
number-group = ,
number-decimal = .

# `FerroPIX`, the product.
product = FerroPIX
# `Console`, the accented word of the lockup.
console-word = Console
# The console's name in the footer and the titles.
console-name = FerroPIX Console

# The navigation's name.
nav-label = Console
# The Work group.
nav-work = Work
# The Patients group.
nav-patients = Patients
# The Registry group.
nav-registry = Registry
# The Trail group.
nav-trail = Trail
# The Console group.
nav-console = Console
# Why the queue's depth is not known.
queue-depth-unknown = Not known: the server did not answer

# The top bar's search field's name.
search-label = Find a patient
# The top bar's search field's placeholder.
search-placeholder = Identifier, name or birth date
# The top bar's search submit.
search-find = Find
# The search panel's last row, which searches patients for the words.
search-patients-for = Search patients for “{ $query }”
# The line under a page the search panel offers.
suggest-page = Page
# The line under a setting the search panel offers.
suggest-setting = Your preferences
# The search panel's group of pages.
suggest-pages = Pages
# The search panel's group of identifier domains.
suggest-domains = Identifier domains
# The search panel's group of settings.
suggest-settings = Settings
# The Appearance tab of Your preferences, as the search panel offers it.
setting-appearance = Appearance
# The Language and region tab, as the search panel offers it.
setting-language = Language and region
# The Keyboard tab, as the search panel offers it.
setting-keyboard = Keyboard
# The search panel's sentence when no page, domain or setting matches.
suggest-none = No page, identifier domain or setting has these words.
# Why the search panel names no identifier domain.
suggest-domains-unread = The FerroPIX server did not answer for the identifier domains.
# The line under it.
suggest-none-line = Press Enter to search patients; every patient search is recorded in the audit trail.
# The legend's group of the shortcuts that work on every screen.
shortcuts-everywhere = Every screen
# The legend's group of the shortcuts of a list.
shortcuts-lists = Lists
# The legend's group of the patient record's shortcuts.
shortcuts-patient = Patient record
# What `/` does.
shortcut-search = Search patients
# What `?` does.
shortcut-legend = Show the keyboard shortcuts
# What `j` does.
shortcut-next = Next row
# What `k` does.
shortcut-previous = Previous row
# What Enter does on a row.
shortcut-open = Open the row

# The status pill when the server answers.
pill-up = Server up
# The status pill when the server does not answer.
pill-unreachable = Unreachable
# The status pill when the server refuses the console.
pill-refused = Refused { $status }
# The status pill when the server answers with a fault.
pill-degraded = Server answered { $status }
# The status pill's tooltip detail: the address the console reaches.
pill-detail = { $base }
# The user menu's line under the steward's name.
menu-line = Signed in at { $provider }
# The steward's name when the provider gave none.
steward-unnamed = Data steward

# Dashboard.
dashboard = Dashboard
# The dashboard's subtitle.
dashboard-sub = The day's state of the index.
# Review queue.
queue = Review queue
# The queue's subtitle.
queue-sub = Pairs of records the matcher could not decide. A row is one pair; open it to compare and decide.
# The compare page's title.
pair = Compare and decide
# The compare page's subtitle.
pair-sub = Your decision is recorded with your name and reason.
# Search.
search = Search
# The search page's subtitle.
search-sub = Find a patient by one identifier or by demographics; results stay in this page and never reach a URL.
# Patient.
patient = Patient
# The patient page's subtitle.
patient-sub = Every identifier of this patient, the records side by side, the history and the pseudonyms.
# The merge page's title.
merge = Merge
# The merge page's subtitle.
merge-sub = Nothing is written until you confirm; the table is the server's answer for this preview.
# The unmerge page's title.
unmerge = Unmerge
# The unmerge page's subtitle.
unmerge-sub = The latest merge of this patient can be reverted; nothing is written until you confirm.
# Sources.
sources = Sources
# The sources page's subtitle.
sources-sub = The systems allowed to feed records, and how each connects.
# The source page's subtitle.
source-sub = Source · how it connects and what it last sent, never a value.
# Identifier domains.
domains = Identifier domains
# The domains page's subtitle.
domains-sub = The assigning authorities, each with the one source that feeds it; a sensitive domain is masked everywhere.
# The domain page's subtitle.
domain-sub = Identifier domain · its definition and its feed activity, never a value.
# Pseudonymisation domains.
pseudonym-domains = Pseudonymisation domains
# The pseudonymisation domains page's subtitle.
pseudonym-domains-sub = The namespaces pseudonyms are created in, and the one place a pseudonym is resolved.
# Configuration.
configuration = Configuration
# The configuration page's subtitle.
configuration-sub = The matching settings this deployment runs with, and where each came from.
# Audit trail.
audit = Audit trail
# The audit trail's subtitle.
audit-sub = Every steward action: who made it, when and why.
# About.
about = About
# The About page's subtitle.
about-sub = The server this console talks to, the versions and the manufacturer.
# Your preferences.
preferences = Your preferences

# What the dashboard cannot show.
what-dashboard = the dashboard
# What the queue cannot show.
what-queue = the queue
# What the compare page cannot show.
what-pair = this pair
# What the patient page cannot show.
what-patient = this patient
# What the merge page cannot show.
what-merge = the merge preview
# What the unmerge page cannot show.
what-unmerge = the unmerge preview
# What the sources page cannot show.
what-sources = the sources
# What the source page cannot show.
what-source = this source
# What the domains page cannot show.
what-domains = the identifier domains
# What the domain page cannot show.
what-domain = this domain
# What the pseudonymisation domains page cannot show.
what-pseudonym-domains = the pseudonymisation domains
# What the configuration page cannot show.
what-configuration = the configuration
# What the audit trail cannot show.
what-audit = the audit trail
# What a search cannot show.
what-search = the results

# The refusal of a page whose server refused the connection.
unreachable-connect = The FerroPIX server refused the connection, so { $what } cannot be shown.
# The refusal of a page whose server did not answer in time.
unreachable-timeout = The FerroPIX server did not answer in time, so { $what } cannot be shown.
# The refusal of a page whose server could not be reached.
unreachable-other = The FerroPIX server could not be reached, so { $what } cannot be shown.
# The refusal of a page whose server refused the console's call.
refused-call = The FerroPIX server refused the console's call, so { $what } cannot be shown.
# The notice of a page whose server serves no operator API yet.
not-served = The FerroPIX server answers, but this version serves no operator API yet, so { $what } cannot be shown.
# The line under that notice: the route the page reads.
not-served-route = The page reads { $route } once the server serves it.
# Try again.
try-again = Try again
# When the console last called the server.
last-tried = Last tried
# The card on the dashboard while the server is away.
away-title = While the server is away
# That card's sentence.
away-body = This console keeps no patient data, so no page can be drawn until the server answers. Your session is kept: you do not need to sign in again.
# The technical detail disclosure.
technical-detail = Technical detail
# The failure row.
detail-failure = Failure
# The request id row.
detail-request-id = Request id
# The time row.
detail-time = Time
# The route row.
detail-route = Route
# The connection was refused.
failure-connect = The server refused the connection
# No answer in time.
failure-timeout = No answer came in time
# Another failure.
failure-other = The call failed before an answer came
# The server answered with a status.
failure-status = The server answered { $status }
# The person id, as the patient's subtitle names it.
person-id = Person { $person }

# Find a patient, the search card's title.
find-patient = Find a patient
# The search segments' name.
search-by = Search by
# By identifier.
by-identifier = By identifier
# By demographics.
by-demographics = By demographics
# Domain.
domain = Domain
# The domain select while the domains are not known.
domains-not-known = Not known
# Identifier.
identifier = Identifier
# The identifier field's help line.
identifier-help = Masked as you type and sent in the request body, never in the address bar.
# The name of the identifier field's hold-to-show eye.
identifier-hold = Show the identifier while held
# Family name.
family-name = Family name
# Given names.
given-names = Given names
# Birth date.
birth-date = Birth date
# The birth date's format hint.
birth-date-hint = YYYY-MM-DD
# Gender.
gender = Gender
# Any gender.
gender-any = Any
# Female.
gender-female = Female
# Male.
gender-male = Male
# Other.
gender-other = Other
# Unknown.
gender-unknown = Unknown
# Postal code.
postal-code = Postal code
# Exact names only.
exact-names = Exact names only
# The exact names check's help.
exact-names-help = On: only exact spellings count.
# The search form's submit.
search-submit = Search
# The line after a search that sent nothing.
nothing-searched = Nothing was searched, and nothing you typed left this console.

# The About page's console row.
version-console = Console
# The About page's server row.
version-server = Server
# The About page's FHIR row.
version-fhir = FHIR
# The About page's FerroUI row.
version-ferroui = FerroUI
# A version the console does not know.
not-known = Not known
# The licence.
licence = Business Source License 1.1
# The documentation link.
documentation = The FerroPIX book
# The `CapabilityStatement` card's title.
capability-title = The CapabilityStatement
# The `CapabilityStatement` card's line.
capability-line = What the server declares at GET [base]/fhir/metadata, as every FHIR client reads it.
# What the About page cannot show without the server.
what-capability = its CapabilityStatement

# The reveal duration row.
reveal-for = Revealed identifiers stay for
# The reveal duration row's description.
reveal-for-desc = How long a revealed identifier shows before it is masked again; the server caps every reveal at 30 s.
# A reveal duration choice.
reveal-seconds = { $count ->
    [one] { $count } second
   *[other] { $count } seconds
}
# The hide on leave row.
hide-on-leave = Hide them when I leave the page
# The hide on leave row's description.
hide-on-leave-desc = Masks every revealed identifier as soon as you open another page or tab.
# The hide on leave switch's name.
hide-on-leave-switch = Hide revealed identifiers when I leave the page
# The theme cards' accent.
accent-plum = Plum

# The sign-in page's title.
sign-in = Sign in
# The sign-in card's title.
sign-in-title = Sign in to the patient index
# The provider's button.
sign-in-with = Sign in with your identity provider
# The provider's button while it redirects.
sign-in-taking = Taking you to your identity provider
# The sign-in card's caption.
sign-in-caption = The console for the data stewards of the patient index. No patient data is shown before you sign in.
# The notice after a sign-out.
signed-out = You are signed out of this browser.
# The notice after a session ended.
session-expired = Your session ended. Sign in again to go on where you were.
# The notice when sign-in is not configured.
sign-in-unavailable = Sign-in is not set up on this console. Ask your administrator to configure an identity provider.

# The not-found page's title.
not-found-title = Page not found
# The not-found page's body.
not-found-body = The console has no page at this address. It may be mistyped, or name a person or a domain this index does not hold.
# The not-found page's way back.
not-found-back = Back to the dashboard
# The not-found page's way to search.
not-found-search = Find a patient

# The answer when the session store cannot be read.
session-unreadable = The console's session store cannot be read; try again in a moment.
# The answer when sign-in is not configured.
sign-in-not-configured = sign-in is not configured on this console
# The answer when the sign-in redirect cannot be written.
sign-in-redirect-unwritten = the sign-in redirect could not be written
# The answer when no sign-in is pending.
no-sign-in-pending = no sign-in is pending for this browser
# The answer when the returned `state` does not match.
sign-in-state-mismatch = the sign-in's state did not match
# The answer when the provider did not sign the steward in.
provider-did-not-sign-in = the identity provider did not sign you in
# The answer when the redirect carries no code.
redirect-carries-no-code = the redirect carries no authorization code
# The answer when the ID Token did not verify.
id-token-did-not-verify = the identity provider's ID Token did not verify
# The answer when the sign-in could not be completed.
sign-in-not-completed = the sign-in could not be completed at the identity provider
# The answer when the session cookie cannot be written.
session-not-begun = the session could not be begun
# The answer when the session pool is full.
sessions-full = the console holds as many sessions as it allows; try again later
# The answer when a sign-in session cannot be begun.
sign-in-session-not-begun = the sign-in could not be begun
# The answer when a session cannot be ended.
session-not-ended = the session could not be ended
# The answer when the sign-out redirect cannot be written.
sign-out-redirect-unwritten = the sign-out redirect could not be written
# The answer to a state-changing request from outside the console's pages.
foreign-request-refused = the console takes this request only from its own pages
# The answer to a preference post that names no preference the console offers.
preference-refused = the preference or its value is not one the console offers

## The pages drawn from the operator API.
# The disclosure of a trail row's references.
action-detail = Detail
# The state of an active domain, person or record.
state-active = Active
# An address.
address = Address
# A source admitted as a Basic user.
admission-basic = Basic user, development only
# A source an issuer vouches for.
admission-issuer = Client of { $issuer }
# A registered HL7 v2 sender.
admission-sender = HL7 v2 sender { $application } at { $facility }
# The trail filter form's submit.
apply-filters = Apply filters
# A built-in rule's description.
built-in-rule = Built into FerroPIX; not configurable.
# The FHIR channel.
channel-fhir = FHIR
# The HL7 v2 channel.
channel-v2 = HL7 v2
# The PMIR channel.
channel-pmir = PMIR
# The action column.
col-action = Action
# The actor column.
col-actor = Actor
# The admission column.
col-admission = Admission
# The channel column.
col-channel = Channel
# The creation time.
col-created = Created
# The detail column.
col-detail = Detail
# The domain column.
col-domain = Domain
# The domains column.
col-domains = Domains
# The grade column.
col-grade = Grade
# The identifiers column.
col-identifiers = Identifiers
# The kind column.
col-kind = Kind
# The last activity column.
col-last-activity = Last activity
# The last fed column.
col-last-fed = Last fed
# The link column.
col-link = Linked by
# The name column.
col-name = Name
# The namespace column.
col-namespace = Namespace
# The outcome column.
col-outcome = Outcome
# The person column.
col-person = Person
# The reason column.
col-reason = Reason
# The records column.
col-records = Records
# Whether values are sealed at rest.
col-sensitive = At rest
# The source column.
col-source = Source
# The state column.
col-state = State
# The system column.
col-system = System
# The time column.
col-time = Time
# The value column.
col-value = Value
# The line when no configuration file was read.
configuration-no-file = Read from the defaults and the environment at start; no configuration file was read.
# A count, formatted for the language.
count = { $count }
# The action id detail.
detail-action = Action id
# The candidate detail.
detail-candidate = Candidate
# The identifier domain detail.
detail-domain = Identifier domain
# The merge detail.
detail-merge = Merge
# The pseudonymisation domain detail.
detail-pseudonym-domain = Pseudonymisation domain
# The record detail.
detail-record = Record
# The domain page's definition card.
domain-definition = Definition
# The empty domain listing.
domains-empty = No identifier domains yet. Seed them with [[domain]] in ferropix.toml, or create one as an administrator.
# A steward or system action on the timeline.
event-action = Action
# A record linked on the timeline.
event-linked = Linked
# A merge on the timeline.
event-merged = Merged
# A record unlinked on the timeline.
event-unlinked = Unlinked
# A reverted merge on the timeline.
event-unmerged = Unmerged
# A version on the timeline.
event-version = Version received
# The trail export's submit.
export-csv = Export as CSV
# A feed that created a record.
feed-created = Created a record
# A feed that revised a record.
feed-revised = Revised a record
# An empty feed listing or chart.
feeds-empty = No feeds in this window.
# The feed activity chart.
feeds-per-hour = Feeds per hour
# The feed chart's summary.
feeds-summary = Versions received per hour by channel over the last { $hours } hours.
# The trail filter's start date.
filter-from = From
# The trail filter's end date.
filter-to = To
# The trail filter card.
filters = Filters
# A person's name.
full-name = Name
# The certain grade.
grade-certain = Certain
# The certainly-not grade.
grade-certainly-not = Certainly not
# The possible grade.
grade-possible = Possible
# The probable grade.
grade-probable = Probable
# A grade with its score.
grade-score = { $grade } · { $score }
# The empty history.
history-empty = Nothing has happened to this person yet.
# The note under a cut history.
history-truncated = Only the newest events are shown; the audit trail holds every one.
# The empty identifiers tab.
identifiers-empty = This person holds no identifier.
# The authoritative domain kind.
kind-authoritative = Authoritative
# The local domain kind.
kind-local = Local
# The master domain kind.
kind-master = Master
# A domain's last feeds.
last-feeds = Last feeds
# A source's last messages.
last-messages = Last messages
# The demographics the person shows.
leading-demographics = Leading demographics
# A deterministic link.
link-deterministic = An authoritative identifier
# A feed link.
link-feed = The first feed
# A link a merge moved.
link-merge = A merge
# A probabilistic link.
link-probabilistic = The matcher
# A link the source made.
link-source = The source
# A steward's link.
link-steward = A data steward
# A listener that is down.
listener-down = Down
# The outbound feed sender.
listener-feed = Feed sender
# The HTTP listener.
listener-http = HTTP
# The MLLP listener.
listener-mllp = MLLP
# A listener that is not running.
listener-off = Off
# A listener that is up.
listener-up = Up
# The listeners card.
listeners = Listeners
# The state of a merged person.
merged = Merged
# The survivor of a merged person.
merged-into = Merged into
# The line of a merge event.
merged-into-line = Into { $person }
# The empty messages listing.
messages-empty = This source has sent no message yet.
# A result without a name.
no-name = No name stated
# A domain the person has no pseudonym in.
no-pseudonym = None yet
# The trail's next page.
older-actions = Older actions
# The oldest pairs card.
oldest-in-queue = Oldest in the queue
# The way to the queue.
open-queue = Open the queue
# The patient tabs' name.
patient-tabs = The patient record
# A percentage.
percent = { $percent } %
# Values kept plain at rest.
plain = Plain
# The empty pseudonyms tab.
pseudonyms-empty = No pseudonymisation domain is configured.
# The queue depth chart.
queue-depth = Queue depth
# The empty queue chart.
queue-depth-empty = The queue has no history yet.
# The queue chart's summary.
queue-depth-summary = Open pairs per day over the last { $days } days.
# The empty oldest card.
queue-empty = The queue is empty.
# The queue chart's series.
queue-open = Open pairs
# A record's version and channel.
record-line = Version { $version } over { $channel }
# The empty records tab.
records-empty = This person holds no source record.
# A removed record.
removed = Removed
# The line of an unmerge event.
restored-line = Restored { $person }
# The search results.
results = Results
# The empty results.
results-empty = No patient matches. Check the domain and the value, or search by demographics.
# A retired domain.
retired = Retired
# The reveal action.
reveal = Reveal
# When a revealed value hides.
reveal-hidden = the value is hidden again
# The reveal reason field.
reveal-reason = Reason
# The note under a revealed value.
reveal-recorded = Recorded in the audit trail as action { $action }.
# The revealed value notice.
revealed = Revealed
# A score.
score = Score { $score }
# Values sealed at rest.
sealed = Sealed
# The blocking keys section.
section-blocking = Blocking keys
# The blocking section's intro.
section-blocking-intro = The keys that decide which records the matcher compares.
# The rules section.
section-rules = Deterministic rules
# The rules section's intro.
section-rules-intro = The rules the deterministic match follows.
# The thresholds section.
section-thresholds = Thresholds
# The thresholds section's intro.
section-thresholds-intro = The scores at which a pair is linked, queued for review or graded possible.
# The weights section.
section-weights = Field weights
# The weights section's intro.
section-weights-intro = The agreement probabilities of each field the scorer compares.
# The source page's definition card.
source-definition = Definition
# The empty sources listing.
sources-empty = No source is configured. Add a client to an issuer or an HL7 v2 sender in ferropix.toml.
# The auto-link rate stat.
stat-auto-link = Linked automatically, 24 h
# The records created beside the rate.
stat-created = { $count } created
# The versions fed stat.
stat-fed-today = Versions fed today
# Subscriptions in error.
stat-in-error = In error
# The open queue stat.
stat-open-queue = Open in the queue
# The subscriptions stat.
stat-subscriptions = Subscriptions in error
# Pairs waiting over a week.
stat-waiting-over-week = { $count } over 7 days
# A subsumed record.
subsumed = Subsumed
# The history tab.
tab-history = History
# The identifiers tab.
tab-identifiers = Identifiers
# The pseudonyms tab.
tab-pseudonyms = Pseudonyms
# The records tab.
tab-records = Records
# A contact point.
telecom = Phone or email
# The empty trail.
trail-empty = No action matches these filters.
# What the listeners card shows.
what-listeners = the listeners
# What a reveal shows.
what-reveal = the value
# A domain write saved.
domain-saved = The domain is saved, and the change is in the audit trail.
# What a refused domain write shows.
what-domain-write = the change
# The create form's disclosure and submit.
domain-create = Add a domain
# The change form's disclosure and submit.
domain-change = Change this domain
# The note that the server records a domain write with its reason.
domain-admin-note = The server records every change with your reason.
# The note drawn in place of the domain forms without the
# administrator scope.
domain-admin-only = Adding or changing a domain needs the administrator scope, which your sign-in was not granted.
# The link threshold.
setting-link-at = Link at
# The review threshold.
setting-review-at = Review at
# The possible threshold.
setting-possible-at = Possible at
# The automatic link switch.
setting-auto-link = Link automatically
# The blocking keys.
setting-blocking-keys = Blocking keys
# A field's agreement weight.
setting-agreement = { $field }, agreement (m)
# A field's chance agreement weight.
setting-chance = { $field }, chance agreement (u)
# The family name field.
field-family-name = Family name
# The given names field.
field-given-names = Given names
# The birth date field.
field-birth-date = Birth date
# The gender field.
field-gender = Gender
# The postal code field.
field-postal-code = Postal code
# The address field.
field-address = Address
# The telephone field.
field-telephone = Telephone
# The email field.
field-email = Email
# The mother's maiden name field.
field-mothers-maiden-name = Mother's maiden name
# The local identifier field.
field-local-identifier = Local identifier
# The authoritative identifier rule.
rule-authoritative = Authoritative identifier
# What the authoritative identifier rule holds.
rule-authoritative-text = Two records carrying one identifier of an authoritative domain are one patient.
# The hard conflict rule.
rule-hard-conflict = Hard conflict
# What the hard conflict rule holds.
rule-hard-conflict-text = A birth year or a stated gender that differs turns the link into a candidate.
# The local domains rule.
rule-local-domains = Local domains
# What the local domains rule holds.
rule-local-domains-text = An identifier of a local domain never links records across domains.
# A steward viewed a person.
action-view = View
# A steward exported the trail.
action-export = Export
# A steward revealed a value.
action-reveal = Reveal
# An administrator added a domain.
action-domain-create = Domain added
# An administrator changed a domain.
action-domain-update = Domain changed
# A source fed a record.
action-feed = Feed
# A source removed a record.
action-remove = Removal
# A source resolved a duplicate.
action-resolve-duplicate = Duplicate resolved
# A steward merged two persons.
action-merge = Merge
# A steward undid a merge.
action-unmerge = Unmerge
# A steward decided a pair.
action-decide = Decision
# An HL7 v2 message was received.
action-message = HL7 v2 message
# The sensitive switch.
domain-sensitive = Seal values at rest
# The sensitive switch's help.
domain-sensitive-help = A sealed value is stored encrypted; a BSN domain is always sealed.
# The retire switch.
domain-retire = Retire the domain
# The retire switch's help.
domain-retire-help = A retired domain answers as unknown and cannot be brought back.
# The consumer column.
col-consumer = Consumer
# The note drawn in place of the pseudonymisation domain forms
# without the administrator scope.
pseudonym-admin-only = Adding or changing a pseudonymisation domain needs the administrator scope, which your sign-in was not granted.
# The change form's disclosure and submit on the pseudonymisation
# domains page.
pseudonym-change = Change a domain
# The retire switch's help for a pseudonymisation domain.
pseudonym-retire-help = A retired domain mints nothing, answers as unknown, and cannot be brought back.
# The resolve form's title, and the patient tab's link to it.
resolve-title = Resolve a pseudonym
# The resolve form's note.
resolve-note = The answer is the patient the pseudonym belongs to. The server records the resolution as a disclosure, with your reason.
# The resolve form's pseudonym field.
resolve-pseudonym = Pseudonym
# The resolve form's submit.
resolve-submit = Resolve
# The note drawn in place of the resolve form with no active domain.
resolve-no-domain = No active pseudonymisation domain holds a pseudonym to resolve.
# The answer's title.
resolved-title = Pseudonym resolved
# The answer's sentence before the person.
resolved-person = The pseudonym belongs to
# The answer's note that the resolution is a disclosure.
resolved-disclosure = The resolution is in the audit trail as a disclosure, with your reason.
# What a refused resolve shows.
what-pseudonym-resolve = the resolution
# The legend's words for the queue row's side panel.
shortcut-glance = Open the pair in a side panel, or defer it
# A deferral from the queue saved.
queue-deferred = The pair is deferred, and the decision is in the audit trail.
# What a refused decision shows.
what-decision = the decision
# The grade filter's choice of every grade.
queue-any-grade = Any grade
# The domain filter.
queue-domain = Domain system
# The age filter.
queue-older = Waiting at least (days)
# The deferred filter.
queue-show-deferred = Only the deferred pairs
# The deferred filter's help.
queue-show-deferred-help = The pairs put off until a date that has not come yet.
# The Clear filters button.
queue-clear = Clear filters
# The queue with nothing matching its filters.
queue-no-match = No pair matches these filters. Clear a filter to see more.
# Before the time of the last decision.
queue-last-decision = Last decision:
# The pair column.
col-pair = Pair
# The score column.
col-score = Score
# The waiting column.
col-waiting = Waiting since
# A row's side panel opener and its column.
queue-glance = At a glance
# The range of the queue's page.
queue-range = { $first }–{ $last } of { $total }
# The queue's key hint.
queue-keys = j and k move, Enter opens, o opens at a glance
# The two names of a pair.
pair-names = { $a } and { $b }
# Before the date a deferred pair comes back.
queue-back-on = Back on
# The deferral's date field.
defer-until = Defer until
# The Defer action.
defer = Defer
# The queue's previous page.
queue-previous = Previous page
# The queue's next page.
queue-next = Next page
# The side panel's Open to decide.
queue-decide = Open to decide
# The notice that a record changed since the pair was scored.
pair-changed = A record of this pair changed after it was scored. A decision may come back as stale; reload to see the latest versions.
# The compare's card and caption.
pair-compare = The two records
# The score breakdown's card.
pair-breakdown = Why the pair scored this
# The persons' card.
pair-persons = The patients the records are in
# The name of the neighbouring pairs' links.
pair-neighbours = The neighbouring pairs
# The previous pair.
pair-previous = Previous pair
# The next pair.
pair-next = Next pair
# A pair decided as one patient.
pair-linked = Linked
# A pair decided as two patients.
pair-not-a-match = Not a match
# Who decided a pair already decided.
pair-decided = { $how } by { $who }.
# A record's version in the compare.
pair-version = Version { $version }
# A field that agrees.
agreement-agree = agrees
# A field that agrees in part.
agreement-partial = close match
# A field that differs.
agreement-disagree = differs
# A field absent on one side.
agreement-absent = absent
# One field's line in the breakdown.
pair-contribution = { $field }: { $agreement }
# Before the blocking key that produced the pair.
pair-blocking-key = Found by the blocking key
# No blocking key: a shared authoritative identifier named the pair.
pair-no-blocking-key = a shared authoritative identifier
# The hard conflicts that held a shared identifier from linking.
pair-conflicts = A shared authoritative identifier did not link this pair, because these fields conflict: { $conflicts }.
# The card that says a link merges two patients.
pair-merges = Linking merges two patients
# What linking this pair does.
pair-merges-note = The two records sit in two patients. Linking them merges the second patient into the first, which is reversible until the next merge.
# The Link button that asks first.
pair-link-ask = Link…
# The danger dialog's question.
pair-merge-title = Link and merge the two patients?
# The danger dialog's sentence.
pair-merge-message = Patient { $b } is merged into patient { $a }: its identifiers move, its pseudonyms resolve to { $a }, and the subscribers are told.
# The danger dialog's confirm.
pair-link-merge = Link and merge
# Reload the pair.
pair-reload = Reload this pair
# The bar's line after a 409.
pair-stale = Someone decided this pair or changed a record first. Nothing was sent.
# The Link decision.
pair-link = Link
# The Not a match decision.
pair-not-a-match-action = Not a match
# The decision bar's name.
pair-decision = Decision
# The reason's hint.
pair-reason-hint = Why you decided this
# The reason's error.
pair-reason-missing = Give a reason to decide.
# The deferral date's help.
pair-until-help = For Defer only: the day the pair comes back.
# The hint inside a date typed as text.
date-hint = YYYY-MM-DD
# The patient's Merge into action.
patient-merge-into = Merge into…
# The patient's Unmerge action.
patient-unmerge = Unmerge…
# Before the time a patient was made.
merge-made = Made
# The identifiers that move.
merge-identifiers = Identifiers moving
# The pseudonyms that resolve elsewhere.
merge-pseudonyms = Pseudonyms moving
# The subscribers told.
merge-subscriptions = Subscriptions told
# The candidates closed.
merge-candidates = Candidates closed
# A merge or revert refused as stale.
merge-stale = Nothing was changed: a patient changed or the merge was reverted after the preview was read. The preview below is read again.
# What a refused merge or revert shows.
what-merge-write = the merge
# What a refused preview shows.
what-merge-preview = the preview
# The card that names the other patient.
merge-other = The other patient
# What naming the other patient does.
merge-other-note = Name the patient to merge with by the id FerroPIX gave it, from a search or a pair. Nothing is written until you confirm the preview.
# The other patient's field.
merge-other-field = Patient id
# The Preview button.
merge-preview = Preview
# Swap which patient survives.
merge-swap = The other patient survives
# The merge dialog's question.
merge-confirm-title = Merge the two patients?
# The merge dialog's sentence.
merge-confirm-message = { $count } identifiers move to the surviving patient. The merge can be reverted until the survivor's next merge.
# The surviving patient.
merge-survivor = Survives
# The patient merged into it.
merge-merged = Merged into it
# The merge preview's card.
merge-effect = What the merge does
# The merge's rule.
merge-reversible = A merge can be reverted until the survivor's next merge. The reason is kept with it.
# The Merge button that asks first.
merge-ask = Merge…
# No merge to revert.
unmerge-none = No merge into this patient is waiting to be reverted.
# The merge to revert first.
unmerge-first = A later merge of this patient has to be reverted first: merge { $merge }.
# The patient a revert restores.
unmerge-restored = Restored
# The revert dialog's question.
unmerge-confirm-title = Revert the merge?
# The revert dialog's sentence.
unmerge-confirm-message = { $count } identifiers move back to the restored patient, and its pseudonyms resolve to it again.
# The Unmerge button that asks first.
unmerge-ask = Unmerge…
# The revert preview's card.
unmerge-effect = What the revert does
