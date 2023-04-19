//! The debug bundle is analogous to other FOSSA "debug bundle" implementations:
//!
//! - Debug bundle in FOSSA CLI
//! - Debug bundle in FOSSA Helm Charts
//!
//! The idea is that users can ask Broker to generate a "debug bundle",
//! which contains all the information FOSSA engineers need to reproduce or debug a problem,
//! while doing our best to not send internal information such as source code to FOSSA.

use std::path::{Path, PathBuf};

use error_stack::{Context, Result, ResultExt};
use thiserror::Error;
use tracing::{debug, error};
use walkdir::WalkDir;

use crate::ext::{error_stack::IntoContext, io::sync::copy_debug_bundle, tracing::span_records};

use super::{bundler::Bundler, Bundle, Config};

/// Errors encountered generating a debug bundle.
#[derive(Debug, Error)]
pub enum Error {
    /// When Broker creates the debug bundle, it initially collects the debug artifacts in a temporary file.
    /// This is so that if the overall process fails Broker doesn't accidentally leave a partially-constructed
    /// debug bundle at the output location, potentially confusing the user.
    #[error("create temporary file")]
    CreateTempFile,

    /// In order to write traces and FOSSA CLI debug bundles, Broker must be able to walk the debug directory.
    #[error("walk contents of directory: '{}'", .0.display())]
    WalkContents(PathBuf),

    /// Broker wasn't able to bundle the directory.
    #[error("bundle contents of file in directory '{}': '{}'", .dir.display(), .file.display())]
    BundleContents {
        /// The root directory for debug artifacts.
        dir: PathBuf,

        /// The file being bundled.
        file: PathBuf,
    },

    /// When finished bundling everything, it's finalized into a [`Bundle`].
    /// If this fails, this error is reported.
    #[error("finalize debug bundle")]
    Finalize,
}

impl Error {
    fn walk_contents(dir: &Path) -> Self {
        Self::WalkContents(dir.to_owned())
    }

    fn bundle_contents(dir: &Path, file: &Path) -> Self {
        Self::BundleContents {
            dir: dir.to_owned(),
            file: file.to_owned(),
        }
    }
}

/// Generate a debug bundle for the application at the specified location.
/// Ultimately this means "write a copy of every file inside the debug root to the bundler".
///
/// In the future, we'd like to decompress and potentially prettify the FOSSA CLI debug bundles
/// before including them in the overall debug bundle; this would yield better compression ratios.
/// For now though we just include them as is.
// #[tracing::instrument(skip(bundler, path), fields(debug_root, path))]
pub fn generate<B, P>(conf: &Config, mut bundler: B, path: P) -> Result<Bundle, Error>
where
    B: Bundler,
    B::Error: Context,
    P: AsRef<Path>,
{
    let debug_root = conf.location().as_path();
    span_records! {
        debug_root => display debug_root.display();
        path => display path.as_ref().display();
    }

    for entry in WalkDir::new(debug_root).follow_links(false) {
        let entry = entry.context_lazy(|| Error::walk_contents(debug_root))?;
        let path = entry.path();
        let rel = match path.strip_prefix(debug_root) {
            Ok(rel) => rel,
            Err(err) => {
                error!(
                    debug_root = %debug_root.display(),
                    path = %path.display(),
                    %err,
                    "Skipping 'path': could not make relative to 'debug_root', see 'err' for details",
                );
                continue;
            }
        };

        if !entry.file_type().is_file() {
            debug!(path = %path.display(), "Skipping '{}': not a file", rel.display());
            continue;
        }

        // Copy the file to temp first, so that it's not changed while the tar is being built.
        // This also decompresses and formats debug bundles if the copied file is in fact a debug bundle.
        let (copy, rel) = copy_debug_bundle(path, rel).change_context(Error::CreateTempFile)?;

        // Add the file to the bundle.
        bundler
            .add_file(copy.path(), &rel)
            .context_lazy(|| Error::bundle_contents(debug_root, &rel))?;
    }

    bundler.finalize(path).context(Error::Finalize)
}
