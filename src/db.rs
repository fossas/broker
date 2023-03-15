//! Interface for interacting with the database, abstracted over database implementation.

use std::path::Path;

use async_trait::async_trait;
use derive_new::new;
use error_stack::{Result, ResultExt};
use semver::Version;
use strum::Display;
use thiserror::Error;

mod sqlite;

/// Errors interacting with the database.
#[derive(Debug, Error)]
pub enum Error {
    /// Encountered when initializing the database.
    ///
    /// "Initialize" may mean connecting or may mean creating a new instance,
    /// this depends on the implementation.
    #[error("initialize database")]
    Initialize,

    /// Encountered at runtime interacting with the database.
    #[error("interact with the database")]
    Interact,

    /// Encountered when the previous version of Broker to use the database
    /// was newer than the current version of Broker.
    ///
    /// Applications should refuse to run when this error is encountered.
    #[error("newer version of Broker has used this database")]
    BrokerOutdated,
}

/// Each integration gets its own coordinate namespace.
///
/// Since integrations may arbitrarily generate [`Coordinate`] items
/// to refer to their remote code at a specific point in time,
/// they must be namespaced to prevent them from potentially colliding.
///
/// When a new integration is written, a representative entry should
/// be added to this namespace.
#[derive(Debug, Clone, PartialEq, Eq, Display, new)]
pub enum Namespace {
    /// The namespace for `git` integrations.
    Git,
}

/// A coordinate is a remote and a reference on that remote.
///
/// This is a distinct type because this allows different integrations
/// to arbitrarily choose how to encode their remotes and references.
/// In git terms, a reference might be a tag, while the state might be the commit sha.
///
/// This is also why it requires a namespace for the integration:
/// since remotes are arbitrarily encoded, it'd be otherwise possible
/// for them to accidentally collide.
#[derive(Debug, Clone, PartialEq, Eq, new)]
pub struct Coordinate {
    namespace: Namespace,
    remote: String,
    reference: String,
}

/// All databases implement this type.
#[async_trait]
pub trait Database {
    /// The last version of Broker used to access the database.
    /// If the DB has never been accessed before, returns `None`.
    ///
    /// This is meant to be checked during initialization,
    /// and if it is newer than the current Broker version,
    /// Broker should exit.
    async fn broker_version(&self) -> Result<Option<Version>, Error>;

    /// Set the current Broker version as the last used version to access the database.
    /// This checks whether the last used version is newer first and returns an error if so.
    async fn claim_broker_version(&self) -> Result<(), Error>;

    /// Get the last scanned state of a given [`Coordinate`].
    async fn state(&self, coordinate: &Coordinate) -> Result<Option<Vec<u8>>, Error>;

    /// Set the state of a given [`Coordinate`].
    async fn set_state(&self, coordinate: &Coordinate, state: &[u8]) -> Result<(), Error>;
}

/// Connect to the sqlite database implementation.
///
/// Note that this function returns [`sqlite::Database`],
/// which is a private type. The intention here is to allow _using_ the type,
/// but not _accepting_ the type.
///
/// Instead, functions should accept [`Database`].
pub async fn connect_sqlite(location: &Path) -> Result<sqlite::Database, Error> {
    sqlite::Database::connect(location)
        .await
        .change_context(Error::Initialize)
}
