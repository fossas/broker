//! Functions and constants for documentation links.

use once_cell::sync::OnceCell;

/// The reference documentation for the config file.
pub fn config_file_reference() -> &'static str {
    // This value is set by Cargo and evaluated at compile time.
    static LAZY: OnceCell<String> = OnceCell::new();
    LAZY.get_or_init(|| {
        let sha = super::build_sha();
        let home = super::repo_home();
        format!("{home}/blob/{sha}/docs/reference/config.md")
    })
}

// TODO: add tests that hit the URLs and validate they exist.
