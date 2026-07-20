//! Task-local pagination state.
//!
//! The task-local pagination snapshot is populated by
//! [`PaginationMiddleware`](crate::middleware::PaginationMiddleware) for the
//! lifetime of a request's task. Repository functions and handlers can then
//! read the current pagination state anywhere in the call stack via
//! [`Pagination::get`] without needing to thread the parameters through every
//! function signature.

use tokio::task_local;

task_local! {
    /// The pagination snapshot for the current request's task.
    ///
    /// Set by [`PaginationMiddleware`](crate::middleware::PaginationMiddleware).
    /// Crate-visible only — read it via [`Pagination::get`].
    pub(crate) static PAGINATION: Pagination;
}

/// A snapshot of the parsed pagination query parameters.
///
/// Read anywhere in the request's async call stack via [`Pagination::get`].
#[derive(Clone, Copy)]
pub struct Pagination {
    /// Zero-based page number (default: `0`).
    pub page: u32,
    /// Maximum number of items per page (default: `100`).
    pub limit: u32,
}

impl Default for Pagination {
    fn default() -> Pagination {
        Pagination {
            page: 0,
            limit: 100,
        }
    }
}

impl Pagination {
    /// Retrieve the pagination parameters set by
    /// [`PaginationMiddleware`](crate::middleware::PaginationMiddleware) for the
    /// current request.
    ///
    /// Falls back to [`Pagination::default`] if called outside a request context or
    /// before the middleware has run.
    pub fn get() -> Pagination {
        PAGINATION.try_with(|p| *p).unwrap_or_default()
    }

    pub fn offset(&self) -> u32 {
        self.page * self.limit
    }
}
