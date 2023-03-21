//! Interact with remote services over SSH.

use std::{fmt::Display, path::PathBuf};

use derive_more::From;
use derive_new::new;
use serde::{Deserialize, Serialize};

use crate::ext::secrecy::ComparableSecretString;

/// SSH authentication can be performed either with a key file or with a static key.
#[derive(Debug, Clone, PartialEq, Eq, From, Deserialize, Serialize, new)]
pub enum Auth {
    /// Uses a key file on disk to perform authentication.
    KeyFile(PathBuf),

    /// Uses a value stored in the config file (and therefore in memory) to perform authentication.
    KeyValue(ComparableSecretString),
}

impl Display for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Auth::KeyFile(path) => write!(f, "key file at '{}'", path.display()),
            Auth::KeyValue(key) => write!(f, "private key {key}"),
        }
    }
}
