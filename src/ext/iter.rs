//! Extensions to iterators.

use error_stack::Report;

use super::result::FlipResult;

/// Extend any iterator for chaining lazily created items.
pub trait ChainOnceWithIter<T> {
    /// Chain an iterator that will lazily produce a single item.
    fn chain_once_with<F>(self, gen: F) -> ChainOnceWith<Self, F>
    where
        Self: Sized,
        F: FnOnce() -> T;
}

impl<I: Iterator<Item = T>, T> ChainOnceWithIter<T> for I {
    fn chain_once_with<F>(self, generator: F) -> ChainOnceWith<Self, F>
    where
        Self: Sized,
        F: FnOnce() -> T,
    {
        ChainOnceWith {
            iter: self,
            generator: Some(generator),
        }
    }
}

/// Given an iterator, chain a lazily evaluated value after the iterator is exhausted.
/// The equivalent of `Iterator::chain(iter::once_with(|| F()))`.
pub struct ChainOnceWith<I, F> {
    iter: I,
    generator: Option<F>,
}

impl<I, T, F> Iterator for ChainOnceWith<I, F>
where
    I: Iterator<Item = T>,
    F: FnOnce() -> T,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(next) = self.iter.next() {
            return Some(next);
        }

        let generator = self.generator.take()?;
        Some(generator())
    }
}

/// Implement `alternative`
pub trait AlternativeIter<T, E> {
    /// Given an iterator over `Result<T, Report<E>>`,
    /// serially fold over multiple fallible operation results, combining their errors into
    /// the final error stack and returning the result of the first successful operation.
    /// If none were successful, `Err` contains the stacked errors from all attempts.
    fn alternative_fold(self) -> Result<T, Report<E>>;
}

impl<I: Iterator<Item = Result<T, Report<E>>>, T, E> AlternativeIter<T, E> for I {
    fn alternative_fold(self) -> Result<T, Report<E>> {
        self
            // `try_fold` early exits on error; meanwhile we want to early exit on success.
            // To get it to work how we want, flip `Result<T, E>` into `Result<E, T>`
            // and collect errors together into a single "successful" result at the end.
            // Finally, flip it back and treat it like a normal error.
            //
            // The only frustrating part here is that to handle the first iteration
            // we have to use `None` (since as of the first iteration, there's no error).
            // As programmers, we _know_ that if the final `Result<T, Option<Report<E>>>`
            // is `Err`, then it _must_ also be `Err(Some(_))`.
            // Unfortunately this means we have to `unwrap` (or provide a much worse API).
            //
            // Possible fixes for the future:
            // - Extend `error_stack` to have an empty-but-not-null error, and then `mem::replace` it.
            // - Build/find a stable version of `try_reduce` and use it.
            .map(|result| result.flip())
            .try_fold(None::<Report<E>>, |mut stack, operation| {
                operation.map(|actually_err| {
                    if let Some(stack) = stack.as_mut() {
                        stack.extend_one(actually_err);
                    } else {
                        stack = Some(actually_err);
                    }
                    stack
                })
            })
            // Flip the `Result<E, T>` back to `Result<T, E>`.
            .flip()
            // In the error case, unwrap the `Option`.
            // While this panics if the error is actually `None`,
            // we know this is safe because we'll only get here if there's no `Ok`
            // in which case there _must_ have been an error.
            .map_err(|err| err.expect("internal invariant: err is known to be Some"))
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, iter};

    use super::*;

    /// `collect` is an infinite sink, so this validates both that `chain_once_with`
    /// actually runs and that it actually only runs once.
    #[test]
    fn chain_once() {
        let values = iter::once_with(|| 1)
            .chain_once_with(|| 2)
            .collect::<Vec<_>>();
        assert_eq!(vec![1, 2], values);
    }

    #[test]
    fn alternative_fold_early_return_on_ok() {
        #[derive(Debug, thiserror::Error)]
        #[error("some error")]
        struct Error;

        // Rust can't statically verify that we don't run both closures below at the same time;
        // we know we don't (because we're passing them in to iterators)
        // so use `RefCell` to move the check to runtime.
        let errors_hit = RefCell::new(0);

        let fold = iter::once_with(|| {
            errors_hit.replace_with(|hits| *hits + 1);
            Err(Report::new(Error))
        })
        .chain_once_with(|| Ok(2))
        .chain_once_with(|| {
            errors_hit.replace_with(|hits| *hits + 1);
            Err(Report::new(Error))
        })
        .alternative_fold();

        assert_eq!(2, fold.expect("must have returned successfully"));
        assert_eq!(1, errors_hit.into_inner(), "must have early returned");
    }

    #[test]
    fn alternative_fold_collects_errors() {
        #[derive(Debug, thiserror::Error)]
        enum Error {
            #[error("some error")]
            Something,
            #[error("some other error")]
            SomethingElse,
        }

        let fold: Result<(), Report<Error>> =
            iter::once_with(|| Err(Report::new(Error::Something)))
                .chain_once_with(|| Err(Report::new(Error::SomethingElse)))
                .alternative_fold();

        let errs = fold.expect_err("must have errored");
        assert!(
            format!("{errs:?}").contains("some error"),
            "must have reported 'some error' in text: {errs:?}"
        );
        assert!(
            format!("{errs:?}").contains("some other error"),
            "must have reported 'some other error' in text: {errs:?}"
        );
    }
}
