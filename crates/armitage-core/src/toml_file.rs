//! File-backed TOML document trait.
//!
//! Many config types in the workspace follow the same pattern: deserialize from
//! a fixed filename inside the org root (or any caller-supplied directory),
//! serialize back with `toml::to_string_pretty`, and treat a missing file as an
//! empty default. [`TomlFile`] captures that pattern.
//!
//! ## Error type plumbing
//!
//! Each domain crate keeps its own [`thiserror`] error enum. To avoid forcing
//! `armitage_core::error::Error` on every implementer, the trait carries an
//! associated `Error` type that must be convertible from `std::io::Error` and
//! `toml::ser::Error`, plus a `parse_error` constructor for the `TomlParse`
//! variant (which is path-aware and can't be expressed via `From`).
//!
//! Types that don't have a fixed on-disk filename (e.g. per-directory
//! `node.toml`) should not implement this trait.
use std::path::{Path, PathBuf};

/// File-backed TOML document with consistent read/write semantics.
///
/// The [`file_name`](TomlFile::file_name) impl pins the on-disk filename;
/// callers pass the directory (typically the org root) and the trait resolves
/// the full path.
pub trait TomlFile: serde::de::DeserializeOwned + serde::Serialize {
    /// The crate-local error type returned by [`read`](TomlFile::read) and
    /// [`write`](TomlFile::write).
    type Error: From<std::io::Error> + From<toml::ser::Error>;

    /// Filename relative to the directory passed to `read`/`write`.
    fn file_name() -> &'static str;

    /// Construct the crate-local error variant for a TOML parse failure with
    /// the offending path attached.
    fn parse_error(path: PathBuf, source: toml::de::Error) -> Self::Error;

    /// Return an empty default when the file does not exist on disk. The blanket
    /// implementation returns `None`; types that implement [`Default`] should
    /// override this to return `Some(Self::default())`.
    fn default_when_missing() -> Option<Self> {
        None
    }

    /// Resolve the on-disk path inside `dir`.
    fn path(dir: &Path) -> PathBuf {
        dir.join(Self::file_name())
    }

    /// Read the file from `dir`. If the file does not exist and
    /// [`default_when_missing`](TomlFile::default_when_missing) returns
    /// `Some(_)`, that value is returned instead.
    fn read(dir: &Path) -> Result<Self, Self::Error> {
        let path = Self::path(dir);
        if !path.exists()
            && let Some(default) = Self::default_when_missing()
        {
            return Ok(default);
        }
        let content = std::fs::read_to_string(&path)?;
        toml::from_str(&content).map_err(|source| Self::parse_error(path, source))
    }

    /// Write the file to `dir`, serializing with `toml::to_string_pretty`.
    fn write(&self, dir: &Path) -> Result<(), Self::Error> {
        let path = Self::path(dir);
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}
