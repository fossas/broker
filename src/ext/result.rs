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

/// Wrap anything into `Ok`
/// This is especially useful when performing long chains or when otherwise wrapping
/// would result in many nested parenthesis (which can be hard to read).
///
/// The intention with this trait is to more ergonomically wrap a chain into an `Ok` value, e.g. instead of
/// ```
/// # enum Error {}
/// fn some_fallible_function(input: &str) -> Result<String, Error> {
///   Ok(input.to_string().to_uppercase())
/// }
/// ```
///
/// One can instead write:
/// ```
/// # use broker::ext::result::WrapOk;
/// # enum Error {}
/// fn some_fallible_function(input: &str) -> Result<String, Error> {
///   input.to_string().to_uppercase().wrap_ok()
/// }
/// ```
pub trait WrapOk<T, E> {
    /// Wrap self in an `Ok`, returning the `Ok` variant
    /// of a result inferred by the destination type.
    fn wrap_ok(self) -> Result<T, E>;
}

impl<T, E> WrapOk<T, E> for T {
    fn wrap_ok(self) -> Result<T, E> {
        Ok(self)
    }
}

/// Wrap anything into `Err`
/// This is especially useful when performing long chains or when otherwise wrapping
/// would result in many nested parenthesis (which can be hard to read).
///
/// The intention with this trait is to more ergonomically wrap a chain into an `Err` value, e.g. instead of
/// ```
/// fn some_fallible_function(_input: &str) -> Result<(), String> {
///   Err(String::from("oh no!"))
/// }
/// ```
///
/// One can instead write:
/// ```
/// # use broker::ext::result::WrapErr;
/// # enum Error {}
/// fn some_fallible_function(_input: &str) -> Result<(), String> {
///   String::from("oh no!").wrap_err()
/// }
/// ```
pub trait WrapErr<T, E> {
    /// Wrap self in an `Err`, returning the `Err` variant
    /// of a result inferred by the destination type.
    fn wrap_err(self) -> Result<T, E>;
}

impl<T, E> WrapErr<T, E> for E {
    fn wrap_err(self) -> Result<T, E> {
        Err(self)
    }
}
