//! The [`Principal`] trait definition.
//!
//! Any type that represents an authenticated user or service principal can implement
//! this trait to integrate with the permissions middleware. The middleware is
//! completely agnostic to how the principal was authenticated (JWT, session cookie,
//! API key, mTLS, etc.).
//!
//! # Example
//!
//! ```
//! use actixutils_permissions::Principal;
//! use uuid::Uuid;
//!
//! struct User {
//!     id: Uuid,
//!     role: u128,
//! }
//!
//! impl Principal for User {
//!     fn role(&self) -> u128 {
//!         self.role
//!     }
//! }
//! ```

/// A principal that carries an authorization role bitset.
///
/// The role is represented as a `u128` where each bit corresponds to a permission.
/// Bit `0` is the least significant bit (`1 << 0`), and bit `127` is the most
/// significant bit (`1 << 127`).
///
/// Implementations should be `'static` so they can be stored in Actix's request
/// extensions and referenced by the generic middleware.
pub trait Principal: 'static {
    /// Returns the `u128` role bitset for this principal.
    ///
    /// A bit set to `1` indicates the principal has the corresponding permission.
    /// A bit set to `0` indicates the principal does not have that permission.
    fn role(&self) -> u128;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct TestPrincipal {
        role: u128,
    }

    impl Principal for TestPrincipal {
        fn role(&self) -> u128 {
            self.role
        }
    }

    #[test]
    fn principal_exposes_role() {
        let p = TestPrincipal { role: 0b1011 };
        assert_eq!(p.role(), 0b1011);
    }

    #[test]
    fn bit_zero_is_rightmost() {
        let p = TestPrincipal { role: 1u128 << 0 };
        assert_eq!(p.role() & (1u128 << 0), 1);
        assert_eq!(p.role() & (1u128 << 1), 0);
    }

    #[test]
    fn bit_127_works() {
        let p = TestPrincipal { role: 1u128 << 127 };
        assert_eq!(p.role() & (1u128 << 127), 1u128 << 127);
        assert_eq!(p.role() & (1u128 << 126), 0);
    }

    #[test]
    fn inactive_bits_are_zero() {
        let p = TestPrincipal { role: 0b1011 };
        assert_eq!(p.role() & (1u128 << 2), 0); // bit 2 is inactive
        assert_ne!(p.role() & (1u128 << 0), 0); // bit 0 is active
        assert_ne!(p.role() & (1u128 << 1), 0); // bit 1 is active
        assert_ne!(p.role() & (1u128 << 3), 0); // bit 3 is active
    }

    #[test]
    fn active_bits_are_nonzero() {
        for bit in 0..128u8 {
            let p = TestPrincipal { role: 1u128 << bit };
            assert_ne!(p.role() & (1u128 << bit), 0, "bit {} should be active", bit);
        }
    }
}
