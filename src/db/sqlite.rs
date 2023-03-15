//! Database implementation using sqlite as a backing store.

use std::{
    fmt::Debug,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use derive_new::new;
use error_stack::{report, Result, ResultExt};
use indoc::indoc;
use semver::Version;
use sqlx::{
    migrate, query, query_as,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use tap::TapFallible;
use thiserror::Error;
use tracing::debug;

use crate::{
    doc::{crate_name, crate_version},
    ext::{
        error_stack::{DescribeContext, ErrorHelper, IntoContext},
        result::WrapErr,
    },
};

use super::Coordinate;

/// Errors interacting with sqlite.
#[derive(Debug, Error)]
pub enum Error {
    /// Encountered when connecting to the database.
    #[error("connect to database")]
    Connect,

    /// Encountered when migrating database state.
    #[error("migrate database")]
    Migrate,

    /// Encountered when parsing a DB value.
    #[error("parse value from DB")]
    Parse,

    /// Encountered with serializing a DB value.
    #[error("serialize value to DB")]
    Serialize,

    /// A general communication error.
    #[error("communication error with DB")]
    Communication,
}

/// A database implemented with sqlite.
#[derive(new)]
pub struct Database {
    location: PathBuf,
    internal: SqlitePool,
}

impl Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("location", &self.location)
            .finish()
    }
}

impl Database {
    /// Connect to the database.
    #[tracing::instrument]
    pub async fn connect(location: &Path) -> Result<Self, Error> {
        let options = SqliteConnectOptions::new()
            .filename(location)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .create_if_missing(true);

        debug!("open db at {location:?} with connect options: {options:?}");
        let db = SqlitePoolOptions::new()
            .max_connections(64)
            .min_connections(1)
            .connect_with(options)
            .await
            .context(Error::Connect)
            .describe_lazy(|| format!("attempted to open sqlite db at {location:?}"))?;

        Self::new(location.to_path_buf(), db).migrate().await
    }

    /// Migrate the database.
    #[tracing::instrument]
    async fn migrate(self) -> Result<Self, Error> {
        migrate!("db/migrations")
            .run(&self.internal)
            .await
            .context(Error::Migrate)
            .describe("migrations are compiled into Broker")
            .help(indoc! {"
            This error likely means the database is corrupted.
            The database is only used to improve overall system performance,
            deleting the database may resolve this error.
            "})
            .map(|_| self)
    }

    #[tracing::instrument]
    async fn update_db_version(&self, version: &Version) -> Result<(), Error> {
        let name = crate_name();
        let version = version.to_string();
        query!(
            r#"
            insert into broker_version values (?, ?)
            on conflict do update set version = excluded.version
            "#,
            name,
            version
        )
        .execute(&self.internal)
        .await
        .map(|result| debug!("result: {result:?}"))
        .context(Error::Communication)
    }
}

#[derive(Debug)]
struct BrokerVersionRow {
    version: String,
}

#[derive(Debug)]
struct RepoStateRow {
    repo_state: Vec<u8>,
}

#[async_trait]
impl super::Database for Database {
    #[tracing::instrument]
    async fn broker_version(&self) -> Result<Option<Version>, super::Error> {
        query_as!(
            BrokerVersionRow,
            "select version from broker_version limit 1"
        )
        .fetch_optional(&self.internal)
        .await
        .tap_ok(|raw| debug!("read: {raw:?}"))
        .context(Error::Communication)
        .change_context(super::Error::Interact)?
        .map(|row| Version::parse(&row.version))
        .transpose()
        .tap_ok(|version| debug!("last used broker version: {version:?}"))
        .context(Error::Parse)
        .describe("Broker versions must be valid semver")
        .help("this likely indicates that the database is corrupted, as all Broker releases are valid semver")
        .change_context(super::Error::Interact)
    }

    #[tracing::instrument]
    async fn claim_broker_version(&self) -> Result<(), super::Error> {
        let current_version = crate_version().clone();
        let db_version = self.broker_version().await?;
        debug!("claiming version {current_version} against db {db_version:?}");

        match db_version {
            None => {
                debug!("db does not have a version set, inserting into db");
                self.update_db_version(&current_version)
                    .await
                    .change_context(super::Error::Interact)
            }
            Some(db_version) if current_version < db_version => {
                debug!("current version is older than db version, bailing");
                report!(super::Error::BrokerOutdated)
                    .wrap_err()
                    .describe(indoc! {"
                        Broker stores the last used version in the DB to ensure
                        that older versions of Broker cannot break invariants added in newer
                        versions of Broker.
                        "})
                    .help("try again with the latest version of Broker")
            }
            Some(db_version) if current_version > db_version => {
                debug!("current version is newer than db version, updating db");
                self.update_db_version(&current_version)
                    .await
                    .change_context(super::Error::Interact)
            }
            Some(_) => {
                debug!("versions were the same");
                Ok(())
            }
        }
    }

    #[tracing::instrument]
    async fn state(&self, coordinate: &Coordinate) -> Result<Option<Vec<u8>>, super::Error> {
        let integration = coordinate.namespace.to_string();
        query_as!(
            RepoStateRow,
            "select repo_state from repo_state where integration = ? and repository = ? and revision = ?",
            integration,
            coordinate.remote,
            coordinate.reference
        )
        .fetch_optional(&self.internal)
        .await
        .tap_ok(|raw| debug!("read: {raw:?}"))
        .context(Error::Communication)
        .change_context(super::Error::Interact)
        .map(|result| result.map(|row| row.repo_state))
    }

    #[tracing::instrument]
    async fn set_state(&self, coordinate: &Coordinate, state: &[u8]) -> Result<(), super::Error> {
        let integration = coordinate.namespace.to_string();
        query!(
            r#"
            insert into repo_state values (?, ?, ?, ?)
            on conflict do update set repo_state = excluded.repo_state
            "#,
            integration,
            coordinate.remote,
            coordinate.reference,
            state
        )
        .execute(&self.internal)
        .await
        .map(|result| debug!("result: {result:?}"))
        .context(Error::Communication)
        .change_context(super::Error::Interact)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    macro_rules! temp_db {
        () => {{
            let tmp = tempdir().expect("must create temporary directory");
            let db = super::Database::connect(&tmp.path().join("test.db"))
                .await
                .expect("must create db");
            (tmp, db)
        }};
    }

    #[tokio::test]
    async fn inserts_version() {
        let (_tmp, db) = temp_db!();

        let version = Version::new(0, 1, 0);
        db.update_db_version(&version)
            .await
            .expect("must insert version");

        let row = query_as!(
            BrokerVersionRow,
            "select version from broker_version limit 1"
        )
        .fetch_one(&db.internal)
        .await
        .expect("must fetch version");

        assert_eq!(row.version, version.to_string());
    }

    #[tokio::test]
    async fn updates_version() {
        let (_tmp, db) = temp_db!();

        let name = crate_name();
        query!("insert into broker_version values (?, ?)", name, "0.1.0")
            .execute(&db.internal)
            .await
            .expect("must insert initial version");

        let version = Version::new(0, 2, 0);
        db.update_db_version(&version)
            .await
            .expect("must update version");

        let row = query_as!(
            BrokerVersionRow,
            "select version from broker_version limit 1"
        )
        .fetch_one(&db.internal)
        .await
        .expect("must fetch version");

        assert_eq!(row.version, version.to_string());
    }
}
