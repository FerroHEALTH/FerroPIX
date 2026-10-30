// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! One headless Chrome session over `WebDriver`, and the waits every journey
//! is built from.
//!
//! Each session runs through a chromedriver of its own (`chromedriver`), so
//! no journey shares a driver with another.
//!
//! The waits are FerroTERM's (FerroHEALTH/FerroTERM#834, #840): every wait
//! starts by waiting for the page in front of the browser to be ready, which
//! FerroUI's loader marks with `data-fui-script` on `<html>` once its modules
//! run (`ferrohealth_ui::script::MARK`), and a press tags the page first, so
//! the wait after it is never satisfied by the page the press is leaving. A
//! session with the modules withheld waits for the document alone, as a
//! browser that never runs them would. Nothing sleeps for a fixed time and
//! hopes. A wait that runs
//! out fails the journey with the address, what the browser logged, a
//! screenshot and the whole document, written to the failure directory the
//! `journeys` CI job uploads.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use thirtyfour::LoggingPrefsLogLevel;
use thirtyfour::common::keys::TypingData;
use thirtyfour::prelude::*;

use crate::journeys::chromedriver::{CHROME_ENV, Chromedriver, installed};
use crate::journeys::stderr::{SERVER_PANIC, Stderr};

/// The environment variable naming the directory a failure writes its
/// evidence into, when it is not [`DEFAULT_FAILURES`].
const FAILURES_ENV: &str = "FERROPIX_JOURNEY_FAILURES";

/// Where the evidence goes when nothing names a directory, under the
/// workspace.
const DEFAULT_FAILURES: &str = "target/journey-failures";

/// How long a wait keeps trying before the journey fails.
pub(crate) const WAIT: Duration = Duration::from_secs(30);

/// How often a wait reads the page again.
const POLL: Duration = Duration::from_millis(100);

/// The level chromedriver reports a console error, an uncaught error or
/// panic, a refused Content-Security-Policy and a failed load at.
const SEVERE: &str = "SEVERE";

/// The window the session starts with, in CSS pixels.
pub(crate) const WINDOW: (u32, u32) = (1440, 900);

/// The narrowest window Chrome draws, in CSS pixels; a narrower viewport is
/// emulated.
const NARROWEST_WINDOW: u32 = 500;

/// How much of a failing page's markup the report carries; the whole
/// document goes to the evidence file beside it.
const MARKUP_IN_A_FAILURE: usize = 20_000;

/// How many words of a wait's description name its evidence files.
const SLUG_WORDS: usize = 8;

/// The address pattern of FerroUI's script modules, the loader among them,
/// which a session withholds to hold every page to its no-script floor.
pub(crate) const MODULES: &str = "*/ferrohealth-ui/*/scripts/*";

/// Whether the page in front of the browser has finished loading and, when
/// it loads FerroUI's loader and the session lets it run (`arguments[0]`),
/// has been marked by it. A page that is not the console's (the provider's
/// redirect, a plain-text refusal) loads no loader and is not waited on.
const READY: &str = r#"
return document.readyState === "complete"
    && (document.documentElement.hasAttribute("data-fui-script")
        || arguments[0] === true
        || document.querySelector("script[src*='/ferrohealth-ui/']") === null);
"#;

/// Tags the page in front of the browser before a press, and records on the
/// tag whether the press starts loading another page: a navigation the
/// Navigation API reports, or a form submission nothing cancelled, which
/// navigates only in a later task
/// (<https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-submission-algorithm>).
///
/// A navigation in the same document (a tab that names its panel in the
/// address) and a post a module took itself load nothing.
const TAG: &str = r#"
if (window.__journeyPress === undefined) {
  navigation.addEventListener("navigate", (event) => {
    if (event.hashChange) { return; }
    if (event.destination.sameDocument) { window.__journeyPress.inPage = true; }
    else { window.__journeyPress.navigating = true; }
  });
  addEventListener("submit", (event) => { window.__journeyPress.submits.push(event); }, true);
}
window.__journeyPress = {
  navigating: false,
  inPage: false,
  submits: [],
};
"#;

/// Where the press [`TAG`] watched has left the browser: `loaded` once the
/// tagged page is gone, `leaving` while it is still in front of the browser
/// and another page is on its way, `staying` when the press loads nothing.
const LEFT: &str = r#"
const press = window.__journeyPress;
if (press === undefined) { return "loaded"; }
const method = (event) =>
  (event.submitter?.getAttribute("formmethod") ?? event.target.method).toLowerCase();
const posted = press.submits.some((event) => !event.defaultPrevented && method(event) !== "dialog");
if (press.navigating || posted) { return "leaving"; }
return "staying";
"#;

/// The workspace root.
pub(crate) fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `what` as a file name: lowercase words joined by hyphens.
fn slug(what: &str) -> String {
    let name: String = what
        .chars()
        .map(|letter| {
            if letter.is_ascii_alphanumeric() {
                letter.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = name
        .split('-')
        .filter(|word| !word.is_empty())
        .take(SLUG_WORDS)
        .collect::<Vec<&str>>()
        .join("-");
    if trimmed.is_empty() {
        String::from("failure")
    } else {
        trimmed
    }
}

/// A browser session a journey drives.
#[derive(Debug)]
pub(crate) struct Journey {
    /// The `WebDriver` session.
    driver: WebDriver,
    /// The chromedriver of this session alone.
    chromedriver: Option<Chromedriver>,
    /// Whether the session withholds FerroUI's script modules.
    withheld: bool,
}

impl Journey {
    /// Starts a headless Chrome session through a chromedriver of its own,
    /// with FerroUI's script modules running.
    pub(crate) async fn start() -> Result<Self, Box<dyn Error>> {
        Self::start_with(false).await
    }

    /// Starts a headless Chrome session that refuses every one of FerroUI's
    /// script modules (`Network.setBlockedURLs` of the Chrome `DevTools`
    /// Protocol), as a browser whose modules never load.
    pub(crate) async fn start_withheld() -> Result<Self, Box<dyn Error>> {
        let session = Self::start_with(true).await?;
        let devtools = session.driver.cdp();
        devtools
            .send_raw("Network.enable", serde_json::json!({}))
            .await?;
        devtools
            .send_raw(
                "Network.setBlockedURLs",
                serde_json::json!({ "urls": [MODULES] }),
            )
            .await?;
        Ok(session)
    }

    /// Starts a session, withholding the modules when `withheld`.
    async fn start_with(withheld: bool) -> Result<Self, Box<dyn Error>> {
        let mut capabilities = DesiredCapabilities::chrome();
        // NOTE: Chrome's headless mode (developer.chrome.com/docs/chromium/headless):
        // a container has neither the kernel sandbox nor a large `/dev/shm`.
        capabilities.add_arg("--headless=new")?;
        capabilities.set_no_sandbox()?;
        capabilities.set_disable_dev_shm_usage()?;
        capabilities.add_arg(&format!("--window-size={},{}", WINDOW.0, WINDOW.1))?;
        capabilities.set_browser_log_level(LoggingPrefsLogLevel::All)?;
        let chrome = std::env::var_os(CHROME_ENV)
            .map(PathBuf::from)
            .or_else(|| installed().1);
        if let Some(chrome) = chrome {
            let chrome = chrome
                .to_str()
                .ok_or_else(|| format!("{} is no UTF-8 path", chrome.display()))?;
            capabilities.set_binary(chrome)?;
        }
        let (chromedriver, endpoint) = Chromedriver::start().await?;
        let driver = WebDriver::new(endpoint.as_str(), capabilities)
            .await
            .map_err(|error| format!("no WebDriver session at {endpoint}: {error}"))?;
        Ok(Self {
            driver,
            chromedriver: Some(chromedriver),
            withheld,
        })
    }

    /// Ends the session, closes the browser and stops its chromedriver.
    pub(crate) async fn quit(mut self) -> Result<(), Box<dyn Error>> {
        let quit = self.driver.clone().quit().await;
        drop(self.chromedriver.take());
        quit?;
        Ok(())
    }

    /// Opens `address` as a fresh load and waits for it to be ready.
    pub(crate) async fn open(&self, address: &str) {
        self.driver
            .goto(address)
            .await
            .unwrap_or_else(|error| panic!("the browser could not open {address}: {error}"));
        self.ready().await;
    }

    /// Whether the session withholds FerroUI's script modules.
    pub(crate) const fn withheld(&self) -> bool {
        self.withheld
    }

    /// Waits until the page in front of the browser has loaded and, when it
    /// is the console's and the session runs the modules, FerroUI's loader
    /// has marked it.
    ///
    /// The server sends a whole page, so its controls are on screen before
    /// their modules run; a press in between reaches markup with no
    /// behaviour yet. The page is read whole on every poll, so a navigation
    /// still settling is judged on the page it lands on.
    pub(crate) async fn ready(&self) {
        let started = Instant::now();
        while started.elapsed() <= WAIT {
            let ready = self
                .driver
                .execute(READY, vec![serde_json::Value::Bool(self.withheld)])
                .await
                .is_ok_and(|answer| answer.convert::<bool>().is_ok_and(|ready| ready));
            if ready {
                return;
            }
            tokio::time::sleep(POLL).await;
        }
        panic!(
            "{}",
            self.failure("the page to be ready", "no data-fui-script mark")
                .await
        );
    }

    /// The first element matching `selector`, once the page is ready and
    /// shows it, scrolled into view.
    pub(crate) async fn element(&self, selector: By, what: &str) -> WebElement {
        self.ready().await;
        let element = match self
            .driver
            .query(selector)
            .wait(WAIT, POLL)
            .and_displayed()
            .first()
            .await
        {
            Ok(element) => element,
            Err(error) => panic!("{}", self.failure(what, &error.to_string()).await),
        };
        // NOTE: WebDriver 2, Element Click: a press on an element whose centre
        // is under the sticky top bar is refused, so the element is centred.
        if let Err(error) = element.scroll_into_view().await {
            panic!("{}", self.failure(what, &error.to_string()).await);
        }
        element
    }

    /// Presses the element matching `selector` and returns once the page the
    /// press loads, if any, is ready.
    pub(crate) async fn press(&self, selector: By, what: &str) {
        let element = self.element(selector, what).await;
        self.press_on(&element, what).await;
    }

    /// Presses `element` and returns once the page the press loads, if any,
    /// is ready.
    pub(crate) async fn press_on(&self, element: &WebElement, what: &str) {
        if let Err(error) = self.acting(what, || element.click()).await {
            panic!("{}", self.failure(what, &error.to_string()).await);
        }
    }

    /// Sends `key` to `element` and returns once the page it loads, if any,
    /// is ready.
    pub(crate) async fn keys(&self, element: &WebElement, key: impl Into<TypingData>, what: &str) {
        let key = key.into();
        if let Err(error) = self.acting(what, || element.send_keys(key)).await {
            panic!("{}", self.failure(what, &error.to_string()).await);
        }
    }

    /// The `WebDriver` session, for a call no wait wraps.
    pub(crate) fn driver(&self) -> &WebDriver {
        &self.driver
    }

    /// The element that holds the focus.
    pub(crate) async fn active(&self) -> WebElement {
        match self.driver.active_element().await {
            Ok(element) => element,
            Err(error) => panic!(
                "{}",
                self.failure("the focused element", &error.to_string())
                    .await
            ),
        }
    }

    /// Runs `act`, a press or keys, on a tagged page, and returns once the
    /// page it loads, if any, is ready.
    ///
    /// A click can return while the page it leaves is still in front of the
    /// browser, ready, and carrying the controls the next step looks for,
    /// so every wait after it would pass on a page about to go away. The tag
    /// goes with that page, which is how the new one is told apart.
    async fn acting<F, A>(&self, what: &str, act: F) -> WebDriverResult<()>
    where
        F: FnOnce() -> A,
        A: Future<Output = WebDriverResult<()>>,
    {
        self.driver.execute(TAG, Vec::new()).await?;
        act().await?;
        let deadline = Instant::now() + WAIT;
        loop {
            // NOTE: a script the navigation interrupts has no answer, which reads
            // as the page still leaving.
            let left = self
                .driver
                .execute(LEFT, Vec::new())
                .await
                .ok()
                .and_then(|answer| answer.convert::<String>().ok());
            match left.as_deref() {
                Some("staying") => return Ok(()),
                Some("loaded") => {
                    self.ready().await;
                    return Ok(());
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                let reason = "the page the press left never went away";
                panic!("{}", self.failure(what, reason).await);
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Presses tab `times` in one `WebDriver` call, the way a keyboard
    /// reader walks a screen.
    pub(crate) async fn tab(&self, times: usize) -> WebDriverResult<()> {
        let presses = TypingData::from(Key::Tab).to_string().repeat(times);
        self.driver.active_element().await?.send_keys(presses).await
    }

    /// Waits until `selector` matches `wanted` elements.
    pub(crate) async fn count_becoming(&self, selector: By, wanted: usize, what: &str) {
        let deadline = Instant::now() + WAIT;
        loop {
            self.ready().await;
            let found = self
                .driver
                .find_all(selector.clone())
                .await
                .map_or(0, |found| found.len());
            if found == wanted {
                return;
            }
            if Instant::now() >= deadline {
                let reason = format!("{found} elements match, not {wanted}");
                panic!("{}", self.failure(what, &reason).await);
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Waits until the address carries `needle`, and returns it.
    pub(crate) async fn address_carrying(&self, needle: &str, what: &str) -> String {
        let deadline = Instant::now() + WAIT;
        loop {
            let address = self
                .driver
                .current_url()
                .await
                .map(|url| url.to_string())
                .unwrap_or_default();
            if address.contains(needle) {
                self.ready().await;
                return address;
            }
            if Instant::now() >= deadline {
                let reason = format!("the address is still `{address}`");
                panic!("{}", self.failure(what, &reason).await);
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// What one script evaluated to in the page, as the string it returned.
    ///
    /// Four things have no `WebDriver` primitive and are read this way: the
    /// colour a browser painted, the stops focus reached, the words a page
    /// draws and whether a page scrolls sideways
    /// (<https://www.w3.org/TR/webdriver2/#execute-script>).
    pub(crate) async fn evaluate(&self, script: &str) -> WebDriverResult<String> {
        self.driver.execute(script, Vec::new()).await?.convert()
    }

    /// What one script evaluated to with `argument`, a string, as its
    /// `arguments[0]`.
    pub(crate) async fn evaluate_with(
        &self,
        script: &str,
        argument: String,
    ) -> WebDriverResult<String> {
        self.driver
            .execute(script, vec![serde_json::Value::String(argument)])
            .await?
            .convert()
    }

    /// Sets the cookie `name` to `value` on the page's own origin, as the
    /// console's preference post would.
    pub(crate) async fn set_cookie(&self, name: &str, value: &str) -> WebDriverResult<()> {
        let mut cookie = Cookie::new(name.to_owned(), value.to_owned());
        cookie.set_path("/");
        self.driver.add_cookie(cookie).await
    }

    /// The cookie the browser holds for the page under `name`, if any.
    pub(crate) async fn cookie(&self, name: &str) -> WebDriverResult<Option<Cookie>> {
        Ok(self
            .driver
            .get_all_cookies()
            .await?
            .into_iter()
            .find(|cookie| cookie.name == name))
    }

    /// Sets the viewport to `width` by `height` CSS pixels, emulating a width
    /// under the narrowest window Chrome draws over the Chrome `DevTools`
    /// Protocol
    /// (<https://chromedevtools.github.io/devtools-protocol/tot/Emulation/#method-setDeviceMetricsOverride>).
    pub(crate) async fn resize(&self, width: u32, height: u32) {
        let window = width.max(NARROWEST_WINDOW);
        self.driver
            .set_window_rect(0, 0, window, height)
            .await
            .unwrap_or_else(|error| panic!("resizing to {width}x{height}: {error}"));
        let devtools = self.driver.cdp();
        let emulated = if width < NARROWEST_WINDOW {
            devtools
                .send_raw(
                    "Emulation.setDeviceMetricsOverride",
                    serde_json::json!({
                        "width": width,
                        "height": height,
                        "deviceScaleFactor": 1,
                        "mobile": true,
                    }),
                )
                .await
        } else {
            devtools
                .send_raw(
                    "Emulation.clearDeviceMetricsOverride",
                    serde_json::json!({}),
                )
                .await
        };
        emulated.unwrap_or_else(|error| panic!("emulating {width}x{height}: {error}"));
    }

    /// Everything the browser logged as severe since the last read.
    async fn console_errors(&self) -> Vec<String> {
        match self.driver.browser_log().await {
            Ok(entries) => entries
                .iter()
                .filter(|entry| entry.level == SEVERE)
                .map(|entry| entry.message.clone())
                .collect(),
            Err(error) => vec![format!("the browser log could not be read: {error}")],
        }
    }

    /// Fails the journey when the browser logged anything severe since the
    /// last read: a script error, a refused policy or a failed load. An
    /// entry naming one of `expected` is the journey working: the not-found
    /// page answers `404` by design, which the browser logs as a failed load,
    /// and a session that withholds the modules logs each one it refused.
    pub(crate) async fn no_console_errors_but(&self, expected: &[&str]) {
        let console: Vec<String> = self
            .console_errors()
            .await
            .into_iter()
            .filter(|entry| !expected.iter().any(|wanted| entry.contains(wanted)))
            .collect();
        let address = self
            .driver
            .current_url()
            .await
            .map_or_else(|error| error.to_string(), |url| url.to_string());
        assert!(
            console.is_empty(),
            "the browser logged {} severe entries at {address}, and a page that logs one is a defect:\n  {}",
            console.len(),
            console.join("\n  ")
        );
    }

    /// The message for a wait that ran out: what it waited for, why it
    /// failed, the address, what the browser logged, where the evidence went,
    /// and the start of the page's markup.
    async fn failure(&self, what: &str, reason: &str) -> String {
        let console = self.console_errors().await;
        let logged = if console.is_empty() {
            String::from("the browser logged nothing severe")
        } else {
            format!("the browser logged:\n  {}", console.join("\n  "))
        };
        let address = self.driver.current_url().await.map_or_else(
            |error| format!("(the address could not be read: {error})"),
            |url| url.to_string(),
        );
        let source = self.driver.source().await;
        let markup = match &source {
            Ok(text) => {
                let trimmed: String = text.chars().take(MARKUP_IN_A_FAILURE).collect();
                format!("the page held:\n{trimmed}")
            }
            Err(error) => format!("(the markup could not be read: {error})"),
        };
        let evidence = self.evidence(what, source.ok().as_deref()).await;
        format!(
            "waiting for {what} failed: {reason}\nthe address was {address}\n\
             {logged}\n{evidence}\n{markup}"
        )
    }

    /// Writes a screenshot and the whole document to the failure directory,
    /// never raising: a browser that cannot answer is already the failure
    /// being reported.
    async fn evidence(&self, what: &str, source: Option<&str>) -> String {
        let dir = std::env::var_os(FAILURES_ENV)
            .map_or_else(|| workspace().join(DEFAULT_FAILURES), PathBuf::from);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            return format!("(no evidence written: creating {}: {error})", dir.display());
        }
        let stem = format!("{}-{}", slug(what), std::process::id());
        let shot = dir.join(format!("{stem}.png"));
        let page = dir.join(format!("{stem}.html"));
        let mut written = Vec::new();
        match self.driver.screenshot(&shot).await {
            Ok(()) => written.push(format!("a screenshot in {}", shot.display())),
            Err(error) => written.push(format!("(no screenshot: {error})")),
        }
        match source {
            Some(text) => match std::fs::write(&page, text) {
                Ok(()) => written.push(format!("the whole document in {}", page.display())),
                Err(error) => written.push(format!(
                    "(no document: writing {}: {error})",
                    page.display()
                )),
            },
            None => written.push(String::from("(no document: the markup was unreadable)")),
        }
        format!("the failure left {}", written.join(", and "))
    }
}

/// Runs `journey` in a fresh browser with FerroUI's modules running, ends
/// the session whatever the journey did, and fails on anything severe the
/// browser logged but the not-found page's own `404`, and on a panic of the
/// console's server, run in process, on the process's standard error.
pub(crate) async fn with_journey<F>(journey: F)
where
    F: AsyncFnOnce(&Journey),
{
    run(false, journey).await;
}

/// Runs `journey` as [`with_journey`] does in a browser that refuses every
/// one of FerroUI's modules, failing on anything severe the browser logged
/// but a refused module and the not-found page's `404`.
pub(crate) async fn with_journey_withheld<F>(journey: F)
where
    F: AsyncFnOnce(&Journey),
{
    run(true, journey).await;
}

/// Runs `journey` in a fresh browser, withholding the modules when
/// `withheld`.
async fn run<F>(withheld: bool, journey: F)
where
    F: AsyncFnOnce(&Journey),
{
    let stderr = Stderr::copy().await;
    let session = if withheld {
        Journey::start_withheld().await
    } else {
        Journey::start().await
    }
    .unwrap_or_else(|error| panic!("no browser session: {error}"));
    journey(&session).await;
    let expected: &[&str] = if withheld {
        &[crate::journeys::NOT_FOUND, "/ferrohealth-ui/"]
    } else {
        &[crate::journeys::NOT_FOUND]
    };
    session.no_console_errors_but(expected).await;
    session
        .quit()
        .await
        .unwrap_or_else(|error| panic!("the browser session did not end cleanly: {error}"));
    let panicked: Vec<String> = stderr
        .end()
        .lines()
        .filter(|line| line.contains(SERVER_PANIC))
        .map(str::to_owned)
        .collect();
    assert!(
        panicked.is_empty(),
        "the console's server panicked while the journey ran:\n  {}",
        panicked.join("\n  ")
    );
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn a_waits_description_names_a_file_that_a_filesystem_accepts() {
        assert_eq!(
            slug("the theme choice of Your preferences, dark"),
            "the-theme-choice-of-your-preferences-dark"
        );
        assert_eq!(slug("/patients/x?y=1"), "patients-x-y-1");
        assert_eq!(slug("..."), "failure", "a name is never empty");
    }
}
