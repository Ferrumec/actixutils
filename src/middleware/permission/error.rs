//! Error types for permission configuration and middleware operations.

use std::io;
use thiserror::Error;

/// Errors that can occur when constructing or loading a [`PermissionSet`].
///
/// This enum covers I/O failures, JSON parsing errors, validation failures
/// for HTTP methods, route patterns, bit IDs, and duplicate permission entries.
///
/// # Examples
///
/// ```
/// use actixutils_permissions::PermissionError;
///
/// let err = PermissionError::InvalidBitId { bit_id: 200 };
/// assert!(err.to_string().contains("200"));
/// ```
#[derive(Debug, Error)]
pub enum PermissionError {
    /// An I/O error occurred while reading the permission file.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// The JSON could not be deserialized into the expected structure.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// The HTTP method string is not a valid HTTP method.
    #[error("Invalid HTTP method: {0}")]
    InvalidMethod(String),

    /// The route pattern could not be parsed as a valid Actix resource definition.
    #[error("Invalid route pattern: {0}")]
    InvalidRoute(String),

    /// The permission bit ID is outside the valid range `0..128`.
    #[error("Invalid bit_id: {bit_id}, must be in range 0..128")]
    InvalidBitId {
        /// The invalid bit ID that was provided.
        bit_id: u64,
    },

    /// A duplicate permission entry was found for the same method and route.
    #[error("Duplicate permission for method {method} and route {route}")]
    DuplicatePermission {
        /// The HTTP method of the duplicate entry.
        method: String,
        /// The route pattern of the duplicate entry.
        route: String,
    },
}
