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
use getset::Getters;
use libflate::gzip;
use tempfile::NamedTempFile;
use thiserror::Error;
use tracing::{debug, error};
use walkdir::WalkDir;

use crate::{
    ext::{error_stack::IntoContext, io, tracing::span_records},
    AppContext,
};

/// Errors encountered generating a debug bundle.
#[derive(Debug, Error)]
pub enum Error {
    /// When Broker creates the debug bundle, it initially collects the debug artifacts in a temporary file.
    /// This is so that if the overall process fails Broker doesn't accidentally leave a partially-constructed
    /// debug bundle at the output location, potentially confusing the user.
    #[error("create temporary file")]
    CreateTempFile,

    /// When instantiating the bundler, if its options are not correct, this error is the result.
    #[error("create bundler")]
    CreateBundler,

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

/// Intended to provide FOSSA employees with all the information required to debug
/// Broker in any environment, without sharing sensitive information.
#[derive(Debug, Clone, Getters)]
#[getset(get = "pub")]
pub struct Bundle {
    /// The location to the debug bundle that was persisted.
    location: PathBuf,
}

/// Provider for writing files to a debug bundle.
pub trait Bundler {
    /// The error returned by this implementation.
    type Error;

    /// Add a new entry from a file on the local file system with the provided name.
    fn add_file<P, N>(&mut self, path: P, name: N) -> std::result::Result<(), Self::Error>
    where
        P: AsRef<Path>,
        N: AsRef<Path>;

    /// Finalize the bundle.
    fn finalize<P: AsRef<Path>>(self, destination: P) -> std::result::Result<Bundle, Self::Error>;
}

/// Metadata for a bundled entry.
pub struct EntryMetadata {}

/// Generate a debug bundle for the application at the specified location.
/// Ultimately this means "write a copy of every file inside the debug root to the bundler".
///
/// In the future, we'd like to decompress and potentially prettify the FOSSA CLI debug bundles
/// before including them in the overall debug bundle; this would yield better compression ratios.
/// For now though we just include them as is.
// #[tracing::instrument(skip(bundler, path), fields(debug_root, path))]
pub fn generate<B, P>(ctx: &AppContext, mut bundler: B, path: P) -> Result<Bundle, Error>
where
    B: Bundler,
    B::Error: Context,
    P: AsRef<Path>,
{
    let debug_root = ctx.data_dir("debug");
    span_records! {
        debug_root => display debug_root.display();
        path => display path.as_ref().display();
    }

    for entry in WalkDir::new(&debug_root).follow_links(false) {
        let entry = entry.context_lazy(|| Error::walk_contents(&debug_root))?;
        let path = entry.path();
        let rel = match path.strip_prefix(&debug_root) {
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
        let copy = io::sync::copy_temp(path).change_context(Error::CreateTempFile)?;
        bundler
            .add_file(copy.path(), rel)
            .context_lazy(|| Error::bundle_contents(&debug_root, rel))?;
    }

    bundler.finalize(path).context(Error::Finalize)
}

/// Implementation for [`Bundler`] which bundles files into a `.tar.gz` file.
pub struct TarGzBundler {
    inner: tar::Builder<gzip::Encoder<NamedTempFile>>,
}

impl TarGzBundler {
    /// Create a new instance.
    ///
    /// This bundles the provided debug artifacts using a backing temp file,
    /// which is moved to a final location with the `finalize` method.
    pub fn new() -> Result<Self, Error> {
        let file = NamedTempFile::new().context(Error::CreateTempFile)?;
        let encoder = gzip::Encoder::new(file).context(Error::CreateBundler)?;
        Ok(Self {
            inner: tar::Builder::new(encoder),
        })
    }
}

impl Bundler for TarGzBundler {
    type Error = std::io::Error;

    fn add_file<P, N>(&mut self, path: P, name: N) -> std::result::Result<(), Self::Error>
    where
        P: AsRef<Path>,
        N: AsRef<Path>,
    {
        self.inner.append_path_with_name(path, name)
    }

    fn finalize<P: AsRef<Path>>(self, destination: P) -> std::result::Result<Bundle, Self::Error> {
        let zip = self.inner.into_inner()?;
        let handle = zip.finish().into_result()?;
        handle.as_file().sync_all()?;
        handle.persist(&destination)?;
        Ok(Bundle {
            location: destination.as_ref().to_path_buf(),
        })
    }
}
