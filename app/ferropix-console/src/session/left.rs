// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What a signed-in session has left, which the session route answers
//! (FerroUI `docs/consoles.md`, "The session's end"). No specification
//! governs the session store: our own design.

use std::time::{Duration, Instant};

use ferrohealth_ui::view::session::Left;

use super::{SessionError, SessionId, Sessions, live};

impl Sessions {
    /// What the live signed-in session `id` has left, in whole seconds.
    ///
    /// The session route's `GET` reads it without touching the session
    /// (FerroUI `docs/consoles.md`, "The session's end"); with `touch` it
    /// first counts the steward's activity, which starts the idle timeout
    /// again but never moves the absolute one, for the route's `POST`.
    /// `None` for a session that is not live, which is dropped.
    ///
    /// # Errors
    /// Returns [`SessionError::Poisoned`] when the store is unusable.
    pub fn left(&self, id: &SessionId, touch: bool) -> Result<Option<Left>, SessionError> {
        let now = Instant::now();
        let mut store = self.lock()?;
        let Some(session) = store.sessions.get_mut(id) else {
            return Ok(None);
        };
        if !live(&self.settings, session, now) {
            store.sessions.remove(id);
            return Ok(None);
        }
        if touch {
            session.touched = now;
        }
        let idle = self
            .settings
            .idle_timeout
            .saturating_sub(now.saturating_duration_since(session.touched));
        let mut absolute = self
            .settings
            .absolute_timeout
            .saturating_sub(now.saturating_duration_since(session.created));
        // NOTE: no specification governs this: our own design; an access token no refresh
        // token can replace ends the session when it expires, as `live` reads it.
        if session.refresh_token.is_none()
            && let Some(expires) = session.expires
        {
            absolute = absolute.min(expires.saturating_duration_since(now));
        }
        Ok(Some(Left {
            idle: whole_seconds(idle),
            absolute: whole_seconds(absolute),
        }))
    }
}

/// The whole seconds in `span`, the most a `u32` holds for a longer one
/// (a timeout past 136 years).
fn whole_seconds(span: Duration) -> u32 {
    u32::try_from(span.as_secs()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use secrecy::SecretString;

    use crate::config::settings::SessionSettings;
    use crate::session::tests::{settings, signed_in};
    use crate::session::{SessionId, Sessions, SignedIn};

    #[test]
    fn a_read_of_what_is_left_never_counts_as_activity_and_an_extension_does() {
        let sessions = Sessions::new(settings());
        let id = sessions.establish(signed_in()).expect("room");
        std::thread::sleep(Duration::from_millis(1100));
        let read = sessions.left(&id, false).expect("usable").expect("live");
        assert!(read.idle <= 58, "a second has passed: {read:?}");
        assert!((598..=599).contains(&read.absolute), "{read:?}");
        let again = sessions.left(&id, false).expect("usable").expect("live");
        assert!(
            again.idle <= read.idle,
            "a read did not restart the idle timeout"
        );
        let extended = sessions.left(&id, true).expect("usable").expect("live");
        assert_eq!(60, extended.idle, "an extension restarts the idle timeout");
        assert!(
            extended.absolute <= read.absolute,
            "an extension never moves the absolute timeout"
        );
    }

    #[test]
    fn an_access_token_no_refresh_token_replaces_bounds_what_is_left() {
        let sessions = Sessions::new(settings());
        let bounded = sessions
            .establish(SignedIn {
                expires_in: Some(Duration::from_secs(30)),
                ..signed_in()
            })
            .expect("room");
        let renewable = sessions
            .establish(SignedIn {
                expires_in: Some(Duration::from_secs(30)),
                refresh_token: Some(SecretString::from("synthetic-refresh-token")),
                ..signed_in()
            })
            .expect("room");
        let left = sessions
            .left(&bounded, false)
            .expect("usable")
            .expect("live");
        assert!(left.absolute <= 30, "{left:?}");
        let left = sessions
            .left(&renewable, false)
            .expect("usable")
            .expect("live");
        assert!(left.absolute > 30, "{left:?}");
    }

    #[test]
    fn an_ended_or_unknown_session_has_nothing_left_and_nothing_to_extend() {
        let sessions = Sessions::new(SessionSettings {
            idle_timeout: Duration::ZERO,
            ..settings()
        });
        let id = sessions.establish(signed_in()).expect("room");
        assert_eq!(None, sessions.left(&id, false).expect("usable"));
        assert_eq!(None, sessions.left(&id, true).expect("usable"));
        assert_eq!(0, sessions.occupancy().expect("usable").sessions);
        let unknown = SessionId::from_cookie("not-a-session");
        assert_eq!(None, sessions.left(&unknown, true).expect("usable"));
    }
}
