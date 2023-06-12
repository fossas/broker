//! Trait and implementations used by `bundle` to create debug bundles.

use std::{fs, path::Path};

use error_stack::Result;
use libflate::gzip;
use tempfile::NamedTempFile;
use thiserror::Error;

use crate::ext::error_stack::IntoContext;

use super::Bundle;

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

/// Implementation for [`Bundler`] which bundles files into a `.tar.gz` file.
pub struct TarGz {
    inner: tar::Builder<gzip::Encoder<NamedTempFile>>,
}

impl TarGz {
    /// Create a new instance.
    ///
    /// This bundles the provided debug artifacts using a backing temp file,
    /// which is moved to a final location with the `finalize` method.
    #[tracing::instrument]
    pub fn new() -> Result<Self, Error> {
        let file = NamedTempFile::new().context(Error::CreateTempFile)?;
        let encoder = gzip::Encoder::new(file).context(Error::CreateBundler)?;
        Ok(Self {
            inner: tar::Builder::new(encoder),
        })
    }
}

impl Bundler for TarGz {
    type Error = std::io::Error;

    #[tracing::instrument(skip_all, fields(path = %path.as_ref().display(), name = %name.as_ref().display()))]
    fn add_file<P, N>(&mut self, path: P, name: N) -> std::result::Result<(), Self::Error>
    where
        P: AsRef<Path>,
        N: AsRef<Path>,
    {
        self.inner.append_path_with_name(path, name)
    }

    #[tracing::instrument(skip_all, fields(destination = %destination.as_ref().display()))]
    fn finalize<P: AsRef<Path>>(self, destination: P) -> std::result::Result<Bundle, Self::Error> {
        let zip = self.inner.into_inner()?;
        let handle = zip.finish().into_result()?;
        handle.as_file().sync_all()?;

        // `handle.persist` fails if asked to persist across mounts, since internally it uses a rename.
        // Instead, just copy the file from temp.
        let path = handle.into_temp_path();
        fs::copy(&path, destination.as_ref())?;

        Ok(Bundle {
            location: destination.as_ref().to_path_buf(),
        })
    }
}
