//! This library exists to decouple functionality from frontend UX, in an attempt to make testing easier
//! and lead to better overall maintainability.
//!
//! While it is possible to import this library from another Rust program, this library
//! may make major breaking changes on _any_ release, as it is not considered part of the API contract
//! for Broker (which is distributed to end users in binary form only).

#![deny(clippy::unwrap_used)]
#![deny(unsafe_code)]
#![deny(missing_docs)]
#![warn(rust_2018_idioms)]

pub mod api;
pub mod config;
pub mod db;
pub mod debug;
pub mod doc;
pub mod download_fossa_cli;
pub mod ext;
pub mod queue;
pub mod subcommand;

/// Get the path to a subdirectory of the data root for the current module
/// with the given context.
///
/// See also [`Context::data_root`], which allows customization of the module name.
///
/// # Example
///
/// ```
/// mod my_module {
///     pub fn my_function(ctx: &broker::AppContext) {
///         let subdir = broker::data_dir!(ctx);
///         subdir.strip_prefix(ctx.data_root()).expect("must be a subdirectory");
///     }
/// }
///
/// # let tmp = tempfile::tempdir().expect("must create tempdir");
/// # let root = tmp.path().to_path_buf();
/// # let ctx = broker::AppContext::new(root);
/// # my_module::my_function(&ctx);
/// ```
#[macro_export]
macro_rules! data_dir {
    ($ctx:ident) => {
        $ctx.data_dir(module_path!())
    };
}

pub use ctx::AppContext;

/// Put this in a submodule so that children can't access internals.
mod ctx {
    use std::path::PathBuf;

    use getset::Getters;

    /// Context that many parts of the program need to know about, arranged into a single type for dependency injection.
    ///
    /// This type should be added to sparingly; definitely prefer to pass in args to functions over using context
    /// unless truly _large_ portions of the program need to reference the same data.
    ///
    /// As an example, the data dir for the program is a good candidate for context, as it is used in basically
    /// all functions that perform IO (which is most functions, since this program almost entirely revolves
    /// around doing IO and running FOSSA CLI).
    #[derive(Debug, Clone, Getters)]
    #[getset(get = "pub")]
    pub struct AppContext {
        /// The root directory for all data that the program needs to store.
        ///
        /// Most modules use subdirectories inside this root;
        /// to play it maximally safe and avoid collisions,
        /// consider using a subdirectory with the [`data_dir`] function.
        data_root: PathBuf,
    }

    impl AppContext {
        /// Create a new context.
        pub fn new(data_root: PathBuf) -> Self {
            // Note: if we get too many things in here, switch to builder pattern via `typed_builder`.
            Self { data_root }
        }

        /// Get the path to a subdirectory of the data root for a given module name.
        ///
        /// Any name may be provided, but it's recommended to use `moudule_path!()`
        /// to minimize chance of collisions with other modules.
        ///
        /// See also [`data_dir!`], which is a macro that automatically uses the current module name.
        ///
        /// # Example
        ///
        /// ```
        /// mod my_module {
        ///     pub fn my_function(ctx: &broker::AppContext) {
        ///         let subdir = ctx.data_dir(module_path!());
        ///         subdir.strip_prefix(ctx.data_root()).expect("must be a subdirectory");
        ///     }
        /// }
        ///
        /// # let tmp = tempfile::tempdir().expect("must create tempdir");
        /// # let root = tmp.path().to_path_buf();
        /// # let ctx = broker::AppContext::new(root);
        /// # my_module::my_function(&ctx);
        /// ```
        #[track_caller]
        pub fn data_dir(&self, module_name: &str) -> PathBuf {
            let module_name = module_name.replace("::", "-");
            self.data_root().join(module_name)
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    // Nest modules to test the submodule path logic.
    mod subtest {
        use super::*;

        #[test]
        fn creates_data_subdir() {
            let tmp = tempdir().expect("must create tempdir");
            let ctx = AppContext::new(tmp.path().to_path_buf());
            let subdir = ctx.data_dir(module_path!());

            subdir
                .strip_prefix(ctx.data_root())
                .expect("must be a subdirectory");
            assert_eq!(subdir, tmp.path().join("broker-tests-subtest"));
        }
    }

    #[test]
    fn creates_data_subdir() {
        let tmp = tempdir().expect("must create tempdir");
        let ctx = AppContext::new(tmp.path().to_path_buf());
        let subdir = ctx.data_dir(module_path!());

        subdir
            .strip_prefix(ctx.data_root())
            .expect("must be a subdirectory");
        assert_eq!(subdir, tmp.path().join("broker-tests"));
    }
}
