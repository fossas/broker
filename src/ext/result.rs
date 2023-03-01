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

/// Discard the successful result, instead returning the default value expected for the destination type.
pub trait DiscardResult<T, E> {
    /// Discard the successful result, instead returning the default value expected for the destination type.
    /// This method relies on type inference for the intended return type.
    fn discard_ok(self) -> Result<T, E>;
}

/// This set of types allows mapping `Result<I, E>` into `Result<O, E>` by supplying a default for `O`.
impl<I, O, E> DiscardResult<O, E> for Result<I, E>
where
    O: Default,
{
    fn discard_ok(self) -> Result<O, E> {
        self.map(|_| Default::default())
    }
}
