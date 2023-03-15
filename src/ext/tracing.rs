//! Extensions to the `tracing` library.

/// Record the provided value in the currently active span context,
/// in the form `span_record!(field, value)`.
///
/// By default, `value` is expected to implement [`tracing::field::Value`]:
/// ```ignore
/// span_record!(result, true);
/// ```
///
/// If desired, one may alternately use the `Display` or `Debug` implementations:
/// ```ignore
/// span_record!(result, display result);
/// span_record!(result, debug result);
/// ```
macro_rules! span_record {
    ($field:expr, $value:expr) => {{
        tracing::Span::current().record(stringify!($field), $value);
    }};
    ($field:expr, display $value:expr) => {{
        tracing::Span::current().record(stringify!($field), format!("{}", $value));
    }};
    ($field:expr, debug $value:expr) => {{
        tracing::Span::current().record(stringify!($field), format!("{:?}", $value));
    }};
}

/// Similar to [`span_record`], but allows multiple records in a single macro invocation.
/// See [`span_record`] for details.
///
/// # Example
///
/// ```ignore
/// span_records! {
///     "rendered_result" => display result;
///     "debug_result" => debug result;
/// };
/// ```
macro_rules! span_records {
    () => {};
    ($field:expr => $value:expr; $($tail:tt)*) => {{
        crate::ext::tracing::span_record!($field, $value);
        span_records!($($tail)*);
    }};
    ($field:expr => display $value:expr; $($tail:tt)*) => {{
        crate::ext::tracing::span_record!($field, display $value);
        span_records!($($tail)*);
    }};
    ($field:expr => debug $value:expr; $($tail:tt)*) => {{
        crate::ext::tracing::span_record!($field, debug $value);
        span_records!($($tail)*);
    }};
}

pub(crate) use span_record;
pub(crate) use span_records;

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::{field, trace_span};

    /// Important: if this fails, make sure to update the docs above.
    ///
    /// Usually this would be a doc comment but I don't want to export the
    /// macro, but would need to do so in order to make doctests work.
    #[test]
    fn validate_single_works() {
        let span = trace_span!("some_span", result = field::Empty);
        let _e = span.enter();

        #[derive(Debug)]
        struct MyValue {
            inner: usize,
        }

        impl std::fmt::Display for MyValue {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "MyValue({})", self.inner)
            }
        }

        let value = MyValue { inner: 10 };
        span_record!(result, display value);
        span_record!(result, debug value);
    }

    #[test]
    fn validate_multiple_works() {
        let span = trace_span!("some_span");
        let _e = span.enter();

        #[derive(Debug)]
        struct MyValue {
            inner: usize,
        }

        impl std::fmt::Display for MyValue {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "MyValue({})", self.inner)
            }
        }

        let value = MyValue { inner: 10 };
        span_records! {
            basic => true;
            result => display value;
            result => debug value;
        };
    }
}
