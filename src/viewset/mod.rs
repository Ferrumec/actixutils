//! actixutils viewset
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

mod context;
mod entity;
mod error;
mod pagination;
mod repository;
mod service;
mod sql;
mod viewset;
pub use context::RequestContext;
pub use entity::Entity;
pub use error::ApiError;
pub use pagination::{Page, PaginationParams, QueryParams, SortDirection};
pub use repository::Repository;
pub use service::Service;
pub use sql::{Field, SqlType, SqlValue};
pub use viewset::ViewSet;
pub use viewset_macros::Entity;
