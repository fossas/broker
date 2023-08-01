//! Async work queue implementation.

use std::{fmt::Debug, marker::PhantomData};

use error_stack::Report;
use serde::{de::DeserializeOwned, Serialize};

use crate::ext::error_stack::IntoContext;

/// Errors encountered using the queue.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// When sending to the queue, the item is serialized.
    /// If that serialize operation fails, this error is returned.
    #[error("serialize item")]
    Serialize,

    /// When receiving from the queue, the item is deserialized.
    /// If that deserialize operation fails, this error is returned.
    #[error("deserialize item")]
    Deserialize,
}

/// The default limit for a queue.
pub const DEFAULT_LIMIT: usize = 1000;

/// A queue implementation specialized to the type of data being sent through it.
pub struct Queue<T> {
    t: PhantomData<T>,
    internal: deadqueue::limited::Queue<Vec<u8>>,
}

impl<T> Queue<T>
where
    T: Serialize + DeserializeOwned,
{
    /// Create a new instance with the specified max size.
    ///
    /// If `size` number of items are enqueued, calls to `send` wait until the queue has space before sending.
    pub fn new(size: usize) -> Self {
        Self {
            t: PhantomData,
            internal: deadqueue::limited::Queue::new(size),
        }
    }
}

impl<T> Default for Queue<T>
where
    T: Serialize + DeserializeOwned,
{
    fn default() -> Self {
        Self::new(DEFAULT_LIMIT)
    }
}

impl<T> Queue<T>
where
    T: Serialize,
{
    /// Sends an item into the queue.
    pub async fn send(&self, item: &T) -> Result<(), Report<Error>> {
        let encoded = serde_json::to_vec(item).context(Error::Serialize)?;
        self.internal.push(encoded.to_vec()).await;
        Ok(())
    }
}

impl<T> Queue<T>
where
    T: DeserializeOwned,
{
    /// Retrieves an element from the queue.
    pub async fn recv(&self) -> Result<T, Report<Error>> {
        let data = self.internal.pop().await;
        serde_json::from_slice(&data).context(Error::Deserialize)
    }
}

impl<T> Debug for Queue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Queue<{}>", std::any::type_name::<T>())
    }
}
