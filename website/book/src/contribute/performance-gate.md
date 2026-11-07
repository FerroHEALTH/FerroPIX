<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The performance gate

Every pull request that changes what the server is built from runs the
`perf` job of CI, a required part of `conclusion`. It also runs on every
push to `main`, and the release workflow runs it on the release commit
before the release is drafted. The job measures the release build of
`ferropix` with
[`ferrohealth-perf`](https://github.com/FerroHEALTH/FerroSYS/issues/249),
the performance gate every FerroHEALTH server shares, and fails when the
server is busy at rest, or slower, heavier or busier under load than the
committed baseline.

The gate exists because of
[FerroPIX #490](https://github.com/FerroHEALTH/FerroPIX/issues/490): the
outbox sender polled PostgreSQL about 8,000 times a second while it had
nothing to send, and no check caught it before it shipped.

## What it runs

`scripts/perf/gate.sh run` starts PostgreSQL and the stub OpenID Provider
from the test image pins of `docs/VERSIONS.md`, then the server with
`perf/ferropix.toml`: the PIXm Manager, the PDQm Supplier, the PMIR
Registry with its feed, the MLLP listener, the operator API and the outbox
sender, all on. It seeds the two patients of the testkit's ITI-104
fixtures and runs `perf/plan.toml` in two phases:

1. **Idle.** No request for 60 seconds. The server's CPU time must stay
   under 2 % of one core and the database under one transaction a second.
   These limits are absolute and compared with no baseline: the outbox
   sender looks at the outbox at most every 30 seconds when it has nothing
   to send.
2. **Load.** 50 requests a second for 60 seconds after a 10-second warm-up,
   open loop, over PIXm ITI-83 by `GET` and by `POST`, the ITI-104 feed,
   PDQm ITI-78 and the ITI-119 match. Every request must be answered with
   the status the plan names. Throughput, latency p50, p95 and p99 per
   group, peak memory, CPU time per request and transactions per request
   are compared with `perf/baseline.toml`.

Then the script plants an outbox poller that never waits beside the server,
the shape of #490, and runs the idle phase of `perf/busy-loop.toml` again.
That run must fail on idle transactions, or the job fails: it proves the
gate still catches the case it was built for.

## Reading the report

A load metric that breaches is judged again on up to two fresh load
phases, each after its own warm-up, and the gate fails only when it breaches
in most of the runs, so a one-off stall clears and a real regression fails
every run; the check says in how many runs it breached. A phase where every
route's p99 is over ten times its p50 at once is a stalled runner: it is
reported and not judged, and the run exits 2.

The job summary holds the tool's report: the outcome, every breach with the
metric, the baseline, the measured value and the allowed threshold, every
check, and the latency of each group with its 95 % interval. A metric fails
when it is worse than the baseline by more than the larger of its tolerance
(latency and CPU 25 %, throughput 20 %, memory 20 %, transactions per
request 10 %) and the noise floor the baseline recorded. The `perf-report`
artifact holds the JSON reports under `target/perf/` with the histograms,
so `ferrohealth-perf compare --report target/perf/latest.json` judges a
report again without a server.

The tool exits 0 for a pass, 1 for a breach or a request answered with
another status, and 2 for a run it cannot judge: no baseline, a baseline
written for another plan, a generator that could not hold its rate, or a
window whose second half differs from its first.

## Updating the baseline

GitHub's hosted runners mix CPU generations and memory sizes, and the same
commit measures 25 to 45 % slower on one than on another, so
`perf/baseline.toml` holds one `[[baseline]]` entry per host shape, keyed by
the CPU model in `/proc/cpuinfo` and the memory size to the GiB. A run is
judged against the entry of its runner's shape. A runner whose shape has no
entry cannot be judged: the job exits 2 and names the shape, and the idle
limits still judge the run.

The entries are measured on the CI runners, never on a workstation. An
update measures five load phases, each after its own warm-up, and writes
each metric as their median and its noise floor as how far the worst phase
strayed from it, so a tail that moves from run to run is judged against the
movement it showed. To write or add one, run the job with `update: true` on
the `perf` job's call of `.github/workflows/perf.yml`, take
`perf/baseline.toml` from the `perf-report` artifact, and commit it; it
holds the committed entries and the run's own. Re-run the job until every
host shape the runners serve has its entry. On the same plan an update only
moves each metric of the runner's entry to the better of the entry and the
run, and leaves the other entries as they were, so every entry is a ratchet.
A change to the plan's rate, windows or request set writes the entry afresh.
Raising a metric is a hand edit in a pull request that says why.

Machines of one shape still differ from each other, so re-run the update job
several times and fold the runs into the committed file with
`ferrohealth-perf merge --baseline <file> <job>...`: each entry then takes the median
over the jobs, and its noise floor the spread between them.

## The limits of the request set

`ferrohealth-perf` sends HTTP, so the HL7 v2 feed (ITI-8) over MLLP is
measured only at rest, with its listener running in the idle phase. Load
over MLLP is tracked in
[#509](https://github.com/FerroHEALTH/FerroPIX/issues/509).
