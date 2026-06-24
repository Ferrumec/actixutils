//! Miscellaneous async utilities.

use std::env;

/// Fetch the RSA public key from a remote URL at runtime.
///
/// The URL is read from the `REMOTE_PUBLIC_KEY` environment variable.  
/// This is useful for downstream services that need to retrieve the auth
/// service's public key dynamically rather than embedding it at compile time.
///
/// # Panics
/// * Panics if `REMOTE_PUBLIC_KEY` is not set.
/// * Panics if the HTTP request fails.
/// * Panics if the response body cannot be read as text.
///
/// # Example
/// ```rust,no_run
/// use actixutils::utils::remote_public_key;
///
/// #[tokio::main]
/// async fn main() {
///     let pem = remote_public_key().await;
///     println!("{}", pem);
/// }
/// ```
pub async fn remote_public_key() -> String {
    let url = env::var("REMOTE_PUBLIC_KEY").expect("missing REMOTE PUBLIC KEY");
    let response = reqwest::get(url)
        .await
        .expect("error occured in getting remote public key");
    response
        .text()
        .await
        .expect("could not get text content of response")
}
