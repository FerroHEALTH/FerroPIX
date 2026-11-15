// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[pixm]`, `[pdqm]` and `[pmir]` sections: each profile's switch and
//! the options its text leaves to the actor.

use std::time::Duration;

use ferrohealth_config::units::Duration as Span;
use ferrohealth_smart::client::outbound::OutboundClient;
use ferrohealth_smart::config::ServerTls;
use ferrohealth_smart::config::endpoint::EndpointSection;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::Profile;
use crate::config::error::Error;
use crate::config::section::{endpoint_secrets, url};

/// The dotted key of the Patient Identity Consumers' endpoint tables.
pub const CONSUMERS: &str = "pmir.feed.consumers";

/// The `[pixm]` section: the PIXm Patient Identifier Cross-reference Manager.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pixm {
    /// Whether ITI-83 and ITI-104 are served.
    pub enabled: bool,
    /// Whether the Remove Patient Option of ITI-104 is served (PIXm 3.1.0
    /// §1:41.2.1, §2:3.104.4.3).
    pub remove_patient: bool,
}

impl Default for Pixm {
    fn default() -> Self {
        Self {
            enabled: true,
            remove_patient: false,
        }
    }
}

/// How a merged person is answered, among the choices PDQm 3.2.0
/// §2:3.78.4.1.3 case 6 leaves to the Supplier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Deprecated {
    /// The person with `active = false` and its `replaced-by` link, the
    /// survivor included.
    #[default]
    Inactive,
    /// The person left out of the answer.
    Omit,
}

/// The `[pdqm]` section: the PDQm Patient Demographics Supplier.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pdqm {
    /// Whether ITI-78 and ITI-119 are served.
    pub enabled: bool,
    /// How a merged person is answered.
    pub deprecated: Deprecated,
}

impl Default for Pdqm {
    fn default() -> Self {
        Self {
            enabled: true,
            deprecated: Deprecated::Inactive,
        }
    }
}

/// The `[pmir.feed]` table: the outbound ITI-93 sender.
///
/// Its retry bounds are spent before a subscription's `status` becomes
/// `error` (PMIR 1.6.0 §2:3.94.4.1.3).
/// Each `[[pmir.feed.consumers]]` entry is the family's endpoint table of
/// one Consumer: `url` is the endpoint prefix its subscriptions' channel
/// endpoints start with, `tls` the client certificate of ATNA mutual TLS,
/// and `credentials` the IUA client credentials grant (PMIR 1.6.0
/// §2:3.93.5). The subscription carries no credential (§2:3.94.5), so the
/// operator names it here, by agreement with the Consumer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Feed {
    /// The most attempts one message is given.
    pub max_attempts: u32,
    /// The wait before the first retry.
    pub initial_backoff: Span,
    /// The longest wait between two retries.
    pub max_backoff: Span,
    /// The Patient Identity Consumers the sender authenticates to.
    pub consumers: Vec<EndpointSection>,
}

impl Default for Feed {
    fn default() -> Self {
        Self {
            max_attempts: 10,
            initial_backoff: Span::from_secs(1),
            max_backoff: Span::from_mins(5),
            consumers: Vec::new(),
        }
    }
}

/// The `[pmir]` section: the PMIR Patient Identity Registry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pmir {
    /// Whether ITI-93 and ITI-94 are served.
    pub enabled: bool,
    /// The outbound ITI-93 sender.
    pub feed: Feed,
}

/// One Patient Identity Consumer, resolved: the endpoint prefix its
/// subscriptions name and the client that authenticates to it.
#[derive(Debug, Clone)]
pub struct FeedConsumer {
    /// The endpoint prefix.
    pub base: Url,
    /// The client, with the Consumer's TLS identity and credential.
    pub client: OutboundClient,
}

impl FeedConsumer {
    /// Returns whether `endpoint` lies under this Consumer's prefix: the
    /// same origin, and a path that equals the prefix's or continues it at
    /// a `/`.
    #[must_use]
    pub fn serves(&self, endpoint: &Url) -> bool {
        if endpoint.origin() != self.base.origin() {
            return false;
        }
        let prefix = self.base.path();
        let path = endpoint.path();
        path == prefix
            || prefix.ends_with('/') && path.starts_with(prefix)
            || path
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

/// The `[pmir]` section, resolved.
#[derive(Debug, Clone)]
pub struct PmirSettings {
    /// Whether ITI-93 and ITI-94 are served.
    pub enabled: bool,
    /// The most attempts one message is given.
    pub max_attempts: u32,
    /// The wait before the first retry.
    pub initial_backoff: Duration,
    /// The longest wait between two retries.
    pub max_backoff: Duration,
    /// The Patient Identity Consumers the sender authenticates to.
    pub consumers: Vec<FeedConsumer>,
}

/// Returns the Consumer of `consumers` whose prefix serves `endpoint`, the
/// longest when several do.
#[must_use]
pub fn consumer_of<'a>(consumers: &'a [FeedConsumer], endpoint: &Url) -> Option<&'a FeedConsumer> {
    consumers
        .iter()
        .filter(|consumer| consumer.serves(endpoint))
        .max_by_key(|consumer| consumer.base.path().len())
}

impl Pmir {
    /// Checks the section under `profile` and returns it resolved, with a
    /// client for each Consumer.
    ///
    /// # Errors
    /// Returns [`Error::Value`] for no attempt, a first wait of 0, a first
    /// wait above the longest one, or a Consumer URL that is not `https` or
    /// `http`, carries userinfo, a query or a fragment, or repeats another;
    /// [`Error::Url`] for a Consumer URL that does not parse;
    /// [`Error::DevelopmentOnly`] for a plain `http` Consumer under
    /// production; [`Error::InlineSecret`] for a client secret set inline
    /// under production; and [`Error::Outbound`] when a Consumer's endpoint
    /// table, its TLS or its credential is refused.
    pub fn resolve(&self, profile: Profile) -> Result<PmirSettings, Error> {
        // NOTE: no specification governs this: our own design; PMIR 1.6.0
        // §2:3.94.4.1.3 leaves the retry policy to the Registry.
        let rule = if self.feed.max_attempts == 0 {
            Some(("pmir.feed.max_attempts", "it must be above 0"))
        } else if self.feed.initial_backoff.is_zero() {
            Some(("pmir.feed.initial_backoff", "it must be above 0"))
        } else if self.feed.initial_backoff > self.feed.max_backoff {
            Some((
                "pmir.feed.max_backoff",
                "it must be at least pmir.feed.initial_backoff",
            ))
        } else {
            None
        };
        if let Some((key, rule)) = rule {
            return Err(Error::Value {
                key: key.to_owned(),
                rule,
            });
        }
        let mut consumers: Vec<FeedConsumer> = Vec::with_capacity(self.feed.consumers.len());
        for (index, table) in self.feed.consumers.iter().enumerate() {
            let consumer = consumer(index, table, profile)?;
            if consumers.iter().any(|known| known.base == consumer.base) {
                return Err(Error::Value {
                    key: format!("{CONSUMERS}[{index}].url"),
                    rule: "another consumer names the same url",
                });
            }
            consumers.push(consumer);
        }
        Ok(PmirSettings {
            enabled: self.enabled,
            max_attempts: self.feed.max_attempts,
            initial_backoff: self.feed.initial_backoff.get(),
            max_backoff: self.feed.max_backoff.get(),
            consumers,
        })
    }
}

/// Resolves the Consumer `table` at `index` under `profile`.
fn consumer(
    index: usize,
    table: &EndpointSection,
    profile: Profile,
) -> Result<FeedConsumer, Error> {
    let key = format!("{CONSUMERS}[{index}].url");
    let base = url(&key, table.url.expose())?;
    match base.scheme() {
        "https" => {}
        // NOTE: PMIR 1.6.0 §2:3.93.5 asks for server authentication, which plain http lacks.
        "http" if profile == Profile::Development => {}
        "http" => {
            return Err(Error::DevelopmentOnly {
                key,
                rule: "a plain http consumer, while every message names the patient",
            });
        }
        _ => {
            return Err(Error::Value {
                key,
                rule: "an https URL",
            });
        }
    }
    if base.cannot_be_a_base()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(Error::Value {
            key,
            rule: "a URL prefix with no userinfo, query or fragment",
        });
    }
    endpoint_secrets(
        &format!("{CONSUMERS}[{index}]"),
        &table.credentials,
        profile,
    )?;
    let client =
        OutboundClient::build(table, CONSUMERS, &ServerTls).map_err(|source| Error::Outbound {
            key: CONSUMERS,
            source: Box::new(source),
        })?;
    Ok(FeedConsumer { base, client })
}

#[cfg(test)]
mod tests {
    use ferrohealth_secret::secret_url::SecretUrl;
    use ferrohealth_smart::config::endpoint::EndpointSection;

    use super::{Feed, Pmir, PmirSettings, consumer_of};
    use crate::config::Profile;
    use crate::config::error::Error;

    fn with_consumers(urls: &[&str]) -> Pmir {
        Pmir {
            enabled: true,
            feed: Feed {
                consumers: urls
                    .iter()
                    .map(|url| EndpointSection {
                        url: SecretUrl::new(*url),
                        ..EndpointSection::default()
                    })
                    .collect(),
                ..Feed::default()
            },
        }
    }

    fn resolved(urls: &[&str]) -> PmirSettings {
        with_consumers(urls)
            .resolve(Profile::Production)
            .expect("resolves")
    }

    #[test]
    fn a_consumer_serves_the_endpoints_under_its_prefix_and_the_longest_wins() {
        let settings = resolved(&[
            "https://ehr.example.org/pmir",
            "https://ehr.example.org/pmir/ward",
        ]);
        let named = |endpoint: &str| {
            consumer_of(
                &settings.consumers,
                &url::Url::parse(endpoint).expect("a URL"),
            )
            .map(|consumer| consumer.base.as_str().to_owned())
        };
        assert_eq!(
            Some(String::from("https://ehr.example.org/pmir")),
            named("https://ehr.example.org/pmir")
        );
        assert_eq!(
            Some(String::from("https://ehr.example.org/pmir")),
            named("https://ehr.example.org/pmir/feed")
        );
        assert_eq!(
            Some(String::from("https://ehr.example.org/pmir/ward")),
            named("https://ehr.example.org/pmir/ward/7")
        );
        assert_eq!(
            None,
            named("https://ehr.example.org/pmirx"),
            "a prefix ends at a segment"
        );
        assert_eq!(None, named("http://ehr.example.org/pmir"), "another origin");
        assert_eq!(
            None,
            named("https://ehr.example.org:8443/pmir"),
            "another port"
        );
    }

    #[test]
    fn a_consumer_url_off_the_rules_is_refused() {
        for (url, wanted) in [
            ("ftp://ehr.example.org/pmir", "value"),
            ("https://user@ehr.example.org/pmir", "value"),
            ("https://ehr.example.org/pmir?x=1", "value"),
            ("not a url", "url"),
            ("http://ehr.example.org/pmir", "development"),
        ] {
            let error = with_consumers(&[url])
                .resolve(Profile::Production)
                .expect_err(url);
            let kind = match error {
                Error::Value { .. } => "value",
                Error::Url { .. } => "url",
                Error::DevelopmentOnly { .. } => "development",
                _ => "other",
            };
            assert_eq!(wanted, kind, "{url}: {error:?}");
        }
        with_consumers(&["http://localhost:8080/pmir"])
            .resolve(Profile::Development)
            .expect("development admits plain http");
        assert!(matches!(
            with_consumers(&[
                "https://ehr.example.org/pmir",
                "https://ehr.example.org/pmir"
            ])
            .resolve(Profile::Production),
            Err(Error::Value { .. })
        ));
    }

    #[test]
    fn retry_bounds_without_an_attempt_or_out_of_order_are_refused() {
        for feed in [
            Feed {
                max_attempts: 0,
                ..Feed::default()
            },
            Feed {
                initial_backoff: super::Span::from_millis(0),
                ..Feed::default()
            },
            Feed {
                initial_backoff: super::Span::from_millis(10),
                max_backoff: super::Span::from_millis(5),
                ..Feed::default()
            },
        ] {
            let pmir = Pmir {
                enabled: true,
                feed: feed.clone(),
            };
            assert!(
                matches!(pmir.resolve(Profile::Production), Err(Error::Value { .. })),
                "{feed:?}"
            );
        }
        Pmir::default()
            .resolve(Profile::Production)
            .expect("the defaults resolve");
    }
}
