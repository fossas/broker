//! Powers integration with code hosts speaking the git protocol.

use derive_more::From;
use derive_new::new;

use crate::api::{http, ssh};

use super::super::Remote;

/// Code hosts speaking the git protocol may support downloading a given repository
/// using any, or a subset, of the below transport.
///
/// Similar to how [`super::Protocol`] enumerates possible overall communication protocols,
/// this type enumerates possible communication methods to use when communicating with a
/// code host that specifically speaks the git protocol.
#[derive(Debug, Clone, PartialEq, Eq, From, new)]
pub enum Transport {
    /// Specifies that the remote code host is configured to use the SSH protocol.
    Ssh {
        /// The URL to the remote code host.
        endpoint: Remote,

        /// Authentication to that host, if applicable.
        auth: Option<ssh::Auth>,
    },

    /// Specifies that the remote code host is configured to use the HTTP protocol.
    Http {
        /// The URL to the remote code host.
        endpoint: Remote,

        /// Authentication to that host, if applicable.
        auth: Option<http::Auth>,
    },
}

/// Auth types available for a transport
pub enum Auth {
    /// SSH
    Ssh(Option<ssh::Auth>),
    /// HTTP
    Http(Option<http::Auth>),
}

impl Transport {
    /// returns the endpoint of a transport
    pub fn endpoint(&self) -> &Remote {
        use Transport::*;
        match self {
            Ssh { endpoint, .. } => endpoint,
            Http { endpoint, .. } => endpoint,
        }
    }

    /// returns the auth info for a transport
    pub fn auth(&self) -> Auth {
        use Transport::*;
        match self {
            Ssh { auth, .. } => Auth::Ssh(auth.clone()),
            Http { auth, .. } => Auth::Http(auth.clone()),
        }
    }
}
