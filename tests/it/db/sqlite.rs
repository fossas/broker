use semver::Version;
use sqlx::{query, Connection};
use tempfile::tempdir;

use broker::{
    db::{connect_sqlite, Coordinate, Database},
    doc::{crate_name, crate_version},
};

use crate::assert_error_stack_snapshot;

/// Open a temporary database.
///
/// If a path is provided, a connection to the existing path is made.
/// Otherwise, a new database is created.
macro_rules! temp_db {
    () => {{
        let tmp = tempdir().expect("must create temporary directory");
        let path = tmp.path().join("test.db");
        let db = connect_sqlite(&path).await.expect("must create db");
        (tmp, db, path)
    }};
    ($path:expr) => {{
        connect_sqlite($path).await.expect("must create db")
    }};
}

/// Create a new raw db and a connection to it.
/// This is used to set up private DB state prior to upgrading via `temp_db`.
///
/// Provide `with_migrations` to migrate the DB as well.
macro_rules! raw_temp_db {
    () => {{
        let tmp = tempdir().expect("must create temporary directory");
        let path = tmp.path().join("test.db");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .create_if_missing(true);

        use sqlx::Connection;
        let conn = sqlx::SqliteConnection::connect_with(&options)
            .await
            .expect("must create db");

        (tmp, conn, path)
    }};
    (with_migrations) => {{
        let (tmp, mut conn, path) = raw_temp_db!();
        sqlx::migrate!("db/migrations")
            .run(&mut conn)
            .await
            .expect("must migrate db");
        (tmp, conn, path)
    }};
}

#[tokio::test]
async fn creates_if_not_exists() {
    let (_tmp, _db, _path) = temp_db!();
}

#[tokio::test]
async fn claims_current_version() {
    let (_tmp, db, _path) = temp_db!();

    db.claim_broker_version()
        .await
        .expect("must claim current version");

    let version = db
        .broker_version()
        .await
        .expect("must get version")
        .expect("must have a version set");

    assert_eq!(&version, crate_version());
}

#[tokio::test]
async fn claim_older_version_fails() {
    let version = crate_version();
    let newer = Version::new(version.major + 1, version.minor, version.patch).to_string();

    // Need a bare connection since setting an arbitrary version is private.
    let (_tmp, mut db, path) = raw_temp_db!(with_migrations);
    let name = crate_name();
    query!("insert into broker_version values (?, ?)", name, newer)
        .execute(&mut db)
        .await
        .expect("must set initial broker version");
    db.close().await.expect("must close db");

    // Now open the actual DB interface at this path and try to claim the current version.
    let err = connect_sqlite(&path)
        .await
        .expect_err("must fail to claim version");
    assert_error_stack_snapshot!(&path, err);
}

#[tokio::test]
async fn gets_initial_version() {
    let (_tmp, db, _path) = temp_db!();

    let version = broker::doc::crate_version();
    let db_version = db
        .broker_version()
        .await
        .expect("must get version")
        .expect("version must be set");
    assert_eq!(db_version.to_string(), version.to_string());
}

#[tokio::test]
async fn gets_empty_state() {
    let (_tmp, db, _path) = temp_db!();

    let coordinate = Coordinate::new(
        broker::db::Namespace::Git,
        String::from("some repo"),
        String::from("some reference"),
    );

    let state = db.state(&coordinate).await.expect("must get state");
    assert!(state.is_none(), "db state was unset, so must be none");
}

#[tokio::test]
async fn roundtrip_state() {
    let (_tmp, db, _path) = temp_db!();

    let coordinate = Coordinate::new(
        broker::db::Namespace::Git,
        String::from("some repo"),
        String::from("some reference"),
    );

    let state = db.state(&coordinate).await.expect("must get state");
    assert!(state.is_none(), "db state was unset, so must be none");

    let state = b"some state";
    db.set_state(&coordinate, state)
        .await
        .expect("must set state");

    let new_state = db
        .state(&coordinate)
        .await
        .expect("must get state")
        .expect("state must have been set");
    assert_eq!(new_state, state);
}
