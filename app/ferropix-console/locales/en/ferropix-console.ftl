# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# The words of the FerroPIX Console, in English, the reference catalog.

# The marks a number is grouped and written with in this language.
number-group = ,
number-decimal = .

product = FerroPIX
console-word = Console
console-name = FerroPIX Console

nav-label = Console
nav-work = Work
nav-patients = Patients
nav-registry = Registry
nav-trail = Trail
nav-console = Console
queue-depth-unknown = Not known: the server did not answer

search-label = Find a patient
search-placeholder = Identifier, name or birth date
search-find = Find

pill-up = Server up
pill-unreachable = Unreachable
pill-refused = Refused { $status }
pill-degraded = Server answered { $status }
pill-detail = { $base }
menu-line = Signed in at { $provider }
steward-unnamed = Data steward

dashboard = Dashboard
dashboard-sub = The day's state of the index.
queue = Review queue
queue-sub = Pairs of records the matcher could not decide. A row is one pair; open it to compare and decide.
pair = Compare and decide
pair-sub = Your decision is recorded with your name and reason.
search = Search
search-sub = Find a patient by one identifier or by demographics; results stay in this page and never reach a URL.
patient = Patient
patient-sub = Every identifier of this patient, the records side by side, the history and the pseudonyms.
merge = Merge
merge-sub = Nothing is written until you confirm; the table is the server's answer for this preview.
unmerge = Unmerge
unmerge-sub = The latest merge of this patient can be reverted; nothing is written until you confirm.
sources = Sources
sources-sub = The systems allowed to feed records, and how each connects.
source-sub = Source · how it connects and what it last sent, never a value.
domains = Identifier domains
domains-sub = The assigning authorities, each with the one source that feeds it; a sensitive domain is masked everywhere.
domain-sub = Identifier domain · its definition and its feed activity, never a value.
pseudonym-domains = Pseudonymisation domains
pseudonym-domains-sub = The namespaces pseudonyms are created in, and the one place a pseudonym is resolved.
configuration = Configuration
configuration-sub = This deployment's settings and their revisions.
audit = Audit trail
audit-sub = Every steward action: who made it, when and why.
about = About
about-sub = The server this console talks to, the versions and the manufacturer.
preferences = Your preferences

what-dashboard = the dashboard
what-queue = the queue
what-pair = this pair
what-patient = this patient
what-merge = the merge preview
what-unmerge = the unmerge preview
what-sources = the sources
what-source = this source
what-domains = the identifier domains
what-domain = this domain
what-pseudonym-domains = the pseudonymisation domains
what-configuration = the configuration
what-audit = the audit trail
what-search = the results

unreachable-connect = the FerroPIX server refused the connection, so { $what } cannot be shown.
unreachable-timeout = the FerroPIX server did not answer in time, so { $what } cannot be shown.
unreachable-other = the FerroPIX server could not be reached, so { $what } cannot be shown.
refused-call = The FerroPIX server refused the console's call, so { $what } cannot be shown.
not-served = The FerroPIX server answers, but this version serves no operator API yet, so { $what } cannot be shown.
not-served-route = The page reads { $route } once the server serves it.
try-again = Try again
last-tried = Last tried
away-title = While the server is away
away-body = This console keeps no patient data, so no page can be drawn until the server answers. Your session is kept: you do not need to sign in again.
technical-detail = Technical detail
detail-failure = Failure
detail-request-id = Request id
detail-time = Time
detail-route = Route
failure-connect = The server refused the connection
failure-timeout = No answer came in time
failure-other = The call failed before an answer came
failure-status = The server answered { $status }
person-id = Person { $person }

find-patient = Find a patient
search-by = Search by
by-identifier = By identifier
by-demographics = By demographics
domain = Domain
domains-not-known = Not known
identifier = Identifier
identifier-help = Masked as you type and sent in the request body, never in the address bar.
family-name = Family name
given-names = Given names
birth-date = Birth date
birth-date-hint = YYYY-MM-DD
gender = Gender
gender-any = Any
gender-female = Female
gender-male = Male
gender-other = Other
gender-unknown = Unknown
postal-code = Postal code
exact-names = Exact names only
exact-names-help = On: only exact spellings count.
search-submit = Search
nothing-searched = Nothing was searched, and nothing you typed left this console.

version-console = Console
version-server = Server
version-fhir = FHIR
version-ferroui = FerroUI
not-known = Not known
licence = Business Source License 1.1
documentation = The FerroPIX book
capability-title = The CapabilityStatement
capability-line = What the server declares at GET [base]/fhir/metadata, as every FHIR client reads it.
what-capability = its CapabilityStatement

reveal-for = Revealed identifiers stay for
reveal-for-desc = How long a revealed identifier shows before it is masked again; the server caps every reveal at 30 s.
reveal-seconds = { $count ->
    [one] { $count } second
   *[other] { $count } seconds
}
hide-on-leave = Hide them when I leave the page
hide-on-leave-desc = Masks every revealed identifier as soon as you open another page or tab.
hide-on-leave-switch = Hide revealed identifiers when I leave the page
accent-plum = Plum

sign-in = Sign in
sign-in-title = Sign in to the patient index
sign-in-with = Sign in with your identity provider
sign-in-taking = Taking you to your identity provider
sign-in-caption = The console for the data stewards of the patient index. No patient data is shown before you sign in.
signed-out = You are signed out of this browser.
session-expired = Your session ended. Sign in again to go on where you were.
sign-in-unavailable = Sign-in is not set up on this console. Ask your administrator to configure an identity provider.

not-found-title = Page not found
not-found-body = The console has no page at this address. It may be mistyped, or name a person or a domain this index does not hold.
not-found-back = Back to the dashboard
not-found-search = Find a patient

session-unreadable = The console's session store cannot be read; try again in a moment.
sign-in-not-configured = sign-in is not configured on this console
sign-in-redirect-unwritten = the sign-in redirect could not be written
no-sign-in-pending = no sign-in is pending for this browser
sign-in-state-mismatch = the sign-in's state did not match
provider-did-not-sign-in = the identity provider did not sign you in
redirect-carries-no-code = the redirect carries no authorization code
id-token-did-not-verify = the identity provider's ID Token did not verify
sign-in-not-completed = the sign-in could not be completed at the identity provider
session-not-begun = the session could not be begun
sessions-full = the console holds as many sessions as it allows; try again later
sign-in-session-not-begun = the sign-in could not be begun
session-not-ended = the session could not be ended
sign-out-redirect-unwritten = the sign-out redirect could not be written
foreign-request-refused = the console takes this request only from its own pages
preference-refused = the preference or its value is not one the console offers
