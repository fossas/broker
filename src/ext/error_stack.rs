//! Extensions to `error_stack`.

use colored::Colorize;
use error_stack::ResultExt;

/// Used to provide help text to an error.
///
/// This is meant to be readable by users of the application;
/// ideally help text is relatively terse and only displayed when
/// you're pretty sure what the user can do to fix the problem.
pub trait ErrorHelper {
    /// Provide help text to the user with what they can do to fix the problem.
    fn help<S: AsRef<str>>(self, help_text: S) -> Self;

    /// Optionally provide help text to the user with what they can do to fix the problem.
    fn help_if<S: AsRef<str>>(self, should_help: bool, help_text: S) -> Self;

    /// Lazily provide help text to the user with what they can do to fix the problem.
    fn help_lazy<S: AsRef<str>, F: FnOnce() -> S>(self, helper: F) -> Self;
}

impl<T, C> ErrorHelper for error_stack::Result<T, C> {
    fn help<S: AsRef<str>>(self, help_text: S) -> Self {
        let help = help_literal();
        let help_text = help_text.as_ref();
        self.attach_printable_lazy(|| format!("{help} {help_text}"))
    }

    fn help_if<S: AsRef<str>>(self, should_help: bool, help_text: S) -> Self {
        if should_help {
            let help = help_literal();
            let help_text = help_text.as_ref();
            self.attach_printable_lazy(|| format!("{help} {help_text}"))
        } else {
            self
        }
    }

    fn help_lazy<S: AsRef<str>, F: FnOnce() -> S>(self, helper: F) -> Self {
        let help = help_literal();
        let help_text = helper();
        let help_text = help_text.as_ref();
        self.attach_printable_lazy(|| format!("{help} {help_text}"))
    }
}

fn help_literal() -> String {
    "help:".bold().blue().to_string()
}

/// Used to provide a description of the operation being performed when an error occurred.
pub trait DescribeContext {
    /// Provide a human-readable description of the context in which the error occurred.
    fn describe<S: AsRef<str>>(self, description: S) -> Self;

    /// Optionally provide a human-readable description of the context in which the error occurred.
    fn describe_if<S: AsRef<str>>(self, should_describe: bool, description: S) -> Self;

    /// Lazily provide a human-readable description of the context in which the error occurred.
    fn describe_lazy<S: AsRef<str>, F: FnOnce() -> S>(self, describer: F) -> Self;
}

impl<T, C> DescribeContext for error_stack::Result<T, C> {
    fn describe<S: AsRef<str>>(self, description: S) -> Self {
        let context = describe_literal();
        let description = description.as_ref();
        self.attach_printable_lazy(|| format!("{context} {description}"))
    }

    fn describe_if<S: AsRef<str>>(self, should_describe: bool, description: S) -> Self {
        if should_describe {
            let context = describe_literal();
            let description = description.as_ref();
            self.attach_printable_lazy(|| format!("{context} {description}"))
        } else {
            self
        }
    }

    fn describe_lazy<S: AsRef<str>, F: FnOnce() -> S>(self, describer: F) -> Self {
        let context = describe_literal();
        let description = describer();
        let description = description.as_ref();
        self.attach_printable_lazy(|| format!("{context} {description}"))
    }
}

fn describe_literal() -> String {
    "context:".bold().green().to_string()
}
