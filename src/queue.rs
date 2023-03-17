//! Async work queue implementation.

use std::{fmt::Debug, ops::Deref, path::PathBuf};

use derive_more::From;
use error_stack::{Report, ResultExt};
use indoc::{formatdoc, indoc};
use strum::Display;

use crate::ext::{
    error_stack::{DescribeContext, ErrorHelper, IntoContext},
    io,
};

/// Errors encountered using the queue.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An underlying IO operation failed.
    #[error("underlying IO operation")]
    IO,

    /// Couldn't construct the queue, which usually means that the named queue is already in use.
    #[error("open queue")]
    Open,
}

/// Queues supported by the application.
#[derive(Debug, Display, PartialEq, Eq, Clone, Copy)]
pub enum Queue {
    /// This queue is just used to send and log messages, used to demonstrate it's working.
    Echo,

    /// The queue for scanning revisions.
    Scan,
}

/// Open both sides of the named queue.
pub async fn open(queue: Queue) -> Result<(Sender, Receiver), Report<Error>> {
    let location = queue_location(queue).await?;
    tokio::try_join!(
        Sender::open_internal(location.clone()),
        Receiver::open_internal(location),
    )
}

async fn queue_location(queue: Queue) -> Result<PathBuf, Report<Error>> {
    io::data_root()
        .await
        .change_context(Error::IO)
        .map(|root| root.join("queue").join(queue.to_string()))
}

/// The sender side of the queue.
pub struct Sender(yaque::Sender);

impl Debug for Sender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sender([OPAQUE yaque::Sender])")
    }
}

impl Sender {
    /// Opens the named queue for sending.
    ///
    /// # Access
    ///
    /// Access is exclusive and controlled by a lock file in the queue's
    /// working directory.
    ///
    /// # Errors
    ///
    /// This function errors if the named queue is already in use for
    /// sending (indicated by a lock file), or if an underlying IO error occurs.
    pub async fn open(queue: Queue) -> Result<Self, Report<Error>> {
        let path = queue_location(queue).await?;
        Self::open_internal(path).await
    }

    /// Sends some data into the queue. One send is always atomic. This function is
    /// `async` because the queue might be full and so we need to `.await` the
    /// receiver to consume enough segments to clear the queue.
    ///
    /// # Errors
    ///
    /// This function returns any underlying errors encountered while writing or
    /// flushing the queue.
    pub async fn send(&mut self, data: &[u8]) -> Result<(), Report<Error>> {
        self.0.send(data).await.context(Error::IO)
    }

    async fn open_internal(path: PathBuf) -> Result<Self, Report<Error>> {
        let lock_path = path.join("send.lock");
        io::spawn_blocking(move || yaque::Sender::open(path))
            .await
            .change_context(Error::Open)
            .help(indoc! {"
            This may be caused by an underlying filesystem error, or the queue may already be open for sending.
            If you are certain no other Broker instances are running, deleting the lock file may recover this error.
            "})
            .describe_lazy(|| formatdoc! {"
            Queue working state is stored on disk, and relies on a lockfile to guard access.
            For this particular queue, this lock file is located at {lock_path:?}.
            "})
            .map(Self)
    }
}

/// The receiver side of the queue.
pub struct Receiver(yaque::Receiver);

impl Debug for Receiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Receiver([OPAQUE yaque::Receiver])")
    }
}

impl Receiver {
    /// Opens the named queue for sending.
    ///
    /// # Access
    ///
    /// Access is exclusive and controlled by a lock file in the queue's
    /// working directory.
    ///
    /// # Errors
    ///
    /// This function errors if the named queue is already in use for
    /// sending (indicated by a lock file), or if an underlying IO error occurs.
    ///
    /// # Panics
    ///
    /// This function panicks if it is not able to set up a notification
    /// handler to watch for file changes.
    pub async fn open(queue: Queue) -> Result<Self, Report<Error>> {
        let path = queue_location(queue).await?;
        Self::open_internal(path).await
    }

    /// Retrieves an element from the queue. The returned value is a
    /// guard that will only commit state changes to the queue when dropped.
    ///
    /// This operation is atomic. If the returned future is not polled to
    /// completion, as, e.g., when calling `select`, the operation will be
    /// undone.
    ///
    /// # Panics
    ///
    /// This function will panic if it has to start reading a new segment and
    /// it is not able to set up the notification handler to watch for file
    /// changes.
    pub async fn recv(&mut self) -> Result<RecvGuard<'_>, Report<Error>> {
        self.0.recv().await.context(Error::IO).map(RecvGuard::from)
    }

    async fn open_internal(path: PathBuf) -> Result<Self, Report<Error>> {
        let lock_path = path.join("recv.lock");
        io::spawn_blocking(move || yaque::Receiver::open(path))
            .await
            .change_context(Error::Open)
            .help(indoc! {"
            This may be caused by an underlying filesystem error, or the queue may already be open for sending.
            If you are certain no other Broker instances are running, deleting the lock file may recover this error.
            "})
            .describe_lazy(|| formatdoc! {"
            Queue working state is stored on disk, and relies on a lockfile to guard access.
            For this particular queue, this lock file is located at {lock_path:?}.
            "})
            .map(Self)
    }
}

/// A guard that will only log changes on the queue state when dropped.
///
/// If it is dropped without a call to `RecvGuard::commit`, changes will be
/// rolled back in a "best effort" policy: if any IO error is encountered
/// during rollback, the state will be committed. If you *can* do something
/// with the IO error, you may use `RecvGuard::rollback` explicitly to catch
/// the error.
#[derive(From)]
pub struct RecvGuard<'a>(yaque::queue::RecvGuard<'a, Vec<u8>>);

impl<'a> RecvGuard<'a> {
    /// Commits the changes to the queue, consuming this `RecvGuard`.
    pub fn commit(self) -> Result<(), Report<Error>> {
        self.0.commit().context(Error::IO)
    }

    /// Rolls the reader back to the previous point, negating the changes made
    /// on the queue. This is also done on drop. However, on drop, the possible
    /// IO error is ignored (but logged as an error) because we cannot have
    /// errors inside drops. Use this if you want to control errors at rollback.
    ///
    /// # Errors
    ///
    /// If there is some error while moving the reader back, this error will be
    /// return.
    pub fn rollback(self) -> Result<(), Report<Error>> {
        self.0.rollback().context(Error::IO)
    }

    /// Returns a reference to the element received.
    pub fn data(&self) -> &[u8] {
        self.0.deref()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    /// Validate that the sender lockfile is still in the same spot.
    /// This is mostly an implementation detail, but if this fails we should update the documentation
    /// in [`Sender::open_internal`].
    #[tokio::test]
    async fn sender_lockfile_location() {
        let tmp = tempdir().expect("must create temporary directory");

        // Keep this variable around for the duration of the test: the lockfile is removed on drop.
        let _tx = Sender::open_internal(tmp.path().to_path_buf())
            .await
            .expect("must open receiver");

        let lockfile = tmp.path().join("send.lock");
        assert!(
            fs::metadata(&lockfile).is_ok(),
            "must create lockfile at {lockfile:?}"
        );
    }

    /// Validate that the receiver lockfile is still in the same spot.
    /// This is mostly an implementation detail, but if this fails we should update the documentation
    /// in [`Receiver::open_internal`].
    #[tokio::test]
    async fn receiver_lockfile_location() {
        let tmp = tempdir().expect("must create temporary directory");

        // Keep this variable around for the duration of the test: the lockfile is removed on drop.
        let _rx = Receiver::open_internal(tmp.path().to_path_buf())
            .await
            .expect("must open receiver");

        let lockfile = tmp.path().join("recv.lock");
        assert!(
            fs::metadata(&lockfile).is_ok(),
            "must create lockfile at {lockfile:?}"
        );
    }
}
