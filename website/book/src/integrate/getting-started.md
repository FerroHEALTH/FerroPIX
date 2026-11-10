<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Quickstart

This page runs FerroPIX on your machine with the compose quickstart, feeds
one synthetic patient as a hospital's Patient Identity Source would, and
asks for it through PIXm and PDQm. It takes a few minutes and needs a
checkout of the repository, Docker Compose (or Podman with a compose
provider), `curl`, `jq` and `openssl`.

> [!NOTE]
> Every command on this page is a step of
> [`scripts/quickstart/tour.sh`](https://github.com/FerroHEALTH/FerroPIX/blob/main/scripts/quickstart/tour.sh),
> which CI runs against the images each change builds, so the commands and
> the answers below are the ones the current code gives. Run the script to
> do every step at once; it switches the usage report off, since a tour is
> no installation.

## Start the server

The quickstart reads every secret from a file under `./secrets`, never from
the environment. From the root of the checkout, make the five files:

```sh
{{#include ../../../../scripts/quickstart/tour.sh:secrets}}
```

Then start the stack and ask for readiness:

```sh
{{#include ../../../../scripts/quickstart/tour.sh:start}}
```

`compose.yaml` runs the published server image beside PostgreSQL 18 and a
development OpenID Provider, with the configuration
`app/ferropix-server/quickstart.toml`: three identifier domains (a
synthetic hospital, `urn:oid:2.999.1.1`, the BSN and the pseudonymised BSN),
one Source client for all three, `quickstart-source`, and the operator API.
Readiness answers `{"state":"up",…}` once the store, the audit spool and the
outbox are up. Every port binds `127.0.0.1`
([Deployment](../operate/deployment.md#the-compose-quickstart)).

> [!WARNING]
> The development issuer signs a token for any client that asks and signs
> in any user name as a data steward. It is for synthetic data on your own
> machine. A deployment names its own `https` issuer
> ([Configuration](../operate/configuration.md#auth)).

## Get a token

A Source is a machine, so it asks the issuer for a client credentials token
(RFC 6749 §4.4). This one carries the scope to feed (`system/Patient.cud`)
and the scope to query (`system/Patient.rs`):

```sh
{{#include ../../../../scripts/quickstart/tour.sh:token}}
```

## Feed a patient (ITI-104)

The Source of the hospital domain adds patient `H-100001`, whose `Patient`
also carries a BSN from the synthetic range no person holds. The
conditional update names the identifier the record is fed under (PIXm 3.1.0
§2:3.104.4.1):

```sh
{{#include ../../../../scripts/quickstart/tour.sh:feed}}
```

The server answers `201 Created` with a `Location` naming the new person
and an `OperationOutcome` whose one `information` issue says the identifier
was added. The BSN is in an `authoritative` domain, so a record of another
hospital that carries the same BSN links to this person without a steward.
Both identifiers are sealed at rest
([The Dutch identifiers](dutch-identifiers.md)).

## Ask for the patient's other identifiers (ITI-83)

A PIXm Consumer that knows the hospital's identifier asks which BSN the
person holds. It posts the parameters in a `Parameters` body, so the
identifier is in no URL:

```sh
{{#include ../../../../scripts/quickstart/tour.sh:identifiers}}
```

The answer is a `Parameters` with one `targetIdentifier` per identifier in
the domains asked for and a `targetId` naming `Patient/{id}`
(PIXm 3.1.0 §2:3.83.4.2.2.1); the command prints the BSN:

```json
{
  "system": "http://fhir.nl/fhir/NamingSystem/bsn",
  "value": "999990007"
}
```

## Find the patient by demographics (ITI-78)

A PDQm Consumer that knows the family name and the birth date searches for
the patient, posting the criteria as a form so they stay out of the URL
(PDQm 3.2.0 §2:3.78.4.1.2):

```sh
{{#include ../../../../scripts/quickstart/tour.sh:demographics}}
```

The answer is a `searchset` Bundle with a `total` of 1. `proefsma` finds
`Proefsma`, because a string parameter matches the start of a name in any
case. [The PDQm Supplier](pdqm-supplier.md) lists every parameter, and
`$match` (ITI-119) scores a patient you describe.

## Open the console

The `console` profile adds the operator console on `http://localhost:3000/`:

```sh
{{#include ../../../../scripts/quickstart/tour.sh:console}}
```

Sign in with any user name at the development issuer's form. The
dashboard counts what you fed, and Search finds the patient by its
identifier or demographics
([The operator console](../operate/console.md)).

## Stop it

```sh
docker compose --profile console down --volumes
```

`--volumes` removes the database and the server's state with the
containers.

## Next

- [The PIXm Manager](pixm-manager.md), [the PDQm Supplier](pdqm-supplier.md)
  and the other pages under Integrate: every transaction, case by case.
- [Configuration](../operate/configuration.md): your own domains, issuers
  and Sources.
- [Deployment](../operate/deployment.md): the images, compose and the Helm
  chart for a real installation.
- [The published crates](../crates/index.md): calling a PIX Manager, a PDQm
  Supplier or another IHE actor from your own Rust code.
