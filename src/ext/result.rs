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
