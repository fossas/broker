//! Constants and functions for shared access to documentation.

use once_cell::sync::OnceCell;
use semver::Version;

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

/// The crate name.
pub fn crate_name() -> &'static str {
    // This value is set by Cargo and evaluated at compile time.
    static LAZY: OnceCell<&'static str> = OnceCell::new();
    LAZY.get_or_init(|| env!("CARGO_PKG_NAME"))
}

/// The crate version.
pub fn crate_version() -> &'static Version {
    // This value is set by Cargo and evaluated at compile time.
    static LAZY: OnceCell<Version> = OnceCell::new();
    LAZY.get_or_init(|| {
        let version = env!("CARGO_PKG_VERSION");
        Version::parse(version).expect("the version compiled into Broker must be valid semver")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version() {
        let version = crate_version().to_string();
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }
}
