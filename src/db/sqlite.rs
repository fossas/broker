//! Database implementation using sqlite as a backing store.
//!
//! # Vulnerability warning
//!
//! sqlx@0.6 uses libsqlite3-sys@0.24, which is vulnerable to CVE-2022-35737:
//! > SQLite 1.0.12 through 3.39.x before 3.39.2 sometimes allows an array-bounds
//! > overflow if billions of bytes are used in a string argument to a C API.
//!
//! This fix is blocked upstream on sqlx: 0.7 is a rewrite, and they're unwilling
//! to release a fix for this as it is a breaking change when a rewrite is pending
//! (historically, they've gotten a lot of heat for releasing "too many breaking changes").
//!
//! We don't allow arbitrary queries from users, and the only values sent
//! come from within the program. So long as we continue not allowing user provided
//! queries, we're unlikely to ever hit "billions of bytes used in a string argument",
//! making this a very low risk vulnerability for this application.
//!
//! ## Further reading
//! - CVE details on NIST: https://nvd.nist.gov/vuln/detail/CVE-2022-35737
//! - CVE report issue:    https://github.com/launchbadge/sqlx/issues/2350
//! - PR fixing CVE:       https://github.com/launchbadge/sqlx/pull/2094
//! - 0.7x tracking issue: https://github.com/launchbadge/sqlx/issues/1163

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

use crate::{
    doc::{crate_name, crate_version},
    ext::{
        error_stack::{DescribeContext, ErrorHelper, IntoContext},
        result::{DiscardResult, WrapErr},
        tracing::{span_record, span_records},
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
#[derive(Clone, new)]
pub struct Database {
    location: PathBuf,
    internal: SqlitePool,
}

impl Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("kind", &"sqlite")
            .field("location", &self.location)
            .finish()
    }
}

impl Database {
    /// Connect to the database.
    #[tracing::instrument(fields(options))]
    pub async fn connect(location: &Path) -> Result<Self, Error> {
        let options = SqliteConnectOptions::new()
            .filename(location)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .create_if_missing(true);

        span_record!(options, debug options);
        let db = SqlitePoolOptions::new()
            .max_connections(64)
            .min_connections(1)
            .connect_with(options)
            .await
            .context(Error::Connect)
            .describe_lazy(|| format!("attempted to open sqlite db at {location:?}"))?;

        let db = Self::new(location.to_path_buf(), db).migrate().await?;

        super::Database::claim_broker_version(&db)
            .await
            .change_context(Error::Connect)
            .describe("during initial connection, Broker validates that it's the latest version connecting to the DB")?;

        Ok(db)
    }

    /// Migrate the database.
    #[tracing::instrument]
    async fn migrate(self) -> Result<Self, Error> {
        migrate!("db/migrations")
            .run(&self.internal)
            .await
            .context(Error::Migrate)
            .describe_lazy(|| format!("migrating db at {:?}", self.location))
            .help(indoc! {"
            This error likely means the database is corrupted.
            The database is only used to improve overall system performance,
            deleting the database may resolve this error.
            "})
            .map(|_| self)
    }

    #[tracing::instrument(fields(result))]
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
        .map(|result| span_record!(result, debug result))
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
    async fn healthcheck(&self) -> Result<(), super::Error> {
        self.broker_version().await.discard_ok()
    }

    #[tracing::instrument(fields(read, parsed))]
    async fn broker_version(&self) -> Result<Option<Version>, super::Error> {
        query_as!(
            BrokerVersionRow,
            "select version from broker_version limit 1"
        )
        .fetch_optional(&self.internal)
        .await
        .tap_ok(|raw| span_record!(read, debug raw))
        .context(Error::Communication)
        .change_context(super::Error::Interact)?
        .map(|row| Version::parse(&row.version))
        .transpose()
        .tap_ok(|version| span_record!(parsed, debug version))
        .context(Error::Parse)
        .describe("Broker versions must be valid semver")
        .help("this likely indicates that the database is corrupted, as all Broker releases are valid semver")
        .change_context(super::Error::Interact)
    }

    #[tracing::instrument(fields(current_version, db_version))]
    async fn claim_broker_version(&self) -> Result<(), super::Error> {
        let current_version = crate_version().clone();
        let db_version = self.broker_version().await?;
        span_records! {
            current_version => debug current_version;
            db_version => debug db_version;
        };

        match db_version {
            None => self
                .update_db_version(&current_version)
                .await
                .change_context(super::Error::Interact),
            Some(db_version) if current_version < db_version => {
                report!(super::Error::BrokerOutdated)
                    .wrap_err()
                    .describe(indoc! {"
                        Broker stores the last used version in the DB to ensure
                        that older versions of Broker cannot break invariants added in newer
                        versions of Broker.
                        "})
                    .help("try again with the latest version of Broker")
            }
            Some(db_version) if current_version > db_version => self
                .update_db_version(&current_version)
                .await
                .change_context(super::Error::Interact),
            Some(_) => Ok(()),
        }
    }

    #[tracing::instrument(fields(repo_state))]
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
        .tap_ok(|raw| span_record!(repo_state, debug raw))
        .context(Error::Communication)
        .change_context(super::Error::Interact)
        .map(|result| result.map(|row| row.repo_state))
    }

    #[tracing::instrument(fields(result))]
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
        .map(|result| span_record!(result, debug result))
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

        let version = crate_version();
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

        let version = crate_version();
        let version = Version::new(version.major + 1, 0, 0);
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
