//! Extensions to the `error_stack` crate.

use error_stack::fmt::HookContext;

/// Provide help text for a given error.
///
/// Use via `.attach(ErrorHelp("some help text"))` on any `error_context::Result`.
///
/// To print help text in error stacks, ensure `main` installs the debug hook with
/// `Report::install_debug_hook(ErrorHelp::debug_hook);`
pub struct ErrorHelp(&'static str);

impl ErrorHelp {
    /// Prints the help text and attaches it to the error context stack.
    pub fn debug_hook(ErrorHelp(content): &Self, context: &mut HookContext<Self>) {
        context.push_body(format!("help: {content}"));
    }
}
