//! Interact with remote services over HTTP!

use std::fmt::Display;

use derive_more::From;
use derive_new::new;
use serde::{Deserialize, Serialize};

use crate::ext::secrecy::ComparableSecretString;

/// HTTP authentication can be performed either with a header or via 'HTTP Basic'.
#[derive(Debug, Clone, PartialEq, Eq, From, Deserialize, Serialize, new)]
pub enum Auth {
    /// Uses a header value for authentication.
    Header(ComparableSecretString),

    /// Uses HTTP Basic to perform authentication.
    Basic {
        /// The username for authentication.
        username: String,

        /// The password for authentication.
        password: ComparableSecretString,
    },
}

impl Display for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Auth::Header(header) => write!(f, "authorization header {header}"),
            Auth::Basic { username, password } => {
                write!(f, "username '{username}' and password '{password}'")
            }
        }
    }
}
