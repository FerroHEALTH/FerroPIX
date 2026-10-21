<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Configuration

There is no FerroPIX server to configure yet. The server, with its
configuration file and environment variables, is planned in the
[v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3), and
its reference replaces this page's first section when it lands.

What you configure today is the crates, in your own code: which features you
compile, and how you build each client.

## Features

Every profile and function is a feature, and none is on by default.

| Crate | Features | Pages |
|---|---|---|
| `ihe-iti` | `pixm`, `pdqm`, `pmir`, `mcsd`, `xcpd`, and the audit records `balp` (with a FHIR profile) and `atna` (with `xcpd`) | [ihe-iti](../crates/ihe-iti.md#features) |
| `nl-generic-functions` | `nvi`, `mitz`, `lrza`, `nuts-auth`, and `authorizer` | [nl-generic-functions](../crates/nl-generic-functions.md#features) |

## Building a client

The settings below hold for every HTTP client of both crates. The API
reference on docs.rs has each constructor and builder method.

| Setting | Where | What to give it |
|---|---|---|
| The server's base URL | the constructor, as a `url::Url` | The FHIR base of the Manager, Supplier, Registry or directory. Its userinfo, if any, is shown as `***` in `Debug`. |
| The HTTP client | the constructor, as a `reqwest::Client` | Built with `redirect::Policy::none()`, and with the mutual TLS identity where the profile asks for one (XCPD, Mitz). |
| A timeout per exchange | each call, as a `Duration` | The bound of the whole exchange; with an audit feature it bounds the audit record as well. |
| Access tokens | `.with_authorizer(…)` | An authorizer that makes the headers of each request (IUA ITI-72); see [Security and audit](security-and-audit.md#access-tokens). |
| The audit recorder | `.audited(…)`, with `balp` or `atna` | The recorder that routes each record to your audit repository; see [Security and audit](security-and-audit.md#audit). |
| The PIXm invocation | `.invoked_by(Invocation::Post)` | `POST`, as every FerroHEALTH client calls a Manager, so the identifier stays out of the URL; leave the `GET` default only for a Manager that refuses the `POST` form ([Lookups use POST](../crates/ihe-iti.md#lookups-by-a-patient-identifier-use-post)). |
| The mCSD budget | `budget::Budget` | The deadline and the caps on pages, bytes and entries of one walk; by default 200 pages, 64 MiB and 50,000 entries. |
