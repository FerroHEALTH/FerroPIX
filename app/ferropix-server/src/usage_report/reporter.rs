// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The usage report client: decides whether the report runs, assembles a
//! [`Report`], sends it, and schedules the start and daily events.
//!
//! The channel is outbound only: nothing the collector answers is read beyond
//! the status code, and no answer changes configuration or behaviour. A send
//! is bounded by the timeouts and the body cap `FerroPULSE` `docs/report-v2.md`
//! §Transport names, and a failure is logged and tried again at the next
//! schedule. The background task is detached from boot and from readiness, so
//! a slow or unreachable collector reaches neither. No specification governs
//! this: our own design.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ferrohealth_health::document::State;
use ferrohealth_health::lifecycle::Lifecycle;
use ferrohealth_health::registry::Registry;
use http::StatusCode;
use tokio::task::JoinHandle;

use crate::config::Profile;
use crate::config::section::usage_report::{Deployment, UsageReportSettings};
use crate::usage_report::payload::{
    CpuBucket, DomainsBucket, Event, LicenceGrant, MAX_UPTIME_S, MemoryBucket, Product, Report,
    SchemaV2, Service, Status, report_git_sha,
};
use crate::usage_report::state::{Instance, StateDir, StateError};
use crate::usage_report::window::{Counts, Window};

/// The book section that carries the full notice.
pub const NOTICE_URL: &str = "https://ferropix.eu/docs/operate/configuration.html#usage_report";

/// The variable that switches the report off.
pub const SWITCH: &str = "FERROPIX__USAGE_REPORT__ENABLED=false";

/// The variable that names the state directory.
pub const STATE_DIR_VARIABLE: &str = "FERROPIX__USAGE_REPORT__STATE_DIR";

/// The commit this binary was built from, or `unknown` outside a checkout.
pub const COMMIT: &str = env!("FERROPIX_BUILD_COMMIT");

/// The largest body the collector accepts, in bytes.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// How long after boot the first daily report is attempted.
pub const FIRST_DAILY_AFTER: Duration = Duration::from_mins(10);

/// How long a daily attempt that did not reach the collector waits to try
/// again.
pub const RETRY_AFTER_FAILURE: Duration = Duration::from_hours(1);

/// How long a connection to the collector may take to open.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long one send may take in total.
pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(10);

/// The readiness indicator of the store, whose state the report carries as
/// `db_ok`.
const DATABASE_INDICATOR: &str = crate::store::indicator::NAME;

/// The readiness indicator of the audit spool, whose state the report carries
/// as `audit_spool_ok`.
const AUDIT_SPOOL_INDICATOR: &str = "audit_spool";

/// Why the report is off for this process.
#[derive(Debug)]
pub enum Off {
    /// `[usage_report] enabled = false`.
    Switch,
    /// A CI variable is set; it names which.
    Ci(&'static str),
    /// The deployment names no state directory.
    NoStateDir,
    /// The state directory does not serve.
    StateDir {
        /// The directory.
        path: PathBuf,
        /// The cause.
        error: StateError,
    },
}

/// What the report needs from disk when it runs.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// The state directory, writable.
    pub state: StateDir,
    /// The instance id, created on the first start.
    pub instance: Instance,
}

/// Decides whether the report runs and, when it does, opens the state
/// directory and reads or creates the instance id.
///
/// `ci` is the CI variable the process environment sets, when one is
/// ([`crate::usage_report::ci_of`]). The switch is read first, then CI, then
/// the state directory, so an explicit `enabled = true` never overrides a CI
/// environment. Nothing is written while the report is off.
///
/// # Errors
/// Returns [`Off`] with the reason the report is off.
pub fn prepare(config: &UsageReportSettings, ci: Option<&'static str>) -> Result<Prepared, Off> {
    if !config.enabled {
        return Err(Off::Switch);
    }
    if let Some(variable) = ci {
        return Err(Off::Ci(variable));
    }
    let Some(root) = &config.state_dir else {
        return Err(Off::NoStateDir);
    };
    let failed = |error| Off::StateDir {
        path: root.clone(),
        error,
    };
    let state = StateDir::open(root).map_err(failed)?;
    let instance = state.instance().map_err(failed)?;
    Ok(Prepared { state, instance })
}

/// Returns the boot line: whether the report is on, what it carries and how
/// to switch it off, or why it is off.
#[must_use]
pub fn announcement(decision: &Result<Prepared, Off>) -> String {
    match decision {
        Ok(_) => format!(
            "usage report ON: instance id, version, licence type, deployment, uptime, host size, \
             profile, the services switched on, the identifier domains as a range, the readiness \
             of the store and the audit spool, and coarse request latency per route group, at \
             each start and once a day; no identifiers, domain names, host names or request \
             paths. Off: {SWITCH}. Details: {NOTICE_URL}"
        ),
        Err(Off::Switch) => format!(
            "usage report OFF: switched off ({SWITCH}); nothing is sent. Details: {NOTICE_URL}"
        ),
        Err(Off::Ci(variable)) => format!(
            "usage report OFF: CI detected ({variable}); nothing is sent. Details: {NOTICE_URL}"
        ),
        Err(Off::NoStateDir) => format!(
            "usage report OFF: no state directory ({STATE_DIR_VARIABLE} is unset), and without a \
             stable instance id each restart would count as a new installation; nothing is sent. \
             On: mount a writable volume and set {STATE_DIR_VARIABLE} to it. Details: {NOTICE_URL}"
        ),
        Err(Off::StateDir { .. }) => format!(
            "usage report OFF: the state directory ({STATE_DIR_VARIABLE}) cannot be written, and \
             without a stable instance id each restart would count as a new installation; \
             nothing is sent. On: mount a writable volume and set {STATE_DIR_VARIABLE} to it. \
             Details: {NOTICE_URL}"
        ),
    }
}

/// Logs the one INFO boot line [`announcement`] writes.
pub fn announce(config: &UsageReportSettings, decision: &Result<Prepared, Off>) {
    let line = announcement(decision);
    match decision {
        Ok(_) => tracing::info!(endpoint = %config.endpoint, "{line}"),
        Err(Off::StateDir { path, error }) => tracing::info!(
            path = %path.display(),
            error = %crate::chain(error),
            "{line}"
        ),
        Err(_) => tracing::info!("{line}"),
    }
}

/// The facts about this build and deployment a report states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The server version.
    pub version: String,
    /// The commit, in the contract's form.
    pub git_sha: String,
    /// The licence grant type.
    pub licence: LicenceGrant,
    /// How the server was deployed.
    pub deployment: Deployment,
}

impl Identity {
    /// Returns the identity of this build under `deployment`.
    ///
    /// The server installs no commercial licence, so it runs under what the
    /// Business Source License grants everyone, the non-commercial grant.
    #[must_use]
    pub fn of_build(deployment: Deployment) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            git_sha: report_git_sha(COMMIT),
            licence: LicenceGrant::NonCommercial,
            deployment,
        }
    }
}

/// The state facts the configuration and the store fix at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The deployment profile.
    pub profile: Profile,
    /// The services the configuration switches on, unique and sorted.
    pub services: Vec<Service>,
    /// The identifier domains the store holds, bucketed.
    pub domains: DomainsBucket,
}

impl Facts {
    /// Returns the facts of `profile` with `services` switched on and
    /// `domains` identifier domains in the store.
    #[must_use]
    pub fn new(profile: Profile, mut services: Vec<Service>, domains: usize) -> Self {
        services.sort_unstable();
        services.dedup();
        Self {
            profile,
            services,
            domains: DomainsBucket::of(domains),
        }
    }
}

/// What the report reads from the running process.
#[derive(Debug, Clone)]
pub struct Sources {
    /// The facts fixed at start.
    pub facts: Facts,
    /// The readiness registry, read at each report.
    pub health: Registry,
    /// The process's lifecycle, which the readiness document names.
    pub lifecycle: Lifecycle,
    /// The window the request log fills.
    pub window: Arc<Window>,
}

/// What a start attempt did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    /// The report was sent and the collector answered with this status.
    Sent(StatusCode),
    /// A start report of this instance went out in the last ten minutes.
    RateLimited,
}

/// Why a report was not sent.
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    /// The HTTP client does not build.
    #[error("cannot build the HTTP client for the collector")]
    Client(#[source] reqwest::Error),
    /// The request did not complete: no connection, a timeout, or a broken
    /// exchange.
    #[error("cannot reach the collector")]
    Request(#[source] reqwest::Error),
    /// The report did not serialize.
    #[error("the report does not serialize")]
    Json(#[source] serde_json::Error),
    /// The rendered report exceeds the collector's body limit.
    #[error("the report body of {0} bytes exceeds the {MAX_BODY_BYTES}-byte limit")]
    TooLarge(usize),
    /// The state directory could not record the start.
    #[error("the state directory")]
    State(#[source] StateError),
    /// The blocking task touching the state directory did not finish.
    #[error("the state directory task")]
    Task(#[source] tokio::task::JoinError),
}

/// The usage report client of one process.
#[derive(Debug)]
pub struct Reporter {
    endpoint: url::Url,
    client: reqwest::Client,
    identity: Identity,
    prepared: Prepared,
    sources: Sources,
    started: Instant,
}

impl Reporter {
    /// Creates the client for `prepared`, reading the state facts, the
    /// readiness and the window from `sources`.
    ///
    /// `started` is when this process started; reports state the uptime from
    /// it.
    ///
    /// # Errors
    /// Returns [`ReportError::Client`] when the HTTP client does not build.
    pub fn new(
        config: &UsageReportSettings,
        prepared: Prepared,
        sources: Sources,
        started: Instant,
    ) -> Result<Self, ReportError> {
        let identity = Identity::of_build(config.deployment);
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TOTAL_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(format!("ferropix/{}", identity.version))
            .build()
            .map_err(ReportError::Client)?;
        Ok(Self {
            endpoint: config.endpoint.clone(),
            client,
            identity,
            prepared,
            sources,
            started,
        })
    }

    /// Returns the start report as a send would build it now.
    pub async fn start_report(&self) -> Report {
        self.assemble(Event::Start {
            first_start: self.prepared.instance.created,
        })
        .await
    }

    /// Returns the daily report over `counts` as a send would build it now.
    pub async fn daily_report(&self, counts: &Counts) -> Report {
        self.assemble(Event::Daily {
            metrics: Box::new(counts.to_metrics()),
        })
        .await
    }

    /// Sends the start report unless one went out in the last ten minutes.
    ///
    /// # Errors
    /// Returns [`ReportError`] when the start cannot be recorded or the send
    /// fails.
    pub async fn send_start(&self) -> Result<StartOutcome, ReportError> {
        let state = self.prepared.state.clone();
        let claimed =
            tokio::task::spawn_blocking(move || state.claim_start(jiff::Timestamp::now()))
                .await
                .map_err(ReportError::Task)?
                .map_err(ReportError::State)?;
        if !claimed {
            return Ok(StartOutcome::RateLimited);
        }
        let report = self.start_report().await;
        Ok(StartOutcome::Sent(self.post(&report).await?))
    }

    /// Sends the daily report over the window since the previous one.
    ///
    /// The window is taken first; a send that does not reach the collector
    /// puts it back, so the retry carries it. Any answer from the collector,
    /// a refusal included, ends the window.
    ///
    /// # Errors
    /// Returns [`ReportError`] when the send fails.
    pub async fn send_daily(&self) -> Result<StatusCode, ReportError> {
        let counts = self.sources.window.take();
        let report = self.daily_report(&counts).await;
        let sent = self.post(&report).await;
        if sent.is_err() {
            self.sources.window.restore(&counts);
        }
        sent
    }

    /// Starts the background task: the start report at once, then the daily
    /// report [`FIRST_DAILY_AFTER`] boot and about every 24 hours.
    ///
    /// Returns the task handle; dropping it detaches the task.
    #[must_use]
    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            match self.send_start().await {
                Ok(StartOutcome::Sent(status)) => log_answer("start", status),
                Ok(StartOutcome::RateLimited) => tracing::debug!(
                    "usage report: a start report of this instance went out in the last ten minutes"
                ),
                Err(error) => tracing::debug!(
                    error = %crate::chain(&error),
                    "usage report: the start report was not sent"
                ),
            }
            let mut wait = FIRST_DAILY_AFTER;
            loop {
                tokio::time::sleep(wait).await;
                wait = match self.send_daily().await {
                    Ok(status) => {
                        log_answer("daily", status);
                        next_daily_wait()
                    }
                    Err(error) => {
                        tracing::debug!(
                            error = %crate::chain(&error),
                            "usage report: the daily report was not sent; it is tried again in an hour"
                        );
                        RETRY_AFTER_FAILURE
                    }
                };
            }
        })
    }

    /// Fills the envelope around `event` from the identity, the sources and
    /// the host.
    async fn assemble(&self, event: Event) -> Report {
        Report {
            schema: SchemaV2,
            event,
            product: Product::FerroPix,
            instance_id: self.prepared.instance.id.hyphenated().to_string(),
            version: self.identity.version.clone(),
            git_sha: self.identity.git_sha.clone(),
            licence: self.identity.licence,
            deployment: self.identity.deployment,
            uptime_s: self.started.elapsed().as_secs().min(MAX_UPTIME_S),
            cpu_bucket: CpuBucket::of(
                std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
            ),
            memory_bucket: MemoryBucket::of(host_memory_bytes().unwrap_or(0)),
            status: status_of(&self.sources).await,
        }
    }

    /// Posts one report and returns the collector's status; the response body
    /// is never read.
    async fn post(&self, report: &Report) -> Result<StatusCode, ReportError> {
        let body = serde_json::to_vec(report).map_err(ReportError::Json)?;
        if body.len() > MAX_BODY_BYTES {
            return Err(ReportError::TooLarge(body.len()));
        }
        let response = self
            .client
            .post(self.endpoint.clone())
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(ReportError::Request)?;
        Ok(response.status())
    }
}

/// Logs the collector's answer to one report: DEBUG when it stored it, WARN
/// when it refused it, which means this build sent a report the contract does
/// not admit or sent too many.
fn log_answer(event: &'static str, status: StatusCode) {
    if status.is_success() {
        tracing::debug!(event, status = status.as_u16(), "usage report: stored");
    } else {
        tracing::warn!(
            event,
            status = status.as_u16(),
            "usage report: the collector refused the report; it is not sent again"
        );
    }
}

/// Logs the boot line and, when the report runs, starts its background task.
///
/// `ci` is the CI variable the process environment sets, when one is
/// ([`crate::usage_report::ci_of`]), so the caller decides where the
/// environment is read. `None` when the report is off, and when the client
/// cannot be built, which is logged at DEBUG: the report never fails boot.
/// The state directory is touched on a blocking task.
pub async fn start(
    config: &UsageReportSettings,
    sources: Sources,
    started: Instant,
    ci: Option<&'static str>,
) -> Option<JoinHandle<()>> {
    let owned = config.clone();
    let decision = match tokio::task::spawn_blocking(move || prepare(&owned, ci)).await {
        Ok(decision) => decision,
        Err(error) => {
            tracing::debug!(error = %error, "usage report: the state directory task failed");
            return None;
        }
    };
    announce(config, &decision);
    let prepared = decision.ok()?;
    match Reporter::new(config, prepared, sources, started) {
        Ok(reporter) => Some(reporter.spawn()),
        Err(error) => {
            tracing::debug!(
                error = %crate::chain(&error),
                "usage report: the client could not be built"
            );
            None
        }
    }
}

/// Returns the status of this process as of now: the facts fixed at start
/// and the store's and the audit spool's readiness.
async fn status_of(sources: &Sources) -> Status {
    let report = sources.health.readiness(&sources.lifecycle).await;
    let up = |name: &str| {
        report
            .indicators
            .iter()
            .find(|indicator| indicator.name == name)
            .map(|indicator| indicator.state == State::Up)
    };
    Status {
        profile: sources.facts.profile,
        services: sources.facts.services.clone(),
        domains: sources.facts.domains,
        db_ok: up(DATABASE_INDICATOR),
        audit_spool_ok: up(AUDIT_SPOOL_INDICATOR),
    }
}

/// Returns the wait before the next daily report: 24 hours, spread at random
/// by up to 30 minutes either way.
fn next_daily_wait() -> Duration {
    // NOTE: a v4 UUID is 122 random bits from the operating system
    // (<https://docs.rs/uuid/latest/uuid/struct.Uuid.html#method.new_v4>).
    let spread = u64::try_from(uuid::Uuid::new_v4().as_u128() % 3601).unwrap_or(0);
    Duration::from_secs(23 * 3600 + 1800 + spread)
}

/// Returns the memory available to the process in bytes: the smaller of the
/// host's memory and the cgroup limit, where Linux reports either.
fn host_memory_bytes() -> Option<u64> {
    // NOTE: proc(5) `/proc/meminfo` and the cgroup v2 and v1 memory files;
    // absent off Linux, where the report states the smallest bucket.
    let total = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| meminfo_total_bytes(&text));
    let limit = std::fs::read_to_string("/sys/fs/cgroup/memory.max")
        .or_else(|_absent| std::fs::read_to_string("/sys/fs/cgroup/memory/memory.limit_in_bytes"))
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok());
    match (total, limit) {
        (Some(total), Some(limit)) => Some(total.min(limit)),
        (total, limit) => total.or(limit),
    }
}

/// Returns the `MemTotal` line of `/proc/meminfo` in bytes (the file states
/// kB).
fn meminfo_total_bytes(meminfo: &str) -> Option<u64> {
    meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kib| kib.parse::<u64>().ok())
        .map(|kib| kib.saturating_mul(1024))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Off, announcement, meminfo_total_bytes, next_daily_wait, prepare};
    use crate::config::section::usage_report::{Deployment, UsageReportSettings};

    fn config(state_dir: Option<std::path::PathBuf>) -> UsageReportSettings {
        UsageReportSettings {
            enabled: true,
            endpoint: url::Url::parse("https://report.ferropulse.eu/v1/report").expect("a URL"),
            state_dir,
            deployment: Deployment::Unknown,
        }
    }

    #[test]
    fn meminfo_total_is_read_in_bytes() {
        let meminfo = "MemTotal:       16303428 kB\nMemFree:         1000 kB\n";
        assert_eq!(meminfo_total_bytes(meminfo), Some(16_303_428 * 1024));
        assert_eq!(meminfo_total_bytes("MemFree: 1 kB\n"), None);
    }

    #[test]
    fn the_daily_wait_is_a_day_give_or_take_half_an_hour() {
        for _ in 0..64 {
            let wait = next_daily_wait();
            assert!(wait >= Duration::from_mins(23 * 60 + 30), "{wait:?}");
            assert!(wait <= Duration::from_mins(24 * 60 + 30), "{wait:?}");
        }
    }

    #[test]
    fn the_switch_then_ci_then_the_state_directory_decide() {
        let dir = tempfile::tempdir().expect("tempdir");
        let with_dir = config(Some(dir.path().join("state")));
        let off = UsageReportSettings {
            enabled: false,
            ..with_dir.clone()
        };
        assert!(matches!(prepare(&off, Some("CI")), Err(Off::Switch)));
        assert!(matches!(
            prepare(&with_dir, Some("GITHUB_ACTIONS")),
            Err(Off::Ci("GITHUB_ACTIONS"))
        ));
        assert!(
            !dir.path().join("state").exists(),
            "nothing is written while the report is off"
        );
        assert!(matches!(prepare(&config(None), None), Err(Off::NoStateDir)));
        let on = prepare(&with_dir, None).expect("runs");
        assert!(on.instance.created);
        assert!(!prepare(&with_dir, None).expect("runs").instance.created);
    }

    #[test]
    fn a_state_directory_that_is_a_file_turns_the_report_off() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("file");
        std::fs::write(&file, b"").expect("writes");
        assert!(matches!(
            prepare(&config(Some(file)), None),
            Err(Off::StateDir { .. })
        ));
    }

    #[test]
    fn the_boot_line_names_the_state_the_reason_and_the_switch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let on = announcement(&prepare(&config(Some(dir.path().to_path_buf())), None));
        assert!(on.starts_with("usage report ON: "), "{on}");
        assert!(
            on.contains("Off: FERROPIX__USAGE_REPORT__ENABLED=false"),
            "{on}"
        );
        for (decision, reason) in [
            (Err(Off::Switch), "switched off"),
            (Err(Off::Ci("CI")), "CI detected (CI)"),
            (Err(Off::NoStateDir), "no state directory"),
        ] {
            let line = announcement(&decision);
            assert!(line.starts_with("usage report OFF: "), "{line}");
            assert!(line.contains(reason), "{line}");
            assert!(line.contains("nothing is sent"), "{line}");
            assert!(line.contains(super::NOTICE_URL), "{line}");
        }
    }
}
