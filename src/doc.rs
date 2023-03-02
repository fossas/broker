//! Constants and functions for shared access to documentation.

use once_cell::sync::OnceCell;

pub mod link;

/// The git SHA for the current build.
pub fn build_sha() -> &'static str {
    // This value is set in `build.rs` and evaluated at compile time.
    static LAZY: OnceCell<&'static str> = OnceCell::new();
    LAZY.get_or_init(|| env!("GIT_HASH"))
}

/// The crate repo URL.
pub fn repo_home() -> &'static str {
    // This value is set by Cargo and evaluated at compile time.
    static LAZY: OnceCell<&'static str> = OnceCell::new();
    LAZY.get_or_init(|| env!("CARGO_PKG_REPOSITORY"))
}
