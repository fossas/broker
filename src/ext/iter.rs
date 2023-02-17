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
    ///
    /// Panics if the iterator does not yield any items.
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
            // The only frustrating part here is that it's possible for the iterator to
            // not actually yield any items, in which case we cannot get a response at all.
            // An ideal way to handle this would be to have some form of "non-empty iterator".
            // Unfortunately this means we have to `unwrap` (or provide a much worse API that forces
            // users to deal with `Option`).
            //
            // Possible fixes for the future:
            // - Create a `NonEmptyIterator` that enforces at compile time that there is at least one item.
            .map(|result| result.flip())
            .try_fold(Vec::new(), |mut errs, operation| {
                operation.map(|actually_err| {
                    errs.push(actually_err);
                    errs
                })
            })
            // Flip the `Result<E, T>` back to `Result<T, E>`.
            .flip()
            // In the error case, reduce all the errors seen into a single one via `extend_one`.
            // Due to the "frustrating part" explained above, we must handle the case that the `Vec` of errors is empty;
            // this occurs if no items were ever given to the iterator.
            // This is clearly a misuse of this API, and so it generates a panic in this scenario,
            // but ideally we'd convert this to a compile time check into a run time check.
            .map_err(collapse_errs_stack)
            .map_err(|stack| stack.expect("invariant: iterator consumed by `alternative_fold` must yield at least one item"))
    }
}

/// Using `extend_one`, collapse an iterable of reports into a single report.
fn collapse_errs_stack<I: IntoIterator<Item = Report<E>>, E>(errs: I) -> Option<Report<E>> {
    errs.into_iter().reduce(|mut stack, err| {
        stack.extend_one(err);
        stack
    })
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

    #[test]
    #[should_panic = "invariant: iterator consumed by `alternative_fold` must yield at least one item"]
    fn alternative_fold_invariant_empty_iter() {
        #[derive(Debug, thiserror::Error)]
        #[error("some error")]
        struct Error;

        let _ = iter::empty::<Result<(), Report<Error>>>().alternative_fold();
    }
}
