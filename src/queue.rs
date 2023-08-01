//! Async work queue implementation.

use std::{fmt::Debug, marker::PhantomData, time::Duration};

use error_stack::Report;
use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    sync::mpsc::{
        channel,
        error::{TryRecvError, TrySendError},
        Receiver,
    },
    time::interval,
};

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

/// A rate limiter, generally used for queue processing operations.
///
/// New rate limits are added to this limiter to be consumed on the provided interval.
/// If there are already `count` limits ready to be used, additional limits are dropped
/// until those are used.
///
/// In other words, this limiter allows bursting up to `count`.
/// No attempt is made to "catch up": additional limits are simply lost.
pub struct RateLimiter {
    period: Duration,
    rx: Receiver<()>,
}

impl RateLimiter {
    /// Create a new instance which allows up to N items every time period.
    ///
    /// # Panics
    ///
    /// Panics if `count` is 0.
    pub fn new(count: usize, period: Duration) -> Self {
        let (tx, rx) = channel::<()>(count);

        // Hydrate the initial limiter by sending until it is full.
        while tx.try_send(()).is_ok() {}

        // Set up a job to hydrate the limiter until the receiver drops.
        tokio::spawn(async move {
            let mut ticker = interval(period);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                ticker.tick().await;

                // Send at most `count` into the limiter.
                //
                // Doing this instead of sending until the buffer is full (as at the start)
                // because that way if there is a concurrent listener that prevents the buffer from filling
                // the rate limit is still observed.
                for _ in 0..count {
                    if let Err(TrySendError::Closed(_)) = tx.try_send(()) {
                        return;
                    }
                }
            }
        });

        Self { period, rx }
    }

    /// Consume a rate limit from the limiter.
    pub async fn consume(&mut self) {
        if self.rx.recv().await.is_none() {
            // This panic only happens in the face of a serious program bug,
            // since the sender side is managed inside a `tokio::spawn` on `RateLimiter` creation
            // and should not exit until the receiver is dropped.
            panic!("RateLimiter: sending channel closed while receiver was still active");
        }
    }

    /// Attempt to consume a rate limit from the limiter.
    /// If successful, returns true; if there is not a rate limit available returns false.
    pub fn try_consume(&mut self) -> bool {
        match self.rx.try_recv() {
            Ok(_) => true,
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                // This panic only happens in the face of a serious program bug,
                // since the sender side is managed inside a `tokio::spawn` on `RateLimiter` creation
                // and should not exit until the receiver is dropped.
                panic!("RateLimiter: sending channel closed while receiver was still active");
            }
        }
    }
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimiter")
            .field("period", &self.period)
            .finish()
    }
}

#[cfg(test)]
mod tests {

    use tokio::{
        select,
        time::{sleep_until, Instant},
    };

    use super::*;

    #[tokio::test]
    async fn rate_limit_limits() {
        let one_ms = Duration::from_millis(1);
        let two_ms = Duration::from_millis(2);

        let start = Instant::now();
        let mut limiter = RateLimiter::new(1, two_ms);

        assert!(limiter.try_consume(), "immediately adds limit");
        assert!(!limiter.try_consume(), "no additional limits");
        limiter.consume().await;

        // Assert on 1ms instead of 2, even though the limit period is 2ms,
        // because time-based tests at this resolution are always fuzzy
        // and I want to keep spurious test failures to a minimum.
        //
        // Even though on its own this introduces _some_ potential for bugs,
        // I think between this and other tests we're covered.
        assert!(
            start.elapsed() > one_ms,
            "limiter should have enforced min time period"
        );
    }

    #[tokio::test]
    async fn rate_limit_multi_limits() {
        let period = Duration::from_millis(100);
        let limit = 3;

        let mut limiter = RateLimiter::new(limit, period);
        let mut ticker = interval(period);

        for i in 0..5 {
            ticker.tick().await;
            for l in 0..limit {
                assert!(
                    limiter.try_consume(),
                    "tick {i}: should have {limit} units available but had {l}"
                );
            }
        }
    }

    #[tokio::test]
    async fn rate_limit_ceiling() {
        let interval = Duration::from_millis(70);
        let limit = 1;

        let mut limiter = RateLimiter::new(limit, interval);
        let mut allowed = 0;

        let start = Instant::now();
        let stop = Duration::from_millis(500);

        // Illustration of this test timeline (in 10 ms chunks):
        //
        //                 100ms     200ms     300ms     400ms     500ms     600ms
        // 10ms ticks   -> |         |         |         |         |         |
        // Timeout      -> |         |         |         |         |X        |
        // Rate limiter -> +      +  |  +      +      +  |   +     |+      + |
        // Consumer     -> |+      + |    +    | +      +|     +   |  +      +
        //
        // Critically:
        // - The limiter starts with 1
        // - With a 70ms interval, that means 0-70ms are the first tick, then _70-140_ are the second.
        //   In other words, the new tick starts inside the same millisecond in which it was consumed.
        //
        // This means we should see 6 70ms ticks inside a 500ms window.
        loop {
            select! {
                _ = limiter.consume() => allowed += 1,
                _ = sleep_until(start + stop) => break,
            }
        }

        let expected = 6;
        assert_eq!(
            allowed,
            expected,
            "should have allowed {expected} rate limit instances in {stop:?} at {interval:?} each; allowed: {allowed}"
        );
    }
}
