//! romdex: identify ROMs.
//!
//! Hashes (CRC32, MD5, SHA1), the contents of archives and CHD disc images, RetroAchievements hashes (from
//! rcheevos itself) and DAT matching, for every RAMRaiders tool. Forma links this library and
//! hashes in its background sync; the `romdex` command writes the import format
//! (`docs/import-format.md`) for anything else.
//!
//! `unsafe` is denied workspace-wide and allowed only in the module that calls rcheevos (C).

pub mod archive;
pub mod batocera;
pub mod cache;
pub mod chd;
pub mod dat;
mod error;
pub mod format;
pub mod hash;
pub mod identify;
pub mod ra;
pub mod scan;

pub use error::{Error, Result};
pub use identify::{Identified, Progress, Report, identify, identify_with};
