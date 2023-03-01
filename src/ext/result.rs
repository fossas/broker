//! Extensions to `Result`.

/// Flip `Result<T, E>` to `Result<E, T>`.
pub trait FlipResult<T, E> {
    /// Flip `Result<T, E>` to `Result<E, T>`.
    fn flip(self) -> Result<E, T>;
}

impl<T, E> FlipResult<T, E> for Result<T, E> {
    fn flip(self) -> Result<E, T> {
        match self {
            Ok(t) => Err(t),
            Err(e) => Ok(e),
        }
    }
}

/// Local reimplementation of [`std::result::Result::inspect_err`], since it is still unstable.
pub trait InspectErr<T, E, F> {
    /// Calls the provided closure with a reference to the contained error (if [`Err`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{fs, io};
    ///
    /// fn read() -> io::Result<String> {
    ///     fs::read_to_string("address.txt")
    ///         .ext_inspect_err(|e| eprintln!("failed to read file: {e}"))
    /// }
    /// ```
    fn ext_inspect_err(self, inspector: F) -> Result<T, E>;
}

impl<T, E, F> InspectErr<T, E, F> for Result<T, E>
where
    F: Fn(&E),
{
    fn ext_inspect_err(self, inspector: F) -> Result<T, E> {
        if let Err(ref e) = self {
            inspector(e);
        }

        self
    }
}
