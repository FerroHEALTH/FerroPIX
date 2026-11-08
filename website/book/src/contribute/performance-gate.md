<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The performance gate

Every pull request that changes what the server is built from runs the
`perf` job of CI, a required part of `conclusion`. It also runs on every
push to `main`, and the release workflow runs it on the release commit
before the release is drafted. The job measures the release build of
`ferropix` with
[`ferrohealth-perf`](https://github.com/FerroHEALTH/FerroSYS/issues/249),
the performance gate every FerroHEALTH server shares. It fails when the
server is busy at rest, or slower, heavier or busier under load than the
build of its base, measured beside it on the same runner in the same job.

The gate exists because of
[FerroPIX #490](https://github.com/FerroHEALTH/FerroPIX/issues/490): the
outbox sender polled PostgreSQL about 8,000 times a second while it had
nothing to send, and no check caught it before it shipped.

## Why the head is judged against its base

GitHub's hosted runners differ: two runners that report the same CPU model
and memory measured one commit 25 to 45 % apart in p50 latency and CPU per
request
([FerroEHR #5095](https://github.com/FerroHEALTH/FerroEHR/issues/5095)).
A baseline stored from one machine failed unchanged code on another, so the
gate keeps none. It builds the base of the change and the change itself,
the head, runs both on one runner, and judges the head over the base. The
difference between machines reaches both alike and cancels.

The base is:

- for a pull request, the merge base of the commit CI checks out with the
  pull request's base branch, which is `main` as the pull request was
  merged into it;
- for a merge group, its base on `main`;
- for a push to `main`, the commit before the push;
- for the release commit and a manual dispatch, the previous release tag.

## What it runs

`scripts/perf/gate.sh build` builds the release server of the base, from
`git archive` of the base commit under `target/perf-base/`, and of the
head. `scripts/perf/gate.sh run` starts PostgreSQL and the stub OpenID
Provider from the test image pins of `docs/VERSIONS.md`, then both servers,
each with a database of its own: the head on port 8080 and MLLP port 2575
with `perf/ferropix.toml`, the base on 8081 and 2576 with the base's own
`perf/ferropix.toml` when it has one. The PIXm Manager, the PDQm Supplier,
the PMIR Registry with its feed, the MLLP listener, the operator API and the
outbox sender are all on. The script seeds the two patients of the
testkit's ITI-104 fixtures into each server and runs `ferrohealth-perf ab`
over `perf/plan.toml`:

1. **Idle.** No request for 60 seconds. The head's CPU time must stay
   under 2 % of one core and its database under one transaction a second.
   These limits are absolute and compared with nothing: the outbox sender
   looks at the outbox at most every 30 seconds when it has nothing to
   send.
2. **Load, in pairs.** Four pairs, each a load phase against both servers
   at once: 50 arrivals a second to each for 60 seconds after a 10-second
   warm-up, open loop, one server's arrivals half an interval after the
   other's, the server that leads alternating by pair. Run side by side,
   both arms see the same runner, whose speed can move by a third from one
   minute to the next. The arrivals go over PIXm ITI-83 by `GET`
   and by `POST`, the ITI-104 feed, PDQm ITI-78, the ITI-119 match, and
   the HL7 v2 feed, ITI-8, as an `ADT^A01` over MLLP
   ([#509](https://github.com/FerroHEALTH/FerroPIX/issues/509)). Every
   request must be answered with the status the plan names, and every
   ITI-8 message with an `AA` acknowledgment of its own MSH-10.

Then the script plants an outbox poller that never waits beside the head,
the shape of #490, and runs the idle phase of `perf/busy-loop.toml` again.
That run must fail on idle transactions, or the job fails: it proves the
gate still catches the case it was built for.

## Reading the report

Each load metric (throughput, latency p50, p95 and p99 per group, peak
memory, CPU time per request and transactions per request) is the median of
an arm's four phases. Its noise floor is the worse of the two arms' spread,
how far an arm's worst phase strayed from its median. A metric breaches when
the head's median is worse than the base's by more than the larger of its
tolerance and that floor; the tolerances are the plan's `[tolerance]`
table: latency and CPU 25 %, throughput 20 %, memory 20 %, transactions per
request 10 %. It fails only when the head also breaches against the base in
most of the pairs, so a stall in one phase clears and a regression, which
repeats in every pair, fails.

The job summary holds the tool's report: the outcome, the base commit, the
head's idle limits, and per metric the base, the head, their ratio, the
tolerance, the floor, the threshold and in how many pairs the head
breached. Requests the head answered other than expected are listed and
fail the gate; those the base answered other than expected, such as a
route the change adds, are notes, and a metric only one arm measured is
noted and not judged. Each attempt uploads its reports as
`perf-report-attempt-<n>`: the JSON reports under `target/perf/`, with every
phase's histograms, both servers' logs and the base commit, so
`ferrohealth-perf compare --report target/perf/latest.json` judges a report
again without a server.

The tool exits 0 for a pass, 1 for a breach or a request answered other
than expected, and 2 for a run it cannot judge: a phase whose generator
could not hold its rate, whose second half differs from its first, or that
stalled, and a base that fell behind the schedule.

## Retries

Some runners stall every route for a whole job: on an AMD EPYC 9V74 the
tails reach 100 to 300 ms while the p50 holds
([FerroPIX #511](https://github.com/FerroHEALTH/FerroPIX/issues/511)). A
phase where every route's p99 is over ten times its p50 at once is a stalled
runner: it is reported, left out of the judgement, and the run exits 2. An
attempt that ends with exit 2 runs again on a fresh runner on any CPU model,
since a run that cannot be judged says nothing about the code. An attempt
that ends with exit 1 runs again only on a CPU model the perf workflow lists
in `STALL_PRONE`. At most two retries run, and the gate passes when the last
attempt passed. A breach on any other model fails at once. The job summary
names each attempt's CPU model, its verdict and why a retry ran.

## Changing the plan or the tolerances

The plan is the head's: a change to `perf/plan.toml` is measured on both
servers in the same pull request. A tolerance is the product's and moves
only in a pull request that says why.
