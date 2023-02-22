//! Interact with remote services over HTTP!

use derive_more::From;
use derive_new::new;

use crate::ext::secrecy::ComparableSecretString;

/// HTTP authentication can be performed either with a header or via 'HTTP Basic'.
#[derive(Debug, Clone, PartialEq, Eq, From, new)]
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
