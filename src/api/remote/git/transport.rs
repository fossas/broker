//! Powers integration with code hosts speaking the git protocol.

use async_trait::async_trait;
use derive_more::From;
use derive_new::new;
use error_stack::{Report, ResultExt};
use tempfile::TempDir;

use crate::{
    api::{
        http,
        remote::{RemoteProvider, RemoteProviderError},
        ssh,
    },
    ext::io,
};

use super::{super::Remote, repository};

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

        /// Authentication to that host. This is not an Option<> because ssh without auth never works
        auth: ssh::Auth,
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
    Ssh(ssh::Auth),
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

#[async_trait]
impl RemoteProvider for Transport {
    type Reference = super::Reference;

    async fn clone_reference(
        &self,
        reference: &Self::Reference,
    ) -> Result<TempDir, Report<RemoteProviderError>> {
        // The git client is currently synchronous, which means we need to do its work in the
        // background thread pool. This also then means we're sending `reference` across threads.
        // Rather than get into tracking lifetimes, just clone it.
        // The cost is irrelevant next to the IO time.
        // let reference = reference.to_owned();
        io::spawn_blocking_stacked(move || repository::clone_reference(self, &reference))
            .await
            .change_context(RemoteProviderError::RunCommand)
    }

    async fn references(&self) -> Result<Vec<Self::Reference>, Report<RemoteProviderError>> {
        // The git client is currently synchronous, which means we need to do its work in the
        // background thread pool. This also then means we're sending `self` across threads.
        // Rather than get into tracking lifetimes, just clone it.
        // The cost is irrelevant next to the IO time.
        // let cloned = self.to_owned();
        io::spawn_blocking_stacked(move || repository::list_references(self))
            .await
            .change_context(RemoteProviderError::RunCommand)
    }
}
