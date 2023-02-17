//! Extensions to other libraries are stored here.
//!
//! Often these are prime candidates for upstreaming, but they may not be for a few reasons:
//! - We haven't had time to try to upstream.
//! - They don't fit the vision of the upstream.
//! - We aren't sure how to make them more generic in order to fit the upstream.

pub mod error_stack;
pub mod iter;
pub mod result;
