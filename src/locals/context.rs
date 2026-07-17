//! Event-stream publishing context.
//!
//! [`Context`] bundles the request ID, the authenticated user's UUID, an
//! [`EventStream`] handle, and a producer name, so that handlers and services
//! can emit domain events without carrying these dependencies through every
//! function signature. It is built and inserted into the request extensions by
//! [`ReadContext<T>`](crate::middleware::ReadContext).

use std::sync::Arc;
use typed_eventbus::{Event, EventStream, Publishable};
use uuid::Uuid;

/// A request-scoped event publishing context.
///
/// `Context` is inserted into the request extensions by
/// [`ReadContext<T>`](crate::middleware::ReadContext). Handlers retrieve it via
/// `req.extensions().get::<Context>()` and call [`publish`](Self::publish).
#[derive(Clone)]
pub struct Context {
    pub(crate) request_id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) es: Arc<dyn EventStream>,
    pub(crate) producer: String,
    pub(crate) user_as_audience: bool,
}

impl Context {
    /// Publish a domain event, attaching the current request ID and user ID as trace
    /// metadata.
    ///
    /// Errors from the underlying [`EventStream`] are logged via `tracing::error!` but
    /// not propagated, to avoid failing a request due to a non-critical observability
    /// side-effect.
    pub async fn publish<T: Publishable + Sync + Send>(&self, payload: Event<T>) {
        let mut event = payload
            .with_producer(self.producer.clone())
            .with_trace_id(self.request_id)
            .with_user_id(self.user_id);
        if self.user_as_audience {
            event = event.add_audience(self.user_id);
        }
        if let Err(e) = event.publish(self.es.clone()).await {
            tracing::error!(error = %e, request_id = %self.request_id, "Event publishing failed");
        };
    }
}

/// Extracts the authenticated user's UUID from any type that implements `GetId`.
///
/// Implement this for your identity or authority type so that
/// [`ReadContext`](crate::middleware::ReadContext) can derive the `user_id` field of
/// the resulting [`Context`].
pub trait GetId {
    /// Return the UUID that identifies the authenticated user.
    fn get_id(&self) -> Uuid;
}

impl GetId for super::Authority {
    fn get_id(&self) -> Uuid {
        self.sub
    }
}

impl GetId for super::Identity {
    fn get_id(&self) -> Uuid {
        self.sub
    }
}
