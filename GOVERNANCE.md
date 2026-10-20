<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Governance

How decisions get made in FerroPIX, who makes them, and how that changes.

## Current structure: one maintainer

FerroPIX has a single maintainer ([MAINTAINERS.md](MAINTAINERS.md)) who holds
final say on every technical decision: what gets built, merged and released.
There is no steering committee and no vote.

Cadasto B.V. is the Licensor and copyright holder (`LICENSE`) and handles the
business side, the commercial licence included: write to
[info@cadasto.com](mailto:info@cadasto.com) or use
<https://www.cadasto.com/contact/>. The maintainer handles the technical side.

## The family is the first customer

FerroPIX serves the FerroHEALTH products: FerroEHR resolves patients through
it and FerroFED pins its identity crates. A change that alters what a caller
sends, receives or must configure names that effect in its pull request and
changelog entry.

## Where decisions are recorded

| Kind of decision          | Where it lives                                                  |
|---------------------------|-----------------------------------------------------------------|
| What to build, and why    | the issue, with its acceptance criteria                         |
| How a transaction behaves | the pull request, citing the specification section              |
| The scope and the order   | `CLAUDE.md` and the milestones                                  |
| A release's contents      | its `CHANGELOG.md` section and its milestone                    |

## Becoming a maintainer

Sustained, reviewed contributions over several releases, a demonstrated grasp
of the specifications and the privacy bar, and the maintainer's invitation.
A new maintainer gains a row in [MAINTAINERS.md](MAINTAINERS.md) and their
handle in [`.github/CODEOWNERS`](.github/CODEOWNERS).

## Changing this document

By pull request, merged by the maintainer.
