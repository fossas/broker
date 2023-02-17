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
