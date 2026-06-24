mod auth;
mod constant_time;
mod fns;
mod request_id;
//#[cfg(feature="es")]
mod context;
mod idempondency;
mod rate_limiter;
mod pagination;
pub use pagination::{Pagination, PaginationMiddleware};
use crate::Authority;
pub use auth::Auth;
pub use constant_time::ResponseEqualizer;
pub use context::{Context, GetId, ReadContext};
pub use fns::{authority, identity};
pub use idempondency::Idempotency;
pub use rate_limiter::RateLimiter;
pub use request_id::{RequestId, RequestIdStr};
use uuid::Uuid;
impl GetId for Authority {
    fn get_id(&self) -> Uuid {
        self.sub.clone()
    }
}
