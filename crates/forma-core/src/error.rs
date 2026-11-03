use std::path::{Path, PathBuf};

/// A failure that stops an operation. Problems Forma can work around (a malformed gamelist
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

    /// A game that cannot be launched, with a reason a user can act on.
    #[error("{0}")]
    Launch(String),

    /// A service could not be reached, or answered with something unreadable.
    #[error("{0}")]
    Network(String),

    /// A feature needs setting up first (e.g. a RetroAchievements API key).
    #[error("{0}")]
    NotConfigured(String),

    /// A request with a value that cannot work (a malformed address, a short password).
    #[error("{0}")]
    Invalid(String),

    /// Something this box cannot do (no Wi-Fi hardware, an 802.1X network).
    #[error("{0}")]
    Unsupported(String),

    /// Something asked for by name or id that is not there (a collection, a game).
    #[error("{0}")]
    NotFound(String),

    /// A view or a path into it that cannot be resolved.
    #[error("{0}")]
    View(String),

    #[error("library database: {0}")]
    Db(#[from] rusqlite::Error),

    /// Identifying ROMs (romdex): its hash cache, or a file it could not read.
    #[error("identifying games: {0}")]
    Identify(#[from] romdex::Error),
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

/// Something Forma skipped or worked around, with the file it came from.
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
