//! Generic extensions that don't fit to a specific library or type.

/// Throw away any value.
pub trait Voided<T> {
    /// Discard the value, returning the default expected for the returned value instead.
    ///
    /// The return type is usually able to be inferred by usage, but for cases where it cannot
    /// it can be provided by explicitly typing the destination variable.
    ///
    /// # Example
    ///
    /// ```
    /// # use broker::ext::generic::Voided;
    /// let generate = || String::from("some text");
    ///
    /// // We can throw away the same type of value.
    /// let value: String = generate().void();
    /// assert_eq!(value, String::from(""));
    ///
    /// // We can modify the destination type as well.
    /// let _: () = generate().void();
    ///
    /// // Usually, the desired type can be inferred.
    /// fn do_thing() -> String {
    ///     String::from("some text").void() // inferred to String here
    /// }
    ///
    /// assert_eq!(do_thing(), String::from(""));
    /// ```
    fn void(self) -> T;
}

impl<T, U> Voided<T> for U
where
    T: Default,
{
    fn void(self) -> T {
        Default::default()
    }
}
