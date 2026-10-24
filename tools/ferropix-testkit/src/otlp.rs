// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! An in-process OTLP collector: the OTLP/gRPC `TraceService` on a loopback
//! port, keeping every export request it receives for a test to read
//! (<https://opentelemetry.io/docs/specs/otlp/>).
//!
//! It runs on a thread and a runtime of its own, so a server exporting from
//! its batch thread, a `serve` process, or a test runtime that has stopped
//! still reaches it, and it answers every export as accepted. No
//! specification governs this test device: our own design.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::{
    TraceService, TraceServiceServer,
};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use opentelemetry_proto::tonic::trace::v1::Span;
use tokio::sync::oneshot;
use tonic::transport::Server;
use tonic::transport::server::TcpIncoming;

/// The export requests received so far, in the order they arrived.
type Received = Arc<Mutex<Vec<ExportTraceServiceRequest>>>;

/// The collector could not start.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CollectorError {
    /// No loopback port could be bound.
    #[error("the collector could not bind a loopback port")]
    Bind(#[source] io::Error),
    /// Its runtime could not be built.
    #[error("the collector's runtime could not be built")]
    Runtime(#[source] io::Error),
}

/// An OTLP/gRPC trace collector on a loopback port, stopped when dropped.
#[derive(Debug)]
pub struct Collector {
    endpoint: String,
    received: Received,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<Result<(), tonic::transport::Error>>>,
}

impl Collector {
    /// Starts a collector on a free loopback port.
    ///
    /// # Errors
    ///
    /// Returns [`CollectorError::Bind`] when no port can be bound, and
    /// [`CollectorError::Runtime`] when the collector's runtime cannot be
    /// built.
    pub fn start() -> Result<Self, CollectorError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(CollectorError::Runtime)?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(CollectorError::Bind)?;
        listener
            .set_nonblocking(true)
            .map_err(CollectorError::Bind)?;
        let address = listener.local_addr().map_err(CollectorError::Bind)?;
        let listener = {
            let _entered = runtime.enter();
            tokio::net::TcpListener::from_std(listener).map_err(CollectorError::Bind)?
        };
        let received = Received::default();
        let service = TraceServiceServer::new(Keeper(Arc::clone(&received)));
        let (stop, stopped) = oneshot::channel::<()>();
        let thread = std::thread::spawn(move || {
            runtime.block_on(
                Server::builder()
                    .add_service(service)
                    .serve_with_incoming_shutdown(TcpIncoming::from(listener), async move {
                        // A dropped sender stops the collector as the sent signal does.
                        let _stopped: Result<(), oneshot::error::RecvError> = stopped.await;
                    }),
            )
        });
        Ok(Self {
            endpoint: format!("http://{address}"),
            received,
            stop: Some(stop),
            thread: Some(thread),
        })
    }

    /// The `http://` URL an OTLP exporter sends to.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Every export request received so far, in the order they arrived.
    #[must_use]
    pub fn received(&self) -> Vec<ExportTraceServiceRequest> {
        self.received
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Every span received so far, across every request, resource and
    /// scope.
    #[must_use]
    pub fn spans(&self) -> Vec<Span> {
        self.received()
            .into_iter()
            .flat_map(|request| request.resource_spans)
            .flat_map(|resource| resource.scope_spans)
            .flat_map(|scope| scope.spans)
            .collect()
    }
}

impl Drop for Collector {
    /// Stops the server and joins its thread, re-raising a panic it raised
    /// unless this thread is already panicking.
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            // A server that already stopped dropped the receiver; nothing is left to stop.
            let _sent: Result<(), ()> = stop.send(());
        }
        if let Some(thread) = self.thread.take()
            && let Err(panic) = thread.join()
            && !std::thread::panicking()
        {
            std::panic::resume_unwind(panic);
        }
    }
}

/// The trace service: keeps each export request and accepts it whole.
#[derive(Debug)]
struct Keeper(Received);

#[async_trait::async_trait]
impl TraceService for Keeper {
    async fn export(
        &self,
        request: tonic::Request<ExportTraceServiceRequest>,
    ) -> Result<tonic::Response<ExportTraceServiceResponse>, tonic::Status> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request.into_inner());
        Ok(tonic::Response::new(ExportTraceServiceResponse::default()))
    }
}
