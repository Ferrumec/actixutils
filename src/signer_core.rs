use anyhow::Result;

pub trait Sign<T>: Send + Sync + 'static {
    fn sign(&self, claims: &T) -> Result<String>;
}

pub trait Validate<T>: Send + Sync + 'static {
    fn validate(&self, token: &str) -> Result<T>;
}
