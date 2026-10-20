// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A wiremock `MockServer` that is dropped outside the test's runtime.
//!
//! Dropping a `MockServer` verifies its mocks through
//! `futures::executor::block_on`, which takes a `tokio` lock. Inside a
//! `tokio` task whose cooperative budget is spent, that lock answers
//! `Pending` and leaves its wake-up to the runtime the drop is blocking, so
//! the thread parks for good
//! (<https://docs.rs/tokio/latest/tokio/task/coop/index.html>). A test that
//! makes many in-process calls before it drops a node reaches that state.
//! [`Server`] drops the `MockServer` on a thread of its own, which has no
//! budget to spend. No specification governs this: our own design.

use std::ops::Deref;

use wiremock::MockServer;

/// A wiremock `MockServer` whose drop runs on a thread outside any runtime.
///
/// It dereferences to the `MockServer`, so a mock mounts on `&server` and a
/// helper that takes `&MockServer` takes `&server` as it is.
#[derive(Debug)]
pub(crate) struct Server {
    /// The server, until the drop hands it to its own thread.
    inner: Option<MockServer>,
}

impl Server {
    /// Starts a `MockServer` from wiremock's pool.
    pub(crate) async fn start() -> Self {
        Self::from(MockServer::start().await)
    }
}

impl From<MockServer> for Server {
    fn from(inner: MockServer) -> Self {
        Self { inner: Some(inner) }
    }
}

impl Deref for Server {
    type Target = MockServer;

    fn deref(&self) -> &MockServer {
        self.inner
            .as_ref()
            .expect("the server should be held until the drop")
    }
}

impl Drop for Server {
    /// Drops the `MockServer` on a thread of its own and re-raises a panic
    /// its verification raised, unless this thread is already panicking.
    fn drop(&mut self) {
        let Some(inner) = self.inner.take() else {
            return;
        };
        if let Err(panic) = std::thread::spawn(move || drop(inner)).join()
            && !std::thread::panicking()
        {
            std::panic::resume_unwind(panic);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Server;
    use wiremock::matchers::method;
    use wiremock::{Mock, ResponseTemplate};

    // The shape that hung before: a long run of calls that never yield to
    // the runtime, then a drop on the runtime thread.
    #[tokio::test]
    async fn a_drop_after_the_budget_is_spent_returns() {
        for _ in 0..64 {
            let server = Server::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200))
                .mount(&server)
                .await;
            drop(server);
        }
    }

    #[tokio::test]
    #[should_panic(expected = "Verifications failed")]
    async fn a_failed_verification_still_fails_the_test() {
        let server = Server::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        drop(server);
    }
}
