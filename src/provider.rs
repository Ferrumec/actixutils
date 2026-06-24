//! Generic provider trait for dependency injection.

/// Produces values of type `T` on demand.
///
/// This is a lightweight DI helper. Implement it on a struct that holds the
/// configuration or resources needed to construct `T`, and pass the struct to
/// any code that needs a `T` without knowing the concrete implementation.
///
/// # Type parameters
/// * `T` — The provided type. May be `?Sized` (e.g. a trait object).
pub trait Provider<T: ?Sized> {
    /// Produce an instance of `T`.
    fn provide(&self) -> T;
}
