use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// Carries everything a request needs as it flows down through the layers.
///
/// `U` is the authenticated-user type and is left generic so each
/// application can plug in its own auth/claims struct. Everything else is
/// concrete because it's infrastructure, not business logic.
///
/// # Design decision: not threaded through `ViewSet`/`Service`/`Repository`
///
/// This struct is deliberately *not* a parameter on any trait method in
/// this crate. Baking it in would force a `RequestContext<U>` generic
/// through every layer for every user of the crate, even the (common)
/// case of a ViewSet that needs none of it — paying for a feature you
/// don't use.
///
/// The intended pattern instead: an application-level `actix-web`
/// middleware extracts whatever it needs (auth claims, tenant id,
/// request id, ...) and makes it available via a `tokio::task_local!`,
/// scoped for the lifetime of that request's task. Any code downstream —
/// a `before_create` hook, an overridden `Repository` method building a
/// tenant-scoped `WHERE` clause, etc. — reads the task-local directly
/// instead of receiving it as a parameter.
///
/// This is sound specifically because `ViewSet::configure`'s default
/// handlers never spawn a new tokio task: `handle_list`/`handle_create`/
/// etc. call straight down through `Service` and `Repository` on the
/// same task actix-web is already running the request on, and
/// `task_local!` values are visible across `.await` points within a
/// single task. The one place this breaks is if an application's own
/// hook override calls `tokio::spawn` (e.g. to fire a background job
/// from `after_create`) — that spawned task does *not* inherit the
/// task-local and needs the value passed in explicitly if it needs it.
///
/// `RequestContext` itself is kept around as a convenience shape for
/// applications that want a single struct to stash in their task-local
/// rather than several independent ones — it's infrastructure a
/// middleware can populate, not something this crate wires up itself.
#[derive(Clone)]
pub struct RequestContext<U = ()> {
    pub db: PgPool,
    pub user: Option<Arc<U>>,
    pub permissions: Arc<Vec<String>>,
    pub tenant_id: Option<Uuid>,
    pub request_id: Uuid,
    pub trace_id: Option<String>,
    pub locale: String,
}

impl<U> RequestContext<U> {
    pub fn new(db: PgPool) -> Self {
        Self {
            db,
            user: None,
            permissions: Arc::new(Vec::new()),
            tenant_id: None,
            request_id: Uuid::new_v4(),
            trace_id: None,
            locale: "en".to_string(),
        }
    }

    pub fn has_permission(&self, perm: &str) -> bool {
        self.permissions.iter().any(|p| p == perm)
    }

    pub fn with_user(mut self, user: U) -> Self {
        self.user = Some(Arc::new(user));
        self
    }

    pub fn with_tenant(mut self, tenant_id: Uuid) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }
}

// `RequestContext` has no `FromRequest` impl in this crate on purpose —
// see the design-decision note on the struct above. Populating and
// reading it (or the individual pieces of it) is entirely up to each
// application's own middleware + `tokio::task_local!`, not something
// wired up here.
