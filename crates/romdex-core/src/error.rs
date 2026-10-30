use std::path::{Path, PathBuf};

/// A failure that stops an operation. Problems RomDex can work around (a malformed gamelist
/// entry, an unreadable overlay) are [`Warning`]s instead, so one bad file never hides a library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path}: {message}")]
    Parse { path: PathBuf, message: String },

    /// A view or a path into it that cannot be resolved.
    #[error("{0}")]
    View(String),

    #[error("library database: {0}")]
    Db(#[from] rusqlite::Error),
}

impl Error {
    pub(crate) fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Something RomDex skipped or worked around, with the file it came from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Warning {
    pub file: PathBuf,
    pub message: String,
}

impl Warning {
    pub(crate) fn new(file: &Path, message: impl Into<String>) -> Self {
        Self {
            file: file.to_path_buf(),
            message: message.into(),
        }
    }
}
