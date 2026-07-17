use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// Carries everything a request needs as it flows down through the layers.
///
/// `U` is the authenticated-user type and is left generic so each
/// application can plug in its own auth/claims struct. Everything else is
/// concrete because it's infrastructure, not business logic.
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

// Extractor glue: applications implement `FromRequest` for
// `RequestContext<YourUser>` in their own crate (it depends on their auth
// middleware), so ferrumec-admin doesn't hard-code an auth mechanism.
// A minimal example lives in `examples/product.rs`.
