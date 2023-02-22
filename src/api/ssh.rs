//! Interact with remote services over SSH.

use std::path::PathBuf;

use derive_more::From;
use derive_new::new;

use crate::ext::secrecy::ComparableSecretString;

/// SSH authentication can be performed either with a key file or with a static key.
#[derive(Debug, Clone, PartialEq, Eq, From, new)]
pub enum Auth {
    /// Uses a key file on disk to perform authentication.
    KeyFile(PathBuf),

    /// Uses a value stored in the config file (and therefore in memory) to perform authentication.
    KeyValue(ComparableSecretString),
}
