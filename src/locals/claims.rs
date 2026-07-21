//! Standard JWT claim structs used throughout actixutils.
//!
//! [`Identity`] represents a minimal bearer token (subject + audience + timestamps).
//! [`Authority`] extends that with a role bitmask and a recipient ID, enabling
//! fine-grained, bitwise permission checks.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Minimal JWT claims for identifying a user.
///
/// Tokens are signed with a 500-second expiry from the moment of creation.
///
/// # Fields
/// * `sub` — Subject: the UUID of the authenticated user.
/// * `aud` — Audiences this token is valid for.
/// * `iat` — Issued-at timestamp (seconds since the Unix epoch).
/// * `exp` — Expiry timestamp (seconds since the Unix epoch).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Identity {
    pub aud: Vec<String>,
    pub iat: usize,
    pub exp: usize,
    pub sub: Uuid,
}

/// Extended JWT claims that carry role-based permissions and a recipient context.
///
/// `role` is a 128-bit bitmask. Each bit position corresponds to a distinct
/// permission. Use [`Authority::check`] to test whether a specific permission
/// bit is set.
///
/// # Fields
/// * `sub`  — Subject: the UUID of the acting user.
/// * `rcpt` — Recipient: the UUID of the target resource or user this token
///            was issued for (e.g. a community or tenant ID).
/// * `role` — Permission bitmask (up to 128 discrete permissions).
/// * `aud`  — Audiences this token is valid for.
/// * `iat`  — Issued-at timestamp (seconds since the Unix epoch).
/// * `exp`  — Expiry timestamp (seconds since the Unix epoch).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Authority {
    pub iat: usize,
    pub exp: usize,
    pub role: u128,
    pub aud: Vec<String>,
    pub sub: Uuid,
    pub rcpt: Uuid,
}

impl Identity {
    /// Construct a new `Identity` valid for 500 seconds.
    ///
    /// `iat` and `exp` are derived from the current UTC wall clock.
    pub fn new(sub: Uuid, aud: Vec<String>) -> Self {
        let iat = Utc::now().timestamp() as usize;
        Self {
            aud,
            iat,
            exp: iat + 500,
            sub,
        }
    }
}

impl Authority {
    /// Construct a new `Authority` valid for 500 seconds.
    ///
    /// `iat` and `exp` are derived from the current UTC wall clock.
    pub fn new(sub: Uuid, role: u128, rcpt: Uuid, aud: Vec<String>) -> Self {
        let iat = Utc::now().timestamp() as usize;
        Self {
            aud,
            iat,
            exp: iat + 500,
            sub,
            role,
            rcpt,
        }
    }

    /// Returns `true` if the bit at position `perm_id` is set in [`role`](Self::role).
    ///
    /// Permissions are addressed by a zero-based index into the bitmask:
    /// bit 0 → `1`, bit 1 → `2`, bit 2 → `4`, …
    ///
    /// # Example
    /// ```rust
    /// # use actixutils::locals::Authority;
    /// # use uuid::Uuid;
    /// let auth = Authority::new(Uuid::new_v4(), 0b101, Uuid::new_v4(), vec![]);
    /// assert!(auth.check(0));   // bit 0 is set
    /// assert!(!auth.check(1)); // bit 1 is not set
    /// assert!(auth.check(2));   // bit 2 is set
    /// ```
    pub fn check(&self, perm_id: u16) -> bool {
        let perm_value = 1 << perm_id;
        self.role & perm_value == perm_value
    }
}
