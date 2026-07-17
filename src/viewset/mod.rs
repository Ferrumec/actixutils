//! ferrumec-admin
//!
//! A generic, Django-REST-Framework-inspired CRUD toolkit for building
//! admin REST APIs on top of actix-web + sqlx + Postgres.
//!
//! Layering (request flows top to bottom, response flows bottom to top):
//!
//!   HTTP request -> ViewSet -> Service -> Repository -> Database
//!
//! Each layer is a trait with default (mostly no-op or delegating)
//! implementations, so a new entity only needs a handful of `impl` blocks
//! plus entity metadata to get a fully working CRUD API.

pub mod context;
pub mod entity;
pub mod error;
pub mod pagination;
pub mod repository;
pub mod service;
pub mod sql;
pub mod viewset;

pub mod prelude {
    pub use super::context::RequestContext;
    pub use super::entity::Entity;
    pub use super::error::ApiError;
    pub use super::pagination::{Page, PaginationParams, QueryParams, SortDirection};
    pub use super::repository::Repository;
    pub use super::service::Service;
    pub use super::sql::{Field, SqlType, SqlValue};
    pub use super::viewset::ViewSet;
    pub use viewset_macros::Entity;
}
