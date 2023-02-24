//! Extensions to the `secrecy` crate. Specifically, to make secrets comparable.

use derive_more::AsRef;
use secrecy::{ExposeSecret, Secret};
use subtle::ConstantTimeEq;

/// [`Secret`], specialized to [`String`], with constant-time comparisons.
///
/// Only implements `From<String>` because this type should take ownership of the secret.
/// It's not possible to "take ownership" of a `&str`, so it's not supported.
/// It's recommended to not use `.clone()` to work around this; instead convert the secret
/// and work with it as this type.
#[derive(Debug, Clone, AsRef)]
pub struct ComparableSecretString(Secret<String>);

impl PartialEq for ComparableSecretString {
    fn eq(&self, other: &Self) -> bool {
        let lhs = self.0.expose_secret().as_bytes();
        let rhs = other.0.expose_secret().as_bytes();
        ConstantTimeEq::ct_eq(lhs, rhs).into()
    }
}

impl Eq for ComparableSecretString {}

impl From<String> for ComparableSecretString {
    fn from(value: String) -> Self {
        let secret = Secret::new(value);
        Self(secret)
    }
}