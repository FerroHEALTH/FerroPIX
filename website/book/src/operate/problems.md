<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Problem types

FerroPIX answers every error outside its FHIR and IHE routes as problem
details, the JSON document of
[RFC 9457](https://www.rfc-editor.org/rfc/rfc9457), with the media type
`application/problem+json`. That covers the [operator API](operator-api.md),
the health and metrics routes, and the refusals the server's HTTP stack
writes itself on those routes: a request past its timeout (`408`), a body
over the limit (`413`), a server at its request limit (`429` or `503`) and a
handler that failed (`500`). The FHIR face under `{base}/fhir` keeps the FHIR
`OperationOutcome` that FHIR R4 prescribes, and the XCPD gateway keeps its
SOAP answers.

A problem looks like this:

```json
{
  "type": "https://ferropix.eu/docs/operate/problems.html#stale",
  "title": "What the request was decided on changed since it was read",
  "status": 409,
  "detail": "the candidate was decided or a record changed since it was read; reload it"
}
```

- `type` names the problem type. Each FerroPIX type is a link to its
  section on this page. `about:blank` means the status code says all there
  is to say, and `title` is then the status phrase, such as `Payload Too
  Large`.
- `title` is the type's summary, the same for every occurrence.
- `status` is the HTTP status of the answer.
- `detail` says what to correct, in a fixed sentence.
- `instance`, on a refusal of the stack and a failed handler, is the
  server's request id as a `urn:uuid:` URI, the id the request log names.

A problem never quotes the request: no identifier, pseudonym, name, query
value, token or path segment you sent appears in it, so a problem is safe to
show and to log. The `X-Request-Id` header of the answer names the request
when you report it. Every problem answer carries `Cache-Control: no-store`.

## The types

| Type | Status | When |
|---|---|---|
| [`invalid-request`](#invalid-request) | `400` | The body or the query is not what the route reads. |
| [`reason-required`](#reason-required) | `400` | A change or a disclosure states no reason, or one too long. |
| [`unauthenticated`](#unauthenticated) | `401` | The request carries no valid access token. |
| [`not-permitted`](#not-permitted) | `403` | The token does not permit the route. |
| [`not-found`](#not-found) | `404` | The person, domain, candidate, merge or source does not exist. |
| [`stale`](#stale) | `409` | What you decided on changed since you read it. |
| [`later-merge`](#later-merge) | `409` | A later merge has to be reverted first. |
| [`retired`](#retired) | `409` | The domain is retired. |
| [`already-held`](#already-held) | `409` | Another domain holds the system or namespace. |
| [`unknown-domain`](#unknown-domain) | `422` | The identifier domain is unknown or retired. |
| [`source-not-admitted`](#source-not-admitted) | `422` | The Source is no client an issuer lists and no Basic user. |
| [`store-unavailable`](#store-unavailable) | `503` | The cross-reference store could not be read. |
| [`audit-unavailable`](#audit-unavailable) | `503` | The audit record could not be stored. |
| [`authorization-unavailable`](#authorization-unavailable) | `503` | The token could not be checked. |

A `413` for a body larger than the server reads, a `415` for a body that is
not `application/json`, a `500` and a `501` are `about:blank` problems.

### `invalid-request`

The request body is not the JSON document the route reads, the query names
a parameter or a value the route does not read, or a field breaks a rule of
the document: a domain without an absolute URI as its system, a merge that
does not name two different persons, a demographics search with no field, a
deferral without a day after today. The detail names the rule and never the
value that broke it. Correct the request and send it again.

### `reason-required`

A candidate decision, a merge or a revert, a domain change, a reveal or a
pseudonym resolution states no reason, or one longer than 500 characters.
State why, in at most 500 characters.

### `unauthenticated`

The request carries no access token, or one the server does not accept:
expired, signed by an issuer it does not know, or addressed to another
audience. The answer carries a `WWW-Authenticate` challenge. Get a token
from the authorization server ([Security and audit](security-and-audit.md)).

### `not-permitted`

The token is valid but its scopes do not admit the route: an operator route
needs the steward scope, the administrator scope, or either, as the
[route table](operator-api.md#routes) says. The challenge names
`insufficient_scope`.

### `not-found`

The id in the path names no person, identifier domain, pseudonymisation
domain, candidate, merge or source, or the handle of a reveal names nothing
the person holds, or a pseudonym resolves to no person. An id that is not a
UUID is answered the same way.

### `stale`

A candidate decision, a merge or a revert names versions of the records or
persons that are no longer current: someone else decided the candidate, a
feed changed a record, or a merge moved a person since you read it. Nothing
was changed. Read the candidate or the preview again and decide on what it
says now.

### `later-merge`

Only the latest merge into a survivor can be reverted. The revert preview
names the later merge; revert that one first.

### `retired`

The identifier domain or the pseudonymisation domain is retired, and a
retired domain stays as it was retired: it takes no change and mints no
pseudonym.

### `already-held`

Another identifier domain holds the system, or another pseudonymisation
domain or an identifier domain holds the namespace. Choose another, or
change the domain that holds it.

### `unknown-domain`

An identifier search names a system that no identifier domain holds, or
whose domain is retired.

### `source-not-admitted`

A domain write names a Source that no `[[auth.issuer]]` lists among its
`clients` and that is no Basic user. Admit the client in the configuration
first ([Configuration](configuration.md)).

### `store-unavailable`

The server could not read or write the PostgreSQL cross-reference store.
Nothing was changed. The readiness route names the store's state
([Operations](operations.md)); send the request again once it is up.

### `audit-unavailable`

The BALP audit record of the request could not be stored, so the server did
not process the request: a read is not answered and a change is not made
when it cannot be audited. The readiness route names the audit spool's
state ([Operations](operations.md)).

### `authorization-unavailable`

The server could not check the token, because the authorization server
could not be read. Send the request again later.
